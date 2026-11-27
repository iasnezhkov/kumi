// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

use embedded_storage::nor_flash::NorFlash;
use kumi_boot_core::journal::{BootState, Journal, SlotStatus};
use kumi_boot_core::policy;
use kumi_boot_core::stage0::BootInfo;
use kumi_diag::AREA_SIZE;
use kumi_diag::log::CrashLog;
use kumi_icd::{
    Ack, CaseError, CountResult, RebootTarget, Slot, SlotInfo, SlotResult, SlotState, SlotTable, UpdateBlock, UpdateManifest,
    Version,
};
use kumi_layout::{BOOT_JOURNAL, CRASH_LOG, FLAG_LAB, HEADER_SIZE, ImageHeader, PAGE, SECTOR, SlotId, WriteGuard, Writer};
use kumi_settings::{Store, StoredSettings};
use sha2::{Digest, Sha256};

pub const HEALTHY_AFTER_MS: u32 = 30_000;
pub const CONFIRMED_HEALTHY_MS: u32 = 10_000;
pub const SETTINGS_SETTLE_MS: u32 = 2_000;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Health {
    since: Option<u32>,
    done: bool,
}

impl Health {
    pub fn due(&mut self, configured: bool, now: u32) -> bool {
        self.due_for(configured, now, HEALTHY_AFTER_MS)
    }

    pub fn due_for(&mut self, configured: bool, now: u32, after_ms: u32) -> bool {
        if self.done || !configured {
            self.since = None;
            return false;
        }
        let since = *self.since.get_or_insert(now);
        now.wrapping_sub(since) >= after_ms
    }

    pub fn finish(&mut self) {
        self.done = true;
    }

    pub fn reached(&self) -> bool {
        self.done
    }
}

struct Incoming {
    manifest: UpdateManifest,
    received: u32,
    hash: Sha256,
}

pub struct Storage<F> {
    flash: F,
    info: Option<BootInfo>,
    state: Option<BootState>,
    table: SlotTable,
    flash_ok: bool,
    incoming: Option<Incoming>,
    reboot: Option<RebootTarget>,
}

pub fn slot_id(slot: Slot) -> SlotId {
    match slot {
        Slot::A => SlotId::A,
        Slot::B => SlotId::B,
    }
}

fn wire_slot(slot: SlotId) -> Slot {
    match slot {
        SlotId::A => Slot::A,
        SlotId::B => Slot::B,
    }
}

fn version(header: &ImageHeader) -> Version {
    Version { major: header.version[0], minor: header.version[1], patch: header.version[2] }
}

impl<F: NorFlash + WriteGuard> Storage<F> {
    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn open(mut flash: F, info: Option<BootInfo>) -> Self {
        flash.set_writer(None);
        let state = Journal::new(&mut flash, BOOT_JOURNAL).ok().and_then(|mut journal| journal.load().ok().flatten());
        let mut table = SlotTable {
            booted: None,
            slots: [
                SlotInfo { slot: Slot::A, state: SlotState::Invalid, version: None, size: 0 },
                SlotInfo { slot: Slot::B, state: SlotState::Invalid, version: None, size: 0 },
            ],
        };
        for slot in SlotId::BOTH {
            let mut raw = [0u8; HEADER_SIZE];
            if flash.read(slot.region().offset, &mut raw).is_err() {
                continue;
            }
            let entry = &mut table.slots[slot.index()];
            if raw.iter().all(|b| *b == 0xff) {
                entry.state = SlotState::Empty;
            } else if let Ok(header) = ImageHeader::decode(&raw, slot.region().size) {
                entry.version = Some(version(&header));
                entry.size = HEADER_SIZE as u32 + header.payload_size;
                if let Some(state) = state
                    && state.slot(slot).generation == header.generation
                {
                    entry.state = match state.slot(slot).status {
                        SlotStatus::Empty | SlotStatus::Invalid => SlotState::Invalid,
                        SlotStatus::Trial => SlotState::Trial,
                        SlotStatus::Confirmed => SlotState::Confirmed,
                    };
                }
            }
        }
        let info = info.filter(|info| {
            state.is_some_and(|state| {
                info.boot_count == state.boot_count
                    && info.generation == state.slot(info.slot).generation
                    && matches!(table.slots[info.slot.index()].state, SlotState::Trial | SlotState::Confirmed)
                    && table.slots[info.slot.index()].version
                        == Some(Version {
                            major: info.image_version[0],
                            minor: info.image_version[1],
                            patch: info.image_version[2],
                        })
            })
        });
        table.booted = info.map(|info| wire_slot(info.slot));
        flash.set_writer(info.map(|info| Writer::Firmware { active: info.slot }));
        Self { flash, info, state, table, flash_ok: true, incoming: None, reboot: None }
    }

    pub fn booted(&self) -> Option<SlotId> {
        self.info.map(|info| info.slot)
    }

    pub fn flash(&mut self) -> &mut F {
        &mut self.flash
    }

    pub fn into_flash(self) -> F {
        self.flash
    }

    pub fn writable(&self) -> bool {
        self.info.is_some() && self.state.is_some() && self.flash_ok
    }

    pub fn slots(&self) -> SlotTable {
        self.table
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn load_settings(&mut self) -> StoredSettings {
        if !self.writable() {
            return StoredSettings::default();
        }
        Store::new(&mut self.flash).map(|mut store| store.load()).unwrap_or_default()
    }

    fn ready(&self) -> Ack {
        if !self.flash_ok {
            Err(CaseError::HardwareFault)
        } else if self.writable() {
            Ok(())
        } else {
            Err(CaseError::Unsupported)
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn save(&mut self, settings: &StoredSettings) -> Ack {
        self.ready()?;
        if self.incoming.is_some() || self.reboot.is_some() {
            return Err(CaseError::Busy);
        }
        Store::new(&mut self.flash).map_err(|_| CaseError::HardwareFault)?.save(settings).map_err(|_| CaseError::HardwareFault)
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    fn persist(&mut self, next: BootState) -> Ack {
        let stored = Journal::new(&mut self.flash, BOOT_JOURNAL)
            .and_then(|mut journal| journal.store(&next))
            .map_err(|_| CaseError::HardwareFault);
        match stored {
            Ok(state) => {
                self.state = Some(state);
                Ok(())
            }
            Err(error) => {
                self.flash_ok = false;
                Err(error)
            }
        }
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn confirm(&mut self) -> Ack {
        self.ready()?;
        if self.incoming.is_some() || self.reboot.is_some() {
            return Err(CaseError::Busy);
        }
        let info = self.info.ok_or(CaseError::WrongState)?;
        let state = self.state.ok_or(CaseError::WrongState)?;
        if state.slot(info.slot).status == SlotStatus::Confirmed && state.unhealthy_starts == 0 {
            return Ok(());
        }
        let next = policy::confirm(&state, info.slot).map_err(|_| CaseError::WrongState)?;
        self.persist(next)?;
        self.table.slots[info.slot.index()].state = SlotState::Confirmed;
        Ok(())
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn update_begin(&mut self, manifest: UpdateManifest) -> Ack {
        self.ready()?;
        if self.incoming.as_ref().is_some_and(|incoming| incoming.manifest == manifest) {
            return Ok(());
        }
        self.incoming = None;
        if self.reboot.is_some() {
            return Err(CaseError::Busy);
        }
        let info = self.info.ok_or(CaseError::WrongState)?;
        let state = self.state.ok_or(CaseError::WrongState)?;
        let target = slot_id(manifest.slot);
        if target == info.slot || state.slot(info.slot).status != SlotStatus::Confirmed {
            return Err(CaseError::WrongState);
        }
        if manifest.size < HEADER_SIZE as u32 || manifest.size > target.region().size {
            return Err(CaseError::InvalidArgument);
        }
        if state.slot(target).status == SlotStatus::Trial {
            return Err(CaseError::Busy);
        }
        let (_, next) = policy::begin_update(&state, info.slot);
        if next != state {
            self.persist(next)?;
        }
        self.table.slots[target.index()] = SlotInfo { slot: manifest.slot, state: SlotState::Empty, version: None, size: 0 };
        self.incoming = Some(Incoming { manifest, received: 0, hash: Sha256::new() });
        Ok(())
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn update_chunk(&mut self, block: UpdateBlock) -> CountResult {
        self.ready()?;
        let incoming = self.incoming.as_mut().ok_or(CaseError::WrongState)?;
        let size = block.data.len() as u32;
        let end = block.offset.checked_add(size).ok_or(CaseError::InvalidArgument)?;
        if size == 0
            || end > incoming.manifest.size
            || !block.offset.is_multiple_of(PAGE)
            || (size != PAGE && end != incoming.manifest.size)
        {
            return Err(CaseError::InvalidArgument);
        }
        let address = slot_id(incoming.manifest.slot).region().offset + block.offset;
        let mut readback = [0u8; PAGE as usize];
        if end <= incoming.received {
            self.flash.read(address, &mut readback[..size as usize]).map_err(|_| CaseError::HardwareFault)?;
            return if readback[..size as usize] == block.data[..] { Ok(end) } else { Err(CaseError::VerifyFailed) };
        }
        if block.offset != incoming.received {
            return Err(CaseError::InvalidArgument);
        }
        let write = (|| {
            if block.offset.is_multiple_of(SECTOR) {
                self.flash.erase(address, address + SECTOR)?;
            }
            self.flash.write(address, &block.data)?;
            self.flash.read(address, &mut readback[..size as usize])
        })();
        if write.is_err() || readback[..size as usize] != block.data[..] {
            self.incoming = None;
            return Err(CaseError::HardwareFault);
        }
        incoming.hash.update(&block.data);
        incoming.received = end;
        Ok(end)
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn update_finish(&mut self, verify: impl FnMut(&mut F, SlotId) -> bool) -> SlotResult {
        self.update_finish_with_policy(false, verify)
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn update_finish_with_policy(&mut self, allow_lab: bool, mut verify: impl FnMut(&mut F, SlotId) -> bool) -> SlotResult {
        self.ready()?;
        let incoming = self.incoming.take().ok_or(CaseError::WrongState)?;
        let manifest = incoming.manifest;
        let digest: [u8; 32] = incoming.hash.finalize().into();
        if incoming.received != manifest.size || digest != manifest.sha256 {
            return Err(CaseError::VerifyFailed);
        }
        let slot = slot_id(manifest.slot);
        let mut raw = [0u8; HEADER_SIZE];
        self.flash.read(slot.region().offset, &mut raw).map_err(|_| CaseError::HardwareFault)?;
        let header = ImageHeader::decode(&raw, slot.region().size).map_err(|_| CaseError::VerifyFailed)?;
        let state = self.state.ok_or(CaseError::WrongState)?;
        let confirmed_floor = policy::min_confirmed_generation(&state);
        if version(&header) != manifest.version
            || header.payload_size + HEADER_SIZE as u32 != manifest.size
            || header.signature[..32] != manifest.signature[0]
            || header.signature[32..] != manifest.signature[1]
            || header.generation < confirmed_floor
            || (!allow_lab && header.flags & FLAG_LAB != 0)
            || !verify(&mut self.flash, slot)
        {
            return Err(CaseError::VerifyFailed);
        }
        self.persist(policy::finish_update(&state, slot, header.generation))?;
        let result =
            SlotInfo { slot: manifest.slot, state: SlotState::Trial, version: Some(manifest.version), size: manifest.size };
        self.table.slots[slot.index()] = result;
        Ok(result)
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn request_reboot(&mut self, target: RebootTarget, mut verify: impl FnMut(&mut F, SlotId) -> bool) -> Ack {
        if target != RebootTarget::OtherSlot {
            if target == RebootTarget::Normal {
                let _ = self.mark_healthy_if_confirmed();
            }
            self.incoming = None;
            self.reboot = Some(target);
            return Ok(());
        }
        self.ready()?;
        if self.incoming.is_some() || self.reboot.is_some() {
            return Err(CaseError::Busy);
        }
        let slot = self.info.ok_or(CaseError::WrongState)?.slot.other();
        let state = self.state.ok_or(CaseError::WrongState)?;
        let next = policy::prefer(&state, slot).map_err(|_| CaseError::WrongState)?;
        if !verify(&mut self.flash, slot) {
            return Err(CaseError::VerifyFailed);
        }
        self.persist(next)?;
        self.reboot = Some(target);
        Ok(())
    }

    pub fn is_confirmed(&self) -> bool {
        self.info.zip(self.state).is_some_and(|(info, state)| state.slot(info.slot).status == SlotStatus::Confirmed)
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn mark_healthy_if_confirmed(&mut self) -> Ack {
        self.ready()?;
        if self.reboot.is_some() {
            return Err(CaseError::Busy);
        }
        let info = self.info.ok_or(CaseError::WrongState)?;
        let state = self.state.ok_or(CaseError::WrongState)?;
        if state.slot(info.slot).status != SlotStatus::Confirmed {
            return Ok(());
        }
        let next = policy::mark_healthy(&state);
        if next != state {
            self.persist(next)?;
        }
        Ok(())
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn reached_health(&mut self) -> Ack {
        self.ready()?;
        if self.reboot.is_some() {
            return Err(CaseError::Busy);
        }
        let info = self.info.ok_or(CaseError::WrongState)?;
        let state = self.state.ok_or(CaseError::WrongState)?;
        let trial = state.slot(info.slot).status == SlotStatus::Trial;
        let next = if trial {
            policy::confirm(&state, info.slot).map_err(|_| CaseError::WrongState)?
        } else {
            policy::mark_healthy(&state)
        };
        if next != state {
            self.persist(next)?;
        }
        if trial {
            self.table.slots[info.slot.index()].state = SlotState::Confirmed;
        }
        Ok(())
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn log_crash(&mut self, area: &[u8; AREA_SIZE]) -> Result<bool, CaseError> {
        self.ready()?;
        CrashLog::new(&mut self.flash, CRASH_LOG)
            .map_err(|_| CaseError::HardwareFault)?
            .append(area)
            .map_err(|_| CaseError::HardwareFault)
    }

    #[inline(never)]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.storage"))]
    pub fn logged_crash(&mut self) -> Option<[u8; AREA_SIZE]> {
        CrashLog::new(&mut self.flash, CRASH_LOG).ok()?.latest().ok().flatten().map(|entry| entry.area)
    }

    pub fn update_in_progress(&self) -> bool {
        self.incoming.is_some()
    }

    pub fn abort_update(&mut self) {
        self.incoming = None;
    }

    pub fn take_reboot(&mut self) -> Option<RebootTarget> {
        self.reboot.take()
    }
}

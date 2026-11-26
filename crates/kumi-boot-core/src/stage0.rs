// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::VerifyingKey;
use embedded_storage::nor_flash::NorFlash;
use kumi_layout::{BOOT_JOURNAL, DISARM, HEADER_SIZE, ImageHeader, SegmentKind, SlotId, crc32, get32};

use kumi_diag::{AREA_SIZE, Diag, stage};

use crate::journal::{BootState, Journal, Scan, SlotRecord, SlotStatus};
use crate::policy::{self, Decision, Halt};
use crate::verify::{stream_payload, verify_header};

pub const BOOT_INFO_MAGIC: [u8; 4] = *b"KBI1";
pub const BOOT_INFO_LEN: usize = 32;
pub const STAGE0_VERSION: [u8; 3] = [0, 2, 0];
const FLAG_PENDING_AT_ENTRY: u8 = 1;
const FLAG_MARKER_STORED: u8 = 2;
const FLAG_DISARMED: u8 = 4;

pub trait Memory {
    fn write(&mut self, address: u32, data: &[u8]);
    fn zero(&mut self, address: u32, len: u32);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootInfo {
    pub slot: SlotId,
    pub safe_mode: bool,
    pub boot_count: u32,
    pub generation: u32,
    pub image_version: [u8; 3],
    pub stage0_version: [u8; 3],
}

impl BootInfo {
    pub fn encode(&self) -> [u8; BOOT_INFO_LEN] {
        let mut out = [0u8; BOOT_INFO_LEN];
        out[0..4].copy_from_slice(&BOOT_INFO_MAGIC);
        out[4] = self.slot.index() as u8;
        out[5] = u8::from(self.safe_mode);
        out[8..12].copy_from_slice(&self.boot_count.to_le_bytes());
        out[12..16].copy_from_slice(&self.generation.to_le_bytes());
        out[16..19].copy_from_slice(&self.image_version);
        out[19..22].copy_from_slice(&self.stage0_version);
        let crc = crc32(&out[..28]);
        out[28..32].copy_from_slice(&crc.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8; BOOT_INFO_LEN]) -> Option<Self> {
        if bytes[0..4] != BOOT_INFO_MAGIC || crc32(&bytes[..28]) != get32(bytes, 28) || bytes[5] > 1 {
            return None;
        }
        Some(Self {
            slot: SlotId::from_index(usize::from(bytes[4]))?,
            safe_mode: bytes[5] == 1,
            boot_count: get32(bytes, 8),
            generation: get32(bytes, 12),
            image_version: [bytes[16], bytes[17], bytes[18]],
            stage0_version: [bytes[19], bytes[20], bytes[21]],
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootRomReason {
    NoBootableSlot,
    CrashStreak,
    JournalUnreadable,
    JournalUnwritable,
    LoadFailed,
    FlashUnavailable,
    NoKey,
    FlashMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Jump { entry: u32, info: BootInfo },
    WouldJump { entry: u32, info: BootInfo },
    BootRom(BootRomReason),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotCheck {
    NotChecked,
    Valid { generation: u32 },
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub journal_found: bool,
    pub before: BootState,
    pub after: BootState,
    pub checks: [SlotCheck; 2],
    pub stored: bool,
    pub previous_boot_crashed: bool,
    pub ram_crashes: u8,
    pub pending_at_entry: bool,
    pub marker_stored: bool,
    pub disarmed: bool,
}

impl Report {
    pub const EMPTY: Self = Self {
        journal_found: false,
        before: BootState::FRESH,
        after: BootState::FRESH,
        checks: [SlotCheck::NotChecked; 2],
        stored: false,
        previous_boot_crashed: false,
        ram_crashes: 0,
        pending_at_entry: false,
        marker_stored: false,
        disarmed: false,
    };
}

pub trait DiagRam {
    fn current(&mut self) -> [u8; AREA_SIZE];
    fn store_current(&mut self, area: &[u8; AREA_SIZE]);
    fn store_last_crash(&mut self, area: &[u8; AREA_SIZE]);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DiagAreas {
    pub current: [u8; AREA_SIZE],
    pub last_crash: [u8; AREA_SIZE],
}

impl DiagRam for DiagAreas {
    fn current(&mut self) -> [u8; AREA_SIZE] {
        self.current
    }

    fn store_current(&mut self, area: &[u8; AREA_SIZE]) {
        self.current = *area;
    }

    fn store_last_crash(&mut self, area: &[u8; AREA_SIZE]) {
        self.last_crash = *area;
    }
}

struct Trail<'d, D: DiagRam> {
    diag: &'d mut D,
    area: [u8; AREA_SIZE],
}

impl<'d, D: DiagRam> Trail<'d, D> {
    fn begin(diag: &'d mut D, boot_count: u32, crashes_before: u8) -> Self {
        let mut area = [0u8; AREA_SIZE];
        Diag::begin(&mut area, boot_count, crashes_before);
        diag.store_current(&area);
        Self { diag, area }
    }

    fn mark(&mut self, stage: u16) {
        if let Some(mut record) = Diag::attach(&mut self.area) {
            record.mark(stage);
        }
        self.diag.store_current(&self.area);
    }
}

pub struct Stage0<'k> {
    pub key: Option<&'k VerifyingKey>,
    pub dry_run: bool,
}

#[derive(Clone, Copy)]
struct Target {
    header: ImageHeader,
    raw: [u8; HEADER_SIZE],
    info: BootInfo,
}

fn first_state<F: NorFlash>(flash: &mut F, key: Option<&VerifyingKey>) -> BootState {
    let mut state = BootState::FRESH;
    let Some(key) = key else { return state };
    let mut best: Option<(SlotId, u32)> = None;
    for slot in SlotId::BOTH {
        if let Ok((header, _)) = verify_header(flash, slot, key) {
            *state.slot_mut(slot) =
                SlotRecord { status: SlotStatus::Trial, attempts: policy::TRIAL_ATTEMPTS, generation: header.generation };
            if best.is_none_or(|(_, g)| header.generation > g) {
                best = Some((slot, header.generation));
            }
        }
    }
    if let Some((slot, _)) = best {
        state.preferred = slot;
    }
    state
}

#[inline(never)]
fn store<F: NorFlash>(flash: F, scan: Option<Scan>, state: &BootState) -> (bool, F) {
    let mut journal = match Journal::new(flash, BOOT_JOURNAL) {
        Ok(journal) => journal,
        Err(_) => unreachable_flash(),
    };
    let stored = match scan {
        Some(scan) => journal.store_with_scan(scan, state).is_ok(),
        None => journal.store(state).is_ok(),
    };
    (stored, journal.into_inner())
}

fn disarm<F: NorFlash>(flash: &mut F) -> bool {
    let zeros = [0u8; DISARM.size as usize];
    if flash.write(DISARM.offset, &zeros).is_err() {
        return false;
    }
    let mut check = [0xFFu8; DISARM.size as usize];
    flash.read(DISARM.offset, &mut check).is_ok() && check == zeros
}

impl Stage0<'_> {
    pub fn run<F: NorFlash, M: Memory, D: DiagRam>(&self, flash: F, memory: &mut M, diag: &mut D) -> (Outcome, Report, F) {
        let entry_area = diag.current();
        let previous = kumi_diag::read(&entry_area);
        let ram_crashes = kumi_diag::consecutive_crashes(&entry_area);
        if !self.dry_run && previous.is_some_and(|r| r.crashed()) {
            diag.store_last_crash(&entry_area);
        }
        let mut report = Report { ram_crashes, ..Report::EMPTY };
        let mut journal = match Journal::new(flash, BOOT_JOURNAL) {
            Ok(journal) => journal,
            Err(_) => unreachable_flash(),
        };
        let loaded = journal.load_scan();
        let mut flash = journal.into_inner();
        let (before, initial_scan) = match loaded {
            Ok(scan) => match scan.latest() {
                Some(state) => {
                    report.journal_found = true;
                    (state, scan)
                }
                None => (first_state(&mut flash, self.key), scan),
            },
            Err(_) => {
                if !self.dry_run {
                    let boot_count = previous.map_or(0, |r| r.boot_count.wrapping_add(1));
                    Trail::begin(diag, boot_count, ram_crashes).mark(stage::STAGE0_BOOTROM);
                }
                return (Outcome::BootRom(BootRomReason::JournalUnreadable), report, flash);
            }
        };
        report.before = before;
        report.pending_at_entry = before.bootrom_pending;
        report.previous_boot_crashed = kumi_diag::previous_boot_crashed(&entry_area, before.boot_count);
        let mut checks = [SlotCheck::NotChecked; 2];
        let mut raws: [Option<(ImageHeader, [u8; HEADER_SIZE])>; 2] = [None, None];
        let mut loaded: Option<SlotId> = None;
        let (target, mut after) = match self.key {
            None => {
                let mut after = before;
                after.boot_count = after.boot_count.wrapping_add(1);
                (Err(BootRomReason::NoKey), after)
            }
            Some(key) => {
                let dry_run = self.dry_run;
                let (decision, after) = policy::decide(&before, |slot| {
                    let checked = verify_header(&mut flash, slot, key)
                        .ok()
                        .filter(|(header, raw)| load(&mut flash, (!dry_run).then_some(&mut *memory), slot, header, raw));
                    match checked {
                        Some((header, raw)) => {
                            checks[slot.index()] = SlotCheck::Valid { generation: header.generation };
                            raws[slot.index()] = Some((header, raw));
                            loaded = (!dry_run).then_some(slot);
                            Some(header.generation)
                        }
                        None => {
                            checks[slot.index()] = SlotCheck::Invalid;
                            None
                        }
                    }
                });
                let target = match decision {
                    Decision::BootRom(Halt::NoBootableSlot) => Err(BootRomReason::NoBootableSlot),
                    Decision::BootRom(Halt::CrashStreak) => Err(BootRomReason::CrashStreak),
                    Decision::Boot { slot, safe_mode } => match raws[slot.index()] {
                        None => Err(BootRomReason::NoBootableSlot),
                        Some((header, raw)) => Ok(Target {
                            header,
                            raw,
                            info: BootInfo {
                                slot,
                                safe_mode,
                                boot_count: after.boot_count,
                                generation: header.generation,
                                image_version: header.version,
                                stage0_version: STAGE0_VERSION,
                            },
                        }),
                    },
                };
                (target, after)
            }
        };
        report.after = after;
        report.checks = checks;
        if self.dry_run {
            let outcome = match target {
                Ok(t) => Outcome::WouldJump { entry: t.header.entry, info: t.info },
                Err(reason) => Outcome::BootRom(reason),
            };
            return (outcome, report, flash);
        }
        let mut record = Trail::begin(diag, after.boot_count, ram_crashes);
        let outcome = match target {
            Err(reason) => Outcome::BootRom(reason),
            Ok(mut target) => {
                after.bootrom_pending = false;
                let (stored, rest) = store(flash, Some(initial_scan), &after);
                flash = rest;
                report.stored = stored;
                record.mark(stage::STAGE0_JOURNAL);
                let trial = after.slot(target.info.slot).status == SlotStatus::Trial;
                if !stored && trial {
                    Outcome::BootRom(BootRomReason::JournalUnwritable)
                } else if !stored && ram_crashes >= policy::BOOTROM_AFTER {
                    Outcome::BootRom(BootRomReason::CrashStreak)
                } else {
                    target.info.safe_mode |= !stored && ram_crashes >= policy::SAFE_MODE_AFTER;
                    record.mark(stage::STAGE0_VERIFIED);
                    if loaded == Some(target.info.slot)
                        || load(&mut flash, Some(memory), target.info.slot, &target.header, &target.raw)
                    {
                        record.mark(stage::STAGE0_LOADED);
                        memory.write(kumi_layout::BOOT_INFO_ADDRESS, &target.info.encode());
                        Outcome::Jump { entry: target.header.entry, info: target.info }
                    } else {
                        Outcome::BootRom(BootRomReason::LoadFailed)
                    }
                }
            }
        };
        if let Outcome::BootRom(_) = outcome {
            if !report.pending_at_entry {
                after = policy::request_boot_rom(&after);
                let (stored, rest) = store(flash, None, &after);
                flash = rest;
                report.marker_stored = stored;
                record.mark(stage::STAGE0_JOURNAL);
            }
            if !report.marker_stored {
                report.disarmed = disarm(&mut flash);
                if report.disarmed {
                    record.mark(stage::STAGE0_DISARMED);
                }
            }
            record.mark(stage::STAGE0_BOOTROM);
        } else {
            record.mark(stage::STAGE0_JUMP);
        }
        report.after = after;
        (outcome, report, flash)
    }
}

fn unreachable_flash() -> ! {
    panic!("flash geometry does not match the journal")
}

#[inline(never)]
fn load<F: NorFlash, M: Memory>(
    flash: &mut F,
    mut memory: Option<&mut M>,
    slot: SlotId,
    header: &ImageHeader,
    verified: &[u8; HEADER_SIZE],
) -> bool {
    let base = slot.region().offset;
    let mut raw = [0u8; HEADER_SIZE];
    if flash.read(base, &mut raw).is_err() || raw != *verified {
        return false;
    }
    if let Some(mem) = memory.as_deref_mut() {
        for segment in header.segments().filter(|s| s.kind == SegmentKind::Zero) {
            mem.zero(segment.destination, segment.size);
        }
    }
    stream_payload(flash, slot, header, &raw, &mut |offset, data| {
        if let Some(mem) = memory.as_deref_mut() {
            let end = offset + data.len() as u32;
            for segment in header.segments().filter(|s| s.kind == SegmentKind::Copy) {
                let from = offset.max(segment.offset);
                let to = end.min(segment.offset + segment.size);
                if from < to {
                    mem.write(
                        segment.destination + (from - segment.offset),
                        &data[(from - offset) as usize..(to - offset) as usize],
                    );
                }
            }
        }
    })
    .unwrap_or(false)
}

pub fn encode_report(outcome: &Outcome, report: &Report) -> [u8; 64] {
    let mut out = [0u8; 64];
    out[0..4].copy_from_slice(b"KS0R");
    let (kind, entry, info) = match outcome {
        Outcome::Jump { entry, info } => (1u8, *entry, Some(*info)),
        Outcome::WouldJump { entry, info } => (2, *entry, Some(*info)),
        Outcome::BootRom(reason) => (
            match reason {
                BootRomReason::NoBootableSlot => 10,
                BootRomReason::CrashStreak => 11,
                BootRomReason::JournalUnreadable => 12,
                BootRomReason::JournalUnwritable => 13,
                BootRomReason::LoadFailed => 14,
                BootRomReason::FlashUnavailable => 15,
                BootRomReason::NoKey => 16,
                BootRomReason::FlashMismatch => 17,
            },
            0,
            None,
        ),
    };
    out[4] = kind;
    out[5] = u8::from(report.journal_found);
    out[6] = u8::from(report.stored);
    out[7] = (if report.pending_at_entry { FLAG_PENDING_AT_ENTRY } else { 0 })
        | (if report.marker_stored { FLAG_MARKER_STORED } else { 0 })
        | (if report.disarmed { FLAG_DISARMED } else { 0 });
    out[8..12].copy_from_slice(&entry.to_le_bytes());
    if let Some(info) = info {
        out[12..44].copy_from_slice(&info.encode());
    }
    for (i, check) in report.checks.iter().enumerate() {
        out[44 + i] = match check {
            SlotCheck::NotChecked => 0,
            SlotCheck::Valid { .. } => 1,
            SlotCheck::Invalid => 2,
        };
    }
    out[46] = report.after.slots[0].attempts;
    out[47] = report.after.slots[1].attempts;
    out[48..52].copy_from_slice(&report.after.boot_count.to_le_bytes());
    out[52] = report.after.unhealthy_starts;
    out[53] = u8::from(report.previous_boot_crashed);
    out[54] = report.ram_crashes;
    let crc = crc32(&out[..60]);
    out[60..64].copy_from_slice(&crc.to_le_bytes());
    out
}

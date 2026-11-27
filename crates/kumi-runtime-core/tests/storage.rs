// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use ed25519_dalek::SigningKey;
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use kumi_boot_core::journal::{BootState, Journal, SlotRecord, SlotStatus};
use kumi_boot_core::stage0::BootInfo;
use kumi_boot_core::{policy, verify_slot};
use kumi_client::update::manifest_for;
use kumi_diag::{AREA_SIZE, Diag, Reason, Registers};
use kumi_icd::{CaseError, RebootTarget, Slot, SlotState, UpdateBlock, UpdateManifest, Version};
use kumi_layout::{BOOT_JOURNAL, CRASH_LOG, FLASH_SIZE, SECTOR, SLOT_A, SlotId, WriteGuard, Writer, region_for_write};
use kumi_runtime_core::{CONFIRMED_HEALTHY_MS, HEALTHY_AFTER_MS, Health, Storage};
use kumi_settings::StoredSettings;

#[derive(Debug, Clone, Copy)]
struct Cut;

impl NorFlashError for Cut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

#[derive(Clone)]
struct Flash {
    data: Vec<u8>,
    budget: Option<usize>,
    writes: usize,
    cut_at: Option<(u32, usize)>,
    writer: Option<Writer>,
}

impl Flash {
    fn spend(&mut self, amount: usize) -> usize {
        self.writes += 1;
        let allowed = self.budget.map_or(amount, |left| left.min(amount));
        self.budget = self.budget.map(|left| left - allowed);
        allowed
    }

    fn permitted(&self, from: u32, to: u32) -> bool {
        self.writer.is_some_and(|writer| region_for_write(from, to - from, writer).is_some())
    }
}

impl WriteGuard for Flash {
    fn set_writer(&mut self, writer: Option<Writer>) {
        self.writer = writer;
    }
}

impl ErrorType for Flash {
    type Error = Cut;
}

impl ReadNorFlash for Flash {
    const READ_SIZE: usize = 1;
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        bytes.copy_from_slice(self.data.get(offset as usize..offset as usize + bytes.len()).ok_or(Cut)?);
        Ok(())
    }
    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl NorFlash for Flash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;
    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Cut> {
        assert!(self.permitted(offset, offset + bytes.len() as u32), "write at {offset:#x} with {:?}", self.writer);
        if let Some((address, left)) = self.cut_at
            && address == offset
        {
            self.budget = Some(left);
        }
        let count = self.spend(bytes.len());
        for (dst, value) in self.data[offset as usize..].iter_mut().zip(&bytes[..count]) {
            *dst &= value;
        }
        if count == bytes.len() { Ok(()) } else { Err(Cut) }
    }
    fn erase(&mut self, from: u32, to: u32) -> Result<(), Cut> {
        assert!(self.permitted(from, to), "erase at {from:#x} with {:?}", self.writer);
        assert!(from.is_multiple_of(SECTOR) && to.is_multiple_of(SECTOR));
        let size = (to - from) as usize;
        let count = self.spend(size);
        self.data[from as usize..from as usize + count].fill(0xff);
        if count == size { Ok(()) } else { Err(Cut) }
    }
}

fn key() -> SigningKey {
    SigningKey::from_bytes(&[0x4b; 32])
}

fn fixture(trial: bool) -> (Flash, BootInfo) {
    let image = kumi_install::sample::image(&key(), [0, 1, 0], 1);
    let mut flash = Flash { data: vec![0xff; FLASH_SIZE as usize], budget: None, writes: 0, cut_at: None, writer: None };
    flash.data[SLOT_A.offset as usize..SLOT_A.offset as usize + image.len()].copy_from_slice(&image);
    let mut state = BootState::FRESH;
    state.boot_count = 1;
    state.unhealthy_starts = 1;
    state.slots[0] =
        SlotRecord { status: if trial { SlotStatus::Trial } else { SlotStatus::Confirmed }, attempts: 2, generation: 1 };
    flash.writer = Some(Writer::Stage0);
    Journal::new(&mut flash, BOOT_JOURNAL).unwrap().store(&state).unwrap();
    flash.writer = None;
    (
        flash,
        BootInfo {
            slot: SlotId::A,
            safe_mode: false,
            boot_count: 1,
            generation: 1,
            image_version: [0, 1, 0],
            stage0_version: [0, 1, 0],
        },
    )
}

fn update(storage: &Storage<Flash>) -> (Vec<u8>, UpdateManifest) {
    let image = kumi_install::sample::image(&key(), [0, 2, 0], 2);
    let manifest = manifest_for(&image, &storage.slots()).unwrap();
    (image, manifest)
}

fn block(offset: usize, data: &[u8]) -> UpdateBlock {
    UpdateBlock { offset: offset as u32, data: data.try_into().unwrap() }
}

fn upload(storage: &mut Storage<Flash>, image: &[u8]) {
    for (i, data) in image.chunks(256).enumerate() {
        assert_eq!(storage.update_chunk(block(i * 256, data)), Ok((i * 256 + data.len()) as u32));
    }
}

fn verify(flash: &mut Flash, slot: SlotId) -> bool {
    verify_slot(flash, slot, &key().verifying_key()).is_ok()
}

fn generation(flash: &mut Flash, slot: SlotId) -> Option<u32> {
    verify_slot(flash, slot, &key().verifying_key()).ok().map(|header| header.generation)
}

#[test]
fn absent_or_inconsistent_boot_information_keeps_storage_read_only() {
    let (flash, info) = fixture(false);
    for boot in [None, Some(BootInfo { boot_count: 99, ..info }), Some(BootInfo { generation: 2, ..info })] {
        let mut storage = Storage::open(flash.clone(), boot);
        assert!(!storage.writable());
        assert_eq!(storage.save(&StoredSettings::default()), Err(CaseError::Unsupported));
        assert_eq!(storage.confirm(), Err(CaseError::Unsupported));
    }
}

#[test]
fn confirmation_is_persisted_and_idempotent_across_restarts() {
    let (flash, info) = fixture(true);
    let mut storage = Storage::open(flash, Some(info));
    assert_eq!(storage.confirm(), Ok(()));
    assert_eq!(storage.confirm(), Ok(()));
    let flash = storage.into_flash();
    assert_eq!(flash.writes, 2);
    let storage = Storage::open(flash, Some(info));
    assert_eq!(storage.slots().slots[0].state, SlotState::Confirmed);
}

#[test]
fn an_update_requires_a_confirmed_active_slot_and_never_writes_it() {
    let (flash, info) = fixture(true);
    let original = flash.data[SLOT_A.offset as usize..SLOT_A.end() as usize].to_vec();
    let mut storage = Storage::open(flash, Some(info));
    let (image, manifest) = update(&storage);
    assert_eq!(storage.update_begin(manifest), Err(CaseError::WrongState));
    storage.confirm().unwrap();
    assert_eq!(storage.update_begin(UpdateManifest { slot: Slot::A, ..manifest }), Err(CaseError::WrongState));
    storage.update_begin(manifest).unwrap();
    assert_eq!(storage.save(&StoredSettings::default()), Err(CaseError::Busy));
    assert_eq!(storage.update_chunk(block(256, &image[256..512])), Err(CaseError::InvalidArgument));
    let first = block(0, &image[..256]);
    assert_eq!(storage.update_chunk(first.clone()), Ok(256));
    assert_eq!(storage.update_chunk(first), Ok(256));
    upload(&mut storage, &image);
    let result = storage.update_finish(verify).unwrap();
    assert_eq!(result.state, SlotState::Trial);
    assert_eq!(result.slot, Slot::B);
    let mut flash = storage.into_flash();
    assert_eq!(&flash.data[SLOT_A.offset as usize..SLOT_A.end() as usize], original);
    let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
    assert_eq!(state.slot(SlotId::B).status, SlotStatus::Trial);
    assert_eq!(
        policy::decide(&state, |s| generation(&mut flash, s)).0,
        policy::Decision::Boot { slot: SlotId::B, safe_mode: false }
    );
}

#[test]
fn incomplete_bad_hash_and_bad_signature_images_do_not_become_bootable() {
    for kind in 0..3 {
        let (flash, info) = fixture(false);
        let mut storage = Storage::open(flash, Some(info));
        let (mut image, mut manifest) = update(&storage);
        if kind == 1 {
            manifest.sha256[0] ^= 1;
        }
        if kind == 2 {
            image[160] ^= 1;
            let crc = kumi_layout::crc32(&image[..252]);
            image[252..256].copy_from_slice(&crc.to_le_bytes());
            manifest = manifest_for(&image, &storage.slots()).unwrap();
        }
        storage.update_begin(manifest).unwrap();
        if kind != 0 {
            upload(&mut storage, &image);
        }
        assert_eq!(storage.update_finish(verify), Err(CaseError::VerifyFailed));
        let mut flash = storage.into_flash();
        let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
        assert_eq!(state.slot(SlotId::B).status, SlotStatus::Empty);
        assert_eq!(
            policy::decide(&state, |s| generation(&mut flash, s)).0,
            policy::Decision::Boot { slot: SlotId::A, safe_mode: false }
        );
    }
}

#[test]
fn rollback_checks_the_target_before_committing_a_preference() {
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash, Some(info));
    assert_eq!(storage.request_reboot(RebootTarget::OtherSlot, verify), Err(CaseError::WrongState));
    let (image, manifest) = update(&storage);
    storage.update_begin(manifest).unwrap();
    upload(&mut storage, &image);
    storage.update_finish(verify).unwrap();
    assert_eq!(storage.request_reboot(RebootTarget::OtherSlot, |_, _| false), Err(CaseError::VerifyFailed));
    assert!(storage.take_reboot().is_none());
    storage.request_reboot(RebootTarget::OtherSlot, verify).unwrap();
    assert_eq!(storage.take_reboot(), Some(RebootTarget::OtherSlot));
}

#[test]
fn torn_settings_writes_preserve_a_complete_old_or_new_record() {
    let (flash, info) = fixture(false);
    let mut initial = Storage::open(flash, Some(info));
    let old = StoredSettings::default();
    initial.save(&old).unwrap();
    let flash = initial.into_flash();
    let mut new = old.clone();
    new.mic.gain_centi_db = 120;
    for cut in [0, 1, 4095, 4096, 4097, 4100, 4110, 4130, 4150, 4300] {
        let mut trial = flash.clone();
        trial.budget = Some(cut);
        let mut storage = Storage::open(trial, Some(info));
        let _ = storage.save(&new);
        let mut result = storage.into_flash();
        result.budget = None;
        let actual = Storage::open(result, Some(info)).load_settings();
        assert!(actual == old || actual == new, "cut={cut}");
    }
}

#[test]
fn torn_journal_or_update_pages_keep_the_confirmed_slot_bootable() {
    for cut in [0, 1, 15, 31, 32, 33, 100, 4127, 4128, 4200, 4383, 4400, 9000] {
        let (mut flash, info) = fixture(false);
        flash.budget = Some(cut);
        let mut storage = Storage::open(flash, Some(info));
        let (image, manifest) = update(&storage);
        if storage.update_begin(manifest).is_ok() {
            for (i, data) in image.chunks(256).enumerate() {
                if storage.update_chunk(block(i * 256, data)).is_err() {
                    break;
                }
            }
            let _ = storage.update_finish(verify);
        }
        let mut flash = storage.into_flash();
        flash.budget = None;
        let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
        assert_eq!(state.slot(SlotId::A).status, SlotStatus::Confirmed);
        assert!(verify(&mut flash, SlotId::A));
        assert_eq!(
            policy::decide(&state, |s| generation(&mut flash, s)).0,
            policy::Decision::Boot { slot: SlotId::A, safe_mode: false }
        );
    }
}

#[test]
fn an_unconfirmed_image_exhausts_its_attempts_and_returns_to_the_previous_slot() {
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash, Some(info));
    let (image, manifest) = update(&storage);
    storage.update_begin(manifest).unwrap();
    upload(&mut storage, &image);
    storage.update_finish(verify).unwrap();
    let mut flash = storage.into_flash();
    flash.writer = Some(Writer::Stage0);
    for expected in [SlotId::B, SlotId::B, SlotId::B, SlotId::A] {
        let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
        let (decision, next) = policy::decide(&state, |slot| generation(&mut flash, slot));
        assert_eq!(decision, policy::Decision::Boot { slot: expected, safe_mode: false });
        Journal::new(&mut flash, BOOT_JOURNAL).unwrap().store(&next).unwrap();
    }
    assert_eq!(
        Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap().slot(SlotId::B).status,
        SlotStatus::Invalid
    );
}

#[test]
fn a_torn_confirmation_is_recovered_from_the_last_complete_journal_record() {
    for cut in [0, 1, 15, 28, 31, 32] {
        let (mut flash, info) = fixture(true);
        flash.budget = Some(cut);
        let mut storage = Storage::open(flash, Some(info));
        let result = storage.confirm();
        if result.is_err() {
            assert!(!storage.writable());
        }
        let mut flash = storage.into_flash();
        flash.budget = None;
        let mut restarted = Storage::open(flash, Some(info));
        assert!(matches!(restarted.slots().slots[0].state, SlotState::Trial | SlotState::Confirmed));
        assert_eq!(restarted.confirm(), Ok(()));
        assert_eq!(restarted.slots().slots[0].state, SlotState::Confirmed);
    }
}

#[test]
fn a_cut_while_committing_a_verified_update_never_selects_an_incomplete_image() {
    for cut in [0, 1, 15, 28, 31, 32] {
        let (mut flash, info) = fixture(false);
        flash.cut_at = Some((BOOT_JOURNAL.offset + 64, cut));
        let mut storage = Storage::open(flash, Some(info));
        let (image, manifest) = update(&storage);
        storage.update_begin(manifest).unwrap();
        upload(&mut storage, &image);
        let _ = storage.update_finish(verify);
        let mut flash = storage.into_flash();
        flash.budget = None;
        flash.cut_at = None;
        let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
        let (decision, _) = policy::decide(&state, |slot| generation(&mut flash, slot));
        let policy::Decision::Boot { slot, .. } = decision else { panic!("cut={cut}") };
        assert!(verify(&mut flash, slot));
        assert_eq!(state.slot(SlotId::A).status, SlotStatus::Confirmed);
    }
}

#[test]
fn normal_and_boot_rom_reboots_need_neither_writable_storage_nor_a_finished_update() {
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash.clone(), None);
    assert_eq!(storage.request_reboot(RebootTarget::OtherSlot, verify), Err(CaseError::Unsupported));
    assert_eq!(storage.request_reboot(RebootTarget::BootRom, verify), Ok(()));
    assert_eq!(storage.take_reboot(), Some(RebootTarget::BootRom));
    let mut storage = Storage::open(flash, Some(info));
    let (image, manifest) = update(&storage);
    storage.update_begin(manifest).unwrap();
    upload(&mut storage, &image[..256]);
    assert_eq!(storage.request_reboot(RebootTarget::OtherSlot, verify), Err(CaseError::Busy));
    assert_eq!(storage.request_reboot(RebootTarget::Normal, verify), Ok(()));
    assert_eq!(storage.take_reboot(), Some(RebootTarget::Normal));
    assert_eq!(storage.update_chunk(block(256, &image[256..512])), Err(CaseError::WrongState));
}

#[test]
fn an_abandoned_update_gives_way_to_a_new_manifest_or_an_abort() {
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash, Some(info));
    let (image, manifest) = update(&storage);
    storage.update_begin(manifest).unwrap();
    upload(&mut storage, &image[..512]);
    assert_eq!(storage.save(&StoredSettings::default()), Err(CaseError::Busy));
    storage.abort_update();
    assert_eq!(storage.save(&StoredSettings::default()), Ok(()));
    storage.update_begin(manifest).unwrap();
    upload(&mut storage, &image[..512]);
    let other = kumi_install::sample::image(&key(), [0, 3, 0], 3);
    let replacement = manifest_for(&other, &storage.slots()).unwrap();
    assert_eq!(storage.update_begin(replacement), Ok(()));
    upload(&mut storage, &other);
    let installed = storage.update_finish(verify).unwrap();
    assert_eq!(installed.version, Some(Version { major: 0, minor: 3, patch: 0 }));
}

fn crash(message: &str) -> [u8; AREA_SIZE] {
    let mut area = [0u8; AREA_SIZE];
    Diag::begin(&mut area, 1, 0).crash(Reason::Trap, Registers { mcause: 2, ..Registers::default() }, 5, message);
    area
}

fn journal(storage: Storage<Flash>) -> (BootState, Flash) {
    let mut flash = storage.into_flash();
    let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
    (state, flash)
}

#[test]
fn the_health_point_confirms_a_trial_and_clears_the_unhealthy_count() {
    let (flash, info) = fixture(true);
    let mut storage = Storage::open(flash, Some(info));
    assert_eq!(storage.reached_health(), Ok(()));
    assert_eq!(storage.slots().slots[0].state, SlotState::Confirmed);
    let (state, flash) = journal(storage);
    assert_eq!(state.slot(SlotId::A).status, SlotStatus::Confirmed);
    assert_eq!(state.unhealthy_starts, 0);
    let mut storage = Storage::open(flash, Some(info));
    let writes = storage.flash().writes;
    assert_eq!(storage.reached_health(), Ok(()));
    assert_eq!(storage.flash().writes, writes, "nothing to record a second time");
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash, Some(info));
    assert_eq!(storage.reached_health(), Ok(()));
    let (state, _) = journal(storage);
    assert_eq!((state.slot(SlotId::A).status, state.unhealthy_starts), (SlotStatus::Confirmed, 0));
}

#[test]
fn a_host_loaded_runtime_never_writes_and_a_pending_reboot_defers_the_health_point() {
    let (flash, info) = fixture(true);
    let before = flash.writes;
    let mut storage = Storage::open(flash.clone(), None);
    assert_eq!(storage.reached_health(), Err(CaseError::Unsupported));
    assert_eq!(storage.log_crash(&crash("ram")), Err(CaseError::Unsupported));
    assert_eq!(storage.into_flash().writes, before);
    let mut storage = Storage::open(flash, Some(info));
    storage.request_reboot(RebootTarget::Normal, verify).unwrap();
    assert_eq!(storage.reached_health(), Err(CaseError::Busy));
}

#[test]
fn the_last_crash_is_saved_to_the_crash_log_once() {
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash, Some(info));
    assert_eq!(storage.logged_crash(), None);
    let area = crash("saved");
    assert_eq!(storage.log_crash(&area), Ok(true));
    assert_eq!(storage.log_crash(&area), Ok(false));
    let flash = storage.into_flash();
    assert!(flash.data[CRASH_LOG.offset as usize..CRASH_LOG.offset as usize + 4] == *b"KCL1");
    let mut reader = Storage::open(flash, None);
    assert_eq!(reader.logged_crash(), Some(area));
}

#[test]
fn health_needs_a_configured_bus_for_the_whole_interval() {
    let mut health = Health::default();
    assert!(!health.due(false, 0));
    assert!(!health.due(true, 100));
    assert!(!health.due(true, 100 + HEALTHY_AFTER_MS - 1));
    assert!(!health.due(false, 100 + HEALTHY_AFTER_MS));
    assert!(!health.due(true, 200 + HEALTHY_AFTER_MS));
    assert!(health.due(true, 200 + 2 * HEALTHY_AFTER_MS));
    assert!(health.due(true, u32::MAX));
    health.finish();
    assert!(health.reached());
    assert!(!health.due(true, 0));
}

#[test]
fn intentional_normal_reboot_clears_unhealthy_starts_on_confirmed_slot_without_confirming_trial() {
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash, Some(info));
    assert!(storage.is_confirmed());
    assert_eq!(storage.request_reboot(RebootTarget::Normal, verify), Ok(()));
    let mut flash = storage.into_flash();
    let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
    assert_eq!(state.unhealthy_starts, 0);

    let (flash, info) = fixture(true);
    let mut storage = Storage::open(flash, Some(info));
    assert!(!storage.is_confirmed());
    assert_eq!(storage.mark_healthy_if_confirmed(), Ok(()));
    assert_eq!(storage.request_reboot(RebootTarget::Normal, verify), Ok(()));
    let mut flash = storage.into_flash();
    let state = Journal::new(&mut flash, BOOT_JOURNAL).unwrap().load().unwrap().unwrap();
    assert_eq!(state.slot(SlotId::A).status, SlotStatus::Trial);

    let mut health = Health::default();
    assert!(!health.due_for(true, 50, CONFIRMED_HEALTHY_MS));
    assert!(!health.due_for(true, 50 + CONFIRMED_HEALTHY_MS - 1, CONFIRMED_HEALTHY_MS));
    assert!(health.due_for(true, 50 + CONFIRMED_HEALTHY_MS, CONFIRMED_HEALTHY_MS));
}

#[test]
fn update_finish_rejects_downgrade_and_lab_images() {
    let (flash, info) = fixture(false);
    let mut storage = Storage::open(flash, Some(info));
    let older = kumi_install::sample::image(&key(), [0, 1, 0], 0);
    let manifest = manifest_for(&older, &storage.slots()).unwrap();
    storage.update_begin(manifest).unwrap();
    upload(&mut storage, &older);
    assert_eq!(storage.update_finish(verify), Err(CaseError::VerifyFailed));

    let (mut lab, _) = update(&storage);
    let mut header = kumi_layout::ImageHeader::decode(&lab[..kumi_layout::HEADER_SIZE].try_into().unwrap(), SLOT_A.size).unwrap();
    header.flags |= kumi_layout::FLAG_LAB;
    let payload = &lab[kumi_layout::HEADER_SIZE..];
    let mut signed = [0u8; kumi_layout::SIGNED_HEADER_LEN];
    signed.copy_from_slice(&header.encode()[..kumi_layout::SIGNED_HEADER_LEN]);
    header.digest = kumi_boot_core::image_digest(&signed, payload);
    use ed25519_dalek::Signer;
    header.signature = key().sign(&header.digest).to_bytes();
    lab[..kumi_layout::HEADER_SIZE].copy_from_slice(&header.encode());
    let lab_manifest = manifest_for(&lab, &storage.slots()).unwrap();
    storage.update_begin(lab_manifest).unwrap();
    upload(&mut storage, &lab);
    assert_eq!(storage.update_finish(verify), Err(CaseError::VerifyFailed));
}

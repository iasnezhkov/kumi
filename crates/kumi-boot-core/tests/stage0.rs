// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::HashMap;

use ed25519_dalek::{Signer, SigningKey};
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use kumi_boot_core::stage0::*;
use kumi_boot_core::*;
use kumi_diag::{AREA_SIZE, Diag, Reason, Registers, stage};
use kumi_layout::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fault;

impl NorFlashError for Fault {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

const STAGE0_HEAD: [u8; 8] = [0x6F, 0x00, 0x40, 0x0A, b'S', b'P', b'V', b'@'];

#[derive(Clone)]
struct Flash {
    data: Vec<u8>,
    writes_fail: bool,
    journal_fails: bool,
    slot_reads_fail_after: Option<usize>,
    change_slot_after_a_write: Option<u32>,
    writes: usize,
    slot_bytes_read: usize,
}

impl Flash {
    fn new() -> Self {
        let mut data = vec![0xFF; FLASH_SIZE as usize];
        data[..8].copy_from_slice(&STAGE0_HEAD);
        Self {
            data,
            writes_fail: false,
            journal_fails: false,
            slot_reads_fail_after: None,
            change_slot_after_a_write: None,
            slot_bytes_read: 0,
            writes: 0,
        }
    }

    fn fails(&self, offset: u32) -> bool {
        self.writes_fail || (self.journal_fails && BOOT_JOURNAL.contains(offset, 1))
    }

    fn disarmed(&self) -> bool {
        self.data[4..8] == [0; 4] && self.data[..4] == STAGE0_HEAD[..4]
    }
}

impl ErrorType for Flash {
    type Error = Fault;
}

impl ReadNorFlash for Flash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Fault> {
        if offset >= SLOT_A.offset && offset < SLOT_B.end() {
            self.slot_bytes_read += bytes.len();
            match self.slot_reads_fail_after.as_mut() {
                Some(0) => return Err(Fault),
                Some(left) => *left -= 1,
                None => {}
            }
        }
        bytes.copy_from_slice(&self.data[offset as usize..offset as usize + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl NorFlash for Flash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = 4096;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Fault> {
        assert!(region_for_write(from, to - from, Writer::Stage0).is_some(), "stage0 erased {from:#x}..{to:#x}");
        if self.fails(from) {
            return Err(Fault);
        }
        self.writes += 1;
        self.data[from as usize..to as usize].fill(0xFF);
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Fault> {
        assert!(region_for_write(offset, bytes.len() as u32, Writer::Stage0).is_some(), "stage0 wrote {offset:#x}");
        if self.fails(offset) {
            return Err(Fault);
        }
        self.writes += 1;
        for (i, b) in bytes.iter().enumerate() {
            self.data[offset as usize + i] &= *b;
        }
        if let Some(at) = self.change_slot_after_a_write.take() {
            self.data[at as usize] ^= 0x10;
        }
        Ok(())
    }
}

#[derive(Default)]
struct Ram {
    bytes: HashMap<u32, u8>,
    writes: usize,
}

impl Memory for Ram {
    fn write(&mut self, address: u32, data: &[u8]) {
        self.writes += 1;
        for (i, b) in data.iter().enumerate() {
            self.bytes.insert(address + i as u32, *b);
        }
    }

    fn zero(&mut self, address: u32, len: u32) {
        self.writes += 1;
        for i in 0..len {
            self.bytes.insert(address + i, 0);
        }
    }
}

impl Ram {
    fn read(&self, address: u32, len: usize) -> Vec<u8> {
        (0..len as u32).map(|i| self.bytes.get(&(address + i)).copied().unwrap_or(0xEE)).collect()
    }

    fn no_boot_info(&self) -> bool {
        (0..BOOT_INFO_LEN as u32).all(|i| !self.bytes.contains_key(&(BOOT_INFO_ADDRESS + i)))
    }
}

fn signer() -> SigningKey {
    SigningKey::from_bytes(&[11; 32])
}

fn image(generation: u32, fill: u8) -> Vec<u8> {
    let code: Vec<u8> = (0..0x3000u32).map(|i| fill ^ (i as u8)).collect();
    let data = vec![fill; 0x800];
    let mut payload = code.clone();
    payload.extend_from_slice(&data);
    let key = signer();
    let mut header = ImageHeader {
        flags: FLAG_SAFE_MODE_CAPABLE,
        version: [1, 2, fill],
        generation,
        payload_size: payload.len() as u32,
        entry: 0x40,
        segments: [
            Some(Segment { kind: SegmentKind::Copy, destination: 0, offset: 0, size: code.len() as u32 }),
            Some(Segment { kind: SegmentKind::Copy, destination: 0x0010_4000, offset: code.len() as u32, size: 0x800 }),
            Some(Segment { kind: SegmentKind::Zero, destination: 0x0010_8000, offset: 0, size: 0x100 }),
            None,
        ],
        key_id: key_id(&key.verifying_key()),
        digest: [0; 32],
        signature: [0; 64],
    };
    let mut signed = [0u8; SIGNED_HEADER_LEN];
    signed.copy_from_slice(&header.encode()[..SIGNED_HEADER_LEN]);
    header.digest = image_digest(&signed, &payload);
    header.signature = key.sign(&header.digest).to_bytes();
    let mut out = header.encode().to_vec();
    out.extend_from_slice(&payload);
    out
}

fn put(flash: &mut Flash, slot: SlotId, bytes: &[u8]) {
    let at = slot.region().offset as usize;
    flash.data[at..at + bytes.len()].copy_from_slice(bytes);
}

struct Case {
    flash: Option<Flash>,
    diag: DiagAreas,
}

impl Case {
    fn new(flash: Flash) -> Self {
        Self { flash: Some(flash), diag: DiagAreas { current: [0xA5; AREA_SIZE], last_crash: [0xA5; AREA_SIZE] } }
    }

    fn with_slot(slot: SlotId, generation: u32) -> Self {
        let mut flash = Flash::new();
        put(&mut flash, slot, &image(generation, generation as u8));
        Self::new(flash)
    }

    fn flash(&mut self) -> &mut Flash {
        self.flash.as_mut().unwrap()
    }

    fn run(&mut self, dry_run: bool) -> (Outcome, Report, Ram) {
        let key = signer().verifying_key();
        let mut ram = Ram::default();
        let stage0 = Stage0 { key: Some(&key), dry_run };
        let (outcome, report, flash) = stage0.run(self.flash.take().unwrap(), &mut ram, &mut self.diag);
        self.flash = Some(flash);
        (outcome, report, ram)
    }

    fn boot(&mut self) -> (Outcome, Report, Ram) {
        self.run(false)
    }

    fn crash(&mut self, message: &str) {
        let registers = Registers { mcause: 7, mepc: 0x1234, ..Registers::default() };
        Diag::attach(&mut self.diag.current).expect("stage0 left a record").crash(Reason::Panic, registers, 50, message);
    }

    fn power_cycle(&mut self) {
        self.diag = DiagAreas { current: [0x3C; AREA_SIZE], last_crash: [0x3C; AREA_SIZE] };
    }

    fn journal(&mut self) -> BootState {
        let mut journal = Journal::new(self.flash.take().unwrap(), BOOT_JOURNAL).unwrap();
        let state = journal.load().unwrap().unwrap();
        self.flash = Some(journal.into_inner());
        state
    }

    fn store(&mut self, state: BootState) {
        let mut journal = Journal::new(self.flash.take().unwrap(), BOOT_JOURNAL).unwrap();
        journal.store(&state).unwrap();
        self.flash = Some(journal.into_inner());
    }

    fn confirm(&mut self, slot: SlotId) {
        let state = self.journal();
        self.store(confirm(&state, slot).unwrap());
    }
}

fn jumped(outcome: Outcome) -> BootInfo {
    match outcome {
        Outcome::Jump { info, .. } => info,
        other => panic!("expected a jump, got {other:?}"),
    }
}

#[test]
fn fresh_install_boots_slot_a_as_a_trial_and_loads_its_segments() {
    let mut flash = Flash::new();
    let a = image(1, 0x5A);
    put(&mut flash, SlotId::A, &a);
    let mut case = Case::new(flash);
    let (outcome, report, ram) = case.boot();
    let Outcome::Jump { entry, info } = outcome else { panic!("{outcome:?}") };
    assert_eq!(entry, 0x40);
    assert_eq!(info.slot, SlotId::A);
    assert!(!info.safe_mode);
    assert!(!report.journal_found);
    assert!(report.stored);
    assert_eq!(report.after.slot(SlotId::A).status, SlotStatus::Trial);
    assert_eq!(report.after.slot(SlotId::A).attempts, policy::TRIAL_ATTEMPTS - 1);
    assert_eq!(ram.read(0, 0x3000), a[HEADER_SIZE..HEADER_SIZE + 0x3000]);
    assert_eq!(ram.read(0x0010_4000, 0x800), vec![0x5A; 0x800]);
    assert_eq!(ram.read(0x0010_8000, 0x100), vec![0; 0x100]);
    let mut raw = [0u8; BOOT_INFO_LEN];
    raw.copy_from_slice(&ram.read(BOOT_INFO_ADDRESS, BOOT_INFO_LEN));
    assert_eq!(BootInfo::decode(&raw), Some(info));
    assert_eq!(case.journal(), BootState { sequence: 1, ..report.after });
    let record = kumi_diag::read(&case.diag.current).unwrap();
    assert_eq!(record.boot_count, info.boot_count);
    assert_eq!(
        record.trail(),
        [stage::RESET, stage::STAGE0_JOURNAL, stage::STAGE0_VERIFIED, stage::STAGE0_LOADED, stage::STAGE0_JUMP]
    );
    assert_eq!(case.diag.last_crash, [0xA5; AREA_SIZE]);
}

#[test]
fn a_crashing_update_falls_back_to_the_confirmed_slot_and_keeps_the_crash_record() {
    let mut case = Case::with_slot(SlotId::A, 1);
    jumped(case.boot().0);
    case.confirm(SlotId::A);
    let (target, state) = begin_update(&case.journal(), SlotId::A);
    put(case.flash(), target, &image(2, 2));
    case.store(finish_update(&state, target, 2));
    for attempt in 0..policy::TRIAL_ATTEMPTS {
        let info = jumped(case.boot().0);
        assert_eq!((info.slot, info.safe_mode), (SlotId::B, false), "attempt {attempt}");
        case.crash("new image crashed");
    }
    let (outcome, report, _) = case.boot();
    let info = jumped(outcome);
    assert_eq!((info.slot, info.safe_mode), (SlotId::A, false));
    assert!(report.previous_boot_crashed);
    assert_eq!(report.after.slot(SlotId::B).status, SlotStatus::Invalid);
    assert_eq!(report.after.unhealthy_starts, 1);
    let last = kumi_diag::read(&case.diag.last_crash).unwrap();
    assert_eq!(last.reason, Reason::Panic);
    assert_eq!(last.message(), "new image crashed");
    assert_eq!(last.registers.mepc, 0x1234);
    jumped(case.boot().0);
    assert_eq!(kumi_diag::read(&case.diag.last_crash), Some(last));
}

#[test]
fn a_corrupted_slot_is_skipped_and_empty_flash_goes_to_the_bootrom() {
    let mut flash = Flash::new();
    put(&mut flash, SlotId::A, &image(1, 1));
    let mut b = image(2, 2);
    b[HEADER_SIZE + 100] ^= 1;
    put(&mut flash, SlotId::B, &b);
    let mut case = Case::new(flash);
    let (outcome, report, _) = case.boot();
    assert_eq!(jumped(outcome).slot, SlotId::A);
    assert_eq!(report.checks[1], SlotCheck::Invalid);
    let mut empty = Case::new(Flash::new());
    let (outcome, _, ram) = empty.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::NoBootableSlot));
    assert_eq!(ram.writes, 0);
    assert_eq!(kumi_diag::read(&empty.diag.current).unwrap().stage, stage::STAGE0_BOOTROM);
}

#[test]
fn a_confirmed_image_that_never_gets_healthy_gets_safe_mode_then_the_bootrom() {
    let mut case = Case::with_slot(SlotId::A, 1);
    jumped(case.boot().0);
    case.confirm(SlotId::A);
    let mut safe = Vec::new();
    for _ in 0..policy::BOOTROM_AFTER {
        safe.push(jumped(case.boot().0).safe_mode);
        case.crash("settings crash");
    }
    assert_eq!(safe, [false, false, true, true]);
    let (outcome, report, _) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::CrashStreak));
    assert!(report.marker_stored && !report.disarmed && !case.flash().disarmed());
    assert!(case.journal().bootrom_pending);
    let (outcome, report, _) = case.boot();
    assert!(jumped(outcome).safe_mode);
    assert!(report.pending_at_entry && !report.disarmed);
    assert!(!case.journal().bootrom_pending);
    assert!(!case.flash().disarmed());
}

#[test]
fn a_healthy_run_clears_the_count_so_isolated_crashes_never_give_safe_mode() {
    let mut case = Case::with_slot(SlotId::A, 1);
    jumped(case.boot().0);
    case.confirm(SlotId::A);
    for _ in 0..6 {
        assert!(!jumped(case.boot().0).safe_mode);
        let healthy = mark_healthy(&case.journal());
        case.store(healthy);
        case.crash("a crash long after start-up");
    }
}

#[test]
fn a_power_cycle_counts_as_an_unhealthy_start_but_not_as_a_crash() {
    let mut case = Case::with_slot(SlotId::A, 1);
    jumped(case.boot().0);
    case.confirm(SlotId::A);
    jumped(case.boot().0);
    case.crash("crash before power loss");
    case.power_cycle();
    let (outcome, report, _) = case.boot();
    assert!(!jumped(outcome).safe_mode);
    assert!(!report.previous_boot_crashed);
    assert_eq!(report.ram_crashes, 0);
    assert_eq!(report.after.unhealthy_starts, 2);
}

#[test]
fn a_bootrom_request_that_does_not_stick_disarms_stage0_on_the_next_start() {
    let mut case = Case::new(Flash::new());
    let (outcome, report, _) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::NoBootableSlot));
    assert!(report.marker_stored && !report.pending_at_entry && !report.disarmed);
    assert!(!case.flash().disarmed());
    let journal = case.flash().data[BOOT_JOURNAL.offset as usize..BOOT_JOURNAL.end() as usize].to_vec();
    let (outcome, report, ram) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::NoBootableSlot));
    assert!(report.pending_at_entry && report.disarmed && !report.marker_stored);
    assert!(case.flash().disarmed());
    assert_eq!(case.flash().data[BOOT_JOURNAL.offset as usize..BOOT_JOURNAL.end() as usize], journal[..]);
    assert_eq!(ram.writes, 0);
    let record = kumi_diag::read(&case.diag.current).unwrap();
    assert_eq!(record.trail(), [stage::RESET, stage::STAGE0_DISARMED, stage::STAGE0_BOOTROM]);
    let encoded = encode_report(&outcome, &report);
    assert_eq!(encoded[7], 1 | 4);
}

#[test]
fn a_bootrom_request_that_cannot_be_recorded_disarms_at_once() {
    let mut case = Case::with_slot(SlotId::A, 1);
    case.flash().journal_fails = true;
    let (outcome, report, ram) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::JournalUnwritable));
    assert!(!report.stored && !report.marker_stored && report.disarmed);
    assert!(case.flash().disarmed());
    assert!(ram.no_boot_info());
}

#[test]
fn a_stage0_without_a_valid_key_records_the_request_and_then_disarms() {
    let mut case = Case::with_slot(SlotId::A, 1);
    let run = |case: &mut Case| {
        let mut ram = Ram::default();
        let (outcome, report, flash) =
            Stage0 { key: None, dry_run: false }.run(case.flash.take().unwrap(), &mut ram, &mut case.diag);
        case.flash = Some(flash);
        (outcome, report)
    };
    let (outcome, report) = run(&mut case);
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::NoKey));
    assert!(report.marker_stored && !case.flash().disarmed());
    let (outcome, report) = run(&mut case);
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::NoKey));
    assert!(report.disarmed && case.flash().disarmed());
}

#[test]
fn the_image_is_verified_in_the_pass_that_loads_it_so_a_later_change_cannot_start() {
    let mut case = Case::with_slot(SlotId::A, 1);
    let original = Case::with_slot(SlotId::A, 1).boot().2;
    case.flash().change_slot_after_a_write = Some(SLOT_A.offset + HEADER_SIZE as u32 + 0x2000);
    let (outcome, report, ram) = case.boot();
    assert_eq!(jumped(outcome).slot, SlotId::A);
    assert!(report.stored);
    assert_eq!(ram.bytes, original.bytes);
    let (outcome, report, ram) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::NoBootableSlot));
    assert_eq!(report.checks[0], SlotCheck::Invalid);
    assert!(ram.no_boot_info());
}

#[test]
fn a_confirmed_slot_rewritten_without_a_journal_update_starts_as_a_trial() {
    let mut case = Case::with_slot(SlotId::A, 1);
    jumped(case.boot().0);
    case.confirm(SlotId::A);
    put(case.flash(), SlotId::A, &image(7, 7));
    let (outcome, report, _) = case.boot();
    let info = jumped(outcome);
    assert_eq!((info.slot, info.generation), (SlotId::A, 7));
    assert_eq!(
        *report.after.slot(SlotId::A),
        SlotRecord { status: SlotStatus::Trial, attempts: policy::TRIAL_ATTEMPTS - 1, generation: 7 }
    );
}

#[test]
fn dry_run_reports_the_decision_and_writes_nothing() {
    let mut case = Case::with_slot(SlotId::B, 3);
    let before = case.flash().data.clone();
    let (outcome, report, ram) = case.run(true);
    assert!(matches!(outcome, Outcome::WouldJump { info: BootInfo { slot: SlotId::B, .. }, .. }));
    assert!(case.flash().data == before);
    assert_eq!(case.flash().writes, 0);
    assert_eq!(ram.writes, 0);
    assert_eq!(case.diag.current, [0xA5; AREA_SIZE]);
    assert_eq!(case.diag.last_crash, [0xA5; AREA_SIZE]);
    let encoded = encode_report(&outcome, &report);
    assert_eq!(&encoded[..4], b"KS0R");
    assert_eq!(encoded[4], 2);
    assert_eq!(encoded[45], 1);
    assert_eq!(kumi_layout::crc32(&encoded[..60]).to_le_bytes(), encoded[60..64]);
}

#[test]
fn an_unwritable_journal_blocks_trials_but_not_confirmed_images() {
    let mut case = Case::with_slot(SlotId::A, 1);
    case.flash().writes_fail = true;
    let (outcome, report, ram) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::JournalUnwritable));
    assert!(!report.stored && !report.disarmed);
    assert!(ram.no_boot_info());
    case.flash().writes_fail = false;
    jumped(case.boot().0);
    case.confirm(SlotId::A);
    case.flash().writes_fail = true;
    let (outcome, report, _) = case.boot();
    assert_eq!(jumped(outcome).slot, SlotId::A);
    assert!(!report.stored);
}

#[test]
fn without_a_writable_journal_the_ram_crash_count_still_reaches_safe_mode_and_the_bootrom() {
    let mut case = Case::with_slot(SlotId::A, 1);
    jumped(case.boot().0);
    case.confirm(SlotId::A);
    case.flash().writes_fail = true;
    let mut safe = Vec::new();
    for _ in 0..policy::BOOTROM_AFTER {
        let (outcome, report, _) = case.boot();
        assert!(!report.stored);
        safe.push(jumped(outcome).safe_mode);
        case.crash("crash loop on a failing flash");
    }
    assert_eq!(safe, [false, false, true, true]);
    let (outcome, report, _) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::CrashStreak));
    assert_eq!(report.ram_crashes, policy::BOOTROM_AFTER);
    assert!(!report.marker_stored && !report.disarmed);
    assert!(!jumped(case.boot().0).safe_mode);
}

#[test]
fn a_flash_read_error_while_loading_never_jumps_into_a_partial_image() {
    let mut case = Case::with_slot(SlotId::A, 1);
    case.flash().slot_reads_fail_after = Some(20);
    let (outcome, report, ram) = case.boot();
    assert_eq!(outcome, Outcome::BootRom(BootRomReason::NoBootableSlot));
    assert_eq!(report.checks[0], SlotCheck::Invalid);
    assert!(report.marker_stored && ram.no_boot_info());
    assert_eq!(kumi_diag::read(&case.diag.current).unwrap().stage, stage::STAGE0_BOOTROM);
}

#[test]
fn every_bootrom_reason_has_its_own_report_kind() {
    let reasons = [
        (BootRomReason::NoBootableSlot, 10),
        (BootRomReason::CrashStreak, 11),
        (BootRomReason::JournalUnreadable, 12),
        (BootRomReason::JournalUnwritable, 13),
        (BootRomReason::LoadFailed, 14),
        (BootRomReason::FlashUnavailable, 15),
        (BootRomReason::NoKey, 16),
        (BootRomReason::FlashMismatch, 17),
    ];
    for (reason, kind) in reasons {
        let encoded = encode_report(&Outcome::BootRom(reason), &Report::EMPTY);
        assert_eq!(encoded[4], kind, "{reason:?}");
        assert_eq!(&encoded[8..44], &[0; 36]);
    }
}

const GOLDEN_REPORT: &str = "4b53305202000000400000004b424931010000000100000003000000010203000100000000000000e5f3478f00010002010000000000000000000000631c2854";

#[test]
fn the_report_layout_matches_the_documented_golden_bytes() {
    let info = BootInfo {
        slot: SlotId::B,
        safe_mode: false,
        boot_count: 1,
        generation: 3,
        image_version: [1, 2, 3],
        stage0_version: [0, 1, 0],
    };
    let mut after = BootState::FRESH;
    after.boot_count = 1;
    after.slots[1] = SlotRecord { status: SlotStatus::Trial, attempts: 2, generation: 3 };
    let report = Report { after, checks: [SlotCheck::NotChecked, SlotCheck::Valid { generation: 3 }], ..Report::EMPTY };
    let encoded = encode_report(&Outcome::WouldJump { entry: 0x40, info }, &report);
    let hex: String = encoded.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(hex, GOLDEN_REPORT);
}

#[test]
fn boot_info_rejects_damage() {
    let info = BootInfo {
        slot: SlotId::B,
        safe_mode: true,
        boot_count: 9,
        generation: 4,
        image_version: [1, 0, 0],
        stage0_version: STAGE0_VERSION,
    };
    let bytes = info.encode();
    assert_eq!(BootInfo::decode(&bytes), Some(info));
    for i in 0..BOOT_INFO_LEN {
        let mut bad = bytes;
        bad[i] ^= 0x20;
        assert_eq!(BootInfo::decode(&bad), None, "byte {i}");
    }
}

#[test]
fn a_boot_reads_the_image_once_and_checks_only_headers_of_a_fresh_journal() {
    let payload = image(1, 1).len() - HEADER_SIZE;
    let mut case = Case::with_slot(SlotId::A, 1);
    jumped(case.boot().0);
    assert!(case.flash().slot_bytes_read <= payload + 4 * HEADER_SIZE, "{}", case.flash().slot_bytes_read);
    case.confirm(SlotId::A);
    case.flash().slot_bytes_read = 0;
    jumped(case.boot().0);
    assert!(case.flash().slot_bytes_read <= payload + 2 * HEADER_SIZE, "{}", case.flash().slot_bytes_read);
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use kumi_diag::log::{CrashLog, ENTRIES_PER_SECTOR, LogError};
use kumi_diag::{AREA_SIZE, Diag, Reason, Registers};
use kumi_layout::{CRASH_LOG, Region, SECTOR};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cut;

impl NorFlashError for Cut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

struct Flash {
    data: Vec<u8>,
    budget: Option<usize>,
    erases: usize,
}

impl Flash {
    fn new() -> Self {
        Self { data: vec![0xFF; (CRASH_LOG.end()) as usize], budget: None, erases: 0 }
    }

    fn spend(&mut self) -> bool {
        match &mut self.budget {
            None => true,
            Some(0) => false,
            Some(n) => {
                *n -= 1;
                true
            }
        }
    }
}

impl ErrorType for Flash {
    type Error = Cut;
}

impl ReadNorFlash for Flash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        bytes.copy_from_slice(&self.data[offset as usize..offset as usize + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl NorFlash for Flash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Cut> {
        assert!(CRASH_LOG.contains(from, to - from));
        self.erases += 1;
        for i in from as usize..to as usize {
            if !self.spend() {
                return Err(Cut);
            }
            self.data[i] = 0xFF;
        }
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Cut> {
        assert!(CRASH_LOG.contains(offset, bytes.len() as u32));
        for (i, b) in bytes.iter().enumerate() {
            if !self.spend() {
                return Err(Cut);
            }
            self.data[offset as usize + i] &= *b;
        }
        Ok(())
    }
}

fn crash(boot: u32, message: &str) -> [u8; AREA_SIZE] {
    let mut area = [0u8; AREA_SIZE];
    Diag::begin(&mut area, boot, 0).crash(Reason::Trap, Registers { mcause: 2, ..Registers::default() }, 10, message);
    area
}

#[test]
fn a_crash_is_logged_once_and_read_back() {
    let mut log = CrashLog::new(Flash::new(), CRASH_LOG).unwrap();
    assert_eq!(log.latest().unwrap(), None);
    let first = crash(3, "first");
    assert_eq!(log.append(&first), Ok(true));
    assert_eq!(log.append(&first), Ok(false));
    let latest = log.latest().unwrap().unwrap();
    assert_eq!((latest.sequence, latest.index, latest.area), (1, 0, first));
    let mut clean = [0u8; AREA_SIZE];
    Diag::begin(&mut clean, 4, 0);
    assert_eq!(log.append(&clean), Ok(false));
    assert_eq!(log.append(&[0x5A; AREA_SIZE]), Ok(false));
    let second = crash(5, "second");
    assert_eq!(log.append(&second), Ok(true));
    assert_eq!(log.latest().unwrap().unwrap().area, second);
    assert_eq!(log.into_inner().erases, 1);
}

#[test]
fn the_log_wraps_by_erasing_the_oldest_sector() {
    let mut log = CrashLog::new(Flash::new(), CRASH_LOG).unwrap();
    let slots = CRASH_LOG.size / 512;
    for n in 0..slots + ENTRIES_PER_SECTOR + 3 {
        assert_eq!(log.append(&crash(n, "loop")), Ok(true));
        let latest = log.latest().unwrap().unwrap();
        assert_eq!(latest.sequence, n + 1);
        assert_eq!(latest.index, n % slots);
    }
    let flash = log.into_inner();
    assert_eq!(flash.erases as u32, CRASH_LOG.sectors() + 2);
}

#[test]
fn a_cut_append_keeps_the_previous_entries_and_the_next_append_succeeds() {
    let mut checked = 0;
    for start in [0, ENTRIES_PER_SECTOR - 1, 3] {
        for budget in (0..4400).step_by(97).chain([0, 1, 267, 268, 4095, 4096, 4097, 4363]) {
            let mut log = CrashLog::new(Flash::new(), CRASH_LOG).unwrap();
            for n in 0..start {
                log.append(&crash(n, "old")).unwrap();
            }
            let before = log.latest().unwrap();
            let mut flash = log.into_inner();
            flash.budget = Some(budget);
            let mut log = CrashLog::new(flash, CRASH_LOG).unwrap();
            let fresh = crash(900, "new");
            let finished = log.append(&fresh).is_ok();
            let mut flash = log.into_inner();
            flash.budget = None;
            let mut log = CrashLog::new(flash, CRASH_LOG).unwrap();
            let latest = log.latest().unwrap();
            if finished {
                assert_eq!(latest.unwrap().area, fresh);
            } else {
                assert!(latest == before || latest.is_some_and(|e| e.area == fresh), "start {start} budget {budget}");
            }
            let next = crash(901, "after");
            assert_eq!(log.append(&next), Ok(true));
            assert_eq!(log.latest().unwrap().unwrap().area, next);
            checked += 1;
        }
    }
    assert!(checked > 100);
}

#[test]
fn the_region_must_hold_whole_sectors() {
    let bad = Region { size: SECTOR, ..CRASH_LOG };
    assert!(matches!(CrashLog::new(Flash::new(), bad), Err(LogError::Region)));
    let unaligned = Region { offset: CRASH_LOG.offset + 512, ..CRASH_LOG };
    assert!(matches!(CrashLog::new(Flash::new(), unaligned), Err(LogError::Region)));
}

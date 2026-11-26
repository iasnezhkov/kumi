// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use kumi_chip::{Error, Identity, Rom, RomFlash, scratch};
use kumi_layout::*;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Fault {
    None,
    IgnoreWrites,
    ReadsErased,
    ReadsZero,
    ProgramsDropWindowBase,
    ProgramsAtPageOffset,
    ProgramsAtZero,
}

struct FakeRom {
    data: Vec<u8>,
    programs: Vec<(u32, usize)>,
    erases: Vec<u32>,
    fault: Fault,
}

impl FakeRom {
    fn new() -> Self {
        let mut data = vec![0xFF; FLASH_SIZE as usize];
        data[..0x30].iter_mut().enumerate().for_each(|(i, b)| *b = (i as u8).wrapping_mul(29) ^ 0x5A);
        Self { data, programs: Vec::new(), erases: Vec::new(), fault: Fault::None }
    }
}

impl Rom for FakeRom {
    fn read(&mut self, offset: u32, buffer: &mut [u8]) -> Result<(), Error> {
        match self.fault {
            Fault::ReadsErased => buffer.fill(0xFF),
            Fault::ReadsZero => buffer.fill(0),
            _ => buffer.copy_from_slice(&self.data[offset as usize..offset as usize + buffer.len()]),
        }
        Ok(())
    }

    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), Error> {
        assert_eq!(offset / PAGE, (offset + data.len() as u32 - 1) / PAGE, "a program call crosses a page");
        self.programs.push((offset, data.len()));
        let target = match self.fault {
            Fault::IgnoreWrites => return Ok(()),
            Fault::ProgramsDropWindowBase => offset & (BLOCK - 1),
            Fault::ProgramsAtPageOffset => offset % PAGE,
            Fault::ProgramsAtZero => 0,
            _ => offset,
        };
        for (i, b) in data.iter().enumerate() {
            self.data[target as usize + i] &= *b;
        }
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), Error> {
        assert!(offset.is_multiple_of(SECTOR));
        self.erases.push(offset);
        if self.fault != Fault::IgnoreWrites {
            self.data[offset as usize..(offset + SECTOR) as usize].fill(0xFF);
        }
        Ok(())
    }
}

fn firmware(active: SlotId) -> Option<Writer> {
    Some(Writer::Firmware { active })
}

#[test]
fn writes_are_split_at_page_boundaries_and_read_back() {
    let mut flash = RomFlash::new(FakeRom::new(), firmware(SlotId::A));
    let data: Vec<u8> = (0..700u32).map(|i| (i * 7) as u8).collect();
    let at = SLOT_B.offset + 200;
    flash.write(at, &data).unwrap();
    let mut back = vec![0u8; data.len()];
    flash.read(at, &mut back).unwrap();
    assert_eq!(back, data);
    let rom = flash.into_rom();
    assert_eq!(rom.programs, [(at, 56), (at + 56, 256), (at + 312, 256), (at + 568, 132)].map(|(a, n)| (a, n as usize)));
}

#[test]
fn the_write_policy_comes_from_the_layout() {
    let mut flash = RomFlash::new(FakeRom::new(), None);
    assert_eq!(flash.write(BOOT_JOURNAL.offset, &[0; 32]), Err(Error::ReadOnly));
    assert_eq!(flash.erase(BOOT_JOURNAL.offset, BOOT_JOURNAL.offset + SECTOR), Err(Error::ReadOnly));
    let mut flash = RomFlash::new(flash.into_rom(), firmware(SlotId::A));
    assert_eq!(flash.write(SLOT_A.offset, &[0; 4]), Err(Error::ReadOnly));
    assert_eq!(flash.erase(SLOT_A.offset, SLOT_A.offset + SECTOR), Err(Error::ReadOnly));
    assert_eq!(flash.write(STAGE0.offset + 4, &[0; 4]), Err(Error::ReadOnly));
    assert_eq!(flash.write(STOCK_COPY.offset, &[0; 4]), Err(Error::ReadOnly));
    flash.erase(SETTINGS.offset, SETTINGS.offset + 2 * SECTOR).unwrap();
    flash.write(CRASH_LOG.offset, &[1; 16]).unwrap();
    flash.write(BOOT_JOURNAL.offset, &[2; 32]).unwrap();
    let mut flash = RomFlash::new(flash.into_rom(), Some(Writer::Stage0));
    assert_eq!(flash.write(SETTINGS.offset, &[0; 4]), Err(Error::ReadOnly));
    assert_eq!(flash.write(0, &[0; 8]), Err(Error::ReadOnly));
    assert_eq!(flash.erase(0, SECTOR), Err(Error::ReadOnly));
    flash.write(SPV_MAGIC_AT, &[0; 4]).unwrap();
    let rom = flash.into_rom();
    assert_eq!(&rom.data[4..8], &[0; 4]);
    assert_eq!(rom.erases, [SETTINGS.offset, SETTINGS.offset + SECTOR]);
}

#[test]
fn a_write_or_erase_that_does_not_read_back_is_an_error() {
    let mut rom = FakeRom::new();
    rom.fault = Fault::IgnoreWrites;
    rom.data[BOOT_JOURNAL.offset as usize] = 0;
    let mut flash = RomFlash::new(rom, Some(Writer::Stage0));
    assert_eq!(flash.write(BOOT_JOURNAL.offset + 64, &[0x12; 32]), Err(Error::Mismatch));
    assert_eq!(flash.erase(BOOT_JOURNAL.offset, BOOT_JOURNAL.offset + SECTOR), Err(Error::Mismatch));
    assert_eq!(flash.write(BOOT_JOURNAL.offset + 64, &[0xFF; 32]), Ok(()));
}

#[test]
fn bounds_and_alignment_are_checked_before_anything_reaches_the_rom() {
    let mut flash = RomFlash::new(FakeRom::new(), firmware(SlotId::B));
    let mut buffer = [0u8; 8];
    assert_eq!(flash.read(FLASH_SIZE - 4, &mut buffer), Err(Error::OutOfBounds));
    assert_eq!(flash.read(u32::MAX, &mut buffer), Err(Error::OutOfBounds));
    assert_eq!(flash.erase(SETTINGS.offset + 1, SETTINGS.offset + SECTOR), Err(Error::NotAligned));
    assert_eq!(flash.erase(SETTINGS.offset + SECTOR, SETTINGS.offset), Err(Error::NotAligned));
    assert_eq!(flash.erase(SETTINGS.offset, SETTINGS.offset), Ok(()));
    assert_eq!(flash.write(FLASH_SIZE, &[]), Ok(()));
    let rom = flash.into_rom();
    assert!(rom.programs.is_empty() && rom.erases.is_empty());
}

#[test]
fn the_flash_identity_rejects_reads_that_are_not_real() {
    let rom = FakeRom::new();
    let mut expected = [0u8; SPV_IDENTITY_LEN];
    expected.copy_from_slice(&rom.data[..SPV_IDENTITY_LEN]);
    let identity = Identity::new(expected);
    let mut flash = RomFlash::new(rom, None);
    assert_eq!(flash.matches(0, identity.bytes()), Ok(true));
    for fault in [Fault::ReadsErased, Fault::ReadsZero] {
        let mut rom = flash.into_rom();
        rom.fault = fault;
        flash = RomFlash::new(rom, None);
        assert_eq!(flash.matches(0, identity.bytes()), Ok(false));
    }
    let mut changed = expected;
    changed[0x2C] ^= 1;
    let mut rom = flash.into_rom();
    rom.fault = Fault::None;
    let mut flash = RomFlash::new(rom, None);
    assert_eq!(flash.matches(0, Identity::new(changed).bytes()), Ok(false));
    assert_eq!(flash.matches(FLASH_SIZE - 4, &expected), Err(Error::OutOfBounds));
}

fn stock_like() -> FakeRom {
    let mut rom = FakeRom::new();
    for (i, byte) in rom.data[..BLOCK as usize].iter_mut().enumerate() {
        *byte = ((i as u32).wrapping_mul(2_654_435_761) >> 13) as u8;
    }
    rom
}

fn ticking(step: u32) -> impl FnMut() -> u32 {
    let mut now = u32::MAX - 3;
    move || {
        now = now.wrapping_add(step);
        now
    }
}

#[test]
fn block_erase_is_aligned_guarded_and_read_back() {
    let mut flash = RomFlash::new(FakeRom::new(), Some(Writer::Scratch));
    assert_eq!(flash.erase_block(SCRATCH.offset + SECTOR), Err(Error::NotAligned));
    assert_eq!(flash.erase_block(SETTINGS.offset), Err(Error::ReadOnly));
    assert_eq!(flash.erase_block(0), Err(Error::ReadOnly));
    assert_eq!(flash.erase_block(FLASH_SIZE), Err(Error::OutOfBounds));
    assert_eq!(flash.write(SCRATCH.offset - 4, &[0; 4]), Err(Error::ReadOnly));
    assert_eq!(flash.erase(SETTINGS.offset, SETTINGS.offset + SECTOR), Err(Error::ReadOnly));
    flash.write(SCRATCH.offset + 0x100, &[0; 4]).unwrap();
    assert_eq!(flash.erased(SCRATCH.offset, BLOCK), Ok(false));
    flash.erase_block(SCRATCH.offset).unwrap();
    assert_eq!(flash.erased(SCRATCH.offset, BLOCK), Ok(true));
    let mut rom = flash.into_rom();
    assert_eq!(rom.erases, (SCRATCH.offset..SCRATCH.end()).step_by(SECTOR as usize).collect::<Vec<_>>());
    rom.fault = Fault::IgnoreWrites;
    rom.data[SCRATCH.offset as usize] = 0;
    let mut flash = RomFlash::new(rom, Some(Writer::Scratch));
    assert_eq!(flash.erase_block(SCRATCH.offset), Err(Error::Mismatch));
    assert_eq!(flash.erased(FLASH_SIZE - 4, 8), Err(Error::OutOfBounds));
}

#[test]
fn the_scratch_test_writes_only_the_window_and_leaves_it_erased() {
    let mut rom = stock_like();
    let before = rom.data.clone();
    let report = scratch::run(&mut rom, ticking(7)).unwrap();
    assert_eq!(
        report,
        scratch::Report {
            window_was_erased: true,
            page_matches: true,
            unaligned_matches: true,
            sector_erased: true,
            block_erased: true,
            page_program_cycles: 7,
            sector_erase_cycles: 7,
            block_erase_cycles: 7,
            head_crc_before: crc32(&before[..SECTOR as usize]),
            head_crc_after: crc32(&before[..SECTOR as usize]),
        }
    );
    assert!(rom.data == before);
    assert_eq!(rom.programs, [(scratch::PAGE_AT, 256), (scratch::UNALIGNED_AT, 4), (scratch::TAIL_AT, 256)]);
    assert_eq!(scratch::UNALIGNED_AT, 0x3F_012C);
    assert_eq!(scratch::TAIL_AT, 0x3F_F000);
    assert_eq!(rom.erases[0], SCRATCH.offset);
    assert_eq!(rom.erases.len(), 1 + (BLOCK / SECTOR) as usize);
    for at in rom.erases.iter().chain(rom.programs.iter().map(|(at, _)| at)) {
        assert!(SCRATCH.contains(*at, 1), "{at:#x}");
    }
}

#[test]
fn patterns_are_the_or_of_every_landing_offset() {
    let mut flash = RomFlash::new(stock_like(), None);
    let mut four = [0u8; 4];
    assert!(scratch::no_op_pattern(&mut flash, 0x3F_012C, &mut four).unwrap());
    assert_eq!(scratch::landing_offsets(0x3F_012C), [0x12C, 0x2C, 0]);
    let data = flash.into_rom().data;
    for (i, value) in four.iter().enumerate() {
        assert_eq!(*value, data[0x12C + i] | data[0x2C + i] | data[i]);
    }
    let mut flash = RomFlash::new(FakeRom::new(), None);
    let mut page = [0u8; 256];
    assert!(!scratch::no_op_pattern(&mut flash, scratch::TAIL_AT + 0x100, &mut page).unwrap());
    assert_eq!(scratch::no_op_pattern(&mut flash, 0, &mut [0u8; 257]), Err(Error::OutOfBounds));
}

#[test]
fn a_misplaced_program_changes_no_flash_byte() {
    for fault in [Fault::ProgramsDropWindowBase, Fault::ProgramsAtPageOffset, Fault::ProgramsAtZero] {
        let mut rom = stock_like();
        rom.fault = fault;
        let before = rom.data.clone();
        let report = scratch::run(&mut rom, ticking(1)).unwrap();
        assert!(report.window_was_erased && report.sector_erased);
        assert!(!report.page_matches && !report.unaligned_matches && !report.block_erased);
        assert_eq!(report.head_crc_after, report.head_crc_before);
        assert!(rom.data == before);
    }
}

#[test]
fn a_window_that_is_not_erased_is_reported_and_never_written() {
    let mut rom = stock_like();
    rom.data[(SCRATCH.end() - 1) as usize] = 0x7F;
    let report = scratch::run(&mut rom, ticking(1)).unwrap();
    assert_eq!(
        report,
        scratch::Report {
            head_crc_before: report.head_crc_before,
            head_crc_after: report.head_crc_before,
            ..scratch::Report::default()
        }
    );
    assert!(rom.programs.is_empty() && rom.erases.is_empty());
    let mut rom = stock_like();
    rom.fault = Fault::IgnoreWrites;
    let report = scratch::run(&mut rom, ticking(1)).unwrap();
    assert!(report.window_was_erased && !report.page_matches && !report.unaligned_matches);
    assert!(report.sector_erased && !report.block_erased);
    rom.fault = Fault::ReadsZero;
    assert_eq!(scratch::run(&mut rom, ticking(1)).map(|report| report.window_was_erased), Ok(false));
}

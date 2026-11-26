// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::{NorFlash, ReadNorFlash};
use kumi_layout::{BLOCK, PAGE, SCRATCH, SECTOR, Writer, crc32_update};

use crate::flash::{Error, Rom, RomFlash, verified};

pub const PAGE_AT: u32 = SCRATCH.offset;
pub const UNALIGNED_AT: u32 = SCRATCH.offset + PAGE + 0x2C;
pub const UNALIGNED_LEN: usize = 4;
pub const TAIL_AT: u32 = SCRATCH.end() - SECTOR;
pub const HEAD_LEN: u32 = SECTOR;
pub const JEDEC: [u8; 3] = [0xC8, 0x65, 0x16];
const CHUNK: usize = 64;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Report {
    pub window_was_erased: bool,
    pub page_matches: bool,
    pub unaligned_matches: bool,
    pub sector_erased: bool,
    pub block_erased: bool,
    pub page_program_cycles: u32,
    pub sector_erase_cycles: u32,
    pub block_erase_cycles: u32,
    pub head_crc_before: u32,
    pub head_crc_after: u32,
}

pub const fn landing_offsets(offset: u32) -> [u32; 3] {
    [offset & (BLOCK - 1), offset % PAGE, 0]
}

pub fn no_op_pattern<R: Rom>(flash: &mut RomFlash<R>, offset: u32, out: &mut [u8]) -> Result<bool, Error> {
    let mut buffer = [0u8; PAGE as usize];
    let found = buffer.get_mut(..out.len()).ok_or(Error::OutOfBounds)?;
    out.fill(0);
    for at in landing_offsets(offset) {
        flash.read(at, found)?;
        out.iter_mut().zip(found.iter()).for_each(|(value, byte)| *value |= *byte);
    }
    Ok(out.iter().any(|byte| *byte != 0xFF))
}

pub fn crc<R: Rom>(flash: &mut RomFlash<R>, offset: u32, len: u32) -> Result<u32, Error> {
    let mut crc = 0xFFFF_FFFF;
    let mut buffer = [0u8; CHUNK];
    let mut done = 0;
    while done < len {
        let part = &mut buffer[..(len - done).min(CHUNK as u32) as usize];
        flash.read(offset + done, part)?;
        crc = crc32_update(crc, part);
        done += part.len() as u32;
    }
    Ok(crc ^ 0xFFFF_FFFF)
}

struct Timed<R, C> {
    rom: R,
    clock: C,
    cycles: u32,
}

impl<R: Rom, C: FnMut() -> u32> Timed<R, C> {
    fn time(&mut self, operation: impl FnOnce(&mut R) -> Result<(), Error>) -> Result<(), Error> {
        let start = (self.clock)();
        let result = operation(&mut self.rom);
        self.cycles = (self.clock)().wrapping_sub(start);
        result
    }
}

impl<R: Rom, C: FnMut() -> u32> Rom for Timed<R, C> {
    fn read(&mut self, offset: u32, buffer: &mut [u8]) -> Result<(), Error> {
        self.rom.read(offset, buffer)
    }

    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), Error> {
        self.time(|rom| rom.program(offset, data))
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), Error> {
        self.time(|rom| rom.erase_sector(offset))
    }

    fn erase_block(&mut self, offset: u32) -> Result<(), Error> {
        self.time(|rom| rom.erase_block(offset))
    }
}

fn program_visible<R: Rom>(flash: &mut RomFlash<R>, offset: u32, data: &mut [u8]) -> Result<bool, Error> {
    let visible = no_op_pattern(flash, offset, data)?;
    Ok(verified(flash.write(offset, data))? && visible)
}

pub fn run<R: Rom, C: FnMut() -> u32>(rom: R, clock: C) -> Result<Report, Error> {
    let mut flash = RomFlash::new(Timed { rom, clock, cycles: 0 }, Some(Writer::Scratch));
    let mut report = Report { head_crc_before: crc(&mut flash, 0, HEAD_LEN)?, ..Report::default() };
    report.window_was_erased = flash.erased(SCRATCH.offset, SCRATCH.size)?;
    if report.window_was_erased {
        let mut page = [0u8; PAGE as usize];
        let programmed = program_visible(&mut flash, PAGE_AT, &mut page)?;
        report.page_program_cycles = flash.rom().cycles;
        report.page_matches = programmed && flash.matches(PAGE_AT, &page)?;
        let mut four = [0u8; UNALIGNED_LEN];
        let programmed = program_visible(&mut flash, UNALIGNED_AT, &mut four)?;
        let page_start = UNALIGNED_AT - UNALIGNED_AT % PAGE;
        let at = (UNALIGNED_AT - page_start) as usize;
        let mut expected = [0xFF; PAGE as usize];
        expected[at..at + UNALIGNED_LEN].copy_from_slice(&four);
        report.unaligned_matches = programmed && flash.matches(page_start, &expected)?;
        report.sector_erased = verified(flash.erase(SCRATCH.offset, SCRATCH.offset + SECTOR))?;
        report.sector_erase_cycles = flash.rom().cycles;
        let tail = program_visible(&mut flash, TAIL_AT, &mut page)?;
        report.block_erased = verified(flash.erase_block(SCRATCH.offset))? && tail;
        report.block_erase_cycles = flash.rom().cycles;
    }
    report.head_crc_after = crc(&mut flash, 0, HEAD_LEN)?;
    Ok(report)
}

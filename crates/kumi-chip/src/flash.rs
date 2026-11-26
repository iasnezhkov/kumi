// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use kumi_layout::{BLOCK, FLASH_SIZE, PAGE, SECTOR, WriteGuard, Writer, region_for_write};

const CHECK_CHUNK: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    OutOfBounds,
    NotAligned,
    ReadOnly,
    Mismatch,
    Unavailable,
}

impl NorFlashError for Error {
    fn kind(&self) -> NorFlashErrorKind {
        match self {
            Error::OutOfBounds | Error::ReadOnly => NorFlashErrorKind::OutOfBounds,
            Error::NotAligned => NorFlashErrorKind::NotAligned,
            Error::Mismatch | Error::Unavailable => NorFlashErrorKind::Other,
        }
    }
}

pub trait Rom {
    fn read(&mut self, offset: u32, buffer: &mut [u8]) -> Result<(), Error>;
    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), Error>;
    fn erase_sector(&mut self, offset: u32) -> Result<(), Error>;

    fn erase_block(&mut self, offset: u32) -> Result<(), Error> {
        for at in (offset..offset + BLOCK).step_by(SECTOR as usize) {
            self.erase_sector(at)?;
        }
        Ok(())
    }
}

impl<R: Rom> Rom for &mut R {
    fn read(&mut self, offset: u32, buffer: &mut [u8]) -> Result<(), Error> {
        R::read(self, offset, buffer)
    }

    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), Error> {
        R::program(self, offset, data)
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), Error> {
        R::erase_sector(self, offset)
    }

    fn erase_block(&mut self, offset: u32) -> Result<(), Error> {
        R::erase_block(self, offset)
    }
}

pub struct RomFlash<R> {
    rom: R,
    writer: Option<Writer>,
}

fn in_flash(offset: u32, len: usize) -> Result<(), Error> {
    let len = u32::try_from(len).map_err(|_| Error::OutOfBounds)?;
    match offset.checked_add(len) {
        Some(end) if end <= FLASH_SIZE => Ok(()),
        _ => Err(Error::OutOfBounds),
    }
}

impl<R: Rom> RomFlash<R> {
    pub const fn new(rom: R, writer: Option<Writer>) -> Self {
        Self { rom, writer }
    }

    pub fn writer(&self) -> Option<Writer> {
        self.writer
    }

    pub fn rom(&mut self) -> &mut R {
        &mut self.rom
    }

    pub fn into_rom(self) -> R {
        self.rom
    }

    fn guard(&self, offset: u32, len: u32) -> Result<(), Error> {
        let writer = self.writer.ok_or(Error::ReadOnly)?;
        region_for_write(offset, len, writer).map(|_| ()).ok_or(Error::ReadOnly)
    }

    #[inline(never)]
    fn check(&mut self, offset: u32, expected: Option<&[u8]>, len: usize) -> Result<(), Error> {
        let mut buffer = [0u8; CHECK_CHUNK];
        let mut done = 0;
        while done < len {
            let n = CHECK_CHUNK.min(len - done);
            self.rom.read(offset + done as u32, &mut buffer[..n])?;
            if buffer[..n].iter().enumerate().any(|(i, b)| *b != expected.map_or(0xFF, |bytes| bytes[done + i])) {
                return Err(Error::Mismatch);
            }
            done += n;
        }
        Ok(())
    }

    pub fn matches(&mut self, offset: u32, expected: &[u8]) -> Result<bool, Error> {
        in_flash(offset, expected.len())?;
        verified(self.check(offset, Some(expected), expected.len()))
    }

    pub fn erased(&mut self, offset: u32, len: u32) -> Result<bool, Error> {
        in_flash(offset, len as usize)?;
        verified(self.check(offset, None, len as usize))
    }

    pub fn erase_block(&mut self, offset: u32) -> Result<(), Error> {
        if !offset.is_multiple_of(BLOCK) {
            return Err(Error::NotAligned);
        }
        in_flash(offset, BLOCK as usize)?;
        self.guard(offset, BLOCK)?;
        self.rom.erase_block(offset)?;
        self.check(offset, None, BLOCK as usize)
    }
}

pub fn verified(result: Result<(), Error>) -> Result<bool, Error> {
    match result {
        Ok(()) => Ok(true),
        Err(Error::Mismatch) => Ok(false),
        Err(error) => Err(error),
    }
}

impl<R> WriteGuard for RomFlash<R> {
    fn set_writer(&mut self, writer: Option<Writer>) {
        self.writer = writer;
    }
}

impl<R: Rom> ErrorType for RomFlash<R> {
    type Error = Error;
}

impl<R: Rom> ReadNorFlash for RomFlash<R> {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Error> {
        in_flash(offset, bytes.len())?;
        if bytes.is_empty() {
            return Ok(());
        }
        self.rom.read(offset, bytes)
    }

    fn capacity(&self) -> usize {
        FLASH_SIZE as usize
    }
}

impl<R: Rom> NorFlash for RomFlash<R> {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Error> {
        if from > to || !from.is_multiple_of(SECTOR) || !to.is_multiple_of(SECTOR) {
            return Err(Error::NotAligned);
        }
        in_flash(from, (to - from) as usize)?;
        if from == to {
            return Ok(());
        }
        self.guard(from, to - from)?;
        for at in (from..to).step_by(SECTOR as usize) {
            self.rom.erase_sector(at)?;
            self.check(at, None, SECTOR as usize)?;
        }
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Error> {
        in_flash(offset, bytes.len())?;
        if bytes.is_empty() {
            return Ok(());
        }
        self.guard(offset, bytes.len() as u32)?;
        let mut done = 0;
        while done < bytes.len() {
            let at = offset + done as u32;
            let count = ((PAGE - at % PAGE) as usize).min(bytes.len() - done);
            let page = &bytes[done..done + count];
            self.rom.program(at, page)?;
            self.check(at, Some(page), count)?;
            done += count;
        }
        Ok(())
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use core::ptr::read_volatile;

use kumi_layout::{BLOCK, FLASH_XIP_BASE, SECTOR, SPV_IDENTITY_LEN};

use crate::flash::{Error, Rom};
use crate::{norc, regs, rom};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Prepared,
    Session,
    Unavailable,
}

pub struct BootRom {
    state: State,
}

fn settle() {
    for _ in 0..regs::SETTLE_LOOPS {
        unsafe { read_volatile(regs::NORC_STATUS as *const u32) };
    }
}

impl BootRom {
    pub const unsafe fn prepared() -> Self {
        Self { state: State::Prepared }
    }

    pub const fn unavailable() -> Self {
        Self { state: State::Unavailable }
    }

    pub unsafe fn begin() -> Self {
        if !unsafe { norc::pause() } {
            return Self::unavailable();
        }
        unsafe {
            regs::write(regs::SPI_CLOCK, 1);
            regs::update(regs::PERIPHERAL_ENABLE, 0, regs::SPI_ENABLE);
            settle();
            rom::sf_init();
        }
        Self { state: State::Session }
    }

    pub fn available(&self) -> bool {
        self.state != State::Unavailable
    }

    pub fn end(self) {
        if self.state == State::Session {
            unsafe {
                rom::sf_deinit();
                regs::update(regs::NORC_CONTROL, regs::NORC_PAUSE, 0);
            }
        }
    }

    pub fn jedec(&mut self) -> Option<[u8; 3]> {
        if self.state != State::Session {
            return None;
        }
        let id = unsafe { rom::sf_jedec() }.to_be_bytes();
        Some([id[1], id[2], id[3]])
    }
}

impl Rom for BootRom {
    fn read(&mut self, offset: u32, buffer: &mut [u8]) -> Result<(), Error> {
        if self.state == State::Unavailable {
            return Err(Error::Unavailable);
        }
        unsafe { rom::sf_read(offset, buffer) };
        Ok(())
    }

    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), Error> {
        if self.state != State::Session {
            return Err(Error::ReadOnly);
        }
        unsafe { rom::sf_write(offset, data) };
        Ok(())
    }

    fn erase_sector(&mut self, offset: u32) -> Result<(), Error> {
        if self.state != State::Session {
            return Err(Error::ReadOnly);
        }
        unsafe { rom::sf_erase(offset, SECTOR) };
        Ok(())
    }

    fn erase_block(&mut self, offset: u32) -> Result<(), Error> {
        if self.state != State::Session {
            return Err(Error::ReadOnly);
        }
        unsafe { rom::sf_erase(offset, BLOCK) };
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Identity([u8; SPV_IDENTITY_LEN]);

impl Identity {
    pub const fn new(bytes: [u8; SPV_IDENTITY_LEN]) -> Self {
        Self(bytes)
    }

    pub unsafe fn from_xip() -> Self {
        let mut bytes = [0u8; SPV_IDENTITY_LEN];
        let (words, _) = bytes.as_chunks_mut::<4>();
        for (i, word) in words.iter_mut().enumerate() {
            *word = unsafe { read_volatile((FLASH_XIP_BASE as usize + 4 * i) as *const u32) }.to_le_bytes();
        }
        Self(bytes)
    }

    pub fn bytes(&self) -> &[u8; SPV_IDENTITY_LEN] {
        &self.0
    }
}

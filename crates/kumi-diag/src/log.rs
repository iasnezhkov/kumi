// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::NorFlash;
use kumi_layout::{Region, SECTOR, crc32, get32};

use crate::AREA_SIZE;

pub const ENTRY_SIZE: u32 = 512;
pub const ENTRIES_PER_SECTOR: u32 = SECTOR / ENTRY_SIZE;
const MAGIC: [u8; 4] = *b"KCL1";
const AREA_AT: usize = 8;
const CRC_AT: usize = AREA_AT + AREA_SIZE;
const ENTRY_LEN: usize = CRC_AT + 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogError<E> {
    Region,
    Flash(E),
    Readback,
}

impl<E> From<E> for LogError<E> {
    fn from(error: E) -> Self {
        LogError::Flash(error)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub sequence: u32,
    pub index: u32,
    pub area: [u8; AREA_SIZE],
}

enum Slot {
    Erased,
    Valid(u32),
    Garbage,
}

pub struct CrashLog<F> {
    flash: F,
    region: Region,
}

impl<F: NorFlash> CrashLog<F> {
    pub fn new(flash: F, region: Region) -> Result<Self, LogError<F::Error>> {
        if F::ERASE_SIZE as u32 != SECTOR
            || !region.offset.is_multiple_of(SECTOR)
            || !region.size.is_multiple_of(SECTOR)
            || region.size < 2 * SECTOR
        {
            return Err(LogError::Region);
        }
        Ok(Self { flash, region })
    }

    pub fn into_inner(self) -> F {
        self.flash
    }

    fn slots(&self) -> u32 {
        self.region.size / ENTRY_SIZE
    }

    fn address(&self, index: u32) -> u32 {
        self.region.offset + index * ENTRY_SIZE
    }

    fn slot(&mut self, index: u32, area: &mut [u8; AREA_SIZE]) -> Result<Slot, F::Error> {
        let mut raw = [0u8; ENTRY_LEN];
        self.flash.read(self.address(index), &mut raw)?;
        if raw.iter().all(|b| *b == 0xFF) {
            return Ok(Slot::Erased);
        }
        if raw[..4] != MAGIC || crc32(&raw[..CRC_AT]) != get32(&raw, CRC_AT) {
            return Ok(Slot::Garbage);
        }
        area.copy_from_slice(&raw[AREA_AT..CRC_AT]);
        Ok(Slot::Valid(get32(&raw, 4)))
    }

    pub fn latest(&mut self) -> Result<Option<Entry>, F::Error> {
        let mut best: Option<Entry> = None;
        let mut hdr = [0u8; 8];
        let mut area = [0u8; AREA_SIZE];
        for index in 0..self.slots() {
            self.flash.read(self.address(index), &mut hdr)?;
            if hdr[..4] != MAGIC {
                continue;
            }
            let sequence = get32(&hdr, 4);
            if best.is_some_and(|b| sequence <= b.sequence) {
                continue;
            }
            if let Slot::Valid(seq) = self.slot(index, &mut area)?
                && best.is_none_or(|b| seq > b.sequence)
            {
                best = Some(Entry { sequence: seq, index, area });
            }
        }
        Ok(best)
    }

    pub fn append(&mut self, area: &[u8; AREA_SIZE]) -> Result<bool, LogError<F::Error>> {
        if !crate::read(area).is_some_and(|report| report.crashed()) {
            return Ok(false);
        }
        let latest = self.latest()?;
        if latest.is_some_and(|entry| entry.area == *area) {
            return Ok(false);
        }
        let slots = self.slots();
        let sequence = latest.map_or(1, |entry| entry.sequence.wrapping_add(1));
        let mut index = latest.map_or(0, |entry| (entry.index + 1) % slots);
        for _ in 0..slots {
            if index.is_multiple_of(ENTRIES_PER_SECTOR) {
                let start = self.address(index);
                self.flash.erase(start, start + SECTOR)?;
                break;
            }
            let mut scratch = [0u8; AREA_SIZE];
            if matches!(self.slot(index, &mut scratch)?, Slot::Erased) {
                break;
            }
            index = (index + 1) % slots;
        }
        let mut raw = [0u8; ENTRY_LEN];
        raw[..4].copy_from_slice(&MAGIC);
        raw[4..8].copy_from_slice(&sequence.to_le_bytes());
        raw[AREA_AT..CRC_AT].copy_from_slice(area);
        let crc = crc32(&raw[..CRC_AT]);
        raw[CRC_AT..].copy_from_slice(&crc.to_le_bytes());
        let address = self.address(index);
        self.flash.write(address, &raw)?;
        let mut check = [0u8; ENTRY_LEN];
        self.flash.read(address, &mut check)?;
        if check != raw {
            return Err(LogError::Readback);
        }
        Ok(true)
    }
}

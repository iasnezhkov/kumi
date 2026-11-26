// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::NorFlash;
use kumi_layout::{Region, SECTOR, SlotId, crc32, get32};

pub const RECORD_SIZE: usize = 32;
pub const RECORDS_PER_SECTOR: u32 = SECTOR / RECORD_SIZE as u32;
const SCAN_CHUNK: usize = 512;
const MAGIC: [u8; 2] = *b"KJ";
const FORMAT: u8 = 1;
const FLAG_BOOTROM_PENDING: u8 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotStatus {
    Empty,
    Trial,
    Confirmed,
    Invalid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SlotRecord {
    pub status: SlotStatus,
    pub attempts: u8,
    pub generation: u32,
}

impl SlotRecord {
    pub const EMPTY: Self = Self { status: SlotStatus::Empty, attempts: 0, generation: 0 };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BootState {
    pub sequence: u32,
    pub boot_count: u32,
    pub preferred: SlotId,
    pub slots: [SlotRecord; 2],
    pub unhealthy_starts: u8,
    pub bootrom_pending: bool,
}

impl BootState {
    pub const FRESH: Self = Self {
        sequence: 0,
        boot_count: 0,
        preferred: SlotId::A,
        slots: [SlotRecord::EMPTY; 2],
        unhealthy_starts: 0,
        bootrom_pending: false,
    };

    pub fn slot(&self, slot: SlotId) -> &SlotRecord {
        &self.slots[slot.index()]
    }

    pub fn slot_mut(&mut self, slot: SlotId) -> &mut SlotRecord {
        &mut self.slots[slot.index()]
    }

    pub fn encode(&self) -> [u8; RECORD_SIZE] {
        let mut out = [0u8; RECORD_SIZE];
        out[0..2].copy_from_slice(&MAGIC);
        out[2] = FORMAT;
        out[3] = self.preferred.index() as u8;
        out[4..8].copy_from_slice(&self.sequence.to_le_bytes());
        out[8..12].copy_from_slice(&self.boot_count.to_le_bytes());
        out[12] = self.unhealthy_starts;
        out[13] = if self.bootrom_pending { FLAG_BOOTROM_PENDING } else { 0 };
        for (i, slot) in self.slots.iter().enumerate() {
            let at = 16 + 6 * i;
            out[at] = match slot.status {
                SlotStatus::Empty => 0,
                SlotStatus::Trial => 1,
                SlotStatus::Confirmed => 2,
                SlotStatus::Invalid => 3,
            };
            out[at + 1] = slot.attempts;
            out[at + 2..at + 6].copy_from_slice(&slot.generation.to_le_bytes());
        }
        let crc = crc32(&out[..28]);
        out[28..32].copy_from_slice(&crc.to_le_bytes());
        out
    }

    pub fn decode(bytes: &[u8; RECORD_SIZE]) -> Option<Self> {
        if bytes[0..2] != MAGIC || bytes[2] != FORMAT || crc32(&bytes[..28]) != get32(bytes, 28) {
            return None;
        }
        let mut slots = [SlotRecord::EMPTY; 2];
        for (i, slot) in slots.iter_mut().enumerate() {
            let at = 16 + 6 * i;
            slot.status = match bytes[at] {
                0 => SlotStatus::Empty,
                1 => SlotStatus::Trial,
                2 => SlotStatus::Confirmed,
                3 => SlotStatus::Invalid,
                _ => return None,
            };
            slot.attempts = bytes[at + 1];
            slot.generation = get32(bytes, at + 2);
        }
        Some(Self {
            sequence: get32(bytes, 4),
            boot_count: get32(bytes, 8),
            preferred: SlotId::from_index(usize::from(bytes[3]))?,
            slots,
            unhealthy_starts: bytes[12],
            bootrom_pending: bytes[13] & FLAG_BOOTROM_PENDING != 0,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalError<E> {
    Flash(E),
    Region,
    Readback,
}

impl<E> From<E> for JournalError<E> {
    fn from(error: E) -> Self {
        JournalError::Flash(error)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Scan {
    latest: Option<(BootState, usize)>,
    last_used: [Option<u32>; 2],
}

impl Scan {
    pub fn latest(&self) -> Option<BootState> {
        self.latest.map(|(state, _)| state)
    }
}

pub struct Journal<F> {
    flash: F,
    region: Region,
}

impl<F: NorFlash> Journal<F> {
    pub fn new(flash: F, region: Region) -> Result<Self, JournalError<F::Error>> {
        if region.size != 2 * SECTOR || F::ERASE_SIZE as u32 != SECTOR || !RECORD_SIZE.is_multiple_of(F::WRITE_SIZE) {
            return Err(JournalError::Region);
        }
        Ok(Self { flash, region })
    }

    pub fn into_inner(self) -> F {
        self.flash
    }

    fn address(&self, sector: usize, index: u32) -> u32 {
        self.region.offset + sector as u32 * SECTOR + index * RECORD_SIZE as u32
    }

    #[inline(never)]
    fn scan(&mut self) -> Result<Scan, F::Error> {
        let mut scan = Scan::default();
        let mut buf = [0u8; SCAN_CHUNK];
        for sector in 0..2 {
            let base = self.region.offset + sector as u32 * SECTOR;
            let mut offset = 0u32;
            while offset < SECTOR {
                self.flash.read(base + offset, &mut buf)?;
                let (records, _) = buf.as_chunks::<RECORD_SIZE>();
                for (i, record) in records.iter().enumerate() {
                    if record.iter().all(|b| *b == 0xFF) {
                        continue;
                    }
                    let index = offset / RECORD_SIZE as u32 + i as u32;
                    scan.last_used[sector] = Some(index);
                    if let Some(state) = BootState::decode(record)
                        && scan.latest.is_none_or(|(best, _)| state.sequence > best.sequence)
                    {
                        scan.latest = Some((state, sector));
                    }
                }
                offset += SCAN_CHUNK as u32;
            }
        }
        Ok(scan)
    }

    pub fn load_scan(&mut self) -> Result<Scan, F::Error> {
        self.scan()
    }

    pub fn load(&mut self) -> Result<Option<BootState>, F::Error> {
        Ok(self.scan()?.latest())
    }

    pub fn store(&mut self, state: &BootState) -> Result<BootState, JournalError<F::Error>> {
        let scan = self.scan()?;
        self.store_with_scan(scan, state)
    }

    #[inline(never)]
    pub fn store_with_scan(&mut self, scan: Scan, state: &BootState) -> Result<BootState, JournalError<F::Error>> {
        let mut record = *state;
        record.sequence = scan.latest.map_or(1, |(best, _)| best.sequence.wrapping_add(1));
        let current = scan.latest.map_or(0, |(_, sector)| sector);
        let next = scan.last_used[current].map_or(0, |i| i + 1);
        let (sector, index) = if next < RECORDS_PER_SECTOR && (scan.latest.is_some() || scan.last_used[current].is_none()) {
            (current, next)
        } else {
            let target = if scan.latest.is_some() { 1 - current } else { 0 };
            let start = self.region.offset + target as u32 * SECTOR;
            self.flash.erase(start, start + SECTOR)?;
            (target, 0)
        };
        let bytes = record.encode();
        let address = self.address(sector, index);
        self.flash.write(address, &bytes)?;
        let mut check = [0u8; RECORD_SIZE];
        self.flash.read(address, &mut check)?;
        if check != bytes {
            return Err(JournalError::Readback);
        }
        Ok(record)
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

pub const FLASH_SIZE: u32 = 4 * 1024 * 1024;
pub const SECTOR: u32 = 4096;
pub const BLOCK: u32 = 0x1_0000;
pub const PAGE: u32 = 256;
pub const FLASH_XIP_BASE: u32 = 0x4800_0000;
pub const SPV_MAGIC_AT: u32 = 0x04;
pub const SPV_MAGIC_LEN: u32 = 4;
pub const SPV_IDENTITY_LEN: usize = 0x30;
pub const SPV_HEADER_RESERVED: u32 = 0x100;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Owner {
    Installer,
    Journal,
    Firmware,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Region {
    pub name: &'static str,
    pub offset: u32,
    pub size: u32,
    pub owner: Owner,
}

impl Region {
    pub const fn end(&self) -> u32 {
        self.offset + self.size
    }

    pub const fn contains(&self, offset: u32, len: u32) -> bool {
        len != 0 && offset >= self.offset && offset <= self.end() && len <= self.end() - offset
    }

    pub const fn sectors(&self) -> u32 {
        self.size / SECTOR
    }
}

pub const STAGE0: Region = Region { name: "stage0", offset: 0x00_0000, size: 0x1_0000, owner: Owner::Installer };
pub const BOOT_JOURNAL: Region = Region { name: "journal", offset: 0x01_0000, size: 2 * SECTOR, owner: Owner::Journal };
pub const SLOT_A: Region = Region { name: "slot-a", offset: 0x04_0000, size: 0x10_0000, owner: Owner::Firmware };
pub const SLOT_B: Region = Region { name: "slot-b", offset: 0x14_0000, size: 0x10_0000, owner: Owner::Firmware };
pub const CRASH_LOG: Region = Region { name: "crash-log", offset: 0x24_0000, size: 0x1_0000, owner: Owner::Firmware };
pub const STOCK_COPY: Region = Region { name: "stock-copy", offset: 0x30_0000, size: 0x4_0000, owner: Owner::Installer };
pub const SETTINGS: Region = Region { name: "settings", offset: 0x3E_0000, size: 0x2_0000, owner: Owner::Firmware };

pub const REGIONS: [Region; 7] = [STAGE0, BOOT_JOURNAL, SLOT_A, SLOT_B, CRASH_LOG, STOCK_COPY, SETTINGS];
pub const SCRATCH: Region = Region { name: "scratch", offset: 0x3F_0000, size: 0x1_0000, owner: Owner::Firmware };

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MapError {
    Unaligned(&'static str),
    OutOfFlash(&'static str),
    Overlap(&'static str, &'static str),
}

pub fn validate_map(regions: &[Region]) -> Result<(), MapError> {
    for (i, r) in regions.iter().enumerate() {
        if r.offset % SECTOR != 0 || r.size % SECTOR != 0 || r.size == 0 {
            return Err(MapError::Unaligned(r.name));
        }
        if r.end() > FLASH_SIZE {
            return Err(MapError::OutOfFlash(r.name));
        }
        for other in &regions[i + 1..] {
            if r.offset < other.end() && other.offset < r.end() {
                return Err(MapError::Overlap(r.name, other.name));
            }
        }
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Writer {
    Stage0,
    Firmware { active: SlotId },
    Scratch,
}

pub const DISARM: Region = Region { name: "spv-magic", offset: SPV_MAGIC_AT, size: SPV_MAGIC_LEN, owner: Owner::Installer };

#[inline]
pub fn region_for_write(offset: u32, len: u32, writer: Writer) -> Option<Region> {
    match writer {
        Writer::Stage0 => {
            if BOOT_JOURNAL.contains(offset, len) {
                Some(BOOT_JOURNAL)
            } else if offset == DISARM.offset && len == DISARM.size {
                Some(DISARM)
            } else {
                None
            }
        }
        Writer::Firmware { active } => {
            [BOOT_JOURNAL, active.other().region(), CRASH_LOG, SETTINGS].into_iter().find(|region| region.contains(offset, len))
        }
        Writer::Scratch => SCRATCH.contains(offset, len).then_some(SCRATCH),
    }
}

pub trait WriteGuard {
    fn set_writer(&mut self, writer: Option<Writer>);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetError {
    PowerLost,
    Transport,
    ProgramVerify,
}

pub trait FlashTarget {
    fn read(&mut self, offset: u32, buf: &mut [u8]) -> Result<(), TargetError>;
    fn erase(&mut self, offset: u32, len: u32) -> Result<(), TargetError>;
    fn program(&mut self, offset: u32, data: &[u8]) -> Result<(), TargetError>;
}

impl<T: WriteGuard> WriteGuard for &mut T {
    fn set_writer(&mut self, writer: Option<Writer>) {
        T::set_writer(self, writer)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotId {
    A,
    B,
}

impl SlotId {
    pub const BOTH: [SlotId; 2] = [SlotId::A, SlotId::B];

    pub const fn region(self) -> Region {
        match self {
            SlotId::A => SLOT_A,
            SlotId::B => SLOT_B,
        }
    }

    pub const fn other(self) -> Self {
        match self {
            SlotId::A => SlotId::B,
            SlotId::B => SlotId::A,
        }
    }

    pub const fn index(self) -> usize {
        match self {
            SlotId::A => 0,
            SlotId::B => 1,
        }
    }

    pub const fn from_index(index: usize) -> Option<Self> {
        match index {
            0 => Some(SlotId::A),
            1 => Some(SlotId::B),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RamWindow {
    pub name: &'static str,
    pub start: u32,
    pub end: u32,
}

impl RamWindow {
    pub const fn contains(&self, start: u32, size: u32) -> bool {
        size != 0 && start >= self.start && start <= self.end && size <= self.end - start
    }
}

pub const ILM_WINDOW: RamWindow = RamWindow { name: "ilm", start: 0x0000_0000, end: 0x0001_0000 };
pub const DLM_WINDOW: RamWindow = RamWindow { name: "dlm", start: 0x0010_2000, end: 0x0012_8000 };
pub const RT40_WINDOW: RamWindow = RamWindow { name: "rt40", start: 0x4000_0000, end: 0x4004_0000 };
pub const RUNTIME_ILM: RamWindow = RamWindow { name: "runtime-ilm", start: 0x0000_2000, end: ILM_WINDOW.end };
pub const RUNTIME_DLM: RamWindow = DLM_WINDOW;
pub const RUNTIME_ENTRY: u32 = RUNTIME_ILM.start;
pub const RUNTIME_TRAP_STACK: u32 = 2048;
pub const RUNTIME_MIN_STACK: u32 = 0x1_0000;
pub const STAGE0_RAM: RamWindow = RamWindow { name: "stage0", start: 0x0012_8000, end: 0x0013_7D00 };
pub const STAGE0_MIN_STACK: u32 = 0x4000;
pub const STAGE0_TRAP_STACK: u32 = 0x400;
pub const LAST_CRASH_ADDRESS: u32 = 0x0013_7D00;
pub const BOOT_INFO_ADDRESS: u32 = 0x0013_7E00;
pub const DIAG_RECORD_ADDRESS: u32 = 0x0013_7F00;
pub const DIAG_RECORD_SIZE: u32 = 256;
pub const STAGE0_REPORT_ADDRESS: u32 = BOOT_INFO_ADDRESS + 64;
pub const STAGE0_REPORT_SIZE: u32 = 64;
pub const STAGE0_RAM_IMAGE_OFFSET: u32 = 0x200;
pub const STAGE0_XIP_ENTRY: u32 = FLASH_XIP_BASE + SPV_HEADER_RESERVED;
pub const STAGE0_BOOT_ENTRY: u32 = STAGE0_RAM.start;
pub const STAGE0_DRY_ENTRY: u32 = STAGE0_RAM.start + 4;
pub const STAGE0_VERIFY_ENTRY: u32 = STAGE0_RAM.start + 8;
pub const STAGE0_VERIFY_MAGIC_ADDRESS: u32 = STAGE0_RAM.start + 12;
pub const STAGE0_VERIFY_MAGIC: [u8; 4] = *b"KSV1";
pub const STAGE0_G2B_ENTRY: u32 = STAGE0_RAM.start + 16;
pub const STAGE0_CALL_FAULTED: u32 = 0xEF;
pub const STAGE0_CALL_PANICKED: u32 = 0xEE;
pub const BOOTROM_STACK: RamWindow = RamWindow { name: "bootrom-stack", start: 0x0013_8000, end: 0x0014_0000 };
pub const BOOTROM_DATA: RamWindow = RamWindow { name: "bootrom-data", start: 0x0010_0000, end: 0x0010_2000 };
pub const LOAD_WINDOWS: [RamWindow; 3] = [ILM_WINDOW, DLM_WINDOW, RT40_WINDOW];

const _: () = {
    assert!(STAGE0_RAM.end == LAST_CRASH_ADDRESS);
    assert!(LAST_CRASH_ADDRESS + DIAG_RECORD_SIZE == BOOT_INFO_ADDRESS);
    assert!(BOOT_INFO_ADDRESS + DIAG_RECORD_SIZE == DIAG_RECORD_ADDRESS);
    assert!(DIAG_RECORD_ADDRESS + DIAG_RECORD_SIZE <= BOOTROM_STACK.start);
    assert!(STAGE0_REPORT_ADDRESS + STAGE0_REPORT_SIZE <= DIAG_RECORD_ADDRESS);
    assert!(RUNTIME_DLM.end <= STAGE0_RAM.start);
    assert!(RUNTIME_DLM.end - RUNTIME_DLM.start > RUNTIME_MIN_STACK + RUNTIME_TRAP_STACK);
    assert!(STAGE0_RAM.end - STAGE0_RAM.start > STAGE0_MIN_STACK + STAGE0_TRAP_STACK);
    assert!(STAGE0_RAM_IMAGE_OFFSET >= SPV_HEADER_RESERVED + 0x100);
    assert!(DISARM.offset + DISARM.size <= SPV_IDENTITY_LEN as u32);
    assert!(SCRATCH.offset.is_multiple_of(BLOCK) && SCRATCH.size == BLOCK && SCRATCH.end() == FLASH_SIZE);
    assert!(SETTINGS.offset <= SCRATCH.offset && SCRATCH.end() <= SETTINGS.end());
};

pub const IMAGE_MAGIC: [u8; 4] = *b"KUMI";
pub const IMAGE_FORMAT: u16 = 1;
pub const HEADER_SIZE: usize = 256;
pub const SIGNED_HEADER_LEN: usize = 128;
pub const MAX_SEGMENTS: usize = 4;
pub const FLAG_SAFE_MODE_CAPABLE: u16 = 1;
pub const FLAG_LAB: u16 = 1 << 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SegmentKind {
    Copy,
    Zero,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Segment {
    pub kind: SegmentKind,
    pub destination: u32,
    pub offset: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImageHeader {
    pub flags: u16,
    pub version: [u8; 3],
    pub generation: u32,
    pub payload_size: u32,
    pub entry: u32,
    pub segments: [Option<Segment>; MAX_SEGMENTS],
    pub key_id: [u8; 8],
    pub digest: [u8; 32],
    pub signature: [u8; 64],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HeaderError {
    Magic,
    Crc,
    Format,
    SegmentCount,
    SegmentKind,
    SegmentPayload,
    SegmentDestination,
    SegmentOverlap,
    Entry,
    TooLarge,
}

impl ImageHeader {
    pub fn segments(&self) -> impl Iterator<Item = &Segment> {
        self.segments.iter().flatten()
    }

    pub fn encode(&self) -> [u8; HEADER_SIZE] {
        let mut out = [0u8; HEADER_SIZE];
        out[0..4].copy_from_slice(&IMAGE_MAGIC);
        out[4..6].copy_from_slice(&IMAGE_FORMAT.to_le_bytes());
        out[6..8].copy_from_slice(&self.flags.to_le_bytes());
        out[8..11].copy_from_slice(&self.version);
        out[11] = self.segments().count() as u8;
        put32(&mut out, 12, self.generation);
        put32(&mut out, 16, self.payload_size);
        put32(&mut out, 20, self.entry);
        for (i, s) in self.segments().enumerate() {
            let at = 24 + 16 * i;
            put32(
                &mut out,
                at,
                match s.kind {
                    SegmentKind::Copy => 1,
                    SegmentKind::Zero => 2,
                },
            );
            put32(&mut out, at + 4, s.destination);
            put32(&mut out, at + 8, s.offset);
            put32(&mut out, at + 12, s.size);
        }
        out[88..96].copy_from_slice(&self.key_id);
        out[128..160].copy_from_slice(&self.digest);
        out[160..224].copy_from_slice(&self.signature);
        let crc = crc32(&out[..252]);
        put32(&mut out, 252, crc);
        out
    }

    pub fn decode(bytes: &[u8; HEADER_SIZE], slot_size: u32) -> Result<Self, HeaderError> {
        if bytes[0..4] != IMAGE_MAGIC {
            return Err(HeaderError::Magic);
        }
        if crc32(&bytes[..252]) != get32(bytes, 252) {
            return Err(HeaderError::Crc);
        }
        if u16::from_le_bytes([bytes[4], bytes[5]]) != IMAGE_FORMAT {
            return Err(HeaderError::Format);
        }
        let count = usize::from(bytes[11]);
        if count == 0 || count > MAX_SEGMENTS {
            return Err(HeaderError::SegmentCount);
        }
        let payload_size = get32(bytes, 16);
        if u64::from(payload_size) + HEADER_SIZE as u64 > u64::from(slot_size) {
            return Err(HeaderError::TooLarge);
        }
        let mut segments = [None; MAX_SEGMENTS];
        for (i, slot) in segments.iter_mut().enumerate().take(count) {
            let at = 24 + 16 * i;
            let kind = match get32(bytes, at) {
                1 => SegmentKind::Copy,
                2 => SegmentKind::Zero,
                _ => return Err(HeaderError::SegmentKind),
            };
            let segment =
                Segment { kind, destination: get32(bytes, at + 4), offset: get32(bytes, at + 8), size: get32(bytes, at + 12) };
            if kind == SegmentKind::Copy && u64::from(segment.offset) + u64::from(segment.size) > u64::from(payload_size) {
                return Err(HeaderError::SegmentPayload);
            }
            if !LOAD_WINDOWS.iter().any(|w| w.contains(segment.destination, segment.size)) {
                return Err(HeaderError::SegmentDestination);
            }
            *slot = Some(segment);
        }
        for (i, a) in segments.iter().flatten().enumerate() {
            for b in segments.iter().flatten().skip(i + 1) {
                if a.destination < b.destination + b.size && b.destination < a.destination + a.size {
                    return Err(HeaderError::SegmentOverlap);
                }
            }
        }
        let entry = get32(bytes, 20);
        if !segments
            .iter()
            .flatten()
            .any(|s| s.kind == SegmentKind::Copy && entry >= s.destination && entry < s.destination + s.size)
        {
            return Err(HeaderError::Entry);
        }
        let mut header = Self {
            flags: u16::from_le_bytes([bytes[6], bytes[7]]),
            version: [bytes[8], bytes[9], bytes[10]],
            generation: get32(bytes, 12),
            payload_size,
            entry,
            segments,
            key_id: [0; 8],
            digest: [0; 32],
            signature: [0; 64],
        };
        header.key_id.copy_from_slice(&bytes[88..96]);
        header.digest.copy_from_slice(&bytes[128..160]);
        header.signature.copy_from_slice(&bytes[160..224]);
        Ok(header)
    }
}

fn put32(out: &mut [u8], at: usize, value: u32) {
    out[at..at + 4].copy_from_slice(&value.to_le_bytes());
}

pub fn get32(bytes: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([bytes[at], bytes[at + 1], bytes[at + 2], bytes[at + 3]])
}

const CRC_TABLE: [u32; 16] = {
    let mut table = [0u32; 16];
    let mut i = 0;
    while i < 16 {
        let mut c = i as u32;
        let mut k = 0;
        while k < 4 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            k += 1;
        }
        table[i] = c;
        i += 1;
    }
    table
};

pub fn crc32(data: &[u8]) -> u32 {
    crc32_update(0xFFFF_FFFF, data) ^ 0xFFFF_FFFF
}

pub fn crc32_update(mut crc: u32, data: &[u8]) -> u32 {
    for &byte in data {
        crc = CRC_TABLE[((crc ^ u32::from(byte)) & 0x0F) as usize] ^ (crc >> 4);
        crc = CRC_TABLE[((crc ^ (u32::from(byte) >> 4)) & 0x0F) as usize] ^ (crc >> 4);
    }
    crc
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header() -> ImageHeader {
        ImageHeader {
            flags: FLAG_SAFE_MODE_CAPABLE,
            version: [0, 1, 0],
            generation: 7,
            payload_size: 0x9000,
            entry: 0x0000_0100,
            segments: [
                Some(Segment { kind: SegmentKind::Copy, destination: 0x0000_0000, offset: 0, size: 0x8000 }),
                Some(Segment { kind: SegmentKind::Copy, destination: 0x0010_2000, offset: 0x8000, size: 0x1000 }),
                Some(Segment { kind: SegmentKind::Zero, destination: 0x0010_3000, offset: 0, size: 0x4000 }),
                None,
            ],
            key_id: [1; 8],
            digest: [2; 32],
            signature: [3; 64],
        }
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn production_map_is_valid_and_guards_writes() {
        validate_map(&REGIONS).unwrap();
        let firmware = Writer::Firmware { active: SlotId::A };
        assert_eq!(region_for_write(STAGE0.offset, SECTOR, firmware), None);
        assert_eq!(region_for_write(STAGE0.offset, SECTOR, Writer::Stage0), None);
        assert_eq!(region_for_write(STOCK_COPY.offset, SECTOR, firmware), None);
        assert_eq!(region_for_write(STOCK_COPY.offset, SECTOR, Writer::Stage0), None);
        assert_eq!(region_for_write(SLOT_B.offset, SLOT_B.size, firmware), Some(SLOT_B));
        assert_eq!(region_for_write(SLOT_B.offset, SLOT_B.size + 1, firmware), None);
        assert_eq!(region_for_write(SLOT_A.offset, SECTOR, firmware), None);
        assert_eq!(region_for_write(SLOT_A.offset, SECTOR, Writer::Firmware { active: SlotId::B }), Some(SLOT_A));
        assert_eq!(region_for_write(SLOT_A.end() - 4, 8, Writer::Firmware { active: SlotId::B }), None);
        assert_eq!(region_for_write(BOOT_JOURNAL.offset, SECTOR, firmware), Some(BOOT_JOURNAL));
        assert_eq!(region_for_write(BOOT_JOURNAL.offset, SECTOR, Writer::Stage0), Some(BOOT_JOURNAL));
        assert_eq!(region_for_write(CRASH_LOG.offset, 512, firmware), Some(CRASH_LOG));
        assert_eq!(region_for_write(SETTINGS.end() - SECTOR, SECTOR, firmware), Some(SETTINGS));
        assert_eq!(region_for_write(CRASH_LOG.offset, 512, Writer::Stage0), None);
        assert_eq!(region_for_write(SETTINGS.offset, SECTOR, Writer::Stage0), None);
        assert_eq!(region_for_write(SLOT_A.offset, SECTOR, Writer::Stage0), None);
        assert_eq!(region_for_write(SPV_MAGIC_AT, SPV_MAGIC_LEN, Writer::Stage0), Some(DISARM));
        assert_eq!(region_for_write(SPV_MAGIC_AT, SPV_MAGIC_LEN, firmware), None);
        for (offset, len) in [(0, 4), (0, 8), (4, 1), (5, 4), (4, 8), (0x2C, 4), (0, SECTOR)] {
            assert_eq!(region_for_write(offset, len, Writer::Stage0), None, "{offset:#x}+{len}");
        }
        assert!(
            validate_map(&[SLOT_A, Region { name: "x", offset: 0x0F_0000, size: 0x2_0000, owner: Owner::Firmware }]).is_err()
        );
        assert!(validate_map(&[Region { name: "x", offset: 0x10, size: SECTOR, owner: Owner::Firmware }]).is_err());
    }

    #[test]
    fn the_scratch_writer_reaches_exactly_the_last_64_kib_and_nothing_else() {
        let scratch = Writer::Scratch;
        assert_eq!(SCRATCH.offset, 0x3F_0000);
        assert_eq!(region_for_write(SCRATCH.offset, BLOCK, scratch), Some(SCRATCH));
        assert_eq!(region_for_write(SCRATCH.offset + 0x12C, 4, scratch), Some(SCRATCH));
        assert_eq!(region_for_write(FLASH_SIZE - PAGE, PAGE, scratch), Some(SCRATCH));
        for (offset, len) in [
            (SCRATCH.offset - 1, 1),
            (SCRATCH.offset - 4, 8),
            (SETTINGS.offset, SECTOR),
            (SETTINGS.offset, SETTINGS.size),
            (SCRATCH.offset, BLOCK + 1),
            (FLASH_SIZE, 1),
            (FLASH_SIZE - 4, 8),
            (SCRATCH.offset, 0),
            (0, 4),
            (SPV_MAGIC_AT, SPV_MAGIC_LEN),
            (BOOT_JOURNAL.offset, SECTOR),
            (CRASH_LOG.offset, 512),
            (SLOT_A.offset, SECTOR),
            (SLOT_B.offset, SECTOR),
            (STOCK_COPY.offset, SECTOR),
            (0x3F_0000 & 0xFFFF, PAGE),
        ] {
            assert_eq!(region_for_write(offset, len, scratch), None, "{offset:#x}+{len}");
        }
        assert!(!REGIONS.contains(&SCRATCH));
        assert!(SETTINGS.contains(SCRATCH.offset, SCRATCH.size));
        assert_eq!(region_for_write(SCRATCH.offset, SECTOR, Writer::Stage0), None);
        assert_eq!(region_for_write(SCRATCH.offset, SECTOR, Writer::Firmware { active: SlotId::A }), Some(SETTINGS));
    }

    #[test]
    fn the_stage0_call_table_and_runtime_windows_fit_the_ram_map() {
        assert_eq!(STAGE0_DRY_ENTRY - STAGE0_BOOT_ENTRY, 4);
        assert_eq!(STAGE0_VERIFY_ENTRY - STAGE0_DRY_ENTRY, 4);
        assert_eq!(STAGE0_VERIFY_MAGIC_ADDRESS - STAGE0_VERIFY_ENTRY, 4);
        assert_eq!(STAGE0_G2B_ENTRY - STAGE0_VERIFY_MAGIC_ADDRESS, 4);
        assert!(STAGE0_RAM.contains(STAGE0_BOOT_ENTRY, 20));
        assert!(ILM_WINDOW.contains(RUNTIME_ILM.start, RUNTIME_ILM.end - RUNTIME_ILM.start));
        assert_eq!(STAGE0_XIP_ENTRY.to_le_bytes(), [0x00, 0x01, 0x00, 0x48]);
        assert_eq!(&STAGE0_VERIFY_MAGIC, b"KSV1");
    }

    #[test]
    fn diagnostics_live_outside_everything_the_bootrom_and_images_touch() {
        let areas = [LAST_CRASH_ADDRESS, BOOT_INFO_ADDRESS, DIAG_RECORD_ADDRESS];
        for area in areas {
            for window in [BOOTROM_STACK, BOOTROM_DATA] {
                assert!(area + DIAG_RECORD_SIZE <= window.start || area >= window.end);
            }
            for window in LOAD_WINDOWS {
                assert!(!window.contains(area, DIAG_RECORD_SIZE));
            }
        }
        for window in LOAD_WINDOWS {
            assert!(window.end <= STAGE0_RAM.start || window.start >= STAGE0_RAM.end);
        }
    }

    #[test]
    fn header_round_trips() {
        let h = header();
        let bytes = h.encode();
        assert_eq!(ImageHeader::decode(&bytes, SLOT_A.size), Ok(h));
    }

    #[test]
    fn header_rejects_corruption_and_unsafe_layouts() {
        let good = header();
        let mut bytes = good.encode();
        bytes[30] ^= 1;
        assert_eq!(ImageHeader::decode(&bytes, SLOT_A.size), Err(HeaderError::Crc));
        let check = |h: ImageHeader| ImageHeader::decode(&h.encode(), SLOT_A.size);
        let mut h = good;
        h.segments[0] = Some(Segment { kind: SegmentKind::Copy, destination: 0x0010_0000, offset: 0, size: 0x100 });
        assert_eq!(check(h), Err(HeaderError::SegmentDestination));
        let mut h = good;
        h.segments[3] = Some(Segment { kind: SegmentKind::Zero, destination: 0x0010_2800, offset: 0, size: 0x100 });
        assert_eq!(check(h), Err(HeaderError::SegmentOverlap));
        let mut h = good;
        h.segments[1] = Some(Segment { kind: SegmentKind::Copy, destination: 0x0010_2000, offset: 0x8F00, size: 0x200 });
        assert_eq!(check(h), Err(HeaderError::SegmentPayload));
        let mut h = good;
        h.entry = 0x0010_3000;
        assert_eq!(check(h), Err(HeaderError::Entry));
        let mut h = good;
        h.payload_size = SLOT_A.size;
        assert_eq!(check(h), Err(HeaderError::TooLarge));
        let mut h = good;
        h.segments = [None; MAX_SEGMENTS];
        assert_eq!(check(h), Err(HeaderError::SegmentCount));
        let mut h = good;
        h.segments[0] = Some(Segment { kind: SegmentKind::Copy, destination: DIAG_RECORD_ADDRESS, offset: 0, size: 4 });
        assert_eq!(check(h), Err(HeaderError::SegmentDestination));
        let mut h = good;
        h.segments[2] =
            Some(Segment { kind: SegmentKind::Zero, destination: BOOTROM_STACK.start - 0x100, offset: 0, size: 0x200 });
        assert_eq!(check(h), Err(HeaderError::SegmentDestination));
        let mut bytes = good.encode();
        bytes[0] = b'X';
        assert_eq!(ImageHeader::decode(&bytes, SLOT_A.size), Err(HeaderError::Magic));
        let mut h = good;
        h.segments[1] = Some(Segment { kind: SegmentKind::Copy, destination: RT40_WINDOW.start, offset: 0x8000, size: 0x1000 });
        assert_eq!(check(h), Ok(h));
        h.segments[1] = Some(Segment { kind: SegmentKind::Copy, destination: RT40_WINDOW.end, offset: 0x8000, size: 0x1000 });
        assert_eq!(check(h), Err(HeaderError::SegmentDestination));
    }
}

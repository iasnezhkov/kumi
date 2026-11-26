// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

pub mod log;

use kumi_layout::{crc32, get32};

pub const AREA_SIZE: usize = 256;
pub const MESSAGE_LEN: usize = kumi_icd::CRASH_MESSAGE_LEN;
pub const TRAIL_LEN: usize = 16;
const MAGIC: [u8; 4] = *b"KDIA";
const FORMAT: u8 = 1;
const CRASHES_AT: usize = 37;
const MESSAGE_AT: usize = 40;
const TRAIL_AT: usize = MESSAGE_AT + MESSAGE_LEN;
const TRAIL_COUNT_AT: usize = TRAIL_AT + 2 * TRAIL_LEN;
const CRC_AT: usize = AREA_SIZE - 4;

pub use kumi_icd::stage;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Reason {
    None,
    Panic,
    Trap,
}

impl Reason {
    const fn wire(self) -> u8 {
        match self {
            Reason::None => 0,
            Reason::Panic => 1,
            Reason::Trap => 2,
        }
    }

    const fn from_wire(value: u8) -> Option<Self> {
        Some(match value {
            0 => Reason::None,
            1 => Reason::Panic,
            2 => Reason::Trap,
            _ => return None,
        })
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Registers {
    pub mcause: u32,
    pub mepc: u32,
    pub mtval: u32,
    pub ra: u32,
    pub sp: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Report {
    pub boot_count: u32,
    pub crashes_before: u8,
    pub reason: Reason,
    pub stage: u16,
    pub registers: Registers,
    pub uptime_ms: u32,
    pub message: [u8; MESSAGE_LEN],
    pub message_len: usize,
    pub trail: [u16; TRAIL_LEN],
    pub trail_len: usize,
}

impl Report {
    pub fn message(&self) -> &str {
        core::str::from_utf8(&self.message[..self.message_len]).unwrap_or("")
    }

    pub fn trail(&self) -> &[u16] {
        &self.trail[..self.trail_len]
    }

    pub fn crashed(&self) -> bool {
        self.reason != Reason::None
    }
}

pub struct Diag<'a> {
    area: &'a mut [u8; AREA_SIZE],
}

impl<'a> Diag<'a> {
    pub fn begin(area: &'a mut [u8; AREA_SIZE], boot_count: u32, crashes_before: u8) -> Self {
        area.fill(0);
        area[0..4].copy_from_slice(&MAGIC);
        area[4] = FORMAT;
        area[8..12].copy_from_slice(&boot_count.to_le_bytes());
        area[CRASHES_AT] = crashes_before;
        let mut diag = Self { area };
        diag.mark(stage::RESET);
        diag
    }

    pub fn attach(area: &'a mut [u8; AREA_SIZE]) -> Option<Self> {
        read(area)?;
        Some(Self { area })
    }

    pub fn mark(&mut self, stage: u16) {
        self.area[6..8].copy_from_slice(&stage.to_le_bytes());
        let count = usize::from(self.area[TRAIL_COUNT_AT]);
        let index = count % TRAIL_LEN;
        self.area[TRAIL_AT + 2 * index..TRAIL_AT + 2 * index + 2].copy_from_slice(&stage.to_le_bytes());
        self.area[TRAIL_COUNT_AT] = if count == 255 { 240 } else { (count + 1) as u8 };
        self.seal();
    }

    pub fn crash(&mut self, reason: Reason, registers: Registers, uptime_ms: u32, message: &str) {
        self.area[5] = reason.wire();
        for (i, value) in
            [registers.mcause, registers.mepc, registers.mtval, registers.ra, registers.sp, uptime_ms].iter().enumerate()
        {
            self.area[12 + 4 * i..16 + 4 * i].copy_from_slice(&value.to_le_bytes());
        }
        let mut len = message.len().min(MESSAGE_LEN);
        while !message.is_char_boundary(len) {
            len -= 1;
        }
        self.area[MESSAGE_AT..MESSAGE_AT + MESSAGE_LEN].fill(0);
        for (dst, &src) in self.area[MESSAGE_AT..MESSAGE_AT + MESSAGE_LEN].iter_mut().zip(message.as_bytes().iter().take(len)) {
            *dst = src;
        }
        self.area[36] = len as u8;
        self.seal();
    }

    fn seal(&mut self) {
        let crc = crc32(&self.area[..CRC_AT]);
        self.area[CRC_AT..].copy_from_slice(&crc.to_le_bytes());
    }
}

pub fn read(area: &[u8; AREA_SIZE]) -> Option<Report> {
    if area[0..4] != MAGIC || area[4] != FORMAT || crc32(&area[..CRC_AT]) != get32(area, CRC_AT) {
        return None;
    }
    let message_len = usize::from(area[36]);
    if message_len > MESSAGE_LEN {
        return None;
    }
    let count = usize::from(area[TRAIL_COUNT_AT]);
    let mut trail = [0u16; TRAIL_LEN];
    let trail_len = count.min(TRAIL_LEN);
    for (i, slot) in trail.iter_mut().enumerate().take(trail_len) {
        let index = if count > TRAIL_LEN { (count + i) % TRAIL_LEN } else { i };
        *slot = u16::from_le_bytes([area[TRAIL_AT + 2 * index], area[TRAIL_AT + 2 * index + 1]]);
    }
    let mut message = [0u8; MESSAGE_LEN];
    message.copy_from_slice(&area[MESSAGE_AT..MESSAGE_AT + MESSAGE_LEN]);
    Some(Report {
        boot_count: get32(area, 8),
        crashes_before: area[CRASHES_AT],
        reason: Reason::from_wire(area[5])?,
        stage: u16::from_le_bytes([area[6], area[7]]),
        registers: Registers {
            mcause: get32(area, 12),
            mepc: get32(area, 16),
            mtval: get32(area, 20),
            ra: get32(area, 24),
            sp: get32(area, 28),
        },
        uptime_ms: get32(area, 32),
        message,
        message_len,
        trail,
        trail_len,
    })
}

pub fn previous_boot_crashed(area: &[u8; AREA_SIZE], last_boot_count: u32) -> bool {
    read(area).is_some_and(|r| r.crashed() && r.boot_count == last_boot_count)
}

pub fn consecutive_crashes(area: &[u8; AREA_SIZE]) -> u8 {
    match read(area) {
        Some(report) if report.crashed() => report.crashes_before.saturating_add(1),
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_boot_leaves_a_trail_and_no_crash() {
        let mut area = [0xAA; AREA_SIZE];
        let mut diag = Diag::begin(&mut area, 41, 0);
        diag.mark(stage::FW_ENTRY);
        diag.mark(stage::FW_CLOCKS);
        let report = read(&area).unwrap();
        assert_eq!(report.boot_count, 41);
        assert_eq!(report.stage, stage::FW_CLOCKS);
        assert_eq!(report.trail(), [stage::RESET, stage::FW_ENTRY, stage::FW_CLOCKS]);
        assert!(!report.crashed());
        assert!(!previous_boot_crashed(&area, 41));
        assert_eq!(consecutive_crashes(&area), 0);
        assert_eq!(consecutive_crashes(&[0x5A; AREA_SIZE]), 0);
    }

    #[test]
    fn crash_report_carries_registers_message_and_stage() {
        let mut area = [0u8; AREA_SIZE];
        let mut diag = Diag::begin(&mut area, 7, 2);
        diag.mark(stage::FW_USB);
        let regs = Registers { mcause: 2, mepc: 0x1234, mtval: 0xDEAD_BEEF, ra: 0x5678, sp: 0x0013_0000 };
        diag.crash(Reason::Trap, regs, 1_500, "illegal instruction in usb::init");
        let report = read(&area).unwrap();
        assert_eq!(report.reason, Reason::Trap);
        assert_eq!(report.stage, stage::FW_USB);
        assert_eq!(report.registers, regs);
        assert_eq!(report.uptime_ms, 1_500);
        assert_eq!(report.message(), "illegal instruction in usb::init");
        assert!(previous_boot_crashed(&area, 7));
        assert!(!previous_boot_crashed(&area, 8));
        assert_eq!(report.crashes_before, 2);
        assert_eq!(consecutive_crashes(&area), 3);
    }

    #[test]
    fn trail_keeps_the_last_stages_in_order() {
        let mut area = [0u8; AREA_SIZE];
        let mut diag = Diag::begin(&mut area, 1, 0);
        for s in 0..400u16 {
            diag.mark(0x1000 + s);
        }
        let report = read(&area).unwrap();
        let expected: Vec<u16> = (384..400).map(|s| 0x1000 + s).collect();
        assert_eq!(report.trail(), expected.as_slice());
    }

    #[test]
    fn long_and_multibyte_messages_are_cut_on_char_boundaries() {
        let mut area = [0u8; AREA_SIZE];
        let mut diag = Diag::begin(&mut area, 1, 0);
        let text = "€".repeat(70);
        diag.crash(Reason::Panic, Registers::default(), 0, &text);
        let report = read(&area).unwrap();
        assert_eq!(report.message().len(), 96);
        assert!(report.message().chars().all(|c| c == '€'));
    }

    #[test]
    fn reasons_keep_their_wire_values_and_unknown_ones_are_rejected() {
        let mut area = [0u8; AREA_SIZE];
        Diag::begin(&mut area, 3, 0).crash(Reason::Trap, Registers::default(), 9, "trap");
        assert_eq!(area[5], 2);
        Diag::begin(&mut area, 3, 0).crash(Reason::Panic, Registers::default(), 9, "panic");
        assert_eq!(area[5], 1);
        for unknown in 3..=6 {
            area[5] = unknown;
            let crc = crc32(&area[..CRC_AT]);
            area[CRC_AT..].copy_from_slice(&crc.to_le_bytes());
            assert_eq!(read(&area), None);
        }
    }

    #[test]
    fn damaged_or_foreign_memory_is_not_a_report() {
        let mut area = [0u8; AREA_SIZE];
        Diag::begin(&mut area, 3, 0).crash(Reason::Trap, Registers::default(), 9, "stuck");
        for i in [0, 5, 9, 40, 200, 255] {
            let mut damaged = area;
            damaged[i] ^= 1;
            assert_eq!(read(&damaged), None, "byte {i}");
        }
        assert_eq!(read(&[0xFF; AREA_SIZE]), None);
        let mut copy = area;
        assert!(Diag::attach(&mut copy).is_some());
        let mut junk = [0u8; AREA_SIZE];
        assert!(Diag::attach(&mut junk).is_none());
    }
}

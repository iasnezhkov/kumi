// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::{String, Vec};
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

pub const WORD_BLOCK: usize = 32;
pub const BYTE_BLOCK: usize = 64;
pub const LOG_LINE: usize = 96;
pub const MAX_UNLOCK_SECONDS: u16 = 600;

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DevUnlockRequest {
    pub seconds: u16,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeekRequest {
    pub address: u32,
    pub words: u8,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PokeRequest {
    pub address: u32,
    pub value: u32,
    pub mask: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct WordBlock {
    pub address: u32,
    pub words: Vec<u32, WORD_BLOCK>,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct ByteBlock {
    pub bytes: Vec<u8, BYTE_BLOCK>,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct I2cRequest {
    pub bus: u8,
    pub address: u8,
    pub write: Vec<u8, BYTE_BLOCK>,
    pub read: u8,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct I2cReply {
    pub acked: bool,
    pub data: Vec<u8, BYTE_BLOCK>,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    pub uptime_ms: u32,
    pub level: LogLevel,
    pub module: String<16>,
    pub text: String<LOG_LINE>,
}

impl PeekRequest {
    pub fn is_valid(&self) -> bool {
        self.address.is_multiple_of(4)
            && (1..=WORD_BLOCK as u8).contains(&self.words)
            && self.address.checked_add(4 * u32::from(self.words) - 1).is_some()
    }
}

impl PokeRequest {
    pub fn is_valid(&self) -> bool {
        self.address.is_multiple_of(4)
    }
}

impl I2cRequest {
    pub const MAX_ADDRESS: u8 = 0x7F;

    pub fn is_valid(&self) -> bool {
        self.address <= Self::MAX_ADDRESS && usize::from(self.read) <= BYTE_BLOCK
    }
}

impl DevUnlockRequest {
    pub fn is_valid(&self) -> bool {
        (1..=MAX_UNLOCK_SECONDS).contains(&self.seconds)
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    IllegalInstruction,
    Breakpoint,
    Panic,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FlashTestReport {
    pub window_was_erased: bool,
    pub page_matches: bool,
    pub unaligned_matches: bool,
    pub sector_erased: bool,
    pub block_erased: bool,
    pub page_program_cycles: u32,
    pub sector_erase_cycles: u32,
    pub block_erase_cycles: u32,
    pub stock_head_crc_before: u32,
    pub stock_head_crc_after: u32,
}

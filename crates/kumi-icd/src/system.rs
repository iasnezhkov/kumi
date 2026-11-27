// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::Vec;
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

use crate::device::Version;

pub const UPDATE_BLOCK: usize = 256;
pub const CRASH_MESSAGE_LEN: usize = 96;

pub mod stage {
    pub const UNKNOWN: u16 = 0x0000;
    pub const RESET: u16 = 0x0001;
    pub const STAGE0_JOURNAL: u16 = 0x0011;
    pub const STAGE0_VERIFIED: u16 = 0x0012;
    pub const STAGE0_LOADED: u16 = 0x0013;
    pub const STAGE0_JUMP: u16 = 0x0014;
    pub const STAGE0_BOOTROM: u16 = 0x0015;
    pub const STAGE0_DISARMED: u16 = 0x0016;
    pub const FW_ENTRY: u16 = 0x0100;
    pub const FW_CLOCKS: u16 = 0x0101;
    pub const FW_UART: u16 = 0x0104;
    pub const FW_USB: u16 = 0x0105;
    pub const FW_STORAGE: u16 = 0x0107;
    pub const FW_RUNNING: u16 = 0x01F0;
    pub const FW_HEALTHY: u16 = 0x01F1;
    pub const FW_SAFE_MODE: u16 = 0x0200;

    pub const NAMES: [(u16, &str); 16] = [
        (UNKNOWN, "unknown"),
        (RESET, "reset"),
        (STAGE0_JOURNAL, "stage0 journal"),
        (STAGE0_VERIFIED, "stage0 verified"),
        (STAGE0_LOADED, "stage0 loaded"),
        (STAGE0_JUMP, "stage0 jump"),
        (STAGE0_BOOTROM, "stage0 boot ROM"),
        (STAGE0_DISARMED, "stage0 disarmed"),
        (FW_ENTRY, "firmware entry"),
        (FW_CLOCKS, "firmware clocks"),
        (FW_UART, "firmware UART"),
        (FW_USB, "firmware USB"),
        (FW_STORAGE, "firmware storage"),
        (FW_RUNNING, "firmware running"),
        (FW_HEALTHY, "firmware healthy"),
        (FW_SAFE_MODE, "firmware safe mode"),
    ];

    pub fn name(stage: u16) -> Option<&'static str> {
        NAMES.iter().find(|(code, _)| *code == stage).map(|(_, name)| *name)
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    A,
    B,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlotState {
    Empty,
    Trial,
    Confirmed,
    Invalid,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotInfo {
    pub slot: Slot,
    pub state: SlotState,
    pub version: Option<Version>,
    pub size: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct SlotTable {
    pub booted: Option<Slot>,
    pub slots: [SlotInfo; 2],
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RebootTarget {
    Normal,
    OtherSlot,
    BootRom,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsScope {
    All,
    Audio,
    Keymap,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpdateManifest {
    pub slot: Slot,
    pub version: Version,
    pub size: u32,
    pub sha256: [u8; 32],
    pub signature: [[u8; 32]; 2],
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct UpdateBlock {
    pub offset: u32,
    pub data: Vec<u8, UPDATE_BLOCK>,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum CrashReason {
    Panic,
    Trap,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct CrashReport {
    pub boot_count: u32,
    pub reason: CrashReason,
    pub stage: u16,
    pub mcause: u32,
    pub mepc: u32,
    pub mtval: u32,
    pub ra: u32,
    pub sp: u32,
    pub uptime_ms: u32,
    pub message: heapless::String<CRASH_MESSAGE_LEN>,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct Diagnostics {
    pub boot_count: u32,
    pub safe_mode: bool,
    pub stage: u16,
    pub uptime_ms: u32,
    pub dropped_events: u32,
    pub watchdog_armed: bool,
    pub last_crash: Option<CrashReport>,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SystemTelemetry {
    pub cpu_mhz: u16,
    pub dcdc_active: bool,
    pub dvdd_mv: u16,
    pub analog_rails_on: bool,
    pub audio_pll_enabled: bool,
    pub wfi_percent_tenths: u16,
    pub wfi_wakeups: u32,
    pub jack_adc: u16,
    pub uptime_ms: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PresenceStatus {
    pub tier: u8,
    pub remaining_ms: u32,
    pub passthrough: bool,
}

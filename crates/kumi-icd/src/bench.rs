// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::Vec;
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

pub const AUDIO_SWEEP_COUNTS: usize = 32;
pub const AUDIO_MONITOR_WORDS: u8 = 15;
pub const AUDIO_MONITOR_CYCLES: usize = 0;
pub const AUDIO_MONITOR_FRAME: usize = 1;
pub const AUDIO_MONITOR_WRAPS: usize = 2;
pub const AUDIO_MONITOR_POLLS: usize = 3;
pub const AUDIO_MONITOR_ERRORS: usize = 4;
pub const AUDIO_MONITOR_DECODING: usize = 5;
pub const AUDIO_MONITOR_PROBE_RATE: usize = 6;
pub const AUDIO_MONITOR_COMPLETIONS: usize = 7;
pub const AUDIO_MONITOR_QUEUED: usize = 8;
pub const AUDIO_MONITOR_FEEDBACK: usize = 9;
pub const AUDIO_MONITOR_PACKETS: usize = 10;
pub const AUDIO_MONITOR_UNDERRUNS: usize = 11;
pub const AUDIO_MONITOR_OVERRUNS: usize = 12;
pub const AUDIO_MONITOR_LOST_PACKETS: usize = 13;
pub const AUDIO_MONITOR_PAUSES: usize = 14;

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioChannel {
    Left,
    Right,
    Both,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStart {
    pub rate_hz: u32,
    pub channel: AudioChannel,
    pub level_centi_dbfs: i16,
    pub tone_hz: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBenchRequest {
    Calibrate,
    Start(AudioStart),
    Stop,
    Status,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBenchState {
    Idle,
    Starting,
    Running,
    Failed,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioPhase {
    Calibrate,
    Setup,
    Clock,
    ResetLow,
    ResetHigh,
    Dac,
    Dma,
    Unmute,
    Stop,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioClockFault {
    NoTrimWindow,
    DisjointTrimWindows,
    InvalidReference { trim: u8, reference: u32 },
    TimerInUse,
    TimeWentBackwards,
    NotReady,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioBusFault {
    Nack,
    Bus,
    Timeout,
    Invalid,
    Unrecovered,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDacFault {
    Bus { address: u8, register: u32, error: AudioBusFault },
    Readback { address: u8, register: u32, expected: u8, actual: u8 },
    Identity { address: u8, bytes: [u8; 3] },
    IdentityChanged { address: u8, bytes: [u8; 3] },
    NegativeControlAcknowledged,
    NegativeControl(AudioBusFault),
    TimeWentBackwards,
    Stopped,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioDmaFault {
    Buffer,
    StopTimeout,
    ResetStuck,
    StartFailed,
    Cursor,
    NotRunning,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioFault {
    Clock(AudioClockFault),
    Setup { address: u32, expected: u32, actual: u32 },
    Dac(AudioDacFault),
    Dma(AudioDmaFault),
    TimeWentBackwards,
    NoDac,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioFailure {
    pub phase: AudioPhase,
    pub fault: AudioFault,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct AudioCalibration {
    pub system_clock: u32,
    pub core_pll: u32,
    pub decoded_cpu_hz: Option<u32>,
    pub frame_clock_hz: Option<u32>,
    pub timers_quiesced: bool,
    pub fault: Option<AudioClockFault>,
    pub trims: [u8; 2],
    pub windows: [u16; 2],
    pub counts: Vec<[u32; 2], AUDIO_SWEEP_COUNTS>,
    pub cycles: u32,
    pub pll: [u32; 3],
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioCursorMap {
    pub remaining: u32,
    pub status: u32,
    pub enable: u32,
    pub i2s_status: u32,
    pub monitor: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStream {
    pub start: AudioStart,
    pub amplitude: i32,
    pub ring_address: u32,
    pub ring_bytes: u32,
    pub periods: u32,
    pub started_us: Option<u32>,
    pub dacs: u8,
    pub cursor: AudioCursorMap,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioHalt {
    pub cycles: u32,
    pub enable: u32,
    pub remaining: [u32; 2],
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct AudioBench {
    pub state: AudioBenchState,
    pub cpu_hz: Option<u32>,
    pub calibration: Option<AudioCalibration>,
    pub stream: Option<AudioStream>,
    pub failure: Option<AudioFailure>,
    pub halt: Option<AudioHalt>,
}

impl AudioStart {
    pub const MIN_LEVEL_CENTI_DBFS: i16 = -9_600;
    pub const MIN_TONE_HZ: u32 = 20;
    pub const MAX_TONE_HZ: u32 = 20_000;

    pub fn is_valid(&self) -> bool {
        (Self::MIN_LEVEL_CENTI_DBFS..=0).contains(&self.level_centi_dbfs)
            && (self.tone_hz == 0
                || (Self::MIN_TONE_HZ..=Self::MAX_TONE_HZ).contains(&self.tone_hz) && 2 * self.tone_hz < self.rate_hz)
    }
}

impl AudioChannel {
    pub const fn slots(self) -> [bool; 2] {
        match self {
            Self::Left => [true, false],
            Self::Right => [false, true],
            Self::Both => [true, true],
        }
    }
}

impl AudioBenchRequest {
    pub fn is_valid(&self) -> bool {
        match self {
            Self::Start(start) => start.is_valid(),
            Self::Calibrate | Self::Stop | Self::Status => true,
        }
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::Vec;
use postcard_schema::Schema;
use serde::{Deserialize, Serialize};

use crate::bench::AudioChannel;

pub const EQ_SLOTS: u8 = 4;
pub const EQ_MAX_BANDS: usize = 10;
pub const METER_FLOOR_CENTI_DB: i16 = -9_600;

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum Jack {
    Empty,
    Headphones,
    Headset,
    Unknown,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Pcm,
    Dop,
    NativeDsd,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamFormat {
    pub kind: StreamKind,
    pub rate_hz: u32,
    pub bits: u8,
    pub channels: u8,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AudioCounters {
    pub underruns: u32,
    pub overruns: u32,
    pub feedback_corrections: u32,
    pub dac_faults: u32,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioStatus {
    pub jack: Jack,
    pub usb_audio_present: bool,
    pub stream: Option<StreamFormat>,
    pub host_volume_centi_db: [i16; 2],
    pub host_muted: bool,
    pub eq_active: Option<u8>,
    pub counters: AudioCounters,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum DacFilter {
    FastLowLatency,
    FastPhaseCompensated,
    SlowLowLatency,
    SlowPhaseCompensated,
    NonOversampling,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutputSettings {
    pub filter: DacFilter,
    pub volume_limit_centi_db: i16,
    pub balance_centi_db: i16,
    pub dsd_direct: bool,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct EqBand {
    pub kind: FilterKind,
    pub frequency_hz: u32,
    pub q_milli: u16,
    pub gain_centi_db: i16,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct EqProfile {
    pub preamp_centi_db: i16,
    pub bands: Vec<EqBand, EQ_MAX_BANDS>,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, PartialEq, Eq)]
pub struct EqSlotUpdate {
    pub slot: u8,
    pub profile: EqProfile,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct MicSettings {
    pub gain_centi_db: i16,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CrossfeedMode {
    #[default]
    Off,
    Subtle,
    Natural,
    Strong,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct DspSettings {
    pub crossfeed: CrossfeedMode,
    pub limiter: bool,
    pub mono: bool,
    pub invert_phase: bool,
    pub loudness: bool,
    pub swap_channels: bool,
}

impl DspSettings {
    pub const fn is_active(&self) -> bool {
        !matches!(self.crossfeed, CrossfeedMode::Off)
            || self.limiter
            || self.mono
            || self.invert_phase
            || self.loudness
            || self.swap_channels
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClassHMode {
    #[default]
    AdaptiveHv,
    LowPowerLv,
    FixedHv,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct DacExtras {
    pub high_pass_filter: bool,
    pub de_emphasis: bool,
    pub line_out: bool,
    pub class_h: ClassHMode,
    pub auto_uac_on_jack: bool,
    pub stay_awake_while_active: bool,
}

impl Default for DacExtras {
    fn default() -> Self {
        Self {
            high_pass_filter: true,
            de_emphasis: false,
            line_out: false,
            class_h: ClassHMode::AdaptiveHv,
            auto_uac_on_jack: true,
            stay_awake_while_active: false,
        }
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImpedanceReport {
    pub left_ohms: Option<u16>,
    pub right_ohms: Option<u16>,
    pub left_ac_ohms: Option<u16>,
    pub right_ac_ohms: Option<u16>,
    pub left_status: u8,
    pub right_status: u8,
    pub hp_status: u8,
    pub load_stat: u8,
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AudioMeters {
    pub active: bool,
    pub peak_left_centi_db: i16,
    pub peak_right_centi_db: i16,
    pub rms_left_centi_db: i16,
    pub rms_right_centi_db: i16,
    pub clipped_samples: u32,
    pub limiter_hits: u32,
}

impl Default for AudioMeters {
    fn default() -> Self {
        Self {
            active: false,
            peak_left_centi_db: METER_FLOOR_CENTI_DB,
            peak_right_centi_db: METER_FLOOR_CENTI_DB,
            rms_left_centi_db: METER_FLOOR_CENTI_DB,
            rms_right_centi_db: METER_FLOOR_CENTI_DB,
            clipped_samples: 0,
            limiter_hits: 0,
        }
    }
}

#[derive(Serialize, Deserialize, Schema, Debug, Clone, Copy, PartialEq, Eq)]
pub struct TestToneRequest {
    pub channel: AudioChannel,
    pub frequency_hz: u16,
    pub duration_ms: u16,
    pub invert_right: bool,
}

impl TestToneRequest {
    pub const MIN_HZ: u16 = 100;
    pub const MAX_HZ: u16 = 12_000;
    pub const MIN_MS: u16 = 100;
    pub const MAX_MS: u16 = 5_000;

    pub fn is_valid(&self) -> bool {
        self.frequency_hz == 0
            || ((Self::MIN_HZ..=Self::MAX_HZ).contains(&self.frequency_hz)
                && (Self::MIN_MS..=Self::MAX_MS).contains(&self.duration_ms))
    }
}

impl EqBand {
    pub const MIN_HZ: u32 = 10;
    pub const MAX_HZ: u32 = 40_000;
    pub const MIN_Q_MILLI: u16 = 100;
    pub const MAX_Q_MILLI: u16 = 20_000;
    pub const MAX_GAIN_CENTI_DB: i16 = 1_800;

    pub fn is_valid(&self) -> bool {
        (Self::MIN_HZ..=Self::MAX_HZ).contains(&self.frequency_hz)
            && (Self::MIN_Q_MILLI..=Self::MAX_Q_MILLI).contains(&self.q_milli)
            && self.gain_centi_db.unsigned_abs() <= Self::MAX_GAIN_CENTI_DB as u16
    }
}

impl EqProfile {
    pub const MIN_PREAMP_CENTI_DB: i16 = -2_400;

    pub fn is_valid(&self) -> bool {
        (Self::MIN_PREAMP_CENTI_DB..=0).contains(&self.preamp_centi_db) && self.bands.iter().all(EqBand::is_valid)
    }
}

impl EqSlotUpdate {
    pub fn is_valid(&self) -> bool {
        self.slot < EQ_SLOTS && self.profile.is_valid()
    }
}

impl OutputSettings {
    pub const MIN_VOLUME_LIMIT_CENTI_DB: i16 = -6_000;
    pub const MAX_BALANCE_CENTI_DB: i16 = 600;

    pub fn is_valid(&self) -> bool {
        (Self::MIN_VOLUME_LIMIT_CENTI_DB..=0).contains(&self.volume_limit_centi_db)
            && self.balance_centi_db.unsigned_abs() <= Self::MAX_BALANCE_CENTI_DB as u16
    }
}

impl MicSettings {
    pub const MAX_GAIN_CENTI_DB: i16 = 3_000;

    pub fn is_valid(&self) -> bool {
        (0..=Self::MAX_GAIN_CENTI_DB).contains(&self.gain_centi_db)
    }
}

pub fn eq_selection_is_valid(selection: crate::EqSelection) -> bool {
    selection.is_none_or(|slot| slot < EQ_SLOTS)
}

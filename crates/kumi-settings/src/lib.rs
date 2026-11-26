// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

use embedded_storage::nor_flash::NorFlash;
use heapless::Vec;
use kumi_icd::{
    COMBO_SLOTS, ClassHMode, CrossfeedMode, DacExtras, DacFilter, DspSettings, EQ_SLOTS, EqProfile, KeyCombo, KeyConfig,
    KeymapEntry, MACRO_SLOTS, MacroProgram, MicSettings, OutputSettings,
};
use kumi_layout::{SECTOR, SETTINGS, crc32_update, get32};
use serde::{Deserialize, Serialize};

pub const FORMAT: u8 = 5;
pub const KEYMAP_MAX: usize = 96;
pub const MAX_RECORD: usize = 2048;

const MAGIC: [u8; 4] = *b"KSET";
const HEADER: usize = 20;
const HALF: u32 = SETTINGS.size / 2;

#[derive(Deserialize)]
struct DspSettingsV2 {
    crossfeed: CrossfeedMode,
    limiter: bool,
    mono: bool,
    invert_phase: bool,
}

impl From<DspSettingsV2> for DspSettings {
    fn from(v: DspSettingsV2) -> Self {
        Self {
            crossfeed: v.crossfeed,
            limiter: v.limiter,
            mono: v.mono,
            invert_phase: v.invert_phase,
            loudness: false,
            swap_channels: false,
        }
    }
}

#[derive(Deserialize)]
struct DacExtrasV4 {
    high_pass_filter: bool,
    de_emphasis: bool,
    line_out: bool,
    class_h: ClassHMode,
    auto_uac_on_jack: bool,
}

impl From<DacExtrasV4> for DacExtras {
    fn from(v: DacExtrasV4) -> Self {
        Self {
            high_pass_filter: v.high_pass_filter,
            de_emphasis: v.de_emphasis,
            line_out: v.line_out,
            class_h: v.class_h,
            auto_uac_on_jack: v.auto_uac_on_jack,
            stay_awake_while_active: false,
        }
    }
}

#[derive(Deserialize)]
struct KeyConfigV3 {
    tapping_term_ms: u16,
    one_shot_timeout_ms: u16,
}

impl From<KeyConfigV3> for KeyConfig {
    fn from(v: KeyConfigV3) -> Self {
        Self {
            tapping_term_ms: v.tapping_term_ms,
            one_shot_timeout_ms: v.one_shot_timeout_ms,
            combo_term_ms: KeyConfig::default().combo_term_ms,
        }
    }
}

#[derive(Deserialize)]
struct StoredSettingsV1 {
    output: OutputSettings,
    mic: MicSettings,
    active_eq: Option<u8>,
    eq_slots: [Option<EqProfile>; EQ_SLOTS as usize],
    keymap: Vec<KeymapEntry, KEYMAP_MAX>,
}

#[derive(Deserialize)]
struct StoredSettingsV2 {
    output: OutputSettings,
    mic: MicSettings,
    active_eq: Option<u8>,
    eq_slots: [Option<EqProfile>; EQ_SLOTS as usize],
    keymap: Vec<KeymapEntry, KEYMAP_MAX>,
    dsp: DspSettingsV2,
}

#[derive(Deserialize)]
struct StoredSettingsV3 {
    output: OutputSettings,
    mic: MicSettings,
    active_eq: Option<u8>,
    eq_slots: [Option<EqProfile>; EQ_SLOTS as usize],
    keymap: Vec<KeymapEntry, KEYMAP_MAX>,
    dsp: DspSettingsV2,
    dac_extras: DacExtrasV4,
    key_config: KeyConfigV3,
    macros: [Option<MacroProgram>; MACRO_SLOTS as usize],
}

#[derive(Deserialize)]
struct StoredSettingsV4 {
    output: OutputSettings,
    mic: MicSettings,
    active_eq: Option<u8>,
    eq_slots: [Option<EqProfile>; EQ_SLOTS as usize],
    keymap: Vec<KeymapEntry, KEYMAP_MAX>,
    dsp: DspSettings,
    dac_extras: DacExtrasV4,
    key_config: KeyConfig,
    macros: [Option<MacroProgram>; MACRO_SLOTS as usize],
    combos: [Option<KeyCombo>; COMBO_SLOTS as usize],
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq)]
pub struct StoredSettings {
    pub output: OutputSettings,
    pub mic: MicSettings,
    pub active_eq: Option<u8>,
    pub eq_slots: [Option<EqProfile>; EQ_SLOTS as usize],
    pub keymap: Vec<KeymapEntry, KEYMAP_MAX>,
    pub dsp: DspSettings,
    pub dac_extras: DacExtras,
    pub key_config: KeyConfig,
    pub macros: [Option<MacroProgram>; MACRO_SLOTS as usize],
    pub combos: [Option<KeyCombo>; COMBO_SLOTS as usize],
}

impl Default for StoredSettings {
    fn default() -> Self {
        Self {
            output: OutputSettings {
                filter: DacFilter::FastLowLatency,
                volume_limit_centi_db: 0,
                balance_centi_db: 0,
                dsd_direct: false,
            },
            mic: MicSettings { gain_centi_db: 0 },
            active_eq: None,
            eq_slots: [None, None, None, None],
            keymap: Vec::new(),
            dsp: DspSettings::default(),
            dac_extras: DacExtras::default(),
            key_config: KeyConfig::default(),
            macros: core::array::from_fn(|_| None),
            combos: [None; COMBO_SLOTS as usize],
        }
    }
}

impl StoredSettings {
    fn from_v1(v1: StoredSettingsV1) -> Self {
        Self {
            output: v1.output,
            mic: v1.mic,
            active_eq: v1.active_eq,
            eq_slots: v1.eq_slots,
            keymap: v1.keymap,
            dsp: DspSettings::default(),
            dac_extras: DacExtras::default(),
            key_config: KeyConfig::default(),
            macros: core::array::from_fn(|_| None),
            combos: [None; COMBO_SLOTS as usize],
        }
    }

    fn from_v2(v2: StoredSettingsV2) -> Self {
        Self {
            output: v2.output,
            mic: v2.mic,
            active_eq: v2.active_eq,
            eq_slots: v2.eq_slots,
            keymap: v2.keymap,
            dsp: v2.dsp.into(),
            dac_extras: DacExtras::default(),
            key_config: KeyConfig::default(),
            macros: core::array::from_fn(|_| None),
            combos: [None; COMBO_SLOTS as usize],
        }
    }

    fn from_v3(v3: StoredSettingsV3) -> Self {
        Self {
            output: v3.output,
            mic: v3.mic,
            active_eq: v3.active_eq,
            eq_slots: v3.eq_slots,
            keymap: v3.keymap,
            dsp: v3.dsp.into(),
            dac_extras: v3.dac_extras.into(),
            key_config: v3.key_config.into(),
            macros: v3.macros,
            combos: [None; COMBO_SLOTS as usize],
        }
    }

    fn from_v4(v4: StoredSettingsV4) -> Self {
        Self {
            output: v4.output,
            mic: v4.mic,
            active_eq: v4.active_eq,
            eq_slots: v4.eq_slots,
            keymap: v4.keymap,
            dsp: v4.dsp,
            dac_extras: v4.dac_extras.into(),
            key_config: v4.key_config,
            macros: v4.macros,
            combos: v4.combos,
        }
    }
}

#[derive(Debug)]
pub enum Error<E> {
    Region,
    Encode,
    Readback,
    Flash(E),
}

pub struct Store<F: NorFlash> {
    flash: F,
    base: u32,
}

pub fn encode(settings: &StoredSettings, out: &mut [u8]) -> Option<usize> {
    postcard::to_slice(settings, out).ok().map(|used| used.len())
}

pub fn decode(format: u8, payload: &[u8]) -> Option<StoredSettings> {
    match format {
        1 => postcard::take_from_bytes::<StoredSettingsV1>(payload)
            .ok()
            .and_then(|(v, rest)| rest.is_empty().then(|| StoredSettings::from_v1(v))),
        2 => postcard::take_from_bytes::<StoredSettingsV2>(payload)
            .ok()
            .and_then(|(v, rest)| rest.is_empty().then(|| StoredSettings::from_v2(v))),
        3 => postcard::take_from_bytes::<StoredSettingsV3>(payload)
            .ok()
            .and_then(|(v, rest)| rest.is_empty().then(|| StoredSettings::from_v3(v))),
        4 => postcard::take_from_bytes::<StoredSettingsV4>(payload)
            .ok()
            .and_then(|(v, rest)| rest.is_empty().then(|| StoredSettings::from_v4(v))),
        5 => postcard::take_from_bytes::<StoredSettings>(payload).ok().and_then(|(v, rest)| rest.is_empty().then_some(v)),
        _ => None,
    }
}

fn record_crc(seq_len: &[u8], payload: &[u8]) -> u32 {
    crc32_update(crc32_update(0xFFFF_FFFF, seq_len), payload) ^ 0xFFFF_FFFF
}

impl<F: NorFlash> Store<F> {
    pub fn new(flash: F) -> Result<Self, Error<F::Error>> {
        if SETTINGS.size != 2 * HALF || !HALF.is_multiple_of(SECTOR) || F::ERASE_SIZE as u32 != SECTOR {
            return Err(Error::Region);
        }
        Ok(Self { flash, base: SETTINGS.offset })
    }

    pub fn into_inner(self) -> F {
        self.flash
    }

    fn half(&self, index: usize) -> u32 {
        self.base + index as u32 * HALF
    }

    fn read_half(&mut self, index: usize, buf: &mut [u8; MAX_RECORD]) -> Result<Option<(u32, StoredSettings)>, F::Error> {
        let mut head = [0u8; HEADER];
        self.flash.read(self.half(index), &mut head)?;
        if head[0..4] != MAGIC {
            return Ok(None);
        }
        let seq = get32(&head, 8);
        let len = get32(&head, 12) as usize;
        if len == 0 || len > MAX_RECORD {
            return Ok(None);
        }
        self.flash.read(self.half(index) + HEADER as u32, &mut buf[..len])?;
        if record_crc(&head[8..16], &buf[..len]) != get32(&head, 16) {
            return Ok(None);
        }
        Ok(decode(head[4], &buf[..len]).map(|settings| (seq, settings)))
    }

    pub fn load(&mut self) -> StoredSettings {
        let mut buf = [0u8; MAX_RECORD];
        let a = self.read_half(0, &mut buf).ok().flatten();
        let b = self.read_half(1, &mut buf).ok().flatten();
        match (a, b) {
            (Some((sa, va)), Some((sb, vb))) => {
                if sb.wrapping_sub(sa) < 0x8000_0000 && sb != sa {
                    vb
                } else {
                    va
                }
            }
            (Some((_, va)), None) => va,
            (None, Some((_, vb))) => vb,
            (None, None) => StoredSettings::default(),
        }
    }

    fn read_half_seq(&mut self, index: usize, record: &mut [u8; HEADER + MAX_RECORD]) -> Result<Option<u32>, F::Error> {
        self.flash.read(self.half(index), &mut record[..HEADER])?;
        if record[0..4] != MAGIC {
            return Ok(None);
        }
        let format = record[4];
        if format == 0 || format > FORMAT {
            return Ok(None);
        }
        let seq = get32(&record[..HEADER], 8);
        let len = get32(&record[..HEADER], 12) as usize;
        if len == 0 || len > MAX_RECORD {
            return Ok(None);
        }
        self.flash.read(self.half(index) + HEADER as u32, &mut record[HEADER..HEADER + len])?;
        if record_crc(&record[8..16], &record[HEADER..HEADER + len]) != get32(&record[..HEADER], 16) {
            return Ok(None);
        }
        Ok(Some(seq))
    }

    pub fn save(&mut self, settings: &StoredSettings) -> Result<(), Error<F::Error>> {
        let mut record = [0u8; HEADER + MAX_RECORD];
        let seq_a = self.read_half_seq(0, &mut record).map_err(Error::Flash)?;
        let seq_b = self.read_half_seq(1, &mut record).map_err(Error::Flash)?;
        let (target, seq) = match (seq_a, seq_b) {
            (Some(sa), Some(sb)) if sb.wrapping_sub(sa) < 0x8000_0000 && sb != sa => (0usize, sb.wrapping_add(1)),
            (Some(sa), Some(_)) => (1usize, sa.wrapping_add(1)),
            (Some(sa), None) => (1usize, sa.wrapping_add(1)),
            (None, Some(sb)) => (0usize, sb.wrapping_add(1)),
            (None, None) => (0usize, 1u32),
        };

        let used = encode(settings, &mut record[HEADER..]).ok_or(Error::Encode)?;
        record[..HEADER].fill(0);
        record[0..4].copy_from_slice(&MAGIC);
        record[4] = FORMAT;
        record[8..12].copy_from_slice(&seq.to_le_bytes());
        record[12..16].copy_from_slice(&(used as u32).to_le_bytes());
        let crc = record_crc(&record[8..16], &record[HEADER..HEADER + used]);
        record[16..20].copy_from_slice(&crc.to_le_bytes());
        let total = HEADER + used;

        let at = self.half(target);
        let body = MAGIC.len();
        self.flash.erase(at, at + SECTOR).map_err(Error::Flash)?;
        self.flash.write(at + body as u32, &record[body..total]).map_err(Error::Flash)?;
        self.verify(at, &record[..total], body)?;
        self.flash.write(at, &record[..body]).map_err(Error::Flash)?;
        self.verify(at, &record[..body], 0)
    }

    fn verify(&mut self, at: u32, record: &[u8], from: usize) -> Result<(), Error<F::Error>> {
        let mut done = from;
        let mut chunk = [0u8; 64];
        while done < record.len() {
            let n = chunk.len().min(record.len() - done);
            self.flash.read(at + done as u32, &mut chunk[..n]).map_err(Error::Flash)?;
            if chunk[..n] != record[done..done + n] {
                return Err(Error::Readback);
            }
            done += n;
        }
        Ok(())
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use heapless::Vec;
use kumi_icd::{
    COMBO_SLOTS, ClassHMode, CrossfeedMode, DacExtras, DacFilter, DspSettings, EqBand, EqProfile, FilterKind, KeyAction,
    KeyCombo, KeyConfig, KeymapEntry, MACRO_SLOTS, MacroProgram, MacroStep, MicSettings, OutputSettings,
};
use kumi_layout::{SECTOR, SETTINGS};
use kumi_settings::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cut;

impl NorFlashError for Cut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

struct Flash {
    data: Vec<u8, { SETTINGS.size as usize }>,
    budget: Option<usize>,
    failing_reads: bool,
}

impl Flash {
    fn new() -> Self {
        let mut data = Vec::new();
        data.resize(SETTINGS.size as usize, 0xFF).unwrap();
        Self { data, budget: None, failing_reads: false }
    }

    fn spend(&mut self, bytes: usize) -> usize {
        match self.budget {
            None => bytes,
            Some(left) => {
                let spent = left.min(bytes);
                self.budget = Some(left - spent);
                spent
            }
        }
    }

    fn at(&mut self, offset: u32) -> usize {
        (offset - SETTINGS.offset) as usize
    }
}

impl ErrorType for Flash {
    type Error = Cut;
}

impl ReadNorFlash for Flash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        if self.failing_reads {
            return Err(Cut);
        }
        let start = self.at(offset);
        bytes.copy_from_slice(&self.data[start..start + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl NorFlash for Flash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Cut> {
        let start = self.at(from);
        let sectors = ((to - from) / SECTOR) as usize;
        let spent = self.spend(sectors);
        for byte in self.data[start..start + spent * SECTOR as usize].iter_mut() {
            *byte = 0xFF;
        }
        if spent < sectors { Err(Cut) } else { Ok(()) }
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Cut> {
        let start = self.at(offset);
        let spent = self.spend(bytes.len());
        for (i, b) in bytes[..spent].iter().enumerate() {
            self.data[start + i] &= *b;
        }
        if spent < bytes.len() { Err(Cut) } else { Ok(()) }
    }
}

fn profile(freq: u32, gain: i16) -> EqProfile {
    let mut bands = Vec::new();
    bands.push(EqBand { kind: FilterKind::Peak, frequency_hz: freq, q_milli: 1_000, gain_centi_db: gain }).unwrap();
    bands.push(EqBand { kind: FilterKind::HighShelf, frequency_hz: 10_000, q_milli: 707, gain_centi_db: -200 }).unwrap();
    EqProfile { preamp_centi_db: -150, bands }
}

fn sample() -> StoredSettings {
    let eq_slots = [Some(profile(60, 300)), None, Some(profile(2_000, -400)), None];
    let mut keymap = Vec::new();
    keymap.push(KeymapEntry { layer: 0, source: 4, action: KeyAction::Consumer(0xE9) }).unwrap();
    keymap.push(KeymapEntry { layer: 1, source: 5, action: KeyAction::LayerHold(2) }).unwrap();
    let mut steps = Vec::new();
    steps.push(MacroStep { usage: 4, modifiers: 2 }).unwrap();
    let mut macros = [const { None }; MACRO_SLOTS as usize];
    macros[0] = Some(MacroProgram { steps });
    let mut combos = [None; COMBO_SLOTS as usize];
    combos[0] = Some(KeyCombo { key_a: 0x0d, key_b: 0x0e, action: KeyAction::Key(0x29) });
    StoredSettings {
        output: OutputSettings {
            filter: DacFilter::SlowPhaseCompensated,
            volume_limit_centi_db: -600,
            balance_centi_db: 50,
            dsd_direct: true,
        },
        mic: MicSettings { gain_centi_db: 1_200 },
        active_eq: Some(1),
        eq_slots,
        keymap,
        dsp: DspSettings {
            crossfeed: CrossfeedMode::Natural,
            limiter: true,
            mono: false,
            invert_phase: true,
            loudness: true,
            swap_channels: false,
        },
        dac_extras: DacExtras {
            high_pass_filter: true,
            de_emphasis: false,
            line_out: false,
            class_h: ClassHMode::LowPowerLv,
            auto_uac_on_jack: true,
            stay_awake_while_active: true,
        },
        key_config: KeyConfig { tapping_term_ms: 180, one_shot_timeout_ms: 2_500, combo_term_ms: 45 },
        macros,
        combos,
    }
}

fn store(flash: Flash) -> Store<Flash> {
    Store::new(flash).unwrap()
}

#[test]
fn blank_flash_reads_as_defaults() {
    let mut s = store(Flash::new());
    assert_eq!(s.load(), StoredSettings::default());
}

#[test]
fn a_saved_profile_round_trips() {
    let mut s = store(Flash::new());
    let wanted = sample();
    s.save(&wanted).unwrap();
    assert_eq!(s.load(), wanted);
}

#[test]
fn the_latest_save_wins_and_the_halves_ping_pong() {
    let mut s = store(Flash::new());
    let mut last = StoredSettings::default();
    for volume in [-100i16, -200, -300, -400, -500] {
        let mut next = sample();
        next.output.volume_limit_centi_db = volume;
        s.save(&next).unwrap();
        assert_eq!(s.load().output.volume_limit_centi_db, volume);
        last = next;
    }
    let flash = s.into_inner();
    let a_used = flash.data[0..4] == *b"KSET";
    let b_used = flash.data[(SETTINGS.size / 2) as usize..][0..4] == *b"KSET";
    assert!(a_used && b_used, "five saves must have used both halves");
    assert_eq!(store(flash).load(), last);
}

#[test]
fn a_power_cut_during_save_keeps_the_previous_settings() {
    let mut first = sample();
    first.output.volume_limit_centi_db = -111;
    let mut second = sample();
    second.output.volume_limit_centi_db = -999;
    second.active_eq = Some(0);

    let mut cut_hit = 0;
    let mut finished = false;
    for budget in 0..4096usize {
        let mut flash = Flash::new();
        let mut s = store(flash);
        s.save(&first).unwrap();
        flash = s.into_inner();
        flash.budget = Some(budget);
        let mut s = store(flash);
        let cut = s.save(&second).is_err();
        flash = s.into_inner();
        flash.budget = None;
        let got = store(flash).load();
        if cut {
            cut_hit += 1;
            assert_eq!(got, first, "a cut at budget {budget} must leave the first settings intact");
        } else {
            assert_eq!(got, second);
            finished = true;
            break;
        }
    }
    assert!(finished, "the sweep must reach a completed save");
    assert!(cut_hit > 16 + 20, "the sweep must cut inside the erase and inside the record program");
}

#[test]
fn a_read_failure_aborts_the_save_before_anything_is_erased() {
    let mut first = sample();
    first.output.volume_limit_centi_db = -321;
    let mut s = store(Flash::new());
    s.save(&first).unwrap();
    let mut flash = s.into_inner();
    let before = flash.data.clone();
    flash.failing_reads = true;
    let mut s = store(flash);
    assert!(matches!(s.save(&sample()), Err(Error::Flash(Cut))));
    let mut flash = s.into_inner();
    assert_eq!(flash.data, before);
    flash.failing_reads = false;
    assert_eq!(store(flash).load(), first);
}

#[test]
fn the_magic_is_written_last_so_an_unfinished_record_is_never_read() {
    let mut s = store(Flash::new());
    s.save(&sample()).unwrap();
    let flash = s.into_inner();
    let mut torn = Flash::new();
    torn.data[4..64].copy_from_slice(&flash.data[4..64]);
    assert_eq!(store(torn).load(), StoredSettings::default());
}

#[test]
fn corruption_in_the_active_half_falls_back() {
    let mut flash = Flash::new();
    let mut s = store(flash);
    let mut older = sample();
    older.output.volume_limit_centi_db = -100;
    s.save(&older).unwrap();
    let mut newer = sample();
    newer.output.volume_limit_centi_db = -200;
    s.save(&newer).unwrap();
    flash = s.into_inner();

    let half1 = (SETTINGS.size / 2) as usize;
    let seq = |d: &[u8], o: usize| u32::from_le_bytes([d[o], d[o + 1], d[o + 2], d[o + 3]]);
    let newer_half = if seq(flash.data.as_slice(), half1 + 8) > seq(flash.data.as_slice(), 8) { half1 } else { 0 };
    flash.data[newer_half + 24] ^= 0xFF;
    assert_eq!(store(flash).load().output.volume_limit_centi_db, -100);
}

fn hex(bytes: &[u8]) -> std::string::String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn from_hex(text: &str) -> std::vec::Vec<u8> {
    (0..text.len()).step_by(2).map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap()).collect()
}

#[test]
fn the_format_one_record_layout_is_pinned() {
    let v1_sample = from_hex(
        "03af096401e012010101ab0202003ce807d80402904ec3058f030001ab020200d00fe8079f0602904ec3058f030002000405e90101050602",
    );
    let v2_sample = from_hex(
        "03af096401e012010101ab0202003ce807d80402904ec3058f030001ab020200d00fe8079f0602904ec3058f030002000405e9010105060202010001",
    );
    let v3_sample = from_hex(
        "03af096401e012010101ab0202003ce807d80402904ec3058f030001ab020200d00fe8079f0602904ec3058f030002000405e90101050602020100010100000101b401c4130101040200000000000000",
    );
    let expected_v1 = StoredSettings {
        dsp: DspSettings::default(),
        dac_extras: DacExtras::default(),
        key_config: KeyConfig::default(),
        macros: [const { None }; MACRO_SLOTS as usize],
        combos: [None; COMBO_SLOTS as usize],
        ..sample()
    };
    let expected_v2 = StoredSettings {
        dsp: DspSettings { loudness: false, swap_channels: false, ..sample().dsp },
        dac_extras: DacExtras::default(),
        key_config: KeyConfig::default(),
        macros: [const { None }; MACRO_SLOTS as usize],
        combos: [None; COMBO_SLOTS as usize],
        ..sample()
    };
    let expected_v3 = StoredSettings {
        dsp: DspSettings { loudness: false, swap_channels: false, ..sample().dsp },
        dac_extras: DacExtras { stay_awake_while_active: false, ..sample().dac_extras },
        key_config: KeyConfig { combo_term_ms: KeyConfig::default().combo_term_ms, ..sample().key_config },
        combos: [None; COMBO_SLOTS as usize],
        ..sample()
    };
    assert_eq!(decode(1, &v1_sample), Some(expected_v1));
    assert_eq!(decode(2, &v2_sample), Some(expected_v2));
    assert_eq!(decode(3, &v3_sample), Some(expected_v3));
    let v4_default = from_hex("00000000000000000000000000000000000100000001c801b8173200000000000000000000000000000000");
    assert_eq!(decode(4, &v4_default), Some(StoredSettings::default()));
    assert_eq!(decode(1, &from_hex("0000000000000000000000")), Some(StoredSettings::default()));
    assert_eq!(decode(2, &from_hex("000000000000000000000000000000")), Some(StoredSettings::default()));

    let mut out = [0u8; MAX_RECORD];
    let used = encode(&sample(), &mut out).unwrap();
    assert_eq!(decode(FORMAT, &out[..used]), Some(sample()));
    assert_eq!(decode(FORMAT + 1, &out[..used]), None);
    let used = encode(&StoredSettings::default(), &mut out).unwrap();
    assert_eq!(decode(FORMAT, &out[..used]), Some(StoredSettings::default()));
    assert!(!hex(&out[..used]).is_empty());
}

#[test]
fn a_record_in_an_unknown_format_is_skipped_and_never_read_as_this_format() {
    let mut s = store(Flash::new());
    let mut older = sample();
    older.output.volume_limit_centi_db = -100;
    s.save(&older).unwrap();
    let mut newer = sample();
    newer.output.volume_limit_centi_db = -200;
    s.save(&newer).unwrap();
    let mut flash = s.into_inner();
    let half1 = (SETTINGS.size / 2) as usize;
    flash.data[half1 + 4] = FORMAT + 1;
    let mut s = store(flash);
    assert_eq!(s.load().output.volume_limit_centi_db, -100);
    let mut third = sample();
    third.output.volume_limit_centi_db = -300;
    s.save(&third).unwrap();
    assert_eq!(s.load(), third);
}

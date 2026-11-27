// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

mod analyze;

use core::f64::consts::PI;

use analyze::Analyzer;
pub use analyze::BAND_CENTERS_HZ;
use kumi_icd::{
    AudioMeters, AudioSpectrum, CrossfeedMode, DspExtras, DspSettings, EqBand, EqBaseSettings, EqProfile, FilterKind,
    METER_FLOOR_CENTI_DB, ToneEqSettings,
};

pub const MAX_BANDS: usize = kumi_icd::EQ_MAX_BANDS;
pub const MIN_SAMPLE_RATE: u32 = 8_000;
pub const MAX_SAMPLE_RATE: u32 = 768_000;
pub const MAX_GAIN_DB: f32 = 18.0;
pub const MIN_Q: f32 = 0.1;
pub const MAX_Q: f32 = 20.0;
pub const BYPASS_ABOVE_NYQUIST_FRACTION: f64 = 0.49;

const PEAK_GRID: usize = 768;
const REFINE_STEPS: usize = 48;
const STATE_FLOOR: f32 = 1e-20;
const CHUNK: usize = 128;

#[inline(always)]
fn mul_add(a: f32, b: f32, c: f32) -> f32 {
    #[cfg(all(target_arch = "riscv32", feature = "rv32f"))]
    unsafe {
        let out: f32;
        core::arch::asm!(
            "fmadd.s {out}, {a}, {b}, {c}",
            out = lateout(freg) out,
            a = in(freg) a,
            b = in(freg) b,
            c = in(freg) c,
            options(pure, nomem, nostack),
        );
        out
    }
    #[cfg(not(all(target_arch = "riscv32", feature = "rv32f")))]
    {
        a * b + c
    }
}

#[inline(always)]
fn mul_sub(a: f32, b: f32, c: f32) -> f32 {
    #[cfg(all(target_arch = "riscv32", feature = "rv32f"))]
    unsafe {
        let out: f32;
        core::arch::asm!(
            "fmsub.s {out}, {a}, {b}, {c}",
            out = lateout(freg) out,
            a = in(freg) a,
            b = in(freg) b,
            c = in(freg) c,
            options(pure, nomem, nostack),
        );
        out
    }
    #[cfg(not(all(target_arch = "riscv32", feature = "rv32f")))]
    {
        a * b - c
    }
}

#[inline(always)]
fn round_i32(value: f32) -> i32 {
    #[cfg(all(target_arch = "riscv32", feature = "rv32f"))]
    unsafe {
        let out: i32;
        core::arch::asm!(
            "fcvt.w.s {out}, {x}, rmm",
            out = out(reg) out,
            x = in(freg) value,
            options(pure, nomem, nostack),
        );
        out
    }
    #[cfg(not(all(target_arch = "riscv32", feature = "rv32f")))]
    {
        libm::roundf(value) as i32
    }
}

#[inline(always)]
fn quantize(sample: f32, scale: f32, clipped: &mut u32) -> i32 {
    let scaled = sample * scale;
    if !scaled.is_finite() {
        *clipped = clipped.wrapping_add(1);
        0
    } else if scaled >= scale {
        *clipped = clipped.wrapping_add(1);
        i32::MAX
    } else if scaled < -scale {
        *clipped = clipped.wrapping_add(1);
        i32::MIN
    } else {
        round_i32(scaled)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Peak,
    LowShelf,
    HighShelf,
    LowPass,
    HighPass,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Band {
    pub kind: Kind,
    pub frequency_hz: f32,
    pub q: f32,
    pub gain_db: f32,
}

impl From<FilterKind> for Kind {
    fn from(kind: FilterKind) -> Self {
        match kind {
            FilterKind::Peak => Kind::Peak,
            FilterKind::LowShelf => Kind::LowShelf,
            FilterKind::HighShelf => Kind::HighShelf,
            FilterKind::LowPass => Kind::LowPass,
            FilterKind::HighPass => Kind::HighPass,
        }
    }
}

impl From<&EqBand> for Band {
    fn from(band: &EqBand) -> Self {
        Band {
            kind: band.kind.into(),
            frequency_hz: band.frequency_hz as f32,
            q: f32::from(band.q_milli) / 1_000.0,
            gain_db: f32::from(band.gain_centi_db) / 100.0,
        }
    }
}

impl Band {
    fn is_flat(&self) -> bool {
        matches!(self.kind, Kind::Peak | Kind::LowShelf | Kind::HighShelf) && self.gain_db == 0.0
    }

    fn above_band_limit(&self, sample_rate: u32) -> bool {
        f64::from(self.frequency_hz) >= f64::from(sample_rate) * BYPASS_ABOVE_NYQUIST_FRACTION
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesignError {
    SampleRate,
    Frequency,
    Q,
    Gain,
    TooManyBands,
    Unstable,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Section {
    pub g: f32,
    pub k: f32,
    pub m0: f32,
    pub m1: f32,
    pub m2: f32,
    a1: f32,
    a2: f32,
    a3: f32,
}

#[derive(Clone, Copy)]
struct Phase {
    cos1: f64,
    cos2: f64,
    sin1: f64,
    sin2: f64,
}

impl Phase {
    fn at(frequency_hz: f64, sample_rate: f64) -> Self {
        let w = 2.0 * PI * frequency_hz / sample_rate;
        Self { cos1: libm::cos(w), cos2: libm::cos(2.0 * w), sin1: libm::sin(w), sin2: libm::sin(2.0 * w) }
    }

    fn power(&self, c: [f64; 3]) -> f64 {
        let re = c[0] + c[1] * self.cos1 + c[2] * self.cos2;
        let im = -(c[1] * self.sin1 + c[2] * self.sin2);
        re * re + im * im
    }
}

fn check(band: &Band, sample_rate: u32) -> Result<(), DesignError> {
    if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
        return Err(DesignError::SampleRate);
    }
    if !(band.frequency_hz > 0.0 && band.frequency_hz.is_finite()) {
        return Err(DesignError::Frequency);
    }
    if !(MIN_Q..=MAX_Q).contains(&band.q) {
        return Err(DesignError::Q);
    }
    if band.gain_db.is_nan() || band.gain_db.abs() > MAX_GAIN_DB {
        return Err(DesignError::Gain);
    }
    Ok(())
}

impl Section {
    pub fn design(band: &Band, sample_rate: u32) -> Result<Self, DesignError> {
        check(band, sample_rate)?;
        if band.above_band_limit(sample_rate) {
            return Err(DesignError::Frequency);
        }
        let fs = f64::from(sample_rate);
        let f0 = f64::from(band.frequency_hz);
        let q = f64::from(band.q);
        let a = libm::pow(10.0, f64::from(band.gain_db) / 40.0);
        let warped = libm::tan(PI * f0 / fs);
        let (g, k, m0, m1, m2) = match band.kind {
            Kind::LowPass => (warped, 1.0 / q, 0.0, 0.0, 1.0),
            Kind::HighPass => (warped, 1.0 / q, 1.0, -1.0 / q, -1.0),
            Kind::Peak => {
                let k = 1.0 / (q * a);
                (warped, k, 1.0, k * (a * a - 1.0), 0.0)
            }
            Kind::LowShelf => (warped / libm::sqrt(a), 1.0 / q, 1.0, (a - 1.0) / q, a * a - 1.0),
            Kind::HighShelf => (warped * libm::sqrt(a), 1.0 / q, a * a, (1.0 - a) * a / q, 1.0 - a * a),
        };
        let a1 = 1.0 / (1.0 + g * (g + k));
        let a2 = g * a1;
        let a3 = g * a2;
        let section = Self {
            g: g as f32,
            k: k as f32,
            m0: m0 as f32,
            m1: m1 as f32,
            m2: m2 as f32,
            a1: a1 as f32,
            a2: a2 as f32,
            a3: a3 as f32,
        };
        if section.g > 0.0 && section.k > 0.0 && section.a1.is_finite() { Ok(section) } else { Err(DesignError::Unstable) }
    }

    fn power(&self, phase: &Phase) -> f64 {
        let (g, k, m0, m1, m2) =
            (f64::from(self.g), f64::from(self.k), f64::from(self.m0), f64::from(self.m1), f64::from(self.m2));
        let (n2, n1, n0) = (m0, m0 * k + m1, m0 + m2);
        let b = [n2 + n1 * g + n0 * g * g, 2.0 * (n0 * g * g - n2), n2 - n1 * g + n0 * g * g];
        let a = [1.0 + k * g + g * g, 2.0 * (g * g - 1.0), 1.0 - k * g + g * g];
        phase.power(b) / phase.power(a)
    }

    pub fn magnitude(&self, frequency_hz: f32, sample_rate: u32) -> f32 {
        libm::sqrt(self.power(&Phase::at(f64::from(frequency_hz), f64::from(sample_rate)))) as f32
    }

    #[inline(always)]
    fn tick(&self, x: f32, state: &mut [f32; 2]) -> f32 {
        let v3 = x - state[1];
        let v1 = self.a1 * state[0] + self.a2 * v3;
        let v2 = state[1] + self.a2 * state[0] + self.a3 * v3;
        state[0] = 2.0 * v1 - state[0];
        state[1] = 2.0 * v2 - state[1];
        self.m0 * x + self.m1 * v1 + self.m2 * v2
    }
}

pub fn db_to_gain(db: f32) -> f32 {
    libm::powf(10.0, db / 20.0)
}

pub fn gain_to_db(gain: f32) -> f32 {
    20.0 * libm::log10f(gain)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    sections: [Option<Section>; MAX_BANDS],
    origins: [Option<(usize, Kind, f32)>; MAX_BANDS],
    count: usize,
}

impl Cascade {
    pub const EMPTY: Self = Self { sections: [None; MAX_BANDS], origins: [None; MAX_BANDS], count: 0 };

    pub fn design(bands: &[Band], sample_rate: u32) -> Result<Self, DesignError> {
        if bands.len() > MAX_BANDS {
            return Err(DesignError::TooManyBands);
        }
        let mut cascade = Self::EMPTY;
        for (index, band) in bands.iter().enumerate() {
            check(band, sample_rate)?;
            if band.is_flat() || band.above_band_limit(sample_rate) {
                continue;
            }
            cascade.sections[cascade.count] = Some(Section::design(band, sample_rate)?);
            cascade.origins[cascade.count] = Some((index, band.kind, band.frequency_hz));
            cascade.count += 1;
        }
        Ok(cascade)
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    pub fn sections(&self) -> impl Iterator<Item = &Section> {
        self.sections[..self.count].iter().flatten()
    }

    fn response_db(&self, frequency_hz: f64, sample_rate: f64) -> f64 {
        let phase = Phase::at(frequency_hz, sample_rate);
        let power = self.sections().map(|s| s.power(&phase)).product::<f64>();
        10.0 * libm::log10(if power.is_finite() { power.max(1e-12) } else { 1e-12 })
    }

    pub fn magnitude_db(&self, frequency_hz: f32, sample_rate: u32) -> f32 {
        self.response_db(f64::from(frequency_hz), f64::from(sample_rate)) as f32
    }

    fn refine(&self, low: f64, high: f64, sample_rate: f64) -> f64 {
        let ratio = (libm::sqrt(5.0) - 1.0) / 2.0;
        let (mut a, mut b) = (libm::log(low), libm::log(high));
        let response = |x: f64| self.response_db(libm::exp(x), sample_rate);
        let mut c = b - ratio * (b - a);
        let mut d = a + ratio * (b - a);
        let (mut fc, mut fd) = (response(c), response(d));
        let mut best = fc.max(fd).max(response(a)).max(response(b));
        for _ in 0..REFINE_STEPS {
            if fc > fd {
                b = d;
                d = c;
                fd = fc;
                c = b - ratio * (b - a);
                fc = response(c);
            } else {
                a = c;
                c = d;
                fc = fd;
                d = a + ratio * (b - a);
                fd = response(d);
            }
            best = best.max(fc).max(fd);
        }
        best
    }

    pub fn peak_db(&self, sample_rate: u32) -> f32 {
        if self.count == 0 {
            return 0.0;
        }
        let fs = f64::from(sample_rate);
        let (low, high) = (1.0, 0.5 * fs);
        let step = libm::pow(high / low, 1.0 / PEAK_GRID as f64);
        let mut best = self.response_db(0.0, fs).max(self.response_db(high, fs));
        let mut window = [(f64::MIN, low); 3];
        for i in 0..=PEAK_GRID {
            let f = (low * libm::pow(step, i as f64)).min(high);
            window = [window[1], window[2], (self.response_db(f, fs), f)];
            best = best.max(window[2].0);
            if i >= 2 && window[1].0 >= window[0].0 && window[1].0 >= window[2].0 {
                best = best.max(self.refine(window[0].1, window[2].1, fs));
            }
        }
        for (_, _, frequency) in self.origins[..self.count].iter().flatten() {
            let f = f64::from(*frequency);
            best = best.max(self.refine((f / step).max(low), (f * step).min(high), fs));
        }
        best as f32
    }
}

pub fn auto_preamp_db(bands: &[Band], sample_rate: u32) -> Result<f32, DesignError> {
    let peak = Cascade::design(bands, sample_rate)?.peak_db(sample_rate);
    Ok(-libm::ceilf(peak.max(0.0) * 2.0) / 2.0)
}

fn profile_bands(profile: &EqProfile) -> ([Band; MAX_BANDS], usize) {
    let mut bands = [Band { kind: Kind::Peak, frequency_hz: 1_000.0, q: 1.0, gain_db: 0.0 }; MAX_BANDS];
    for (slot, band) in bands.iter_mut().zip(&profile.bands) {
        *slot = band.into();
    }
    (bands, profile.bands.len().min(MAX_BANDS))
}

pub fn profile_cascade(profile: &EqProfile, sample_rate: u32) -> Result<Cascade, DesignError> {
    let (bands, count) = profile_bands(profile);
    Cascade::design(&bands[..count], sample_rate)
}

pub fn auto_preamp_centi_db(profile: &EqProfile, sample_rate: u32) -> Result<i16, DesignError> {
    let (bands, count) = profile_bands(profile);
    let preamp = auto_preamp_db(&bands[..count], sample_rate)?;
    Ok(((preamp * 100.0) as i16).max(EqProfile::MIN_PREAMP_CENTI_DB))
}

const LIMITER_THRESHOLD: f32 = 0.891_250_9;
const LIMITER_CEILING: f32 = 0.999_999;
const LIMITER_HEADROOM: f32 = LIMITER_CEILING - LIMITER_THRESHOLD;

#[inline(always)]
fn soft_limit(sample: f32) -> f32 {
    let mag = sample.abs();
    if mag <= LIMITER_THRESHOLD {
        return sample;
    }
    let excess = mag - LIMITER_THRESHOLD;
    let limited = LIMITER_THRESHOLD + (LIMITER_HEADROOM * excess) / (LIMITER_HEADROOM + excess);
    if sample < 0.0 { -limited } else { limited }
}

#[derive(Clone, Copy, Debug)]
struct Crossfeed {
    alpha: f32,
    direct: f32,
    cross: f32,
    lp: [f32; 2],
}

impl Crossfeed {
    const OFF: Self = Self { alpha: 0.0, direct: 1.0, cross: 0.0, lp: [0.0; 2] };

    fn design(mode: CrossfeedMode, sample_rate: u32) -> Self {
        let (cutoff_hz, feed) = match mode {
            CrossfeedMode::Off => return Self::OFF,
            CrossfeedMode::Subtle => (700.0, 0.25),
            CrossfeedMode::Natural => (700.0, 0.45),
            CrossfeedMode::Strong => (650.0, 0.70),
        };
        let fs = f64::from(sample_rate.max(MIN_SAMPLE_RATE));
        let alpha = (1.0 - libm::exp(-2.0 * PI * cutoff_hz / fs)).clamp(1e-5, 0.999) as f32;
        let direct = (1.0 / (1.0 + feed)) as f32;
        let cross = (feed / (1.0 + feed)) as f32;
        Self { alpha, direct, cross, lp: [0.0; 2] }
    }
}

const LOUDNESS_THRESHOLD_CENTI_DB: i16 = -1_200;
const LOUDNESS_SPAN_CENTI_DB: f32 = 3_600.0;
const LOUDNESS_MAX_BASS_DB: f32 = 6.5;
const LOUDNESS_MAX_TREBLE_DB: f32 = 3.0;
const MIN_FADE_FRAMES: u32 = 64;
const MAX_FADE_FRAMES: u32 = 1_920;

fn fade_frames(sample_rate: u32) -> u32 {
    (sample_rate / 100).clamp(MIN_FADE_FRAMES, MAX_FADE_FRAMES)
}

#[derive(Clone, Copy, Debug)]
struct Chain {
    cascade: Cascade,
    state: [[[f32; 2]; MAX_BANDS]; 2],
    base_cascade: Cascade,
    base_state: [[[f32; 2]; MAX_BANDS]; 2],
    base_preamp: f32,
    tone_sections: [Option<Section>; 5],
    tone_state: [[[f32; 2]; 5]; 2],
    tone_preamp: f32,
    tone: ToneEqSettings,
    subsonic: Option<Section>,
    subsonic_state: [[f32; 2]; 2],
    mono_bass: Option<Section>,
    mono_bass_state: [[f32; 2]; 2],
    extras: DspExtras,
    loudness_sections: [Option<Section>; 2],
    loudness_state: [[[f32; 2]; 2]; 2],
    loudness_preamp: f32,
    preamp: f32,
    dsp: DspSettings,
    crossfeed: Crossfeed,
}

const TONE_BAND_SPECS: [(Kind, f32, f32); 5] = [
    (Kind::LowShelf, 60.0, 0.707),
    (Kind::Peak, 200.0, 0.9),
    (Kind::Peak, 1_000.0, 0.9),
    (Kind::Peak, 3_500.0, 1.0),
    (Kind::HighShelf, 10_000.0, 0.707),
];

impl Chain {
    const EMPTY: Self = Self {
        cascade: Cascade::EMPTY,
        state: [[[0.0; 2]; MAX_BANDS]; 2],
        base_cascade: Cascade::EMPTY,
        base_state: [[[0.0; 2]; MAX_BANDS]; 2],
        base_preamp: 1.0,
        tone_sections: [None; 5],
        tone_state: [[[0.0; 2]; 5]; 2],
        tone_preamp: 1.0,
        tone: ToneEqSettings {
            enabled: false,
            sub_bass_centi_db: 0,
            bass_centi_db: 0,
            mid_centi_db: 0,
            presence_centi_db: 0,
            treble_centi_db: 0,
        },
        subsonic: None,
        subsonic_state: [[0.0; 2]; 2],
        mono_bass: None,
        mono_bass_state: [[0.0; 2]; 2],
        extras: DspExtras {
            auto_level: false,
            auto_level_target_lufs: DspExtras::DEFAULT_TARGET_LUFS,
            subsonic_hz: 0,
            mono_bass_hz: 0,
        },
        loudness_sections: [None; 2],
        loudness_state: [[[0.0; 2]; 2]; 2],
        loudness_preamp: 1.0,
        preamp: 1.0,
        dsp: DspSettings {
            crossfeed: CrossfeedMode::Off,
            limiter: false,
            mono: false,
            invert_phase: false,
            loudness: false,
            swap_channels: false,
        },
        crossfeed: Crossfeed::OFF,
    };

    fn active_section_count(&self) -> usize {
        self.cascade.count
            + self.base_cascade.count
            + self.tone_sections.iter().filter(|s| s.is_some()).count()
            + usize::from(self.subsonic.is_some())
            + usize::from(self.mono_bass.is_some())
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    #[inline(never)]
    fn run_stereo_section(section: Section, state_l: &mut [f32; 2], state_r: &mut [f32; 2], buf: &mut [[f32; 2]]) {
        let (a1, a2, a3) = (section.a1, section.a2, section.a3);
        let (m0, m1, m2) = (section.m0, section.m1, section.m2);
        let [mut l0, mut l1] = *state_l;
        let [mut r0, mut r1] = *state_r;
        if m0 == 1.0 && m2 == 0.0 {
            for slot in buf.iter_mut() {
                let xl = slot[0];
                let xr = slot[1];
                let t1l = a1 * l0;
                let t1r = a1 * r0;
                let t2l = mul_add(a2, l0, l1);
                let t2r = mul_add(a2, r0, r1);
                let v3l = xl - l1;
                let v3r = xr - r1;
                let v1l = mul_add(a2, v3l, t1l);
                let v1r = mul_add(a2, v3r, t1r);
                let v2l = mul_add(a3, v3l, t2l);
                let v2r = mul_add(a3, v3r, t2r);
                l0 = mul_sub(2.0, v1l, l0);
                r0 = mul_sub(2.0, v1r, r0);
                l1 = mul_sub(2.0, v2l, l1);
                r1 = mul_sub(2.0, v2r, r1);
                slot[0] = mul_add(m1, v1l, xl);
                slot[1] = mul_add(m1, v1r, xr);
            }
        } else {
            for slot in buf.iter_mut() {
                let xl = slot[0];
                let xr = slot[1];
                let t1l = a1 * l0;
                let t1r = a1 * r0;
                let t2l = mul_add(a2, l0, l1);
                let t2r = mul_add(a2, r0, r1);
                let v3l = xl - l1;
                let v3r = xr - r1;
                let y0l = m0 * xl;
                let y0r = m0 * xr;
                let v1l = mul_add(a2, v3l, t1l);
                let v1r = mul_add(a2, v3r, t1r);
                let v2l = mul_add(a3, v3l, t2l);
                let v2r = mul_add(a3, v3r, t2r);
                l0 = mul_sub(2.0, v1l, l0);
                r0 = mul_sub(2.0, v1r, r0);
                let y1l = mul_add(m1, v1l, y0l);
                let y1r = mul_add(m1, v1r, y0r);
                l1 = mul_sub(2.0, v2l, l1);
                r1 = mul_sub(2.0, v2r, r1);
                slot[0] = mul_add(m2, v2l, y1l);
                slot[1] = mul_add(m2, v2r, y1r);
            }
        }
        let clean = |v: f32| if !v.is_finite() || v.abs() < STATE_FLOOR { 0.0 } else { v };
        *state_l = [clean(l0), clean(l1)];
        *state_r = [clean(r0), clean(r1)];
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    fn process_linear(&mut self, chunk: &[i32], buf: &mut [[f32; 2]]) {
        const INV_SCALE: f32 = 1.0 / 2_147_483_648.0;
        let base_gain = self.preamp * self.base_preamp * self.tone_preamp * self.loudness_preamp * INV_SCALE;
        let gain = if self.dsp.invert_phase { -base_gain } else { base_gain };
        let swap = self.dsp.swap_channels;
        let mono = self.dsp.mono;
        let use_crossfeed = !mono && !matches!(self.dsp.crossfeed, CrossfeedMode::Off);
        if swap {
            for (slot, pair) in buf.iter_mut().zip(chunk.chunks_exact(2)) {
                slot[0] = pair[1] as f32 * gain;
                slot[1] = pair[0] as f32 * gain;
            }
        } else {
            for (slot, pair) in buf.iter_mut().zip(chunk.chunks_exact(2)) {
                slot[0] = pair[0] as f32 * gain;
                slot[1] = pair[1] as f32 * gain;
            }
        }
        if let Some(subsonic) = self.subsonic {
            let (sl, sr) = self.subsonic_state.split_at_mut(1);
            Self::run_stereo_section(subsonic, &mut sl[0], &mut sr[0], buf);
        }
        for band in 0..self.base_cascade.count {
            let Some(section) = self.base_cascade.sections[band] else { continue };
            let (sl, sr) = self.base_state.split_at_mut(1);
            Self::run_stereo_section(section, &mut sl[0][band], &mut sr[0][band], buf);
        }
        for band in 0..self.cascade.count {
            let Some(section) = self.cascade.sections[band] else { continue };
            let (sl, sr) = self.state.split_at_mut(1);
            Self::run_stereo_section(section, &mut sl[0][band], &mut sr[0][band], buf);
        }
        for idx in 0..5 {
            let Some(section) = self.tone_sections[idx] else { continue };
            let (sl, sr) = self.tone_state.split_at_mut(1);
            Self::run_stereo_section(section, &mut sl[0][idx], &mut sr[0][idx], buf);
        }
        for shelf in 0..2 {
            let Some(section) = self.loudness_sections[shelf] else { continue };
            let (sl, sr) = self.loudness_state.split_at_mut(1);
            Self::run_stereo_section(section, &mut sl[0][shelf], &mut sr[0][shelf], buf);
        }
        if mono {
            for slot in buf.iter_mut() {
                let mid = 0.5 * (slot[0] + slot[1]);
                slot[0] = mid;
                slot[1] = mid;
            }
        } else {
            if let Some(mb) = self.mono_bass {
                let (a1, a2, a3) = (mb.a1, mb.a2, mb.a3);
                let [mut m0, mut m1] = self.mono_bass_state[0];
                let [mut s0, mut s1] = self.mono_bass_state[1];
                for slot in buf.iter_mut() {
                    let mid = 0.5 * (slot[0] + slot[1]);
                    let side = 0.5 * (slot[0] - slot[1]);
                    let v3m = mid - m1;
                    let v1m = mul_add(a2, v3m, a1 * m0);
                    let v2m = mul_add(a3, v3m, mul_add(a2, m0, m1));
                    m0 = mul_sub(2.0, v1m, m0);
                    m1 = mul_sub(2.0, v2m, m1);
                    let mid_ap = mid - 2.0 * (v1m + v2m);
                    let v3s = side - s1;
                    let v1s = mul_add(a2, v3s, a1 * s0);
                    let v2s = mul_add(a3, v3s, mul_add(a2, s0, s1));
                    s0 = mul_sub(2.0, v1s, s0);
                    s1 = mul_sub(2.0, v2s, s1);
                    let side_hp = side - 2.0 * v1s - v2s;
                    slot[0] = mid_ap + side_hp;
                    slot[1] = mid_ap - side_hp;
                }
                let clean = |v: f32| if !v.is_finite() || v.abs() < STATE_FLOOR { 0.0 } else { v };
                self.mono_bass_state[0] = [clean(m0), clean(m1)];
                self.mono_bass_state[1] = [clean(s0), clean(s1)];
            }
            if use_crossfeed {
                let alpha = self.crossfeed.alpha;
                let direct = self.crossfeed.direct;
                let cross = self.crossfeed.cross;
                let [mut lp_l, mut lp_r] = self.crossfeed.lp;
                for slot in buf.iter_mut() {
                    let l = slot[0];
                    let r = slot[1];
                    lp_l = mul_add(alpha, l - lp_l, lp_l);
                    lp_r = mul_add(alpha, r - lp_r, lp_r);
                    slot[0] = mul_add(cross, lp_r, direct * l);
                    slot[1] = mul_add(cross, lp_l, direct * r);
                }
                let clean = |v: f32| if !v.is_finite() || v.abs() < STATE_FLOOR { 0.0 } else { v };
                self.crossfeed.lp = [clean(lp_l), clean(lp_r)];
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Equalizer {
    active: Chain,
    previous: Chain,
    fade_pos: u32,
    fade_len: u32,
    has_processed: bool,
    loudness_vol_centi_db: i16,
    agc_applied: f32,
    scratch: [[f32; 2]; CHUNK],
    old_scratch: [[f32; 2]; CHUNK],
    sample_rate: u32,
    clipped: u32,
    peak_l: f32,
    peak_r: f32,
    sum_sq_l: f32,
    sum_sq_r: f32,
    meter_frames: u32,
    limiter_hits: u32,
    analyzer: Analyzer,
}

impl Default for Equalizer {
    fn default() -> Self {
        Self::new()
    }
}

fn level_centi_db(linear: f32) -> i16 {
    if linear <= 1.584_893_2e-5 {
        return METER_FLOOR_CENTI_DB;
    }
    let db = gain_to_db(linear) * 100.0;
    db.clamp(f32::from(METER_FLOOR_CENTI_DB), 0.0) as i16
}

impl Equalizer {
    pub const fn new() -> Self {
        Self {
            active: Chain::EMPTY,
            previous: Chain::EMPTY,
            fade_pos: 0,
            fade_len: 0,
            has_processed: false,
            loudness_vol_centi_db: 0,
            agc_applied: 1.0,
            scratch: [[0.0; 2]; CHUNK],
            old_scratch: [[0.0; 2]; CHUNK],
            sample_rate: 0,
            clipped: 0,
            peak_l: 0.0,
            peak_r: 0.0,
            sum_sq_l: 0.0,
            sum_sq_r: 0.0,
            meter_frames: 0,
            limiter_hits: 0,
            analyzer: Analyzer::new(),
        }
    }

    fn begin_transition(&mut self) {
        if !self.has_processed || !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&self.sample_rate) {
            return;
        }
        if self.sample_rate >= 352_800 && self.active.active_section_count() > 10 {
            return;
        }
        if self.fade_len == 0 {
            self.previous = self.active;
            self.fade_pos = 0;
            self.fade_len = fade_frames(self.sample_rate);
        }
    }

    fn clear_transition(&mut self) {
        self.fade_pos = 0;
        self.fade_len = 0;
        self.has_processed = false;
    }

    fn update_loudness(&mut self, allow_fade: bool) {
        if !self.active.dsp.loudness
            || !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&self.sample_rate)
            || self.loudness_vol_centi_db >= LOUDNESS_THRESHOLD_CENTI_DB
        {
            if self.active.loudness_sections[0].is_some() || self.active.loudness_sections[1].is_some() {
                if allow_fade {
                    self.begin_transition();
                }
                self.active.loudness_sections = [None; 2];
                self.active.loudness_state = [[[0.0; 2]; 2]; 2];
                self.active.loudness_preamp = 1.0;
            }
            return;
        }
        let depth =
            (f32::from(LOUDNESS_THRESHOLD_CENTI_DB - self.loudness_vol_centi_db) / LOUDNESS_SPAN_CENTI_DB).clamp(0.0, 1.0);
        let bass_db = LOUDNESS_MAX_BASS_DB * depth;
        let treble_db = LOUDNESS_MAX_TREBLE_DB * depth;
        let low = Band { kind: Kind::LowShelf, frequency_hz: 90.0, q: 0.707, gain_db: bass_db };
        let high = Band { kind: Kind::HighShelf, frequency_hz: 9_000.0, q: 0.707, gain_db: treble_db };
        let s0 = Section::design(&low, self.sample_rate).ok();
        let s1 = Section::design(&high, self.sample_rate).ok();
        let preamp = db_to_gain(-bass_db);
        if self.active.loudness_sections == [s0, s1] && self.active.loudness_preamp == preamp {
            return;
        }
        if allow_fade {
            self.begin_transition();
        }
        let was_active = self.active.loudness_sections[0].is_some();
        self.active.loudness_sections[0] = s0;
        self.active.loudness_sections[1] = s1;
        self.active.loudness_preamp = preamp;
        if !was_active {
            self.active.loudness_state = [[[0.0; 2]; 2]; 2];
        }
    }

    pub fn set_loudness_volume(&mut self, volume_centi_db: i16) {
        self.analyzer.set_allow_boost(volume_centi_db < 0);
        let quantized = (volume_centi_db / 50) * 50;
        if quantized == self.loudness_vol_centi_db {
            return;
        }
        self.loudness_vol_centi_db = quantized;
        self.update_loudness(true);
    }

    pub fn loudness_active(&self) -> bool {
        self.active.loudness_sections[0].is_some() || self.active.loudness_sections[1].is_some()
    }

    pub fn configure(&mut self, bands: &[Band], preamp_db: f32, sample_rate: u32) -> Result<(), DesignError> {
        if !(-48.0..=0.0).contains(&preamp_db) {
            return Err(DesignError::Gain);
        }
        let cascade = Cascade::design(bands, sample_rate)?;
        let preamp = db_to_gain(preamp_db);
        let rate_changed = sample_rate != self.sample_rate;
        if !rate_changed && cascade == self.active.cascade && preamp == self.active.preamp {
            return Ok(());
        }
        if rate_changed {
            self.clear_transition();
        } else {
            self.begin_transition();
        }
        for index in 0..MAX_BANDS {
            let same = |a: Option<(usize, Kind, f32)>, b: Option<(usize, Kind, f32)>| matches!((a, b), (Some((ia, ka, _)), Some((ib, kb, _))) if ia == ib && ka == kb);
            if rate_changed || !same(cascade.origins[index], self.active.cascade.origins[index]) {
                self.active.state[0][index] = [0.0; 2];
                self.active.state[1][index] = [0.0; 2];
            }
        }
        self.active.cascade = cascade;
        self.active.preamp = preamp;
        self.sample_rate = sample_rate;
        self.analyzer.configure_rate(sample_rate);
        if rate_changed {
            self.active.crossfeed = Crossfeed::design(self.active.dsp.crossfeed, sample_rate);
            self.active.loudness_state = [[[0.0; 2]; 2]; 2];
            self.update_loudness(false);
            self.rebuild_tone_and_extras(sample_rate);
        }
        Ok(())
    }

    pub fn configure_profile(&mut self, profile: &EqProfile, sample_rate: u32) -> Result<(), DesignError> {
        let (bands, count) = profile_bands(profile);
        self.configure(&bands[..count], f32::from(profile.preamp_centi_db) / 100.0, sample_rate)
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    pub fn configure_base_eq(&mut self, base: &EqBaseSettings, sample_rate: u32) -> Result<(), DesignError> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(DesignError::SampleRate);
        }
        let (cascade, preamp) = if base.enabled && !base.profile.bands.is_empty() {
            let (bands, count) = profile_bands(&base.profile);
            let c = Cascade::design(&bands[..count], sample_rate)?;
            let p = db_to_gain(f32::from(base.profile.preamp_centi_db) / 100.0);
            (c, p)
        } else {
            (Cascade::EMPTY, 1.0)
        };
        let rate_changed = sample_rate != self.sample_rate;
        if !rate_changed && cascade == self.active.base_cascade && preamp == self.active.base_preamp {
            return Ok(());
        }
        if rate_changed {
            self.clear_transition();
        } else {
            self.begin_transition();
        }
        for index in 0..MAX_BANDS {
            let same = |a: Option<(usize, Kind, f32)>, b: Option<(usize, Kind, f32)>| matches!((a, b), (Some((ia, ka, _)), Some((ib, kb, _))) if ia == ib && ka == kb);
            if rate_changed || !same(cascade.origins[index], self.active.base_cascade.origins[index]) {
                self.active.base_state[0][index] = [0.0; 2];
                self.active.base_state[1][index] = [0.0; 2];
            }
        }
        self.active.base_cascade = cascade;
        self.active.base_preamp = preamp;
        self.sample_rate = sample_rate;
        self.analyzer.configure_rate(sample_rate);
        Ok(())
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    fn rebuild_tone_and_extras(&mut self, sample_rate: u32) {
        let gains = [
            self.active.tone.sub_bass_centi_db,
            self.active.tone.bass_centi_db,
            self.active.tone.mid_centi_db,
            self.active.tone.presence_centi_db,
            self.active.tone.treble_centi_db,
        ];
        let mut max_boost = 0.0f32;
        for i in 0..5 {
            let g = if self.active.tone.enabled { f32::from(gains[i]) / 100.0 } else { 0.0 };
            if g == 0.0 {
                self.active.tone_sections[i] = None;
                self.active.tone_state[0][i] = [0.0; 2];
                self.active.tone_state[1][i] = [0.0; 2];
            } else {
                let (kind, frequency_hz, q) = TONE_BAND_SPECS[i];
                let band = Band { kind, frequency_hz, q, gain_db: g };
                let sec = if band.above_band_limit(sample_rate) { None } else { Section::design(&band, sample_rate).ok() };
                if self.active.tone_sections[i].is_none() && sec.is_some() {
                    self.active.tone_state[0][i] = [0.0; 2];
                    self.active.tone_state[1][i] = [0.0; 2];
                }
                self.active.tone_sections[i] = sec;
                if g > max_boost {
                    max_boost = g;
                }
            }
        }
        self.active.tone_preamp = if max_boost > 0.0 { db_to_gain(-max_boost) } else { 1.0 };

        let sub = if self.active.extras.subsonic_hz != 0 {
            let band = Band {
                kind: Kind::HighPass,
                frequency_hz: f32::from(self.active.extras.subsonic_hz),
                q: core::f32::consts::FRAC_1_SQRT_2,
                gain_db: 0.0,
            };
            Section::design(&band, sample_rate).ok()
        } else {
            None
        };
        if sub.is_none() || self.active.subsonic.is_none() {
            self.active.subsonic_state = [[0.0; 2]; 2];
        }
        self.active.subsonic = sub;

        let mb = if self.active.extras.mono_bass_hz != 0 {
            let band =
                Band { kind: Kind::HighPass, frequency_hz: f32::from(self.active.extras.mono_bass_hz), q: 0.5, gain_db: 0.0 };
            Section::design(&band, sample_rate).ok()
        } else {
            None
        };
        if mb.is_none() || self.active.mono_bass.is_none() {
            self.active.mono_bass_state = [[0.0; 2]; 2];
        }
        self.active.mono_bass = mb;
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    pub fn configure_tone_eq(&mut self, tone: ToneEqSettings, sample_rate: u32) -> Result<(), DesignError> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(DesignError::SampleRate);
        }
        if !tone.is_valid() {
            return Err(DesignError::Gain);
        }
        let rate_changed = sample_rate != self.sample_rate;
        if !rate_changed && tone == self.active.tone {
            return Ok(());
        }
        if rate_changed {
            self.clear_transition();
        } else {
            self.begin_transition();
        }
        self.active.tone = tone;
        self.sample_rate = sample_rate;
        self.analyzer.configure_rate(sample_rate);
        self.rebuild_tone_and_extras(sample_rate);
        Ok(())
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    pub fn configure_dsp_extras(&mut self, extras: DspExtras, sample_rate: u32) -> Result<(), DesignError> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(DesignError::SampleRate);
        }
        if !extras.is_valid() {
            return Err(DesignError::Gain);
        }
        let rate_changed = sample_rate != self.sample_rate;
        if !rate_changed && extras == self.active.extras {
            return Ok(());
        }
        if rate_changed {
            self.clear_transition();
        } else {
            self.begin_transition();
        }
        if !extras.auto_level {
            self.analyzer.reset_agc();
        }
        self.active.extras = extras;
        self.sample_rate = sample_rate;
        self.analyzer.configure_rate(sample_rate);
        self.rebuild_tone_and_extras(sample_rate);
        Ok(())
    }

    pub fn set_spectrum_enabled(&mut self, enabled: bool) {
        self.analyzer.set_spectrum_enabled(enabled);
    }

    pub fn configure_dsp(&mut self, dsp: DspSettings, sample_rate: u32) -> Result<(), DesignError> {
        if !(MIN_SAMPLE_RATE..=MAX_SAMPLE_RATE).contains(&sample_rate) {
            return Err(DesignError::SampleRate);
        }
        let rate_changed = sample_rate != self.sample_rate;
        if !rate_changed && dsp == self.active.dsp {
            return Ok(());
        }
        if rate_changed {
            self.clear_transition();
        } else {
            self.begin_transition();
        }
        if dsp.crossfeed != self.active.dsp.crossfeed || rate_changed {
            let keep = self.active.dsp.crossfeed != CrossfeedMode::Off && dsp.crossfeed != CrossfeedMode::Off && !rate_changed;
            let lp = if keep { self.active.crossfeed.lp } else { [0.0; 2] };
            self.active.crossfeed = Crossfeed::design(dsp.crossfeed, sample_rate);
            self.active.crossfeed.lp = lp;
        }
        self.active.dsp = dsp;
        self.sample_rate = sample_rate;
        self.analyzer.configure_rate(sample_rate);
        if rate_changed {
            self.active.loudness_state = [[[0.0; 2]; 2]; 2];
            self.rebuild_tone_and_extras(sample_rate);
        }
        self.update_loudness(false);
        Ok(())
    }

    pub fn dsp(&self) -> DspSettings {
        self.active.dsp
    }

    pub fn dsp_extras(&self) -> DspExtras {
        self.active.extras
    }

    pub fn band_count(&self) -> usize {
        self.active.cascade.count
    }

    pub fn total_band_count(&self) -> usize {
        self.active.active_section_count()
    }

    pub fn bypass_eq(&mut self) {
        if self.is_eq_bypassed() {
            return;
        }
        self.begin_transition();
        self.active.cascade = Cascade::EMPTY;
        self.active.preamp = 1.0;
        self.active.state = [[[0.0; 2]; MAX_BANDS]; 2];
    }

    pub fn bypass(&mut self) {
        self.active = Chain::EMPTY;
        self.previous = Chain::EMPTY;
        self.clear_transition();
        self.agc_applied = 1.0;
        self.clipped = 0;
        self.limiter_hits = 0;
    }

    pub fn is_eq_bypassed(&self) -> bool {
        self.active.cascade.count == 0 && self.active.preamp == 1.0
    }

    pub fn is_dsp_active(&self) -> bool {
        !matches!(self.active.dsp.crossfeed, CrossfeedMode::Off)
            || self.active.dsp.limiter
            || self.active.dsp.mono
            || self.active.dsp.invert_phase
            || self.active.dsp.swap_channels
            || self.loudness_active()
            || self.active.base_cascade.count > 0
            || self.active.base_preamp != 1.0
            || self.active.tone_sections.iter().any(Option::is_some)
            || self.active.subsonic.is_some()
            || self.active.mono_bass.is_some()
            || self.active.extras.auto_level
    }

    pub fn is_transitioning(&self) -> bool {
        self.fade_len > 0
    }

    pub fn is_bypassed(&self) -> bool {
        self.is_eq_bypassed() && !self.is_dsp_active() && self.fade_len == 0
    }

    pub fn clipped_samples(&self) -> u32 {
        self.clipped
    }

    pub fn limiter_hits(&self) -> u32 {
        self.limiter_hits
    }

    pub fn reset_meters(&mut self) {
        self.peak_l = 0.0;
        self.peak_r = 0.0;
        self.sum_sq_l = 0.0;
        self.sum_sq_r = 0.0;
        self.meter_frames = 0;
    }

    pub fn reset_stream(&mut self) {
        self.reset_meters();
        self.clear_transition();
        self.active.state = [[[0.0; 2]; MAX_BANDS]; 2];
        self.active.base_state = [[[0.0; 2]; MAX_BANDS]; 2];
        self.active.tone_state = [[[0.0; 2]; 5]; 2];
        self.active.subsonic_state = [[0.0; 2]; 2];
        self.active.mono_bass_state = [[0.0; 2]; 2];
        self.active.loudness_state = [[[0.0; 2]; 2]; 2];
        self.active.crossfeed.lp = [0.0; 2];
        self.agc_applied = 1.0;
        self.analyzer.reset_stream();
    }

    pub fn spectrum_enabled(&self) -> bool {
        self.analyzer.spectrum_enabled()
    }

    pub fn spectrum_busy(&self) -> bool {
        self.analyzer.spectrum_busy()
    }

    pub fn start_spectrum(&mut self, active: bool) -> Option<AudioSpectrum> {
        self.analyzer.start_spectrum(active, self.active.extras.auto_level)
    }

    pub fn step_spectrum(&mut self) -> Option<AudioSpectrum> {
        self.analyzer.step_spectrum()
    }

    pub fn take_spectrum(&mut self, active: bool) -> AudioSpectrum {
        self.analyzer.take_spectrum(active, self.active.extras.auto_level)
    }

    pub fn take_spectrum_with(&mut self, active: bool, poll: impl FnMut()) -> AudioSpectrum {
        self.analyzer.take_spectrum_with(active, self.active.extras.auto_level, poll)
    }

    pub fn take_meters(&mut self, active: bool) -> AudioMeters {
        let frames = self.meter_frames;
        let peak_l = self.peak_l;
        let peak_r = self.peak_r;
        let sum_sq_l = self.sum_sq_l;
        let sum_sq_r = self.sum_sq_r;
        self.reset_meters();
        if !active || frames == 0 {
            return AudioMeters {
                active,
                clipped_samples: self.clipped,
                limiter_hits: self.limiter_hits,
                ..AudioMeters::default()
            };
        }
        let inv = 1.0 / frames as f32;
        AudioMeters {
            active: true,
            peak_left_centi_db: level_centi_db(peak_l),
            peak_right_centi_db: level_centi_db(peak_r),
            rms_left_centi_db: level_centi_db(libm::sqrtf(sum_sq_l * inv)),
            rms_right_centi_db: level_centi_db(libm::sqrtf(sum_sq_r * inv)),
            clipped_samples: self.clipped,
            limiter_hits: self.limiter_hits,
        }
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn observe_stereo(&mut self, frames: &[i32]) {
        const INV_SCALE: f32 = 1.0 / 2_147_483_648.0;
        let pairs = frames.len() / 2;
        if pairs == 0 {
            return;
        }
        let analyze = self.analyzer.spectrum_enabled();
        let stride = if analyze || pairs <= 16 { 1 } else { 4 };
        let mut peak_l = self.peak_l;
        let mut peak_r = self.peak_r;
        let mut sum_sq_l = self.sum_sq_l;
        let mut sum_sq_r = self.sum_sq_r;
        let mut count = self.meter_frames;
        let mut index = 0;
        while index < pairs {
            let sl = frames[index * 2] as f32 * INV_SCALE;
            let sr = frames[index * 2 + 1] as f32 * INV_SCALE;
            if analyze {
                self.analyzer.push_output_frame(sl, sr, sl, sr, false, 0.0);
            }
            let l = sl.abs();
            let r = sr.abs();
            if l > peak_l {
                peak_l = l;
            }
            if r > peak_r {
                peak_r = r;
            }
            sum_sq_l = mul_add(l, l, sum_sq_l);
            sum_sq_r = mul_add(r, r, sum_sq_r);
            count = count.saturating_add(1);
            index += stride;
        }
        self.peak_l = peak_l;
        self.peak_r = peak_r;
        self.sum_sq_l = sum_sq_l;
        self.sum_sq_r = sum_sq_r;
        self.meter_frames = count;
        self.has_processed = true;
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn process_stereo(&mut self, frames: &mut [i32]) {
        if self.is_bypassed() {
            if frames.len() >= 2 {
                self.has_processed = true;
            }
            return;
        }
        self.has_processed = true;
        const SCALE: f32 = 2_147_483_648.0;
        const INV_SCALE: f32 = 1.0 / SCALE;
        let mut clipped = self.clipped;
        let mut limiter_hits = self.limiter_hits;
        let mut peak_l = self.peak_l;
        let mut peak_r = self.peak_r;
        let mut sum_sq_l = self.sum_sq_l;
        let mut sum_sq_r = self.sum_sq_r;
        let mut meter_frames = self.meter_frames;
        let auto_level = self.active.extras.auto_level;
        let target_lufs = f32::from(self.active.extras.auto_level_target_lufs);
        let run_analyzer = self.analyzer.spectrum_enabled() || auto_level;
        for chunk in frames.chunks_mut(CHUNK * 2) {
            let pairs = chunk.len() / 2;
            let agc_target = if auto_level { self.analyzer.agc_gain_lin() } else { 1.0 };
            let buf = &mut self.scratch[..pairs];
            self.active.process_linear(chunk, buf);
            if self.fade_len > 0 {
                let old_buf = &mut self.old_scratch[..pairs];
                self.previous.process_linear(chunk, old_buf);
                let inv_len = 1.0 / self.fade_len as f32;
                let mut pos = self.fade_pos;
                for (cur, old) in buf.iter_mut().zip(old_buf.iter()) {
                    if pos < self.fade_len {
                        pos += 1;
                        let t = (pos as f32 * inv_len).min(1.0);
                        let w = t * t * (3.0 - 2.0 * t);
                        cur[0] = mul_add(w, cur[0] - old[0], old[0]);
                        cur[1] = mul_add(w, cur[1] - old[1], old[1]);
                    }
                }
                if pos >= self.fade_len {
                    self.fade_pos = 0;
                    self.fade_len = 0;
                } else {
                    self.fade_pos = pos;
                }
            }
            let mut agc_cur = self.agc_applied;
            let agc_step = if pairs > 0 { (agc_target - agc_cur) / pairs as f32 } else { 0.0 };
            let limiter = self.active.dsp.limiter || (auto_level && (agc_target > 1.0 || agc_cur > 1.0));
            for (slot, pair) in buf.iter().zip(chunk.chunks_exact_mut(2)) {
                let pre_l = slot[0];
                let pre_r = slot[1];
                agc_cur += agc_step;
                let mut sl = pre_l * agc_cur;
                let mut sr = pre_r * agc_cur;
                if limiter {
                    if sl.abs() > LIMITER_THRESHOLD {
                        limiter_hits = limiter_hits.wrapping_add(1);
                        sl = soft_limit(sl);
                    }
                    if sr.abs() > LIMITER_THRESHOLD {
                        limiter_hits = limiter_hits.wrapping_add(1);
                        sr = soft_limit(sr);
                    }
                }
                if run_analyzer {
                    self.analyzer.push_output_frame(sl, sr, pre_l, pre_r, auto_level, target_lufs);
                }
                let l = sl.abs();
                let r = sr.abs();
                if l > peak_l {
                    peak_l = l;
                }
                if r > peak_r {
                    peak_r = r;
                }
                sum_sq_l = mul_add(l, l, sum_sq_l);
                sum_sq_r = mul_add(r, r, sum_sq_r);
                meter_frames = meter_frames.saturating_add(1);
                pair[0] = quantize(sl, SCALE, &mut clipped);
                pair[1] = quantize(sr, SCALE, &mut clipped);
            }
            self.agc_applied = agc_target;
            if chunk.len() & 1 != 0 {
                let last = chunk.len() - 1;
                let count = self.active.cascade.count;
                let eq_preamp = self.active.preamp * self.active.base_preamp * self.active.tone_preamp;
                let base_gain = eq_preamp * self.active.loudness_preamp * agc_target * INV_SCALE;
                let gain = if self.active.dsp.invert_phase { -base_gain } else { base_gain };
                let mut x = chunk[last] as f32 * gain;
                for (section, z) in self.active.cascade.sections[..count].iter().flatten().zip(self.active.state[0].iter_mut()) {
                    x = section.tick(x, z);
                }
                if self.active.dsp.limiter && x.abs() > LIMITER_THRESHOLD {
                    limiter_hits = limiter_hits.wrapping_add(1);
                    x = soft_limit(x);
                }
                chunk[last] = quantize(x, SCALE, &mut clipped);
            }
        }
        self.clipped = clipped;
        self.limiter_hits = limiter_hits;
        self.peak_l = peak_l;
        self.peak_r = peak_r;
        self.sum_sq_l = sum_sq_l;
        self.sum_sq_r = sum_sq_r;
        self.meter_frames = meter_frames;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn peak(f: f32, q: f32, g: f32) -> Band {
        Band { kind: Kind::Peak, frequency_hz: f, q, gain_db: g }
    }

    #[test]
    fn peak_hits_its_gain_at_the_centre_and_is_flat_far_away() {
        for rate in [44_100, 48_000, 96_000, 192_000, 384_000] {
            for (f, g) in [(31.0, 6.0), (1_000.0, -9.0), (8_000.0, 12.0)] {
                let c = Section::design(&peak(f, 1.0, g), rate).unwrap();
                assert!((gain_to_db(c.magnitude(f, rate)) - g).abs() < 0.05, "{rate} {f}");
                let far = if f < 1_000.0 { 10_000.0 } else { 20.0 };
                assert!(gain_to_db(c.magnitude(far, rate)).abs() < 0.6, "{rate} {f}");
            }
        }
    }

    #[test]
    fn shelves_and_passes_have_textbook_responses() {
        let rate = 96_000;
        let low = Section::design(&Band { kind: Kind::LowShelf, frequency_hz: 200.0, q: 0.707, gain_db: 6.0 }, rate).unwrap();
        assert!((gain_to_db(low.magnitude(10.0, rate)) - 6.0).abs() < 0.1);
        assert!(gain_to_db(low.magnitude(20_000.0, rate)).abs() < 0.1);
        let high =
            Section::design(&Band { kind: Kind::HighShelf, frequency_hz: 8_000.0, q: 0.707, gain_db: -4.0 }, rate).unwrap();
        assert!((gain_to_db(high.magnitude(40_000.0, rate)) + 4.0).abs() < 0.1);
        assert!(gain_to_db(high.magnitude(50.0, rate)).abs() < 0.1);
        let lp = Section::design(
            &Band { kind: Kind::LowPass, frequency_hz: 1_000.0, q: core::f32::consts::FRAC_1_SQRT_2, gain_db: 0.0 },
            rate,
        )
        .unwrap();
        assert!((gain_to_db(lp.magnitude(1_000.0, rate)) + 3.01).abs() < 0.05);
        let hp = Section::design(
            &Band { kind: Kind::HighPass, frequency_hz: 1_000.0, q: core::f32::consts::FRAC_1_SQRT_2, gain_db: 0.0 },
            rate,
        )
        .unwrap();
        assert!((gain_to_db(hp.magnitude(1_000.0, rate)) + 3.01).abs() < 0.05);
    }

    #[test]
    fn design_rejects_bad_input_and_stays_stable_across_the_range() {
        assert_eq!(Section::design(&peak(1_000.0, 1.0, 3.0), 4_000), Err(DesignError::SampleRate));
        assert_eq!(Section::design(&peak(30_000.0, 1.0, 3.0), 48_000), Err(DesignError::Frequency));
        assert_eq!(Section::design(&peak(1_000.0, 0.01, 3.0), 48_000), Err(DesignError::Q));
        assert_eq!(Section::design(&peak(1_000.0, 1.0, 30.0), 48_000), Err(DesignError::Gain));
        assert_eq!(Section::design(&peak(1_000.0, 1.0, f32::NAN), 48_000), Err(DesignError::Gain));
        for rate in [44_100, 768_000] {
            for f in [20.0, 31.0, 1_000.0, 16_000.0] {
                for kind in [Kind::Peak, Kind::LowShelf, Kind::HighShelf, Kind::LowPass, Kind::HighPass] {
                    for g in [-18.0, 0.0, 18.0] {
                        let c = Section::design(&Band { kind, frequency_hz: f, q: 0.5, gain_db: g }, rate).unwrap();
                        assert!(c.g > 0.0 && c.k > 0.0 && c.a1 > 0.0 && c.a1 <= 1.0);
                    }
                }
            }
        }
    }

    #[test]
    fn auto_preamp_covers_the_boost() {
        let bands = [peak(62.0, 1.0, 6.0), peak(125.0, 1.0, 4.0)];
        let preamp = auto_preamp_db(&bands, 48_000).unwrap();
        let peak_db = Cascade::design(&bands, 48_000).unwrap().peak_db(48_000);
        assert!(preamp <= -peak_db && preamp > -peak_db - 0.6, "{preamp} {peak_db}");
        assert_eq!(auto_preamp_db(&[peak(1_000.0, 1.0, -6.0)], 48_000).unwrap(), 0.0);
    }

    #[test]
    fn flat_setting_is_bit_perfect() {
        let mut eq = Equalizer::new();
        eq.configure(&[peak(1_000.0, 1.0, 0.0)], 0.0, 48_000).unwrap();
        assert!(eq.is_bypassed());
        let original = [i32::MIN, -1, 0, 1, 12_345_678, i32::MAX];
        let mut frames = original;
        eq.process_stereo(&mut frames);
        assert_eq!(frames, original);
    }

    #[test]
    fn processing_matches_the_designed_gain() {
        let rate = 48_000;
        let mut eq = Equalizer::new();
        eq.configure(&[peak(1_000.0, 1.0, 6.0)], -6.0, rate).unwrap();
        let amplitude = 0.25 * 2_147_483_648.0;
        let mut frames = [0i32; 9_600];
        for (i, frame) in frames.chunks_mut(2).enumerate() {
            let s = (amplitude * libm::sinf(2.0 * core::f32::consts::PI * 1_000.0 * i as f32 / rate as f32)) as i32;
            frame[0] = s;
            frame[1] = -s;
        }
        eq.process_stereo(&mut frames);
        let tail = &frames[4_800..];
        let peak_left = tail.iter().step_by(2).map(|s| (*s as f32).abs()).fold(0.0, f32::max);
        let peak_right = tail.iter().skip(1).step_by(2).map(|s| (*s as f32).abs()).fold(0.0, f32::max);
        assert!((peak_left / amplitude - 1.0).abs() < 0.02, "{}", peak_left / amplitude);
        assert!((peak_right / peak_left - 1.0).abs() < 0.001);
        assert_eq!(eq.clipped_samples(), 0);
    }

    fn measured_gain(eq: &mut Equalizer, frequency: f32, rate: u32, settle_periods: f32, measure_periods: f32) -> f32 {
        let amplitude = 0.125 * 2_147_483_648.0f64;
        let period = f64::from(rate) / f64::from(frequency);
        let settle = (f64::from(settle_periods) * period) as usize;
        let measure = (f64::from(measure_periods) * period).round() as usize;
        let (mut re, mut im) = (0.0f64, 0.0f64);
        for i in 0..settle + measure {
            let phase = 2.0 * PI * f64::from(frequency) * i as f64 / f64::from(rate);
            let s = (amplitude * libm::sin(phase)) as i32;
            let mut frame = [s, s];
            eq.process_stereo(&mut frame);
            if i >= settle {
                re += f64::from(frame[0]) * libm::sin(phase);
                im += f64::from(frame[0]) * libm::cos(phase);
            }
        }
        (2.0 * libm::sqrt(re * re + im * im) / measure as f64 / amplitude) as f32
    }

    #[test]
    fn low_bands_stay_accurate_at_768k_in_f32() {
        let rate = 768_000;
        for (frequency, gain) in [(20.0, 6.0), (31.0, -6.0), (62.0, 9.0)] {
            let mut eq = Equalizer::new();
            eq.configure(&[peak(frequency, 1.0, gain)], 0.0, rate).unwrap();
            let expected = db_to_gain(gain);
            let got = measured_gain(&mut eq, frequency, rate, 6.0, 2.0);
            assert!((got / expected - 1.0).abs() < 0.01, "{frequency} Hz: {got} vs {expected}");
        }
    }

    #[test]
    fn simulated_and_analytic_responses_agree() {
        let rate = 96_000;
        let bands = [
            Band { kind: Kind::LowShelf, frequency_hz: 120.0, q: 0.7, gain_db: 5.0 },
            peak(1_000.0, 2.0, -7.0),
            Band { kind: Kind::HighShelf, frequency_hz: 9_000.0, q: 0.7, gain_db: 3.0 },
        ];
        let cascade = Cascade::design(&bands, rate).unwrap();
        for probe in [60.0, 1_000.0, 4_000.0, 16_000.0] {
            let mut eq = Equalizer::new();
            eq.configure(&bands, -6.0, rate).unwrap();
            let got = gain_to_db(measured_gain(&mut eq, probe, rate, 40.0, 50.0)) + 6.0;
            let expected = cascade.magnitude_db(probe, rate);
            assert!((got - expected).abs() < 0.1, "{probe} Hz: {got} vs {expected}");
        }
    }

    #[test]
    fn overload_saturates_instead_of_wrapping() {
        let mut eq = Equalizer::new();
        eq.configure(&[peak(1_000.0, 0.5, 18.0)], 0.0, 48_000).unwrap();
        let mut frames = [i32::MAX; 2_000];
        eq.process_stereo(&mut frames);
        assert!(frames.iter().all(|s| *s > 0));
        assert!(eq.clipped_samples() > 0);
    }

    fn brute_peak(cascade: &Cascade, rate: u32) -> f32 {
        let fs = f64::from(rate);
        let points = 400_000;
        let mut best = f64::MIN;
        for i in 0..=points {
            let f = 0.1 * libm::pow(0.5 * fs / 0.1, i as f64 / points as f64);
            best = best.max(cascade.response_db(f, fs));
        }
        best as f32
    }

    #[test]
    fn the_peak_search_finds_narrow_and_out_of_band_maxima() {
        let rate = 48_000;
        let old_grid = libm::powf(21_600.0 / 20.0, 1.0 / 240.0);
        let between = 20.0 * libm::powf(old_grid, 100.5);
        let narrow = [peak(between, 20.0, 18.0)];
        let cascade = Cascade::design(&narrow, rate).unwrap();
        assert!((cascade.peak_db(rate) - 18.0).abs() < 0.01, "{}", cascade.peak_db(rate));
        assert!(auto_preamp_db(&narrow, rate).unwrap() <= -18.0);

        let low = [Band { kind: Kind::LowShelf, frequency_hz: 12.0, q: 0.7, gain_db: 12.0 }];
        assert!(auto_preamp_db(&low, rate).unwrap() <= -12.0);
        let high = [Band { kind: Kind::HighShelf, frequency_hz: 30_000.0, q: 0.7, gain_db: 12.0 }];
        assert!(auto_preamp_db(&high, 96_000).unwrap() <= -12.0);
        let bump = [Band { kind: Kind::LowShelf, frequency_hz: 15.0, q: 4.0, gain_db: 9.0 }];
        let bumped = Cascade::design(&bump, rate).unwrap();
        assert!(bumped.peak_db(rate) > 9.5, "a resonant shelf overshoots its plateau: {}", bumped.peak_db(rate));

        let profiles: [&[Band]; 5] = [
            &narrow,
            &[peak(31.0, 0.5, 6.0), peak(62.0, 12.0, 9.0), peak(8_000.0, 20.0, 18.0)],
            &[
                Band { kind: Kind::LowShelf, frequency_hz: 90.0, q: 0.7, gain_db: 6.0 },
                peak(2_500.0, 15.0, 8.0),
                peak(2_600.0, 15.0, 8.0),
                Band { kind: Kind::HighShelf, frequency_hz: 18_000.0, q: 2.5, gain_db: 4.0 },
            ],
            &bump,
            &[peak(10.0, 0.1, 18.0), peak(20_000.0, 0.1, 18.0)],
        ];
        for bands in profiles {
            for rate in [44_100, 96_000, 384_000] {
                let cascade = Cascade::design(bands, rate).unwrap();
                let (found, brute) = (cascade.peak_db(rate), brute_peak(&cascade, rate));
                assert!(found >= brute - 0.005 && found <= brute + 0.05, "{rate}: {found} vs {brute} for {bands:?}");
            }
        }
    }

    #[test]
    fn flat_shelves_and_bands_above_the_rate_are_bypassed() {
        let flat = [
            Band { kind: Kind::LowShelf, frequency_hz: 100.0, q: 0.7, gain_db: 0.0 },
            Band { kind: Kind::HighShelf, frequency_hz: 8_000.0, q: 0.7, gain_db: 0.0 },
            peak(1_000.0, 1.0, 0.0),
            peak(30_000.0, 1.0, 6.0),
        ];
        let mut eq = Equalizer::new();
        eq.configure(&flat, 0.0, 48_000).unwrap();
        assert!(eq.is_bypassed());
        let original = [i32::MIN, -1, 0, 1, 0x7FFF_FF01, i32::MAX];
        let mut frames = original;
        eq.process_stereo(&mut frames);
        assert_eq!(frames, original);
        eq.configure(&flat, 0.0, 96_000).unwrap();
        assert!(!eq.is_bypassed(), "the 30 kHz band applies at 96 kHz");
        assert_eq!(Cascade::design(&flat, 44_100).unwrap().peak_db(44_100), 0.0);
        assert_eq!(Cascade::design(&[peak(0.0, 1.0, 3.0)], 48_000), Err(DesignError::Frequency));
        assert_eq!(Cascade::design(&[peak(30_000.0, 0.01, 3.0)], 48_000), Err(DesignError::Q));
    }

    #[test]
    fn filter_state_follows_its_band_when_the_layout_changes() {
        let rate = 48_000;
        let mut eq = Equalizer::new();
        let a = peak(100.0, 1.0, 6.0);
        let b = Band { kind: Kind::HighShelf, frequency_hz: 6_000.0, q: 0.7, gain_db: 3.0 };
        eq.configure(&[a, b], 0.0, rate).unwrap();
        let mut frames = [0x1000_0000i32; 64];
        eq.process_stereo(&mut frames);
        let before = eq.active.state;
        eq.configure(&[Band { gain_db: 4.0, ..a }, b], 0.0, rate).unwrap();
        assert_eq!(eq.active.state, before, "a gain change keeps the running state");
        eq.configure(&[b, a], 0.0, rate).unwrap();
        assert_eq!(eq.active.state, [[[0.0; 2]; MAX_BANDS]; 2], "swapped bands start from rest");
        eq.process_stereo(&mut frames);
        eq.configure(&[Band { gain_db: 0.0, ..b }, a], 0.0, rate).unwrap();
        assert_eq!(eq.active.state[0][0], [0.0; 2], "a band that moved to another section starts from rest");
    }

    #[test]
    fn silence_after_a_signal_settles_to_exact_zero_state() {
        let mut eq = Equalizer::new();
        eq.configure(&[peak(40.0, 8.0, 12.0), peak(12_000.0, 4.0, -9.0)], -12.0, 48_000).unwrap();
        let mut burst = [0x4000_0000i32; 256];
        eq.process_stereo(&mut burst);
        let mut silence = [0i32; 4_096];
        for _ in 0..200 {
            eq.process_stereo(&mut silence);
        }
        assert!(silence.iter().all(|s| *s == 0));
        assert!(eq.active.state.iter().flatten().flatten().all(|v| *v == 0.0 || v.is_normal()));
        assert!(eq.active.state.iter().flatten().flatten().all(|v| *v == 0.0), "{:?}", eq.active.state);
    }

    #[test]
    fn protocol_profiles_convert_and_get_a_safe_preamp() {
        let mut bands = heapless::Vec::new();
        bands.push(EqBand { kind: FilterKind::Peak, frequency_hz: 1_000, q_milli: 20_000, gain_centi_db: 1_800 }).unwrap();
        bands.push(EqBand { kind: FilterKind::Peak, frequency_hz: 1_000, q_milli: 1_000, gain_centi_db: 1_800 }).unwrap();
        let profile = EqProfile { preamp_centi_db: 0, bands };
        assert_eq!(Band::from(&profile.bands[0]), peak(1_000.0, 20.0, 18.0));
        assert_eq!(auto_preamp_centi_db(&profile, 48_000), Ok(EqProfile::MIN_PREAMP_CENTI_DB));
        let gentle = EqProfile { preamp_centi_db: 0, bands: profile.bands[1..].iter().copied().collect() };
        assert_eq!(auto_preamp_centi_db(&gentle, 48_000), Ok(-1_800));
        let cascade = profile_cascade(&gentle, 48_000).unwrap();
        assert!((cascade.magnitude_db(1_000.0, 48_000) - 18.0).abs() < 0.01);
        assert_eq!(auto_preamp_centi_db(&EqProfile { preamp_centi_db: 0, bands: heapless::Vec::new() }, 48_000), Ok(0));
        let mut eq = Equalizer::new();
        eq.configure_profile(&gentle, 48_000).unwrap();
        assert_eq!(eq.band_count(), 1);
        assert!(!eq.is_bypassed());
    }

    #[test]
    fn ten_mixed_bands_match_analytic_response_across_chunk_boundaries() {
        let rate = 96_000;
        let bands = [
            Band { kind: Kind::LowShelf, frequency_hz: 60.0, q: 0.707, gain_db: 4.0 },
            peak(125.0, 1.4, 3.0),
            peak(250.0, 1.4, -2.0),
            peak(500.0, 1.4, 2.5),
            peak(1_000.0, 1.4, -3.0),
            peak(2_000.0, 1.4, 2.0),
            peak(4_000.0, 1.4, -1.5),
            peak(8_000.0, 1.4, 3.0),
            Band { kind: Kind::HighShelf, frequency_hz: 12_000.0, q: 0.707, gain_db: 2.0 },
            Band { kind: Kind::HighPass, frequency_hz: 25.0, q: 0.707, gain_db: 0.0 },
        ];
        let cascade = Cascade::design(&bands, rate).unwrap();
        let mut eq = Equalizer::new();
        eq.configure(&bands, -6.0, rate).unwrap();
        assert_eq!(eq.band_count(), 10);
        let freq = 1_000.0f64;
        let amplitude = 0.125 * 2_147_483_648.0f64;
        let total = 4_800usize;
        let mut frames = std::vec![0i32; total * 2];
        for i in 0..total {
            let phase = 2.0 * PI * freq * i as f64 / f64::from(rate);
            let s = (amplitude * libm::sin(phase)) as i32;
            frames[i * 2] = s;
            frames[i * 2 + 1] = -s;
        }
        eq.process_stereo(&mut frames);
        let (mut re_l, mut im_l, mut re_r, mut im_r) = (0.0, 0.0, 0.0, 0.0);
        let settle = 2_400usize;
        for i in settle..total {
            let phase = 2.0 * PI * freq * i as f64 / f64::from(rate);
            let (s, c) = (libm::sin(phase), libm::cos(phase));
            re_l += f64::from(frames[i * 2]) * s;
            im_l += f64::from(frames[i * 2]) * c;
            re_r += f64::from(frames[i * 2 + 1]) * s;
            im_r += f64::from(frames[i * 2 + 1]) * c;
        }
        let n = (total - settle) as f64;
        let gain_l = gain_to_db((2.0 * libm::sqrt(re_l * re_l + im_l * im_l) / n / amplitude) as f32) + 6.0;
        let gain_r = gain_to_db((2.0 * libm::sqrt(re_r * re_r + im_r * im_r) / n / amplitude) as f32) + 6.0;
        let expected = cascade.magnitude_db(1_000.0, rate);
        assert!((gain_l - expected).abs() < 0.15, "L {gain_l} vs {expected}");
        assert!((gain_r - expected).abs() < 0.15, "R {gain_r} vs {expected}");
        assert_eq!(eq.clipped_samples(), 0);
    }

    #[test]
    fn crossfeed_blends_low_frequencies_and_preserves_unity_mono_gain() {
        let rate = 48_000;
        let mut eq = Equalizer::new();
        eq.configure_dsp(DspSettings { crossfeed: CrossfeedMode::Natural, ..DspSettings::default() }, rate).unwrap();
        assert!(!eq.is_bypassed());
        assert!(eq.is_eq_bypassed());
        let level = 0x2000_0000i32;
        let mut mono = [level; 2_048];
        eq.process_stereo(&mut mono);
        assert!((mono[2_046] - level).abs() < 2_000, "mono DC gain drifted: {}", mono[2_046]);
        assert!((mono[2_047] - level).abs() < 2_000);

        let mut eq_pan = Equalizer::new();
        eq_pan.configure_dsp(DspSettings { crossfeed: CrossfeedMode::Natural, ..DspSettings::default() }, rate).unwrap();
        let mut left_only = [0i32; 4_096];
        for pair in left_only.chunks_exact_mut(2) {
            pair[0] = level;
            pair[1] = 0;
        }
        eq_pan.process_stereo(&mut left_only);
        let out_l = left_only[4_094] as f32 / level as f32;
        let out_r = left_only[4_095] as f32 / level as f32;
        assert!((out_l + out_r - 1.0).abs() < 0.01, "sum={}", out_l + out_r);
        assert!(out_r > 0.25 && out_r < 0.36, "crossfeed bleed out of range: {out_r}");
    }

    #[test]
    fn soft_limiter_prevents_hard_clipping_and_stays_linear_below_threshold() {
        let mut eq = Equalizer::new();
        eq.configure(&[peak(1_000.0, 0.5, 18.0)], 0.0, 48_000).unwrap();
        eq.configure_dsp(DspSettings { limiter: true, ..DspSettings::default() }, 48_000).unwrap();
        let mut frames = [i32::MAX / 2; 2_000];
        eq.process_stereo(&mut frames);
        assert_eq!(eq.clipped_samples(), 0);
        assert!(frames.iter().all(|&s| s > 0 && s < i32::MAX));
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.5), -0.5);
        assert!(soft_limit(5.0) < 1.0 && soft_limit(5.0) > LIMITER_THRESHOLD);
        assert!(soft_limit(-5.0) > -1.0 && soft_limit(-5.0) < -LIMITER_THRESHOLD);
    }

    #[test]
    fn mono_and_phase_invert_transform_stereo_pairs_accurately() {
        let mut eq = Equalizer::new();
        eq.configure_dsp(DspSettings { mono: true, invert_phase: true, ..DspSettings::default() }, 48_000).unwrap();
        let mut frames = [0x2000_0000i32, 0x1000_0000, -0x2000_0000, 0x2000_0000];
        eq.process_stereo(&mut frames);
        assert!((frames[0] + 0x1800_0000).abs() <= 2);
        assert_eq!(frames[0], frames[1]);
        assert_eq!(frames[2], 0);
        assert_eq!(frames[3], 0);
    }

    #[test]
    fn swap_channels_and_volume_adaptive_loudness_behave_cleanly() {
        let rate = 48_000;
        let mut swap = Equalizer::new();
        swap.configure_dsp(DspSettings { swap_channels: true, ..DspSettings::default() }, rate).unwrap();
        let mut pair = [0x1234_0000i32, -0x2345_0000];
        swap.process_stereo(&mut pair);
        assert!((pair[0] + 0x2345_0000).abs() <= 2);
        assert!((pair[1] - 0x1234_0000).abs() <= 2);

        let mut eq = Equalizer::new();
        eq.configure_dsp(DspSettings { loudness: true, ..DspSettings::default() }, rate).unwrap();
        eq.set_loudness_volume(-600);
        assert!(eq.is_bypassed());
        eq.set_loudness_volume(-4_800);
        assert!(!eq.is_bypassed());
        assert!(eq.loudness_active());
        let bass = gain_to_db(measured_gain(&mut eq, 50.0, rate, 12.0, 20.0));
        let mid = gain_to_db(measured_gain(&mut eq, 1_000.0, rate, 30.0, 40.0));
        let treble = gain_to_db(measured_gain(&mut eq, 14_000.0, rate, 60.0, 80.0));
        assert!(bass - mid > 5.0 && bass - mid < 7.2, "bass={bass} mid={mid}");
        assert!(treble - mid > 2.0 && treble - mid < 3.6, "treble={treble} mid={mid}");
        assert!(bass <= 0.2, "anti-clipping preamp must prevent bass boost above 0 dB: {bass}");
        assert_eq!(eq.clipped_samples(), 0);
    }

    #[test]
    fn meters_report_peak_rms_limiter_hits_and_reset_between_reads() {
        let mut eq = Equalizer::new();
        let mut sine = [0i32; 960];
        let amp = 0.5 * 2_147_483_648.0f32;
        for (i, pair) in sine.chunks_exact_mut(2).enumerate() {
            let phase = 2.0 * core::f32::consts::PI * i as f32 / 48.0;
            pair[0] = (amp * libm::sinf(phase)) as i32;
            pair[1] = 0;
        }
        eq.observe_stereo(&sine);
        let m = eq.take_meters(true);
        assert!(m.active);
        assert!((m.peak_left_centi_db + 602).abs() < 30, "peak L={}", m.peak_left_centi_db);
        assert!((m.rms_left_centi_db + 903).abs() < 35, "rms L={}", m.rms_left_centi_db);
        assert_eq!(m.peak_right_centi_db, METER_FLOOR_CENTI_DB);
        assert_eq!(m.rms_right_centi_db, METER_FLOOR_CENTI_DB);
        let empty = eq.take_meters(false);
        assert!(!empty.active);
        assert_eq!(empty.peak_left_centi_db, METER_FLOOR_CENTI_DB);
        eq.configure(&[peak(1_000.0, 0.5, 18.0)], 0.0, 48_000).unwrap();
        eq.configure_dsp(DspSettings { limiter: true, ..DspSettings::default() }, 48_000).unwrap();
        let mut hot = [i32::MAX / 2; 256];
        eq.process_stereo(&mut hot);
        let after = eq.take_meters(true);
        assert!(after.limiter_hits > 0);
        assert!(after.peak_left_centi_db > -100);
    }

    fn max_step(samples: &[i32]) -> f32 {
        let scale = 2_147_483_648.0f32;
        let mut worst = 0.0f32;
        for w in samples.windows(4).step_by(2) {
            let dl = ((w[2] as f32) - (w[0] as f32)).abs() / scale;
            let dr = ((w[3] as f32) - (w[1] as f32)).abs() / scale;
            worst = worst.max(dl).max(dr);
        }
        worst
    }

    #[test]
    fn band_four_high_shelf_and_preset_switches_crossfade_without_clicks() {
        let rate = 48_000u32;
        let harman = [
            Band { kind: Kind::LowShelf, frequency_hz: 105.0, q: 0.71, gain_db: 5.0 },
            peak(250.0, 1.1, -1.5),
            peak(3_000.0, 1.4, 2.5),
            Band { kind: Kind::HighShelf, frequency_hz: 10_000.0, q: 0.71, gain_db: 6.0 },
        ];
        let band4_cut =
            [harman[0], harman[1], harman[2], Band { kind: Kind::HighShelf, frequency_hz: 10_000.0, q: 0.71, gain_db: -6.0 }];
        let band4_flat =
            [harman[0], harman[1], harman[2], Band { kind: Kind::HighShelf, frequency_hz: 10_000.0, q: 0.71, gain_db: 0.0 }];
        let warm = [
            Band { kind: Kind::LowShelf, frequency_hz: 110.0, q: 0.71, gain_db: 4.0 },
            peak(220.0, 0.9, 1.5),
            peak(6_500.0, 2.0, -2.0),
            Band { kind: Kind::HighShelf, frequency_hz: 10_000.0, q: 0.71, gain_db: -4.0 },
        ];
        let mut eq = Equalizer::new();
        eq.configure(&harman, -6.0, rate).unwrap();

        let amp = 0.25 * 2_147_483_648.0f64;
        let gen_block = |start: usize, frames: usize| -> std::vec::Vec<i32> {
            let mut out = std::vec![0i32; frames * 2];
            for i in 0..frames {
                let t = (start + i) as f64 / f64::from(rate);
                let l = 0.5 * libm::sin(2.0 * PI * 400.0 * t) + 0.5 * libm::sin(2.0 * PI * 3_500.0 * t);
                let r = 0.5 * libm::sin(2.0 * PI * 400.0 * t + 0.4) + 0.5 * libm::sin(2.0 * PI * 3_500.0 * t + 0.7);
                out[i * 2] = (amp * l) as i32;
                out[i * 2 + 1] = (amp * r) as i32;
            }
            out
        };

        let mut cursor = 0usize;
        let mut steady = gen_block(cursor, 960);
        cursor += 960;
        eq.process_stereo(&mut steady);
        let steady_max = max_step(&steady[960..]);

        let transitions: [&dyn Fn(&mut Equalizer); 4] = [
            &|e| e.configure(&band4_cut, -5.0, rate).unwrap(),
            &|e| e.configure(&band4_flat, -5.0, rate).unwrap(),
            &|e| e.configure(&warm, -4.5, rate).unwrap(),
            &|e| e.bypass_eq(),
        ];
        let mut prev_tail = [steady[steady.len() - 2], steady[steady.len() - 1]];
        for apply in transitions {
            apply(&mut eq);
            assert!(eq.is_transitioning());
            let mut block = gen_block(cursor, 960);
            cursor += 960;
            eq.process_stereo(&mut block);
            let mut stitched = std::vec::Vec::with_capacity(block.len() + 2);
            stitched.extend_from_slice(&prev_tail);
            stitched.extend_from_slice(&block);
            let peak_delta = max_step(&stitched);
            assert!(
                peak_delta < steady_max * 1.65,
                "click spike detected during transition: {peak_delta} vs steady {steady_max}"
            );
            prev_tail = [block[block.len() - 2], block[block.len() - 1]];
        }
        assert!(eq.is_bypassed());
    }

    #[test]
    fn dsp_toggles_crossfade_smoothly_without_clicks() {
        let rate = 48_000u32;
        let mut eq = Equalizer::new();
        eq.configure_dsp(DspSettings::default(), rate).unwrap();
        let amp = 0.25 * 2_147_483_648.0f64;
        let make_sine = |start: usize, frames: usize| -> std::vec::Vec<i32> {
            let mut out = std::vec![0i32; frames * 2];
            for i in 0..frames {
                let t = (start + i) as f64 / f64::from(rate);
                out[i * 2] = (amp * libm::sin(2.0 * PI * 1_000.0 * t)) as i32;
                out[i * 2 + 1] = (amp * libm::cos(2.0 * PI * 1_000.0 * t)) as i32;
            }
            out
        };
        let mut cursor = 0usize;
        let mut warm = make_sine(cursor, 960);
        cursor += 960;
        eq.process_stereo(&mut warm);
        let steady = max_step(&warm);

        eq.configure_dsp(DspSettings { invert_phase: true, crossfeed: CrossfeedMode::Strong, ..DspSettings::default() }, rate)
            .unwrap();
        assert!(eq.is_transitioning());
        let mut after = make_sine(cursor, 960);
        eq.process_stereo(&mut after);
        let mut stitched = std::vec::Vec::with_capacity(after.len() + 2);
        stitched.push(warm[warm.len() - 2]);
        stitched.push(warm[warm.len() - 1]);
        stitched.extend_from_slice(&after);
        let delta = max_step(&stitched);
        assert!(delta < steady * 1.5, "phase/crossfeed toggle clicked: {delta} vs {steady}");
        assert!(!eq.is_transitioning());
    }

    #[test]
    fn reset_stream_clears_filter_delay_state() {
        let rate = 48_000u32;
        let mut eq = Equalizer::new();
        eq.configure(&[Band { kind: Kind::LowShelf, frequency_hz: 120.0, q: 0.71, gain_db: 12.0 }], -6.0, rate).unwrap();
        eq.configure_dsp(DspSettings { crossfeed: CrossfeedMode::Strong, ..DspSettings::default() }, rate).unwrap();
        let mut loud = [i32::MAX / 4; 256];
        eq.process_stereo(&mut loud);
        eq.reset_stream();
        let mut silence = [0i32; 64];
        eq.process_stereo(&mut silence);
        assert!(silence.iter().all(|&s| s == 0), "stale filter tail leaked into new stream: {silence:?}");
    }

    #[test]
    fn spectrum_separates_sub_bass_and_midrange_tones_across_rates() {
        for rate in [48_000u32, 192_000u32] {
            for (tone_hz, min_band, max_band) in [(60.0f32, 2usize, 4usize), (1_000.0f32, 12usize, 14usize)] {
                let mut eq = Equalizer::new();
                eq.configure(&[], 0.0, rate).unwrap();
                eq.set_spectrum_enabled(true);
                let frames = (rate as usize) / 2;
                let amp = 0.5 * 2_147_483_648.0f64;
                let mut pcm = std::vec![0i32; frames * 2];
                for i in 0..frames {
                    let s = (amp * libm::sin(2.0 * PI * f64::from(tone_hz) * i as f64 / f64::from(rate))) as i32;
                    pcm[i * 2] = s;
                    pcm[i * 2 + 1] = s;
                }
                eq.observe_stereo(&pcm);
                let spec = eq.take_spectrum(true);
                assert!(spec.active);
                let (peak_idx, peak_val) =
                    spec.bands_db_x2.iter().enumerate().max_by_key(|&(_, &v)| v).map(|(i, &v)| (i, v)).unwrap();
                assert!(
                    (min_band..=max_band).contains(&peak_idx),
                    "rate={rate} tone={tone_hz} peaked at band {peak_idx} ({:?})",
                    spec.bands_db_x2
                );
                assert!(peak_val > 130, "expected strong peak for -6 dBFS tone, got {peak_val}");
                assert!(spec.lufs_momentary_centi > -1_400 && spec.lufs_momentary_centi < -300);
            }
        }
    }

    #[test]
    fn true_peak_catches_intersample_overshoot_and_agc_levels_loud_signal() {
        let rate = 48_000u32;
        let mut eq = Equalizer::new();
        eq.configure(&[], 0.0, rate).unwrap();
        eq.set_spectrum_enabled(true);
        let mut inter = [0i32; 256];
        let val = (0.95 * 2_147_483_648.0) as i32;
        for (i, pair) in inter.chunks_exact_mut(2).enumerate() {
            let s = match i & 3 {
                0 | 1 => val,
                _ => -val,
            };
            pair[0] = s;
            pair[1] = s;
        }
        eq.observe_stereo(&inter);
        let spec = eq.take_spectrum(true);
        assert!(spec.true_peak_left_centi_db > 0, "expected >0 dBTP inter-sample peak, got {}", spec.true_peak_left_centi_db);

        let mut agc_eq = Equalizer::new();
        agc_eq
            .configure_dsp_extras(
                DspExtras { auto_level: true, auto_level_target_lufs: -18, subsonic_hz: 0, mono_bass_hz: 0 },
                rate,
            )
            .unwrap();
        let amp = 0.7 * 2_147_483_648.0f64;
        for block in 0..18 {
            let mut pcm = std::vec![0i32; 9_600];
            for i in 0..4_800 {
                let idx = block * 4_800 + i;
                let s = (amp * libm::sin(2.0 * PI * 1_000.0 * idx as f64 / f64::from(rate))) as i32;
                pcm[i * 2] = s;
                pcm[i * 2 + 1] = s;
            }
            agc_eq.process_stereo(&mut pcm);
        }
        let after = agc_eq.take_spectrum(true);
        assert!(
            after.auto_level_gain_centi_db < -300,
            "AGC should attenuate loud signal, got {}",
            after.auto_level_gain_centi_db
        );
    }

    #[test]
    fn two_layer_eq_subsonic_and_mono_bass_behave_accurately() {
        let rate = 48_000u32;
        let mut base_bands = heapless::Vec::new();
        base_bands.push(EqBand { kind: FilterKind::Peak, frequency_hz: 1_000, q_milli: 1_000, gain_centi_db: 400 }).unwrap();
        let base = EqBaseSettings { enabled: true, profile: EqProfile { preamp_centi_db: -400, bands: base_bands } };
        let tone = ToneEqSettings { enabled: true, mid_centi_db: 300, ..ToneEqSettings::default() };
        let mut eq = Equalizer::new();
        eq.configure_base_eq(&base, rate).unwrap();
        eq.configure_tone_eq(tone, rate).unwrap();
        let g1k = gain_to_db(measured_gain(&mut eq, 1_000.0, rate, 20.0, 30.0));
        let g100 = gain_to_db(measured_gain(&mut eq, 100.0, rate, 12.0, 16.0));
        assert!((g1k - g100 - 7.0).abs() < 0.4, "base +4 dB + tone +3 dB at 1 kHz: g1k={g1k}, g100={g100}");

        let mut mb = Equalizer::new();
        mb.configure_dsp_extras(
            DspExtras { auto_level: false, auto_level_target_lufs: -16, subsonic_hz: 30, mono_bass_hz: 120 },
            rate,
        )
        .unwrap();
        let amp = 0.25 * 2_147_483_648.0f64;
        let mut low_left = std::vec![0i32; 9_600];
        for i in 0..4_800 {
            low_left[i * 2] = (amp * libm::sin(2.0 * PI * 45.0 * i as f64 / f64::from(rate))) as i32;
        }
        mb.process_stereo(&mut low_left);
        let tail = &low_left[4_800..];
        let max_l = tail.iter().step_by(2).map(|s| s.unsigned_abs()).max().unwrap() as f64;
        let max_r = tail.iter().skip(1).step_by(2).map(|s| s.unsigned_abs()).max().unwrap() as f64;
        assert!(max_r > max_l * 0.6, "mono-bass should collapse 45 Hz L-only into R: L={max_l} R={max_r}");

        let mut hi_left = std::vec![0i32; 4_800];
        for i in 0..2_400 {
            hi_left[i * 2] = (amp * libm::sin(2.0 * PI * 2_000.0 * i as f64 / f64::from(rate))) as i32;
        }
        mb.process_stereo(&mut hi_left);
        let hi_tail = &hi_left[2_400..];
        let hi_l = hi_tail.iter().step_by(2).map(|s| s.unsigned_abs()).max().unwrap() as f64;
        let hi_r = hi_tail.iter().skip(1).step_by(2).map(|s| s.unsigned_abs()).max().unwrap() as f64;
        assert!(hi_r < hi_l * 0.05, "mono-bass must keep 2 kHz stereo separate: L={hi_l} R={hi_r}");
    }

    #[test]
    fn agc_never_boosts_without_hardware_volume_and_respects_peak_headroom_with_spectrum_off() {
        let rate = 48_000u32;
        let mut android_eq = Equalizer::new();
        android_eq
            .configure_dsp_extras(
                DspExtras { auto_level: true, auto_level_target_lufs: -14, subsonic_hz: 0, mono_bass_hz: 0 },
                rate,
            )
            .unwrap();
        android_eq.set_loudness_volume(0);
        let quiet_amp = 0.05 * 2_147_483_648.0f64;
        for block in 0..30 {
            let mut pcm = std::vec![0i32; 9_600];
            for i in 0..4_800 {
                let idx = block * 4_800 + i;
                let s = (quiet_amp * libm::sin(2.0 * PI * 1_000.0 * idx as f64 / f64::from(rate))) as i32;
                pcm[i * 2] = s;
                pcm[i * 2 + 1] = s;
            }
            android_eq.process_stereo(&mut pcm);
        }
        assert_eq!(android_eq.take_spectrum(true).auto_level_gain_centi_db, 0);

        let mut hw_eq = Equalizer::new();
        hw_eq
            .configure_dsp_extras(
                DspExtras { auto_level: true, auto_level_target_lufs: -14, subsonic_hz: 0, mono_bass_hz: 0 },
                rate,
            )
            .unwrap();
        hw_eq.set_loudness_volume(-2_000);
        for block in 0..40 {
            let mut pcm = std::vec![0i32; 9_600];
            for i in 0..4_800 {
                let idx = block * 4_800 + i;
                let s = (quiet_amp * libm::sin(2.0 * PI * 1_000.0 * idx as f64 / f64::from(rate))) as i32;
                pcm[i * 2] = s;
                pcm[i * 2 + 1] = s;
            }
            hw_eq.process_stereo(&mut pcm);
        }
        let boosted = hw_eq.take_spectrum(true).auto_level_gain_centi_db;
        assert!(boosted > 250 && boosted <= 400, "hardware volume should cap gentle boost at +4 dB, got {boosted}");

        let mut spiky_eq = Equalizer::new();
        spiky_eq
            .configure_dsp_extras(
                DspExtras { auto_level: true, auto_level_target_lufs: -14, subsonic_hz: 0, mono_bass_hz: 0 },
                rate,
            )
            .unwrap();
        spiky_eq.set_loudness_volume(-2_000);
        let peak_amp = (0.85 * 2_147_483_648.0) as i32;
        for block in 0..30 {
            let mut pcm = std::vec![0i32; 9_600];
            for i in 0..4_800 {
                let idx = block * 4_800 + i;
                let s = if i == 100 {
                    peak_amp
                } else {
                    (quiet_amp * libm::sin(2.0 * PI * 1_000.0 * idx as f64 / f64::from(rate))) as i32
                };
                pcm[i * 2] = s;
                pcm[i * 2 + 1] = s;
            }
            spiky_eq.process_stereo(&mut pcm);
        }
        let guarded = spiky_eq.take_spectrum(true).auto_level_gain_centi_db;
        assert!(guarded <= 110, "headroom guard must hold with spectrum_enabled=false, got {guarded}");
    }

    #[test]
    fn take_spectrum_with_polls_across_fft_stages_and_matches_take_spectrum() {
        let rate = 48_000u32;
        let mut a = Equalizer::new();
        let mut b = Equalizer::new();
        a.configure(&[], 0.0, rate).unwrap();
        b.configure(&[], 0.0, rate).unwrap();
        a.set_spectrum_enabled(true);
        b.set_spectrum_enabled(true);
        let frames = 24_000usize;
        let amp = 0.5 * 2_147_483_648.0f64;
        let mut pcm = std::vec![0i32; frames * 2];
        for i in 0..frames {
            let s = (amp * libm::sin(2.0 * PI * 1_000.0 * i as f64 / f64::from(rate))) as i32;
            pcm[i * 2] = s;
            pcm[i * 2 + 1] = s;
        }
        a.observe_stereo(&pcm);
        b.observe_stereo(&pcm);
        let plain = a.take_spectrum(true);
        let mut polls = 0u32;
        let sliced = b.take_spectrum_with(true, || polls += 1);
        assert_eq!(plain, sliced);
        assert!(polls >= 15, "expected fine-grained FFT poll slices, got {polls}");
    }

    #[test]
    fn incremental_spectrum_steps_match_take_spectrum() {
        let rate = 48_000u32;
        let mut a = Equalizer::new();
        let mut b = Equalizer::new();
        a.configure(&[], 0.0, rate).unwrap();
        b.configure(&[], 0.0, rate).unwrap();
        a.set_spectrum_enabled(true);
        b.set_spectrum_enabled(true);
        let frames = 24_000usize;
        let amp = 0.5 * 2_147_483_648.0f64;
        let mut pcm = std::vec![0i32; frames * 2];
        for i in 0..frames {
            let s = (amp * libm::sin(2.0 * PI * 440.0 * i as f64 / f64::from(rate))) as i32;
            pcm[i * 2] = s;
            pcm[i * 2 + 1] = s;
        }
        a.observe_stereo(&pcm);
        b.observe_stereo(&pcm);
        let expected = a.take_spectrum(true);
        assert!(b.start_spectrum(true).is_none());
        assert!(b.spectrum_busy());
        let mut steps = 0u32;
        let mut got = None;
        while got.is_none() {
            steps += 1;
            got = b.step_spectrum();
            assert!(steps < 64);
        }
        assert!(!b.spectrum_busy());
        assert_eq!(expected, got.unwrap());
        assert!(steps >= 20, "expected fine-grained incremental steps, got {steps}");
    }
}

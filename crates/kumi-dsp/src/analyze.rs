// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use core::f32::consts::TAU;

use kumi_icd::{AudioSpectrum, LUFS_FLOOR_CENTI, METER_FLOOR_CENTI_DB, SPECTRUM_BANDS};

use crate::{Band, Kind, Section, db_to_gain, gain_to_db, mul_add, mul_sub};

const HI_LEN: usize = 256;
const LO_LEN: usize = 128;
const LO_DECIM: u8 = 16;
const LUFS_BLOCKS: usize = 30;
const MOMENTARY_BLOCKS: usize = 4;
const AGC_GATE_LUFS: f32 = -45.0;
const AGC_MIN_DB: f32 = -15.0;
const AGC_MAX_BOOST_DB: f32 = 4.0;
const AGC_ATTACK_DB_PER_BLOCK: f32 = 0.60;
const AGC_RELEASE_DB_PER_BLOCK: f32 = 0.15;
const AGC_HEADROOM_DB: f32 = -0.5;
const SPECTRUM_FLOOR_DB: f32 = -80.0;
const LOW_BAND_CUTOFF_HZ: f32 = 240.0;

pub const BAND_CENTERS_HZ: [f32; SPECTRUM_BANDS] = [
    25.0, 33.0, 44.0, 59.0, 78.0, 104.0, 138.0, 184.0, 245.0, 326.0, 434.0, 578.0, 770.0, 1_025.0, 1_365.0, 1_818.0, 2_420.0,
    3_223.0, 4_292.0, 5_715.0, 7_610.0, 10_133.0, 13_493.0, 17_966.0,
];

#[inline]
pub fn decim_step_for(sample_rate: u32) -> u8 {
    let step = (sample_rate.saturating_add(24_000) / 48_000).clamp(1, 16);
    step as u8
}

const SPECTRUM_BANDS_PER_STEP: usize = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SpectrumStage {
    Idle,
    LoWindow,
    LoBitRev,
    LoFft(usize),
    LoMag,
    HiWindow,
    HiBitRev,
    HiFft(usize),
    HiMag,
    Bands(usize),
}

#[derive(Clone, Copy, Debug)]
struct SpectrumJob {
    stage: SpectrumStage,
    re: [f32; HI_LEN],
    im: [f32; HI_LEN],
    lo_mag: [f32; LO_LEN / 2],
    hi_mag: [f32; HI_LEN / 2],
    bands: [u8; SPECTRUM_BANDS],
    fs_hi: f32,
    fs_lo: f32,
    tp_l: f32,
    tp_r: f32,
    auto_level: bool,
    has_lo: bool,
}

impl SpectrumJob {
    const fn new() -> Self {
        Self {
            stage: SpectrumStage::Idle,
            re: [0.0; HI_LEN],
            im: [0.0; HI_LEN],
            lo_mag: [0.0; LO_LEN / 2],
            hi_mag: [0.0; HI_LEN / 2],
            bands: [0; SPECTRUM_BANDS],
            fs_hi: 48_000.0,
            fs_lo: 3_000.0,
            tp_l: 0.0,
            tp_r: 0.0,
            auto_level: false,
            has_lo: false,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Analyzer {
    sample_rate: u32,
    decim_step: u8,
    inv_decim: f32,
    decim_sub: u8,
    decim_l: f32,
    decim_r: f32,
    hi_ring: [f32; HI_LEN],
    hi_pos: u16,
    hi_filled: bool,
    lo_ring: [f32; LO_LEN],
    lo_pos: u16,
    lo_sub: u8,
    lo_acc: f32,
    lo_filled: bool,
    k_pre: Option<Section>,
    k_rlb: Option<Section>,
    k_state: [[[f32; 2]; 2]; 2],
    tp_hist: [[f32; 4]; 2],
    tp_l: f32,
    tp_r: f32,
    block_energy: f32,
    block_frames: u32,
    block_target: u32,
    block_pre_peak: f32,
    blocks: [f32; LUFS_BLOCKS],
    block_idx: u8,
    blocks_filled: u8,
    agc_gain_db: f32,
    agc_gain_lin: f32,
    allow_boost: bool,
    spectrum_enabled: bool,
    job: SpectrumJob,
}

impl Analyzer {
    pub const fn new() -> Self {
        Self {
            sample_rate: 0,
            decim_step: 1,
            inv_decim: 1.0,
            decim_sub: 0,
            decim_l: 0.0,
            decim_r: 0.0,
            hi_ring: [0.0; HI_LEN],
            hi_pos: 0,
            hi_filled: false,
            lo_ring: [0.0; LO_LEN],
            lo_pos: 0,
            lo_sub: 0,
            lo_acc: 0.0,
            lo_filled: false,
            k_pre: None,
            k_rlb: None,
            k_state: [[[0.0; 2]; 2]; 2],
            tp_hist: [[0.0; 4]; 2],
            tp_l: 0.0,
            tp_r: 0.0,
            block_energy: 0.0,
            block_frames: 0,
            block_target: 4_800,
            block_pre_peak: 0.0,
            blocks: [0.0; LUFS_BLOCKS],
            block_idx: 0,
            blocks_filled: 0,
            agc_gain_db: 0.0,
            agc_gain_lin: 1.0,
            allow_boost: false,
            spectrum_enabled: false,
            job: SpectrumJob::new(),
        }
    }

    pub fn set_spectrum_enabled(&mut self, enabled: bool) {
        self.spectrum_enabled = enabled;
    }

    pub fn spectrum_enabled(&self) -> bool {
        self.spectrum_enabled
    }

    pub fn set_allow_boost(&mut self, allow: bool) {
        self.allow_boost = allow;
        if !allow && self.agc_gain_db > 0.0 {
            self.agc_gain_db = 0.0;
            self.agc_gain_lin = 1.0;
        }
    }

    pub fn agc_gain_lin(&self) -> f32 {
        self.agc_gain_lin
    }

    pub fn agc_gain_centi_db(&self) -> i16 {
        (self.agc_gain_db * 100.0).clamp(-1_800.0, 1_200.0) as i16
    }

    pub fn reset_agc(&mut self) {
        self.block_pre_peak = 0.0;
        self.agc_gain_db = 0.0;
        self.agc_gain_lin = 1.0;
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    pub fn configure_rate(&mut self, sample_rate: u32) {
        if sample_rate == self.sample_rate || sample_rate == 0 {
            return;
        }
        self.sample_rate = sample_rate;
        let step = decim_step_for(sample_rate);
        self.decim_step = step;
        self.inv_decim = 1.0 / f32::from(step);
        let eff_rate = (sample_rate / u32::from(step)).max(8_000);
        self.block_target = (eff_rate / 10).max(1);
        self.k_pre =
            Section::design(&Band { kind: Kind::HighShelf, frequency_hz: 1_681.97, q: 0.707_175, gain_db: 4.0 }, eff_rate).ok();
        self.k_rlb =
            Section::design(&Band { kind: Kind::HighPass, frequency_hz: 38.135, q: 0.500_327, gain_db: 0.0 }, eff_rate).ok();
        self.reset_stream();
    }

    pub fn reset_stream(&mut self) {
        self.decim_sub = 0;
        self.decim_l = 0.0;
        self.decim_r = 0.0;
        self.hi_ring = [0.0; HI_LEN];
        self.hi_pos = 0;
        self.hi_filled = false;
        self.lo_ring = [0.0; LO_LEN];
        self.lo_pos = 0;
        self.lo_sub = 0;
        self.lo_acc = 0.0;
        self.lo_filled = false;
        self.k_state = [[[0.0; 2]; 2]; 2];
        self.tp_hist = [[0.0; 4]; 2];
        self.tp_l = 0.0;
        self.tp_r = 0.0;
        self.block_energy = 0.0;
        self.block_frames = 0;
        self.block_pre_peak = 0.0;
        self.blocks = [0.0; LUFS_BLOCKS];
        self.block_idx = 0;
        self.blocks_filled = 0;
        self.agc_gain_db = 0.0;
        self.agc_gain_lin = 1.0;
        self.job.stage = SpectrumStage::Idle;
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    #[inline(never)]
    fn update_true_peak_channel(hist: &mut [f32; 4], sample: f32, peak: &mut f32) {
        let x0 = hist[1];
        let x1 = hist[2];
        let x2 = hist[3];
        let x3 = sample;
        *hist = [x0, x1, x2, x3];
        let a1 = x1.abs();
        let a2 = x2.abs();
        let cur = *peak;
        if a2 > cur {
            *peak = a2;
        }
        if a1 > cur * 0.5 || a2 > cur * 0.5 {
            let c0 = x1;
            let c1 = 0.5 * (x2 - x0);
            let c2 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
            let c3 = 0.5 * (x3 - x0) + 1.5 * (x1 - x2);
            let mut p = *peak;
            for t in [0.25f32, 0.50f32, 0.75f32] {
                let v = mul_add(mul_add(mul_add(c3, t, c2), t, c1), t, c0).abs();
                if v > p {
                    p = v;
                }
            }
            *peak = p;
        }
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    #[inline(never)]
    pub fn push_output_frame(&mut self, out_l: f32, out_r: f32, pre_l: f32, pre_r: f32, auto_level: bool, target_lufs: f32) {
        if !self.spectrum_enabled && !auto_level {
            return;
        }
        if auto_level {
            let p = pre_l.abs().max(pre_r.abs());
            if p > self.block_pre_peak {
                self.block_pre_peak = p;
            }
        }
        if self.spectrum_enabled {
            Self::update_true_peak_channel(&mut self.tp_hist[0], out_l, &mut self.tp_l);
            Self::update_true_peak_channel(&mut self.tp_hist[1], out_r, &mut self.tp_r);
        }
        self.decim_l += pre_l;
        self.decim_r += pre_r;
        self.decim_sub += 1;
        if self.decim_sub < self.decim_step {
            return;
        }
        self.decim_sub = 0;
        let inv = self.inv_decim;
        let dl = self.decim_l * inv;
        let dr = self.decim_r * inv;
        self.decim_l = 0.0;
        self.decim_r = 0.0;

        if self.spectrum_enabled {
            let mono = 0.5 * (out_l + out_r);
            let hp = usize::from(self.hi_pos);
            self.hi_ring[hp] = mono;
            let next_hp = hp + 1;
            if next_hp >= HI_LEN {
                self.hi_pos = 0;
                self.hi_filled = true;
            } else {
                self.hi_pos = next_hp as u16;
            }
            self.lo_acc += mono;
            self.lo_sub += 1;
            if self.lo_sub >= LO_DECIM {
                self.lo_sub = 0;
                let lp = usize::from(self.lo_pos);
                self.lo_ring[lp] = self.lo_acc * (1.0 / f32::from(LO_DECIM));
                self.lo_acc = 0.0;
                let next_lp = lp + 1;
                if next_lp >= LO_LEN {
                    self.lo_pos = 0;
                    self.lo_filled = true;
                } else {
                    self.lo_pos = next_lp as u16;
                }
            }
        }

        let mut kl = dl;
        let mut kr = dr;
        if let Some(pre) = self.k_pre {
            kl = pre.tick(kl, &mut self.k_state[0][0]);
            kr = pre.tick(kr, &mut self.k_state[1][0]);
        }
        if let Some(rlb) = self.k_rlb {
            kl = rlb.tick(kl, &mut self.k_state[0][1]);
            kr = rlb.tick(kr, &mut self.k_state[1][1]);
        }
        self.block_energy = mul_add(kl, kl, mul_add(kr, kr, self.block_energy));
        self.block_frames += 1;
        if self.block_frames >= self.block_target {
            let mean_sq = self.block_energy / self.block_frames as f32;
            self.block_energy = 0.0;
            self.block_frames = 0;
            let idx = usize::from(self.block_idx);
            self.blocks[idx] = mean_sq;
            self.block_idx = ((idx + 1) % LUFS_BLOCKS) as u8;
            if usize::from(self.blocks_filled) < LUFS_BLOCKS {
                self.blocks_filled += 1;
            }
            if auto_level {
                self.step_agc(target_lufs);
            } else {
                self.block_pre_peak = 0.0;
                self.agc_gain_db = 0.0;
                self.agc_gain_lin = 1.0;
            }
        }
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    fn step_agc(&mut self, target_lufs: f32) {
        let peak = self.block_pre_peak;
        self.block_pre_peak = 0.0;
        let count = usize::from(self.blocks_filled);
        if count == 0 {
            return;
        }
        let window = if count >= 6 { count } else { count.min(MOMENTARY_BLOCKS) };
        let measured = self.lufs_over(window);
        let max_db = if self.allow_boost { AGC_MAX_BOOST_DB } else { 0.0 };
        if measured > AGC_GATE_LUFS {
            let desired = (target_lufs - measured).clamp(AGC_MIN_DB, max_db);
            let cur = self.agc_gain_db.min(max_db);
            let next = if desired < cur {
                (cur - AGC_ATTACK_DB_PER_BLOCK).max(desired)
            } else {
                (cur + AGC_RELEASE_DB_PER_BLOCK).min(desired)
            };
            let safe = if peak > 1e-5 {
                let safe_max = (AGC_HEADROOM_DB - gain_to_db(peak)).clamp(AGC_MIN_DB, max_db);
                next.min(safe_max)
            } else {
                next
            };
            self.agc_gain_db = safe;
            self.agc_gain_lin = db_to_gain(safe);
        } else if self.agc_gain_db > max_db {
            self.agc_gain_db = max_db;
            self.agc_gain_lin = db_to_gain(max_db);
        }
    }

    fn lufs_over(&self, window: usize) -> f32 {
        let filled = usize::from(self.blocks_filled);
        let take = window.min(filled);
        if take == 0 {
            return -70.0;
        }
        let mut sum = 0.0f32;
        let head = usize::from(self.block_idx);
        for k in 0..take {
            let idx = (head + LUFS_BLOCKS - 1 - k) % LUFS_BLOCKS;
            sum += self.blocks[idx];
        }
        let mean = sum / take as f32;
        if mean <= 1e-10 { -70.0 } else { (-0.691 + 10.0 * libm::log10f(mean)).max(-70.0) }
    }

    pub fn spectrum_busy(&self) -> bool {
        !matches!(self.job.stage, SpectrumStage::Idle)
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    #[inline(never)]
    pub fn start_spectrum(&mut self, active: bool, auto_level: bool) -> Option<AudioSpectrum> {
        let tp_l = self.tp_l;
        let tp_r = self.tp_r;
        self.tp_l = 0.0;
        self.tp_r = 0.0;
        if !active || self.sample_rate == 0 {
            self.job.stage = SpectrumStage::Idle;
            return Some(AudioSpectrum::default());
        }
        self.job.tp_l = tp_l;
        self.job.tp_r = tp_r;
        self.job.auto_level = auto_level;
        self.job.bands = [0u8; SPECTRUM_BANDS];
        let fs_hi = (self.sample_rate as f32 / f32::from(self.decim_step)).max(8_000.0);
        self.job.fs_hi = fs_hi;
        self.job.fs_lo = fs_hi / f32::from(LO_DECIM);
        let has_hi = self.hi_filled || self.hi_pos >= 64;
        let has_lo = self.lo_filled || self.lo_pos >= 32;
        self.job.has_lo = has_lo;
        if !has_hi {
            self.job.stage = SpectrumStage::Idle;
            return Some(self.finish_spectrum());
        }
        self.job.stage = if has_lo { SpectrumStage::LoWindow } else { SpectrumStage::HiWindow };
        None
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    #[inline(never)]
    pub fn step_spectrum(&mut self) -> Option<AudioSpectrum> {
        match self.job.stage {
            SpectrumStage::Idle => None,
            SpectrumStage::LoWindow => {
                copy_hann_window(&self.lo_ring, usize::from(self.lo_pos), &mut self.job.re[..LO_LEN]);
                self.job.im[..LO_LEN].fill(0.0);
                self.job.stage = SpectrumStage::LoBitRev;
                None
            }
            SpectrumStage::LoBitRev => {
                bit_reverse(&mut self.job.re[..LO_LEN], &mut self.job.im[..LO_LEN]);
                self.job.stage = SpectrumStage::LoFft(2);
                None
            }
            SpectrumStage::LoFft(len) => {
                fft_stage(&mut self.job.re[..LO_LEN], &mut self.job.im[..LO_LEN], len);
                self.job.stage = if len < LO_LEN { SpectrumStage::LoFft(len << 1) } else { SpectrumStage::LoMag };
                None
            }
            SpectrumStage::LoMag => {
                let norm_lo = 16.0 / ((LO_LEN * LO_LEN) as f32);
                for k in 1..(LO_LEN / 2) {
                    self.job.lo_mag[k] = mul_add(self.job.re[k], self.job.re[k], self.job.im[k] * self.job.im[k]) * norm_lo;
                }
                self.job.stage = SpectrumStage::HiWindow;
                None
            }
            SpectrumStage::HiWindow => {
                copy_hann_window(&self.hi_ring, usize::from(self.hi_pos), &mut self.job.re);
                self.job.im.fill(0.0);
                self.job.stage = SpectrumStage::HiBitRev;
                None
            }
            SpectrumStage::HiBitRev => {
                bit_reverse(&mut self.job.re, &mut self.job.im);
                self.job.stage = SpectrumStage::HiFft(2);
                None
            }
            SpectrumStage::HiFft(len) => {
                fft_stage(&mut self.job.re, &mut self.job.im, len);
                self.job.stage = if len < HI_LEN { SpectrumStage::HiFft(len << 1) } else { SpectrumStage::HiMag };
                None
            }
            SpectrumStage::HiMag => {
                let norm_hi = 16.0 / ((HI_LEN * HI_LEN) as f32);
                for k in 1..(HI_LEN / 2) {
                    self.job.hi_mag[k] = mul_add(self.job.re[k], self.job.re[k], self.job.im[k] * self.job.im[k]) * norm_hi;
                }
                self.job.stage = SpectrumStage::Bands(0);
                None
            }
            SpectrumStage::Bands(start) => {
                let end = (start + SPECTRUM_BANDS_PER_STEP).min(SPECTRUM_BANDS);
                let df_lo = self.job.fs_lo / LO_LEN as f32;
                let df_hi = self.job.fs_hi / HI_LEN as f32;
                for b in start..end {
                    let fc = BAND_CENTERS_HZ[b];
                    let f_lo_edge = if b == 0 { 20.0 } else { libm::sqrtf(BAND_CENTERS_HZ[b - 1] * fc) };
                    let f_hi_edge = if b + 1 == SPECTRUM_BANDS {
                        (self.job.fs_hi * 0.48).min(20_000.0)
                    } else {
                        libm::sqrtf(fc * BAND_CENTERS_HZ[b + 1])
                    };
                    let power = if fc < LOW_BAND_CUTOFF_HZ && self.job.has_lo {
                        band_peak_power(&self.job.lo_mag, df_lo, f_lo_edge, f_hi_edge)
                    } else {
                        band_peak_power(&self.job.hi_mag, df_hi, f_lo_edge, f_hi_edge)
                    };
                    self.job.bands[b] = power_to_u8_db_x2(power);
                }
                if end < SPECTRUM_BANDS {
                    self.job.stage = SpectrumStage::Bands(end);
                    None
                } else {
                    self.job.stage = SpectrumStage::Idle;
                    Some(self.finish_spectrum())
                }
            }
        }
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    fn finish_spectrum(&self) -> AudioSpectrum {
        let auto_level = self.job.auto_level;
        let gain_offset = if auto_level { self.agc_gain_db } else { 0.0 };
        let mom = self.lufs_over(MOMENTARY_BLOCKS);
        let st = self.lufs_over(LUFS_BLOCKS);
        let mom_out = if mom <= -69.9 { LUFS_FLOOR_CENTI } else { ((mom + gain_offset) * 100.0).clamp(-7_000.0, 600.0) as i16 };
        let st_out = if st <= -69.9 { LUFS_FLOOR_CENTI } else { ((st + gain_offset) * 100.0).clamp(-7_000.0, 600.0) as i16 };
        AudioSpectrum {
            active: true,
            bands_db_x2: self.job.bands,
            true_peak_left_centi_db: true_peak_centi_db(self.job.tp_l),
            true_peak_right_centi_db: true_peak_centi_db(self.job.tp_r),
            lufs_momentary_centi: mom_out,
            lufs_short_term_centi: st_out,
            auto_level_gain_centi_db: if auto_level { self.agc_gain_centi_db() } else { 0 },
        }
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    #[inline(never)]
    pub fn take_spectrum(&mut self, active: bool, auto_level: bool) -> AudioSpectrum {
        self.take_spectrum_with(active, auto_level, || {})
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
    #[inline(never)]
    pub fn take_spectrum_with(&mut self, active: bool, auto_level: bool, mut poll: impl FnMut()) -> AudioSpectrum {
        if let Some(done) = self.start_spectrum(active, auto_level) {
            return done;
        }
        loop {
            poll();
            if let Some(done) = self.step_spectrum() {
                return done;
            }
        }
    }
}

fn true_peak_centi_db(linear: f32) -> i16 {
    if linear <= 1.584_893_2e-5 {
        return METER_FLOOR_CENTI_DB;
    }
    let db = gain_to_db(linear) * 100.0;
    db.clamp(f32::from(METER_FLOOR_CENTI_DB), 600.0) as i16
}

fn band_peak_power(mag: &[f32], df: f32, f_lo: f32, f_hi: f32) -> f32 {
    let max_bin = mag.len().saturating_sub(1);
    if max_bin < 1 || df <= 0.0 {
        return 0.0;
    }
    let b0 = (libm::floorf(f_lo / df) as isize).clamp(1, max_bin as isize) as usize;
    let b1 = (libm::ceilf(f_hi / df) as isize).clamp(b0 as isize, max_bin as isize) as usize;
    let mut peak = 0.0f32;
    let mut sum = 0.0f32;
    for &p in &mag[b0..=b1] {
        if p > peak {
            peak = p;
        }
        sum += p;
    }
    let avg = sum / ((b1 - b0 + 1) as f32);
    0.65 * peak + 0.35 * avg
}

fn power_to_u8_db_x2(power: f32) -> u8 {
    if power <= 1e-8 {
        return 0;
    }
    let db = 10.0 * libm::log10f(power);
    let scaled = (db - SPECTRUM_FLOOR_DB) * 2.0;
    scaled.clamp(0.0, 160.0) as u8
}

fn copy_hann_window(ring: &[f32], pos: usize, out: &mut [f32]) {
    let n = ring.len();
    let step = TAU / (n as f32);
    let c_step = libm::cosf(step);
    let s_step = libm::sinf(step);
    let mut c = 1.0f32;
    let mut s = 0.0f32;
    let mut idx = pos % n;
    for slot in out.iter_mut() {
        let w = 0.5 * (1.0 - c);
        *slot = ring[idx] * w;
        idx += 1;
        if idx >= n {
            idx = 0;
        }
        let next_c = mul_sub(c, c_step, s * s_step);
        let next_s = mul_add(s, c_step, c * s_step);
        c = next_c;
        s = next_s;
    }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
fn bit_reverse(re: &mut [f32], im: &mut [f32]) {
    let n = re.len();
    let mut j = 0usize;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.cold"))]
fn fft_stage(re: &mut [f32], im: &mut [f32], len: usize) {
    let n = re.len();
    let half = len >> 1;
    let angle = -TAU / (len as f32);
    let wr_step = libm::cosf(angle);
    let wi_step = libm::sinf(angle);
    let mut start = 0usize;
    while start < n {
        let mut wr = 1.0f32;
        let mut wi = 0.0f32;
        for k in 0..half {
            let u = start + k;
            let v = u + half;
            let vr = re[v];
            let vi = im[v];
            let tr = mul_sub(wr, vr, wi * vi);
            let ti = mul_add(wr, vi, wi * vr);
            let ur = re[u];
            let ui = im[u];
            re[u] = ur + tr;
            im[u] = ui + ti;
            re[v] = ur - tr;
            im[v] = ui - ti;
            let next_wr = mul_sub(wr, wr_step, wi * wi_step);
            let next_wi = mul_add(wr, wi_step, wi * wr_step);
            wr = next_wr;
            wi = next_wi;
        }
        start += len;
    }
}

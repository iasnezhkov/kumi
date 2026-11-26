// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::Vec;
use kumi_icd::{AUDIO_SWEEP_COUNTS, AudioBusFault, AudioClockFault, AudioDacFault, AudioDmaFault, AudioFault, AudioPhase};

use crate::clock::{ClockError, TIMER_MEASURED_COUNT, TIMER_REFERENCE_COUNT};
use crate::dac::DacError;
use crate::flow::FrameStore;
use crate::i2c::I2cError;
use crate::i2s::I2sError;
use crate::regs::{Mismatch, Registers};
use crate::stream::{Stage, StreamError};

const Q30: i64 = 1 << 30;
const Q31: u64 = 1 << 31;
const QUADRANT: u32 = 1 << 30;
const HALF_TURN: u32 = 1 << 31;
const SINE_Q30: [i64; 6] = [1_686_629_713, -693_598_668, 85_569_306, -5_026_995, 172_272, -3_864];
const DB_Q31: [u64; 20] = [
    2_147_483_648,
    1_913_946_816,
    1_705_806_895,
    1_520_301_996,
    1_354_970_580,
    1_207_618_800,
    1_076_291_389,
    959_245_710,
    854_928_639,
    761_955_951,
    679_093_957,
    605_243_126,
    539_423_504,
    480_761_704,
    428_479_319,
    381_882_595,
    340_353_221,
    303_340_128,
    270_352_174,
    240_951_628,
];
const LN10_PER_2000_Q31: u64 = 2_472_382;
const CENTI_DB_PER_STEP: u32 = 100;
const CENTI_DB_PER_DECADE: u32 = 2_000;

pub struct CountTap<'a, R> {
    registers: &'a mut R,
    reference: Option<u32>,
    counts: Vec<[u32; 2], AUDIO_SWEEP_COUNTS>,
}

impl<'a, R: Registers> CountTap<'a, R> {
    pub fn new(registers: &'a mut R) -> Self {
        Self { registers, reference: None, counts: Vec::new() }
    }

    pub fn counts(&self) -> &[[u32; 2]] {
        &self.counts
    }

    pub fn into_counts(self) -> Vec<[u32; 2], AUDIO_SWEEP_COUNTS> {
        self.counts
    }
}

impl<R: Registers> Registers for CountTap<'_, R> {
    fn read(&mut self, address: u32) -> u32 {
        let value = self.registers.read(address);
        if address == TIMER_REFERENCE_COUNT {
            self.reference = Some(value);
        } else if address == TIMER_MEASURED_COUNT {
            let _ = self.counts.push([self.reference.take().unwrap_or(0), value]);
        }
        value
    }

    fn write(&mut self, address: u32, value: u32) {
        self.registers.write(address, value);
    }

    fn write_pair(&mut self, first: (u32, u32), second: (u32, u32)) {
        self.registers.write_pair(first, second);
    }

    fn read_pair(&mut self, first: u32, second: u32) -> (u32, u32) {
        let (a, b) = self.registers.read_pair(first, second);
        if (first, second) == (TIMER_REFERENCE_COUNT, TIMER_MEASURED_COUNT) {
            let _ = self.counts.push([a, b]);
        }
        (a, b)
    }

    fn delay_us(&mut self, microseconds: u32) {
        self.registers.delay_us(microseconds);
    }

    fn update(&mut self, address: u32, clear: u32, set: u32) {
        self.registers.update(address, clear, set);
    }
}

pub fn sine_q30(phase: u32) -> i32 {
    let offset = i64::from(phase & (QUADRANT - 1));
    let x = if phase & QUADRANT == 0 { offset } else { Q30 - offset };
    let x2 = (x * x) >> 30;
    let mut sum = SINE_Q30[SINE_Q30.len() - 1];
    for coefficient in SINE_Q30[..SINE_Q30.len() - 1].iter().rev() {
        sum = *coefficient + ((sum * x2) >> 30);
    }
    let value = ((sum * x) >> 30).min(Q30) as i32;
    if phase & HALF_TURN == 0 { value } else { -value }
}

pub fn amplitude(level_centi_dbfs: i16) -> Option<i32> {
    if level_centi_dbfs > 0 {
        return None;
    }
    let attenuation = u32::from(level_centi_dbfs.unsigned_abs());
    let rest = attenuation % CENTI_DB_PER_DECADE;
    let y = u64::from(rest % CENTI_DB_PER_STEP) * LN10_PER_2000_Q31;
    let y2 = (y * y) >> 31;
    let y3 = (y2 * y) >> 31;
    let y4 = (y3 * y) >> 31;
    let fraction = Q31 - y + y2 / 2 - y3 / 6 + y4 / 24;
    let mut value = (DB_Q31[(rest / CENTI_DB_PER_STEP) as usize] * fraction) >> 31;
    for _ in 0..attenuation / CENTI_DB_PER_DECADE {
        value /= 10;
    }
    Some(value.min(i32::MAX as u64) as i32)
}

const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let rest = a % b;
        a = b;
        b = rest;
    }
    a
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tone {
    rate_hz: u32,
    tone_hz: u32,
    amplitude: i32,
    slots: [bool; 2],
    invert_right: bool,
    frames: usize,
    periods: u32,
}

impl Tone {
    pub const fn silence(capacity: usize) -> Option<Self> {
        if capacity == 0 {
            return None;
        }
        Some(Self { rate_hz: 0, tone_hz: 0, amplitude: 0, slots: [false; 2], invert_right: false, frames: capacity, periods: 0 })
    }

    pub fn sine(rate_hz: u32, tone_hz: u32, level_centi_dbfs: i16, slots: [bool; 2], capacity: usize) -> Option<Self> {
        if tone_hz == 0 || tone_hz.checked_mul(2)? >= rate_hz {
            return None;
        }
        let amplitude = amplitude(level_centi_dbfs)?;
        let divisor = gcd(rate_hz, tone_hz);
        let period = (rate_hz / divisor) as usize;
        let blocks = capacity / period;
        if blocks == 0 {
            return None;
        }
        Some(Self {
            rate_hz,
            tone_hz,
            amplitude,
            slots,
            invert_right: false,
            frames: blocks * period,
            periods: blocks as u32 * (tone_hz / divisor),
        })
    }

    pub const fn with_inverted_right(mut self, invert: bool) -> Self {
        self.invert_right = invert;
        self
    }

    pub const fn frames(&self) -> usize {
        self.frames
    }

    pub const fn periods(&self) -> u32 {
        self.periods
    }

    pub const fn amplitude(&self) -> i32 {
        self.amplitude
    }

    pub fn frame(&self, index: usize) -> [i32; 2] {
        if self.tone_hz == 0 {
            return [0; 2];
        }
        let step = (index as u64 * u64::from(self.tone_hz)) % u64::from(self.rate_hz);
        let phase = ((step << 32) / u64::from(self.rate_hz)) as u32;
        let value = ((i64::from(sine_q30(phase)) * i64::from(self.amplitude)) >> 30) as i32;
        let left = if self.slots[0] { value } else { 0 };
        let right = if self.slots[1] { if self.invert_right { value.wrapping_neg() } else { value } } else { 0 };
        [left, right]
    }

    pub fn fill<S: FrameStore + ?Sized>(&self, store: &mut S) -> usize {
        let frames = self.frames.min(store.capacity());
        for index in 0..frames {
            store.store(index, self.frame(index));
        }
        frames
    }
}

pub const fn phase(stage: Stage) -> AudioPhase {
    match stage {
        Stage::Clock => AudioPhase::Clock,
        Stage::ResetLow => AudioPhase::ResetLow,
        Stage::ResetHigh => AudioPhase::ResetHigh,
        Stage::Dac | Stage::Ready => AudioPhase::Dac,
    }
}

impl From<ClockError> for AudioClockFault {
    fn from(error: ClockError) -> Self {
        match error {
            ClockError::NoTrimWindow => Self::NoTrimWindow,
            ClockError::DisjointTrimWindows => Self::DisjointTrimWindows,
            ClockError::InvalidReference { trim, reference } => Self::InvalidReference { trim, reference },
            ClockError::TimerInUse => Self::TimerInUse,
            ClockError::TimeWentBackwards => Self::TimeWentBackwards,
            ClockError::NotReady => Self::NotReady,
        }
    }
}

impl From<I2cError> for AudioBusFault {
    fn from(error: I2cError) -> Self {
        match error {
            I2cError::Nack => Self::Nack,
            I2cError::Bus => Self::Bus,
            I2cError::Timeout => Self::Timeout,
            I2cError::Invalid => Self::Invalid,
            I2cError::Unrecovered => Self::Unrecovered,
        }
    }
}

impl From<DacError> for AudioDacFault {
    fn from(error: DacError) -> Self {
        match error {
            DacError::Bus { dac, register, error } => Self::Bus { address: dac.address(), register, error: error.into() },
            DacError::Readback { dac, register, expected, actual } => {
                Self::Readback { address: dac.address(), register, expected, actual }
            }
            DacError::Identity { dac, bytes } => Self::Identity { address: dac.address(), bytes },
            DacError::IdentityChanged { dac, bytes } => Self::IdentityChanged { address: dac.address(), bytes },
            DacError::NegativeControlAcknowledged => Self::NegativeControlAcknowledged,
            DacError::NegativeControl(error) => Self::NegativeControl(error.into()),
            DacError::TimeWentBackwards => Self::TimeWentBackwards,
            DacError::Stopped => Self::Stopped,
        }
    }
}

impl From<I2sError> for AudioDmaFault {
    fn from(error: I2sError) -> Self {
        match error {
            I2sError::Buffer => Self::Buffer,
            I2sError::StopTimeout => Self::StopTimeout,
            I2sError::ResetStuck => Self::ResetStuck,
            I2sError::StartFailed => Self::StartFailed,
            I2sError::Cursor | I2sError::Probe(_) => Self::Cursor,
            I2sError::NotRunning => Self::NotRunning,
        }
    }
}

impl From<StreamError> for AudioFault {
    fn from(error: StreamError) -> Self {
        match error {
            StreamError::Clock(error) => Self::Clock(error.into()),
            StreamError::Dac(error) => Self::Dac(error.into()),
            StreamError::Dma(error) => Self::Dma(error.into()),
            StreamError::TimeWentBackwards => Self::TimeWentBackwards,
        }
    }
}

impl From<Mismatch> for AudioFault {
    fn from(mismatch: Mismatch) -> Self {
        Self::Setup { address: mismatch.address, expected: mismatch.expected, actual: mismatch.actual }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{Calibration, Family};
    use crate::dac::Dac;
    use crate::testing::Recorder;
    use std::vec::Vec as StdVec;

    fn scripted(r: &mut Recorder) -> StdVec<[u32; 2]> {
        let mut pairs = StdVec::new();
        for (family, window) in [(Family::Rate48k, 4..=9), (Family::Rate44k1, 10..=15)] {
            for trim in 0..16u32 {
                let reference = 3_100 + trim;
                let (low, high) = family.pll().bounds(reference).unwrap();
                let measured = if window.contains(&trim) { (low + high) / 2 } else { high + 1 + trim };
                r.script(TIMER_REFERENCE_COUNT, &[reference]);
                r.script(TIMER_MEASURED_COUNT, &[measured]);
                pairs.push([reference, measured]);
            }
        }
        pairs
    }

    #[test]
    fn the_tap_records_every_sweep_count_without_changing_the_calibration() {
        let mut plain = Recorder::default();
        scripted(&mut plain);
        let expected = Calibration::run(&mut plain);
        let mut tapped = Recorder::default();
        let pairs = scripted(&mut tapped);
        let mut tap = CountTap::new(&mut tapped);
        assert_eq!(Calibration::run(&mut tap), expected);
        assert_eq!(tap.counts(), pairs.as_slice());
        assert_eq!(tap.into_counts().len(), AUDIO_SWEEP_COUNTS);
        assert_eq!(tapped.log, plain.log);
        assert_eq!(expected, Ok(Calibration { trims: [6, 12], windows: [0x03f0, 0xfc00] }));
    }

    #[test]
    fn a_failed_calibration_still_reports_the_counts_it_read() {
        let mut r = Recorder::default();
        let mut tap = CountTap::new(&mut r);
        assert_eq!(Calibration::run(&mut tap), Err(ClockError::InvalidReference { trim: 0, reference: 0 }));
        assert_eq!(tap.counts(), [[0, 0]]);
    }

    #[test]
    fn the_fixed_point_sine_follows_the_float_sine() {
        let mut worst = 0f64;
        for step in 0..100_003u64 {
            let phase = (step * 42_949) as u32;
            let exact = (f64::from(phase) / 4_294_967_296.0 * std::f64::consts::TAU).sin() * Q30 as f64;
            worst = worst.max((f64::from(sine_q30(phase)) - exact).abs());
        }
        assert!(worst < 80.0, "{worst}");
        assert_eq!(sine_q30(0), 0);
        assert_eq!(sine_q30(HALF_TURN), 0);
        assert_eq!(sine_q30(QUADRANT), -sine_q30(3 * QUADRANT));
        assert!(Q30 - i64::from(sine_q30(QUADRANT)) < 80);
        assert!((0..=u32::MAX).step_by(65_537).all(|phase| i64::from(sine_q30(phase)).abs() <= Q30));
    }

    #[test]
    fn levels_scale_full_scale_by_their_decibels() {
        for level in kumi_icd::AudioStart::MIN_LEVEL_CENTI_DBFS..=0 {
            let exact = (2_147_483_648.0 * 10f64.powf(f64::from(level) / 2_000.0)).min(f64::from(i32::MAX));
            let actual = f64::from(amplitude(level).unwrap());
            assert!((actual - exact).abs() <= exact * 1e-4 + 1.0, "{level}: {actual} {exact}");
        }
        assert_eq!(amplitude(0), Some(i32::MAX));
        assert_eq!(amplitude(-2_000), Some(214_748_364));
        assert_eq!(amplitude(1), None);
        assert!(amplitude(i16::MIN).is_some());
    }

    #[test]
    fn a_tone_fills_whole_periods_so_a_circular_ring_plays_on_without_a_seam() {
        let tone = Tone::sine(48_000, 1_000, -2_000, [true, false], 2_048).unwrap();
        assert_eq!((tone.frames(), tone.periods()), (2_016, 42));
        assert_eq!(tone.frame(tone.frames()), tone.frame(0));
        assert!(tone.amplitude() - tone.frame(12)[0] < 32 && tone.frame(12)[1] == 0);
        assert!((0..tone.frames()).all(|i| tone.frame(i)[1] == 0 && tone.frame(i)[0].abs() <= tone.amplitude()));
        let cd = Tone::sine(44_100, 1_000, -2_000, [true, true], 2_048).unwrap();
        assert_eq!((cd.frames(), cd.periods()), (1_764, 40));
        assert_eq!(cd.frame(cd.frames() - 1), cd.frame(2 * cd.frames() - 1));
        let peak = (0..cd.frames()).map(|i| cd.frame(i)[0]).max().unwrap();
        assert!(i64::from(cd.amplitude() - peak) < i64::from(cd.amplitude()) / 1_000);
        assert!((0..cd.frames()).all(|i| cd.frame(i)[0] == cd.frame(i)[1]));
        let right = Tone::sine(96_000, 3_000, 0, [false, true], 64).unwrap();
        assert_eq!((right.frames(), right.periods(), right.frame(8)[0]), (64, 2, 0));
        assert!(i32::MAX - right.frame(8)[1] < 256);
        let phase = Tone::sine(48_000, 1_000, -2_000, [true, true], 2_048).unwrap().with_inverted_right(true);
        assert_eq!(phase.frame(12)[0], -phase.frame(12)[1]);
    }

    #[test]
    fn tones_that_cannot_repeat_inside_the_ring_are_refused() {
        assert_eq!(Tone::sine(48_000, 997, -2_000, [true; 2], 2_048), None);
        assert_eq!(Tone::sine(48_000, 24_000, -2_000, [true; 2], 2_048), None);
        assert_eq!(Tone::sine(48_000, 0, -2_000, [true; 2], 2_048), None);
        assert_eq!(Tone::sine(48_000, 1_000, 1, [true; 2], 2_048), None);
        assert_eq!(Tone::sine(48_000, 1_000, -2_000, [true; 2], 47), None);
        assert_eq!(Tone::sine(0, 1_000, -2_000, [true; 2], 2_048), None);
        assert_eq!(Tone::sine(u32::MAX, u32::MAX / 2 + 1, -2_000, [true; 2], 2_048), None);
        assert_eq!(Tone::silence(0), None);
    }

    #[test]
    fn fill_writes_every_frame_of_the_ring_once() {
        let mut ring = [[7i32; 2]; 100];
        let silence = Tone::silence(80).unwrap();
        assert_eq!((silence.fill(ring.as_mut_slice()), silence.periods(), silence.amplitude()), (80, 0, 0));
        assert!(ring[..80].iter().all(|frame| *frame == [0, 0]));
        assert_eq!(ring[80], [7, 7]);
        let tone = Tone::sine(48_000, 6_000, -600, [true, true], 100).unwrap();
        assert_eq!(tone.fill(&mut ring[..50]), 50);
        assert_eq!(tone.frames(), 96);
        assert!((0..50).all(|i| ring[i] == tone.frame(i)));
    }

    #[test]
    fn errors_keep_their_details_on_the_wire() {
        let readback = DacError::Readback { dac: Dac::Secondary, register: 0x02_0000, expected: 1, actual: 0xf2 };
        assert_eq!(
            AudioFault::from(StreamError::Dac(readback)),
            AudioFault::Dac(AudioDacFault::Readback { address: 0x31, register: 0x02_0000, expected: 1, actual: 0xf2 })
        );
        let bus = DacError::Bus { dac: Dac::Primary, register: 0x01_0000, error: I2cError::Nack };
        assert_eq!(
            AudioDacFault::from(bus),
            AudioDacFault::Bus { address: 0x33, register: 0x01_0000, error: AudioBusFault::Nack }
        );
        assert_eq!(
            AudioFault::from(StreamError::Clock(ClockError::InvalidReference { trim: 3, reference: 70_000 })),
            AudioFault::Clock(AudioClockFault::InvalidReference { trim: 3, reference: 70_000 })
        );
        assert_eq!(AudioFault::from(StreamError::Dma(I2sError::StopTimeout)), AudioFault::Dma(AudioDmaFault::StopTimeout));
        assert_eq!(
            AudioFault::from(Mismatch { address: 0x2007_0000, expected: 0x1_7800, actual: 0 }),
            AudioFault::Setup { address: 0x2007_0000, expected: 0x1_7800, actual: 0 }
        );
        assert_eq!(
            [Stage::Clock, Stage::ResetLow, Stage::ResetHigh, Stage::Dac, Stage::Ready].map(phase),
            [AudioPhase::Clock, AudioPhase::ResetLow, AudioPhase::ResetHigh, AudioPhase::Dac, AudioPhase::Dac]
        );
    }
}

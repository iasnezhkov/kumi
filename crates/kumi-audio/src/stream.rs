// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::board::{self, DAC_RESET_HOLD_US};
use crate::clock::{Calibration, ClockError, Family, PllSwitch};
use crate::dac::{self, DacError, Filter, Program, Progress, Runner, Volume};
use crate::i2c::I2cBus;
use crate::i2s::{I2sDma, I2sError};
use crate::pcm::{DsdRate, PcmProfile};
use crate::regs::{Registers, apply, apply_one};
use kumi_icd::DacExtras;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StreamError {
    Clock(ClockError),
    Dac(DacError),
    Dma(I2sError),
    TimeWentBackwards,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stage {
    Clock,
    ResetLow,
    ResetHigh,
    Dac,
    Ready,
}

pub struct Start {
    stage: Stage,
    pll: PllSwitch,
    runner: Runner,
    wake_us: u64,
    last_us: u64,
}

impl Start {
    pub fn pcm<R: Registers>(
        registers: &mut R,
        calibration: &Calibration,
        profile: PcmProfile,
        filter: Filter,
        volume: Volume,
        now_us: u64,
    ) -> Self {
        Self::with_program(registers, calibration, profile.family(), dac::pcm_start(profile, filter, volume), now_us)
    }

    pub fn pcm_on<R: Registers>(
        registers: &mut R,
        calibration: &Calibration,
        profile: PcmProfile,
        filter: Filter,
        volume: Volume,
        now_us: u64,
        dacs: &[dac::Dac],
    ) -> Self {
        let program = dac::only(&dac::pcm_start(profile, filter, volume), dacs);
        Self::with_program(registers, calibration, profile.family(), program, now_us)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn pcm_on_with_extras<R: Registers>(
        registers: &mut R,
        calibration: &Calibration,
        profile: PcmProfile,
        filter: Filter,
        extras: DacExtras,
        volume: Volume,
        now_us: u64,
        dacs: &[dac::Dac],
    ) -> Self {
        let program = dac::only(&dac::pcm_start_with_extras(profile, filter, extras, volume), dacs);
        Self::with_program(registers, calibration, profile.family(), program, now_us)
    }

    pub fn dsd_on<R: Registers>(
        registers: &mut R,
        calibration: &Calibration,
        rate: DsdRate,
        volume: Volume,
        now_us: u64,
        dacs: &[dac::Dac],
    ) -> Self {
        let program = dac::only(&dac::dsd_start_rate(rate, volume), dacs);
        Self::with_program(registers, calibration, Family::Rate44k1, program, now_us)
    }

    pub fn dsd_on_with_extras<R: Registers>(
        registers: &mut R,
        calibration: &Calibration,
        rate: DsdRate,
        extras: DacExtras,
        volume: Volume,
        now_us: u64,
        dacs: &[dac::Dac],
    ) -> Self {
        let program = dac::only(&dac::dsd_start_with_extras(rate, extras, volume), dacs);
        Self::with_program(registers, calibration, Family::Rate44k1, program, now_us)
    }

    pub fn with_program<R: Registers>(
        registers: &mut R,
        calibration: &Calibration,
        family: Family,
        program: Program,
        now_us: u64,
    ) -> Self {
        apply(registers, &board::QUIET);
        Self {
            stage: Stage::Clock,
            pll: PllSwitch::begin(registers, calibration, family, now_us),
            runner: Runner::new(program),
            wake_us: now_us,
            last_us: now_us,
        }
    }

    pub const fn stage(&self) -> Stage {
        self.stage
    }

    pub fn poll<R: Registers, B: I2cBus>(
        &mut self,
        registers: &mut R,
        bus: &mut B,
        now_us: u64,
    ) -> Result<Progress, StreamError> {
        if now_us < self.last_us {
            return Err(StreamError::TimeWentBackwards);
        }
        self.last_us = now_us;
        match self.stage {
            Stage::Clock => {
                if !self.pll.poll(registers, now_us).map_err(StreamError::Clock)? {
                    return Ok(Progress::Pending { wake_us: self.pll.wake_us() });
                }
                apply_one(registers, board::dac_reset(true));
                self.wake_us = now_us.saturating_add(u64::from(DAC_RESET_HOLD_US));
                self.stage = Stage::ResetLow;
            }
            Stage::ResetLow | Stage::ResetHigh if now_us < self.wake_us => {
                return Ok(Progress::Pending { wake_us: Some(self.wake_us) });
            }
            Stage::ResetLow => {
                apply_one(registers, board::dac_reset(false));
                self.wake_us = now_us.saturating_add(u64::from(DAC_RESET_HOLD_US));
                self.stage = Stage::ResetHigh;
            }
            Stage::ResetHigh => self.stage = Stage::Dac,
            Stage::Dac => match self.runner.poll(bus, now_us).map_err(StreamError::Dac)? {
                Progress::Done => self.stage = Stage::Ready,
                pending => return Ok(pending),
            },
            Stage::Ready => {}
        }
        Ok(if self.stage == Stage::Ready { Progress::Done } else { Progress::Pending { wake_us: None } })
    }

    pub fn abort<R: Registers>(mut self, registers: &mut R) {
        self.runner.abort();
        apply(registers, &board::QUIET);
        if self.stage == Stage::Clock {
            self.pll.abort(registers);
        }
    }
}

pub fn stop<R: Registers, B: I2cBus>(registers: &mut R, bus: &mut B, dma: &mut I2sDma) -> Result<(), StreamError> {
    stop_kind(registers, bus, dma, false)
}

pub fn stop_dsd<R: Registers, B: I2cBus>(registers: &mut R, bus: &mut B, dma: &mut I2sDma) -> Result<(), StreamError> {
    stop_kind(registers, bus, dma, true)
}

fn stop_kind<R: Registers, B: I2cBus>(registers: &mut R, bus: &mut B, dma: &mut I2sDma, dsd: bool) -> Result<(), StreamError> {
    apply_one(registers, board::output(false));
    let halted = dma.halt(registers).map_err(StreamError::Dma);
    let mute = if dsd { dac::dsd_volume(Volume::MUTED) } else { dac::mute() };
    let muted = dac::run_blocking(bus, &mute, |ms| registers.delay_us(1_000 * u32::from(ms)))
        .and_then(|()| {
            dac::run_blocking(bus, &dac::power_down(dac::Dac::Primary), |ms| registers.delay_us(1_000 * u32::from(ms)))
        })
        .map_err(StreamError::Dac);
    apply_one(registers, board::dac_reset(true));
    halted.and(muted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{AUDIO_PLL, AUDIO_PLL_FRACTION};
    use crate::dac::{reg, register_prefix};
    use crate::i2c::I2cError;
    use crate::i2s::{DMA_ENABLE, DmaBuffer};
    use crate::testing::Recorder;
    use std::collections::BTreeMap;
    use std::vec::Vec;

    #[derive(Default)]
    struct Bus {
        registers: BTreeMap<(u8, u32), u8>,
        transactions: usize,
        fail: bool,
    }

    fn register(bytes: &[u8]) -> u32 {
        (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2])
    }

    impl I2cBus for Bus {
        fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), I2cError> {
            self.transactions += 1;
            if self.fail {
                return Err(I2cError::Nack);
            }
            self.registers.insert((address, register(bytes)), bytes[4]);
            Ok(())
        }

        fn write_then_read(&mut self, address: u8, prefix: &[u8], buffer: &mut [u8]) -> Result<(), I2cError> {
            self.transactions += 1;
            assert_eq!(prefix[3], register_prefix(0)[3]);
            buffer[0] = self.registers.get(&(address, register(prefix))).copied().unwrap_or(0);
            Ok(())
        }

        fn probe(&mut self, _: u8) -> Result<(), I2cError> {
            Err(I2cError::Nack)
        }
    }

    const CALIBRATION: Calibration = Calibration { trims: [6, 12], windows: [0, 0] };

    #[test]
    fn start_switches_the_clock_pulses_reset_then_programs_the_dacs() {
        let mut r = Recorder::default();
        let mut bus = Bus::default();
        let profile = PcmProfile::from_hz(44_100).unwrap();
        let mut start = Start::pcm(&mut r, &CALIBRATION, profile, Filter::FastLowLatency, Volume::UNITY, 0);
        assert_eq!(r.writes()[..2], [(board::OUTPUT_ENABLE, 0xd0), (board::DAC_RESET, 0x10)]);
        let mut now = 0;
        let mut stages = Vec::new();
        let mut polls = 0;
        loop {
            polls += 1;
            let before = bus.transactions;
            let progress = start.poll(&mut r, &mut bus, now).unwrap();
            assert!(bus.transactions - before <= 1);
            if stages.last() != Some(&start.stage()) {
                stages.push(start.stage());
            }
            match progress {
                Progress::Done => break,
                Progress::Pending { wake_us: Some(wake) } => now = wake,
                Progress::Pending { wake_us: None } => now += 50,
            }
            assert!(polls < 1_000);
        }
        assert_eq!(stages, [Stage::Clock, Stage::ResetLow, Stage::ResetHigh, Stage::Dac, Stage::Ready]);
        assert_eq!(r.get(AUDIO_PLL_FRACTION), Family::Rate44k1.pll().fraction);
        assert_eq!(r.get(AUDIO_PLL) & 1, 1);
        assert_eq!(r.get(board::DAC_RESET), 0x210);
        assert_eq!(bus.registers[&(0x31, reg::POWER_DOWN)], 1);
        assert_eq!(bus.registers[&(0x33, reg::SP_SAMPLE_RATE)], 1);
        assert!(now >= 60 + 1_000 + 2_000 + 8_000 + 60_000, "{now}");
        assert_eq!(r.get(board::OUTPUT_ENABLE), 0xd0);
        assert_eq!(start.poll(&mut r, &mut bus, now), Ok(Progress::Done));
        assert_eq!(start.poll(&mut r, &mut bus, now - 1), Err(StreamError::TimeWentBackwards));
    }

    #[test]
    fn a_dac_failure_surfaces_and_abort_leaves_the_board_quiet() {
        let mut r = Recorder::default();
        r.set(AUDIO_PLL, 0x0004_0243);
        let mut bus = Bus { fail: true, ..Bus::default() };
        let profile = PcmProfile::from_hz(48_000).unwrap();
        let mut start = Start::pcm(&mut r, &CALIBRATION, profile, Filter::FastLowLatency, Volume::UNITY, 0);
        let mut now = 0;
        let error = loop {
            match start.poll(&mut r, &mut bus, now) {
                Ok(Progress::Pending { wake_us: Some(wake) }) => now = wake,
                Ok(_) => now += 10,
                Err(error) => break error,
            }
        };
        assert!(matches!(error, StreamError::Dac(DacError::Bus { error: I2cError::Nack, .. })));
        start.abort(&mut r);
        assert_eq!((r.get(board::OUTPUT_ENABLE), r.get(board::DAC_RESET)), (0xd0, 0x10));
        let mut r2 = Recorder::default();
        r2.set(AUDIO_PLL, 0x0004_0243);
        let early = Start::pcm(&mut r2, &CALIBRATION, profile, Filter::FastLowLatency, Volume::UNITY, 0);
        early.abort(&mut r2);
        assert_eq!(r2.get(AUDIO_PLL), 0x0004_0243);
    }

    #[test]
    fn stop_reaches_the_quiet_state_even_when_the_dac_does_not_answer() {
        let mut r = Recorder::default();
        let mut dma = I2sDma::new(DmaBuffer::new(0x0010_4000, 64).unwrap());
        let mut bus = Bus::default();
        assert_eq!(stop(&mut r, &mut bus, &mut dma), Ok(()));
        assert_eq!(bus.registers[&(0x33, dac::VOLUME_LEFT)], dac::MUTE);
        assert_eq!(bus.registers[&(0x33, reg::POWER_DOWN)], dac::POWER_DOWN_ALL);
        assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_2)], dac::DSD_PATH_2_OFF);
        assert_eq!(r.writes().first(), Some(&(board::OUTPUT_ENABLE, 0xd0)));
        assert_eq!(r.writes().last(), Some(&(board::DAC_RESET, 0x10)));
        let mut failing = Bus { fail: true, ..Bus::default() };
        let mut r = Recorder::default();
        r.script(DMA_ENABLE, &[4; 4_096]);
        assert_eq!(stop(&mut r, &mut failing, &mut dma), Err(StreamError::Dma(I2sError::StopTimeout)));
        assert_eq!(failing.transactions, 1);
        assert_eq!(r.writes().last(), Some(&(board::DAC_RESET, 0x10)));
    }

    #[test]
    fn dsd_start_and_stop_program_dsd_path_and_mute_dsd_volume() {
        for (rate, expected_path2) in [(DsdRate::Dsd64, 0x13), (DsdRate::Dsd128, 0x17), (DsdRate::Dsd256, 0x1b)] {
            let mut r = Recorder::default();
            let mut bus = Bus::default();
            let mut start = Start::dsd_on(&mut r, &CALIBRATION, rate, Volume::MUTED, 0, &[dac::Dac::Primary]);
            let mut now = 0;
            loop {
                match start.poll(&mut r, &mut bus, now).unwrap() {
                    Progress::Done => break,
                    Progress::Pending { wake_us: Some(wake) } => now = wake,
                    Progress::Pending { wake_us: None } => now += 50,
                }
            }
            assert_eq!(r.get(AUDIO_PLL_FRACTION), Family::Rate44k1.pll().fraction);
            assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_1)], 0xe0);
            assert_eq!(bus.registers[&(0x33, reg::DSD_INTERFACE)], 0x00);
            assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_2)], expected_path2);
            assert_eq!(bus.registers[&(0x33, reg::POWER_DOWN)], 0x01);
            let mut dma = I2sDma::new(DmaBuffer::new(0x0010_4000, 64).unwrap());
            assert_eq!(stop_dsd(&mut r, &mut bus, &mut dma), Ok(()));
            assert_eq!(bus.registers[&(0x33, dac::DSD_VOLUME_LEFT)], dac::MUTE);
            assert_eq!(bus.registers[&(0x33, dac::DSD_VOLUME_RIGHT)], dac::MUTE);
            assert_eq!(bus.registers[&(0x33, reg::POWER_DOWN)], dac::POWER_DOWN_ALL);
            assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_2)], dac::DSD_PATH_2_OFF);
        }
    }
}

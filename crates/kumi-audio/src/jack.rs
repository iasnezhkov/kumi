// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_chip::regs as hw;
use kumi_icd::Jack;

use crate::regs::{MmioOp, Registers, apply};

pub const CMU_ENABLE: u32 = hw::CMU_CLKEN0 as u32;
pub const RMU_RESET: u32 = hw::RMU_RSTEN0 as u32;
pub const AON_BIT: u32 = 1 << 27;
pub const AON_CMU_ENABLE: u32 = hw::AON_CMU_ENABLE as u32;
pub const AON_LRADC_CLK: u32 = hw::AON_LRADC_CLK as u32;
pub const AON_RMU_RESET: u32 = hw::AON_RMU_RESET as u32;
pub const AON_PMU_CONTROL: u32 = hw::AON_PMU_PWR_CTL as u32;
pub const AON_PMU_LRADC_BIAS: u32 = 0x1000;
pub const SPIO0_PAD: u32 = hw::AON_SPIO0_PAD as u32;
pub const SPIO0_INPUT_PULLUP: u32 = 0xE0;
pub const LRADC_BASE: u32 = hw::LRADC_BASE as u32;
pub const LRADC0_CTRL: u32 = LRADC_BASE;
pub const LRADC_CFG: u32 = LRADC_BASE + 0x2C;
pub const LRADC_SAMPLE: u32 = LRADC_BASE + 0x30;
pub const LRADC_ENABLE: u32 = LRADC_BASE + 0x40;
pub const LRADC_PENDING: u32 = LRADC_BASE + 0x4C;
pub const LRADC0_DATA: u32 = LRADC_BASE + 0x50;
pub const LRADC0_CTRL_VALUE: u32 = 0x0035_2C0F;
pub const LRADC0_READY: u32 = 1;
pub const UNPLUGGED_THRESHOLD: u32 = 900;
pub const PLUGGED_THRESHOLD: u32 = 850;
pub const DEBOUNCE_SAMPLES: u8 = 4;

pub const SETUP: [MmioOp; 12] = [
    MmioOp::Update { address: RMU_RESET, clear: 0, set: AON_BIT },
    MmioOp::Update { address: CMU_ENABLE, clear: 0, set: AON_BIT },
    MmioOp::Update { address: AON_RMU_RESET, clear: 0, set: 1 },
    MmioOp::Update { address: AON_CMU_ENABLE, clear: 0, set: 1 },
    MmioOp::Write { address: AON_LRADC_CLK, value: 0 },
    MmioOp::Write { address: LRADC_CFG, value: 3 },
    MmioOp::Update { address: AON_PMU_CONTROL, clear: 0, set: AON_PMU_LRADC_BIAS },
    MmioOp::Write { address: LRADC0_CTRL, value: LRADC0_CTRL_VALUE },
    MmioOp::Write { address: SPIO0_PAD, value: SPIO0_INPUT_PULLUP },
    MmioOp::Write { address: LRADC_SAMPLE, value: 0x101 },
    MmioOp::Write { address: LRADC_ENABLE, value: 1 },
    MmioOp::Write { address: LRADC_PENDING, value: LRADC0_READY },
];

pub fn classify(raw: u32, current: Jack) -> Jack {
    if raw >= UNPLUGGED_THRESHOLD {
        Jack::Empty
    } else if raw <= PLUGGED_THRESHOLD {
        Jack::Headphones
    } else {
        current
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Detector {
    initialized: bool,
    state: Jack,
    candidate: Jack,
    streak: u8,
    last_raw: Option<u32>,
}

impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}

impl Detector {
    pub const fn new() -> Self {
        Self { initialized: false, state: Jack::Unknown, candidate: Jack::Unknown, streak: 0, last_raw: None }
    }

    pub fn init<R: Registers>(&mut self, registers: &mut R) {
        apply(registers, &SETUP);
        self.initialized = true;
    }

    pub fn init_and_sample<R: Registers>(&mut self, registers: &mut R, polls: u32) -> Jack {
        self.init(registers);
        for _ in 0..polls {
            if registers.read(LRADC_PENDING) & LRADC0_READY != 0 {
                let raw = registers.read(LRADC0_DATA) & 0x3FF;
                registers.write(LRADC_PENDING, LRADC0_READY);
                self.last_raw = Some(raw);
                let detected = classify(raw, Jack::Unknown);
                if detected != Jack::Unknown {
                    self.state = detected;
                    self.candidate = detected;
                    self.streak = 0;
                }
                return detected;
            }
            registers.delay_us(100);
        }
        Jack::Unknown
    }

    pub fn state(&self) -> Jack {
        self.state
    }

    pub fn last_raw(&self) -> Option<u32> {
        self.last_raw
    }

    pub fn poll<R: Registers>(&mut self, registers: &mut R) -> Option<Jack> {
        if !self.initialized || registers.read(LRADC_PENDING) & LRADC0_READY == 0 {
            return None;
        }
        let raw = registers.read(LRADC0_DATA) & 0x3FF;
        registers.write(LRADC_PENDING, LRADC0_READY);
        self.observe(raw)
    }

    pub fn observe(&mut self, raw: u32) -> Option<Jack> {
        self.last_raw = Some(raw);
        let next = classify(raw, self.state);
        if next == Jack::Unknown || next == self.state {
            self.candidate = self.state;
            self.streak = 0;
            return None;
        }
        if next == self.candidate {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.candidate = next;
            self.streak = 1;
        }
        if self.streak >= DEBOUNCE_SAMPLES {
            self.state = next;
            self.streak = 0;
            Some(next)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Recorder;

    #[test]
    fn setup_enables_aon_lradc0_with_spio0_pullup() {
        let mut r = Recorder::default();
        let mut detector = Detector::new();
        detector.init(&mut r);
        assert_eq!(r.get(RMU_RESET) & AON_BIT, AON_BIT);
        assert_eq!(r.get(CMU_ENABLE) & AON_BIT, AON_BIT);
        assert_eq!(r.get(AON_RMU_RESET) & 1, 1);
        assert_eq!(r.get(AON_CMU_ENABLE) & 1, 1);
        assert_eq!(r.get(AON_LRADC_CLK), 0);
        assert_eq!(r.get(LRADC_CFG), 3);
        assert_eq!(r.get(AON_PMU_CONTROL) & AON_PMU_LRADC_BIAS, AON_PMU_LRADC_BIAS);
        assert_eq!(r.get(LRADC0_CTRL), LRADC0_CTRL_VALUE);
        assert_eq!(r.get(SPIO0_PAD), SPIO0_INPUT_PULLUP);
        assert_eq!(r.get(LRADC_SAMPLE), 0x101);
        assert_eq!(r.get(LRADC_ENABLE), 1);
    }

    #[test]
    fn detector_debounces_unplugged_and_plugged_readings_with_hysteresis() {
        let mut r = Recorder::default();
        let mut detector = Detector::new();
        detector.init(&mut r);
        assert_eq!(detector.state(), Jack::Unknown);
        assert_eq!(detector.poll(&mut r), None);
        r.script(LRADC_PENDING, &[LRADC0_READY; 10]);
        r.script(LRADC0_DATA, &[1023, 1023, 1023, 1023, 875, 12, 10, 14, 11]);
        for _ in 0..3 {
            assert_eq!(detector.poll(&mut r), None);
            assert_eq!(detector.state(), Jack::Unknown);
        }
        assert_eq!(detector.poll(&mut r), Some(Jack::Empty));
        assert_eq!(detector.state(), Jack::Empty);
        assert_eq!(detector.last_raw(), Some(1023));
        assert_eq!(detector.poll(&mut r), None);
        assert_eq!(detector.state(), Jack::Empty);
        for _ in 0..3 {
            assert_eq!(detector.poll(&mut r), None);
        }
        assert_eq!(detector.poll(&mut r), Some(Jack::Headphones));
        assert_eq!(detector.state(), Jack::Headphones);
        assert_eq!(detector.last_raw(), Some(11));
    }

    #[test]
    fn init_and_sample_seeds_initial_jack_state_from_first_ready_reading() {
        let mut r = Recorder::default();
        r.script(LRADC_PENDING, &[0, LRADC0_READY]);
        r.script(LRADC0_DATA, &[0]);
        let mut detector = Detector::new();
        assert_eq!(detector.init_and_sample(&mut r, 10), Jack::Headphones);
        assert_eq!(detector.state(), Jack::Headphones);
        assert_eq!(detector.last_raw(), Some(0));
    }
}

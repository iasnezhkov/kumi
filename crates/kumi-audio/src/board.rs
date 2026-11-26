// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_chip::regs as hw;

use crate::regs::MmioOp;

pub const GPIO_BASE: u32 = hw::GPIO_BASE as u32;
pub const DAC_RESET_PIN: u32 = 34;
pub const OUTPUT_ENABLE_PIN: u32 = 5;
pub const PIN_LOW: u32 = 0x10;
pub const PIN_HIGH: u32 = 0x210;
pub const OUTPUT_PIN_PULL: u32 = 0xc0;
pub const DAC_RESET_HOLD_US: u32 = 4_000;

pub const fn pad(pin: u32) -> u32 {
    GPIO_BASE + 4 * pin
}

pub const DAC_RESET: u32 = pad(DAC_RESET_PIN);
pub const OUTPUT_ENABLE: u32 = pad(OUTPUT_ENABLE_PIN);

pub const POWER_ON: [MmioOp; 3] = [
    MmioOp::Write { address: hw::CMU_AUDIO_ANALOG as u32, value: 0x106 },
    MmioOp::Write { address: GPIO_BASE + 0x300, value: 1 },
    MmioOp::Write { address: GPIO_BASE + 0x310, value: 0x400 },
];

pub const POWER_OFF: [MmioOp; 5] = [
    MmioOp::Write { address: GPIO_BASE + 0x310, value: 0 },
    MmioOp::Write { address: GPIO_BASE + 0x300, value: 0 },
    MmioOp::Write { address: hw::CMU_AUDIO_ANALOG as u32, value: 0 },
    MmioOp::Update { address: crate::clock::AUDIO_PLL, clear: crate::clock::PLL_ENABLE, set: 0 },
    MmioOp::Update { address: crate::i2s::PERIPHERAL_GATE, clear: crate::i2s::I2S_BLOCK_BIT, set: 0 },
];

pub const fn dac_reset(asserted: bool) -> MmioOp {
    MmioOp::Write { address: DAC_RESET, value: if asserted { PIN_LOW } else { PIN_HIGH } }
}

pub const fn output(enabled: bool) -> MmioOp {
    MmioOp::Write { address: OUTPUT_ENABLE, value: (if enabled { PIN_HIGH } else { PIN_LOW }) | OUTPUT_PIN_PULL }
}

pub const DAC_RESET_PULSE: [MmioOp; 4] = [
    dac_reset(true),
    MmioOp::Delay { microseconds: DAC_RESET_HOLD_US },
    dac_reset(false),
    MmioOp::Delay { microseconds: DAC_RESET_HOLD_US },
];

pub const QUIET: [MmioOp; 2] = [output(false), dac_reset(true)];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regs::apply;
    use crate::testing::Recorder;

    #[test]
    fn pads_and_levels_match_the_stock_writes() {
        assert_eq!(DAC_RESET, 0x2011_0088);
        assert_eq!(OUTPUT_ENABLE, 0x2011_0014);
        assert_eq!(output(false), MmioOp::Write { address: 0x2011_0014, value: 0xd0 });
        assert_eq!(output(true), MmioOp::Write { address: 0x2011_0014, value: 0x2d0 });
        let mut r = Recorder::default();
        apply(&mut r, &POWER_ON);
        apply(&mut r, &DAC_RESET_PULSE);
        assert_eq!(
            r.writes(),
            [(0x2000_20f8, 0x106), (0x2011_0300, 1), (0x2011_0310, 0x400), (0x2011_0088, 0x10), (0x2011_0088, 0x210)]
        );
        assert_eq!(r.delays(), [4_000, 4_000]);
        apply(&mut r, &QUIET);
        assert_eq!((r.get(OUTPUT_ENABLE), r.get(DAC_RESET)), (0xd0, 0x10));
    }

    #[test]
    fn power_off_clears_analog_rails_and_audio_pll_enable() {
        let mut r = Recorder::default();
        r.set(crate::clock::AUDIO_PLL, 0x0004_0243);
        r.set(crate::i2s::PERIPHERAL_GATE, crate::i2s::AUDIO_BLOCK_BITS | (1 << 20));
        apply(&mut r, &POWER_ON);
        apply(&mut r, &POWER_OFF);
        assert_eq!((r.get(0x2011_0310), r.get(0x2011_0300), r.get(0x2000_20f8)), (0, 0, 0));
        assert_eq!(r.get(crate::clock::AUDIO_PLL), 0x0004_0242);
        assert_eq!(r.get(crate::i2s::PERIPHERAL_GATE), 1 | (1 << 20));
    }
}

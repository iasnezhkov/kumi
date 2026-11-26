// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use core::ptr::{read_volatile, write_volatile};

pub const CHIP_ID: usize = 0x2000_0000;
pub const CMU_CLKEN0: usize = 0x2000_2000;
pub const CMU_SYSCLK: usize = 0x2000_2010;
pub const SPI_CLOCK: usize = 0x2000_2020;
pub const CMU_I2C0_CLK: usize = 0x2000_2028;
pub const NOR_CLOCK: usize = 0x2000_203C;
pub const NOR_CLOCK_DEFAULT: u32 = 0x0000_0030;
pub const NOR_CLOCK_SYS_DIV2: u32 = 0x0000_0010;
pub const CMU_TIMER_CLK_BASE: usize = 0x2000_2060;
pub const CMU_I2S_CLK: usize = 0x2000_2078;
pub const CMU_AUDIO_ANALOG: usize = 0x2000_20F8;
pub const RMU_RSTEN0: usize = 0x2000_3000;
pub const RESET_CONTROL: usize = 0x2000_3024;
pub const RESET_REQUEST: u32 = 1 << 1;
pub const DMA_REQUEST: usize = 0x2000_801C;
pub const TIMER_BASE: usize = 0x2001_0000;
pub const DMA_CH2_BASE: usize = 0x2002_0200;
pub const DMA_ENABLE: usize = 0x2002_1000;
pub const DMA_INT_ENABLE: usize = 0x2002_1004;
pub const DMA_STATUS: usize = 0x2002_1008;
pub const I2C0_BASE: usize = 0x2007_0000;
pub const I2C1_BASE: usize = 0x2008_0000;
pub const NORC_CONTROL: usize = 0x2009_0000;
pub const NORC_STATUS: usize = 0x2009_0008;
pub const NORC_PAUSE: u32 = 1 << 24;
pub const NORC_IDLE: u32 = 1 << 5;
pub const NORC_WAIT_LIMIT: u32 = 1_000_000;
pub const NORC_CTL_DEFAULT_RUN: u32 = 0x0000_F700;
pub const NORC_CTL_QUAD_90: u32 = 0x0236_A50E;
pub const USB_FRAME: usize = 0x200D_000C;
pub const USB_FRAME_MASK: u32 = 0x7FF;
pub const GPIO_BASE: usize = 0x2011_0000;
pub const I2S_BASE: usize = 0x2014_0000;
pub const AON_OSC_XTAL: usize = 0x2100_0004;
pub const AON_PLL_TRIM: usize = 0x2100_0010;
pub const AON_PLL_CORE: usize = 0x2100_1000;
pub const AON_PLL_AUDIO: usize = 0x2100_1010;
pub const AON_PLL_AUDIO_FRAC: usize = 0x2100_1014;
pub const AON_CMU_ENABLE: usize = 0x2100_2000;
pub const AON_LRADC_CLK: usize = 0x2100_2010;
pub const AON_RMU_RESET: usize = 0x2100_3000;
pub const AON_PMU_PWR_CTL: usize = 0x2100_4000;
pub const AON_PMU_VOL_CTL: usize = 0x2100_4004;
pub const AON_SPIO0_PAD: usize = 0x2100_5000;
pub const RETAINED: usize = 0x2100_801C;
pub const SERVICE_ON_RESET: u32 = 1 << 31;
pub const LRADC_BASE: usize = 0x2101_0000;
pub const PERIPHERAL_ENABLE: usize = CMU_CLKEN0;
pub const SPI_ENABLE: u32 = 1 << 7;
pub const SETTLE_LOOPS: u32 = 100;

pub unsafe fn read(address: usize) -> u32 {
    unsafe { read_volatile(address as *const u32) }
}

pub unsafe fn write(address: usize, value: u32) {
    unsafe { write_volatile(address as *mut u32, value) }
}

pub unsafe fn update(address: usize, clear: u32, set: u32) {
    unsafe { write(address, (read(address) & !clear) | set) }
}

pub fn usb_frame() -> u16 {
    (unsafe { read(USB_FRAME) } & USB_FRAME_MASK) as u16
}

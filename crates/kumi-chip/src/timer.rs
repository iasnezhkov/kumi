// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::regs;

pub const CMU_ENABLE: usize = regs::CMU_CLKEN0;
pub const RMU_RESET: usize = regs::RMU_RSTEN0;
pub const GATE_BIT: u32 = 1 << 5;
pub const CLOCK_SELECT_BASE: usize = regs::CMU_TIMER_CLK_BASE;
pub const TIMER_BASE: usize = regs::TIMER_BASE;
pub const INT_ENABLE: usize = TIMER_BASE + 0x04;
pub const INT_STATUS: usize = TIMER_BASE + 0x08;
pub const XTAL_HZ: u32 = 24_000_000;
pub const TICKS_PER_MS: u32 = XTAL_HZ / 1_000;
pub const CLOCK_XTAL: u32 = 5;
pub const CTL_ENABLE: u32 = 1 << 0;
pub const CTL_PERIODIC: u32 = 1 << 1;
pub const CAPTURE_MODE: u32 = 0x23;
pub const CAPTURE2_IRQ_BIT: u32 = 1 << 10;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimerId {
    T0 = 0,
    T1 = 1,
    T2 = 2,
    T3 = 3,
}

impl TimerId {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn block(self) -> usize {
        TIMER_BASE + 0x20 * (self.index() + 1)
    }

    pub const fn clock_select(self) -> usize {
        CLOCK_SELECT_BASE + 4 * self.index()
    }

    pub const fn irq_bit(self) -> u32 {
        1 << self.index()
    }

    pub const fn plic_source(self) -> u32 {
        8 + self.index() as u32
    }
}

pub const fn periodic_load(milliseconds: u32) -> u32 {
    milliseconds.saturating_mul(TICKS_PER_MS)
}

pub const fn capture_ctl(gpio_pin: u8) -> u32 {
    ((gpio_pin as u32) << 8) | CAPTURE_MODE
}

pub const fn capture_frequency_hz(captured_ticks: u32, reference_hz: u32) -> Option<u32> {
    if captured_ticks == 0 {
        return None;
    }
    Some(reference_hz / captured_ticks)
}

pub unsafe fn start_periodic(timer: TimerId, milliseconds: u32, irq: bool) {
    unsafe {
        regs::update(CMU_ENABLE, 0, GATE_BIT);
        regs::update(RMU_RESET, 0, GATE_BIT);
        #[cfg(target_arch = "riscv32")]
        core::arch::asm!("fence iorw, iorw", options(nostack));
        regs::write(timer.clock_select(), CLOCK_XTAL);
        regs::write(timer.block() + 0x04, periodic_load(milliseconds));
        regs::write(INT_STATUS, timer.irq_bit());
        regs::write(timer.block(), CTL_ENABLE | CTL_PERIODIC);
        if irq {
            regs::update(INT_ENABLE, 0, timer.irq_bit());
        }
    }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn is_running(timer: TimerId) -> bool {
    unsafe {
        let active = regs::read(CMU_ENABLE) & GATE_BIT != 0 && regs::read(RMU_RESET) & GATE_BIT != 0;
        active && regs::read(timer.block()) & CTL_ENABLE != 0 && regs::read(INT_ENABLE) & timer.irq_bit() != 0
    }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn pending(timer: TimerId) -> bool {
    unsafe {
        let active = regs::read(CMU_ENABLE) & GATE_BIT != 0 && regs::read(RMU_RESET) & GATE_BIT != 0;
        active && regs::read(INT_STATUS) & timer.irq_bit() != 0
    }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn clear_pending(timer: TimerId) {
    unsafe {
        if regs::read(CMU_ENABLE) & GATE_BIT != 0 && regs::read(RMU_RESET) & GATE_BIT != 0 {
            regs::write(INT_STATUS, timer.irq_bit());
        }
    }
}

pub unsafe fn stop(timer: TimerId) {
    unsafe {
        regs::update(timer.block(), CTL_ENABLE, 0);
        regs::update(INT_ENABLE, timer.irq_bit(), 0);
        regs::write(INT_STATUS, timer.irq_bit());
    }
}

pub unsafe fn start_capture2(gpio_pin: u8) {
    unsafe {
        regs::update(CMU_ENABLE, 0, GATE_BIT);
        regs::update(RMU_RESET, 0, GATE_BIT);
        regs::write(TimerId::T2.clock_select(), CLOCK_XTAL);
        regs::write(TimerId::T2.block() + 0x04, XTAL_HZ);
        regs::write(TimerId::T2.block(), capture_ctl(gpio_pin));
        regs::update(INT_ENABLE, 0, CAPTURE2_IRQ_BIT);
    }
}

pub unsafe fn read_capture2() -> u32 {
    unsafe { regs::read(TimerId::T2.block() + 0x0C) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses_loads_and_capture_encoding_match_hardware() {
        assert_eq!(TimerId::T0.block(), 0x2001_0020);
        assert_eq!(TimerId::T2.block(), 0x2001_0060);
        assert_eq!(TimerId::T3.clock_select(), 0x2000_206C);
        assert_eq!(TimerId::T0.plic_source(), 8);
        assert_eq!(periodic_load(10), 240_000);
        assert_eq!(capture_ctl(10), 0x0A23);
        assert_eq!(capture_frequency_hz(24_000, XTAL_HZ), Some(1_000));
        assert_eq!(capture_frequency_hz(0, XTAL_HZ), None);
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::regs;

pub const CONTROL: usize = 0x2000_3020;
pub const RESET_CAUSE: usize = 0x2000_3040;
pub const RESET_CAUSE_WATCHDOG: u32 = 1 << 3;

pub const ENABLE: u32 = 1 << 0;
pub const CLEAR: u32 = 1 << 1;
pub const RESET_ENABLE: u32 = 1 << 3;
pub const PERIOD_SHIFT: u32 = 4;
pub const PERIOD_MASK: u32 = 0x7 << PERIOD_SHIFT;
pub const IRQ_ENABLE: u32 = 1 << 7;
pub const PRE_SHIFT: u32 = 8;
pub const PRE_MASK: u32 = 0x3 << PRE_SHIFT;
pub const PENDING: u32 = 1 << 10;

pub const STOCK_PERIOD: u8 = 5;
pub const SAFE_PERIOD: u8 = 7;
pub const STOCK_PRE: u8 = 3;
pub const STOCK_CONFIG: u32 = config_word(STOCK_PERIOD, STOCK_PRE, true, true);
pub const SAFE_RESET_CONFIG: u32 = config_word(SAFE_PERIOD, STOCK_PRE, true, true);
pub const IRQ_ONLY_CONFIG: u32 = config_word(STOCK_PERIOD, STOCK_PRE, true, false);

pub const fn config_word(period: u8, pre: u8, irq: bool, reset: bool) -> u32 {
    ENABLE
        | CLEAR
        | (((period as u32) << PERIOD_SHIFT) & PERIOD_MASK)
        | (((pre as u32) << PRE_SHIFT) & PRE_MASK)
        | if irq { IRQ_ENABLE } else { 0 }
        | if reset { RESET_ENABLE } else { 0 }
}

pub unsafe fn arm_irq_only() {
    unsafe { regs::write(CONTROL, IRQ_ONLY_CONFIG) };
}

pub unsafe fn arm_reset() {
    unsafe { regs::write(CONTROL, STOCK_CONFIG) };
}

pub unsafe fn arm_safe_reset() {
    unsafe { regs::write(CONTROL, SAFE_RESET_CONFIG) };
}

pub unsafe fn armed() -> bool {
    unsafe { regs::read(CONTROL) & ENABLE != 0 }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn kick() {
    unsafe { regs::update(CONTROL, 0, CLEAR) };
}

pub unsafe fn disable() {
    unsafe { regs::update(CONTROL, ENABLE | RESET_ENABLE, 0) };
}

pub unsafe fn pending() -> bool {
    unsafe { regs::read(CONTROL) & PENDING != 0 }
}

pub unsafe fn clear_pending() {
    unsafe { regs::update(CONTROL, 0, PENDING | CLEAR) };
}

pub unsafe fn caused_last_reset() -> bool {
    unsafe { regs::read(RESET_CAUSE) & RESET_CAUSE_WATCHDOG != 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_and_irq_only_words_match_disassembly() {
        assert_eq!(STOCK_CONFIG, 0x3DB);
        assert_eq!(SAFE_RESET_CONFIG, 0x3FB);
        assert_eq!(IRQ_ONLY_CONFIG, 0x3D3);
        assert_eq!(STOCK_CONFIG ^ IRQ_ONLY_CONFIG, RESET_ENABLE);
    }
}

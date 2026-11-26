// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::regs;

pub const BASE: usize = 0x1000_0000;
pub const FEATURE: usize = BASE;
pub const PRIORITY_BASE: usize = BASE + 0x0004;
pub const PENDING_BASE: usize = BASE + 0x1000;
pub const ENABLE_BASE: usize = BASE + 0x2000;
pub const THRESHOLD: usize = BASE + 0x20_0000;
pub const CLAIM: usize = BASE + 0x20_0004;
pub const DEV_WFI: usize = 0x2000_5000;

pub const IRQ_TIMER3: u32 = 11;
pub const IRQ_UART0: u32 = 27;
pub const IRQ_USB_MAC: u32 = 34;
pub const MEIE_BIT: u32 = 1 << 11;

pub const fn priority_address(source: u32) -> usize {
    PRIORITY_BASE + 4 * (source as usize - 1)
}

pub const fn enable_masks(sources: &[u32]) -> [u32; 2] {
    let mut masks = [0u32; 2];
    let mut i = 0;
    while i < sources.len() {
        let source = sources[i];
        if source >= 1 && source < 64 {
            masks[(source / 32) as usize] |= 1u32 << (source % 32);
        }
        i += 1;
    }
    masks
}

pub unsafe fn enable_sources(sources: &[u32]) {
    let masks = enable_masks(sources);
    unsafe {
        regs::write(THRESHOLD, 0);
        for &source in sources {
            if (1..64).contains(&source) {
                regs::write(priority_address(source), 1);
            }
        }
        regs::write(ENABLE_BASE, masks[0]);
        regs::write(ENABLE_BASE + 4, masks[1]);
        #[cfg(target_arch = "riscv32")]
        {
            core::arch::asm!("csrci mstatus, 8", options(nomem, nostack));
            core::arch::asm!("csrs mie, {0}", in(reg) MEIE_BIT, options(nomem, nostack));
        }
    }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn claim() -> u32 {
    unsafe { regs::read(CLAIM) }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn complete(source: u32) {
    unsafe { regs::write(CLAIM, source) }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn drain_stale_abort_on(abort_mask: u64) -> bool {
    let mut seen = 0u64;
    for _ in 0..16 {
        let source = unsafe { claim() };
        if source == 0 {
            return false;
        }
        unsafe { complete(source) };
        if source >= 64 {
            return true;
        }
        let bit = 1u64 << source;
        if (abort_mask | seen) & bit != 0 {
            return true;
        }
        seen |= bit;
    }
    true
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn drain_stale() -> bool {
    unsafe { drain_stale_abort_on(0) }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn wait_for_interrupt() {
    #[cfg(target_arch = "riscv32")]
    unsafe {
        core::arch::asm!("fence iorw, iorw", "wfi", options(nostack));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wake_sources_map_to_expected_plic_registers_and_masks() {
        assert_eq!(priority_address(IRQ_TIMER3), 0x1000_002C);
        assert_eq!(priority_address(IRQ_UART0), 0x1000_006C);
        assert_eq!(priority_address(IRQ_USB_MAC), 0x1000_0088);
        assert_eq!(enable_masks(&[IRQ_TIMER3, IRQ_UART0, IRQ_USB_MAC]), [0x0800_0800, 0x0000_0004]);
        assert_eq!(enable_masks(&[0, 64, IRQ_TIMER3]), [1 << 11, 0]);
    }
}

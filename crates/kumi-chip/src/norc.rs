// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use core::ptr::read_volatile;

use crate::{regs, rom};

pub const FLASH_XIP_UNCACHED: u32 = 0x6C00_0000;

fn fence() {
    #[cfg(target_arch = "riscv32")]
    unsafe {
        core::arch::asm!("fence iorw, iorw", "fence.i", options(nostack));
    }
}

pub unsafe fn pause() -> bool {
    unsafe {
        regs::update(regs::NORC_CONTROL, 0, regs::NORC_PAUSE);
        for _ in 0..regs::SETTLE_LOOPS {
            let _ = read_volatile(regs::NORC_STATUS as *const u32);
        }
        let mut waited = 0;
        while regs::read(regs::NORC_STATUS) & regs::NORC_IDLE == 0 {
            waited += 1;
            if waited > regs::NORC_WAIT_LIMIT {
                regs::update(regs::NORC_CONTROL, regs::NORC_PAUSE, 0);
                return false;
            }
        }
    }
    true
}

pub unsafe fn enter_quad_90mhz(cpu_hz: u64) -> Option<u32> {
    if !unsafe { pause() } {
        return None;
    }
    if !unsafe { rom::has_quad_spi_init() } {
        unsafe { regs::update(regs::NORC_CONTROL, regs::NORC_PAUSE, 0) };
        fence();
        return Some(0);
    }
    let id = unsafe { rom::norctrl_quad_spi_init(0, 0, cpu_hz) };
    if id <= 2 || id == 0x00FF_FFFF {
        unsafe { restore_default() };
        return None;
    }
    if !unsafe { pause() } {
        unsafe { restore_default() };
        return None;
    }
    unsafe {
        regs::write(regs::NOR_CLOCK, regs::NOR_CLOCK_SYS_DIV2);
        let _ = regs::read(regs::NOR_CLOCK);
        regs::write(regs::NORC_CONTROL, regs::NORC_CTL_QUAD_90);
        let _ = regs::read(regs::NORC_CONTROL);
    }
    fence();
    Some(id)
}

pub unsafe fn restore_default() {
    let _ = unsafe { pause() };
    unsafe {
        regs::write(regs::NOR_CLOCK, regs::NOR_CLOCK_DEFAULT);
        let _ = regs::read(regs::NOR_CLOCK);
        regs::write(regs::NORC_CONTROL, regs::NORC_CTL_DEFAULT_RUN);
        let _ = regs::read(regs::NORC_CONTROL);
    }
    fence();
}

#[inline(never)]
pub unsafe fn read_xip(base: u32, offset: u32, out: &mut [u8]) {
    let start = base.wrapping_add(offset) as usize;
    if start.is_multiple_of(4) {
        let (words, tail) = out.as_chunks_mut::<4>();
        for (i, chunk) in words.iter_mut().enumerate() {
            *chunk = unsafe { read_volatile((start + 4 * i) as *const u32) }.to_le_bytes();
        }
        let tail_start = start + 4 * words.len();
        for (i, byte) in tail.iter_mut().enumerate() {
            *byte = unsafe { read_volatile((tail_start + i) as *const u8) };
        }
        return;
    }
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = unsafe { read_volatile((start + i) as *const u8) };
    }
}

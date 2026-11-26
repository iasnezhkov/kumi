// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#[cfg(target_arch = "riscv32")]
use crate::regs::{NORC_CONTROL, NORC_PAUSE, RESET_CONTROL, RESET_REQUEST, RETAINED, SERVICE_ON_RESET};
#[cfg(target_arch = "riscv32")]
use kumi_layout::BOOT_INFO_ADDRESS;

#[cfg(target_arch = "riscv32")]
core::arch::global_asm!(
    ".section .text.hot.reset, \"ax\"",
    ".balign 4",
    ".global kumi_chip_fault_reset_normal",
    "kumi_chip_fault_reset_normal:",
    "li a0, 0",
    "j kumi_chip_reset",
    ".balign 4",
    ".global kumi_chip_fault_reset_service",
    "kumi_chip_fault_reset_service:",
    "li a0, 1",
    ".global kumi_chip_reset",
    "kumi_chip_reset:",
    "csrci mstatus, 8",
    "li t0, {boot_info}",
    "sw zero, 0(t0)",
    "li t0, {norc}",
    "lw t1, 0(t0)",
    "li t2, {norc_keep}",
    "and t1, t1, t2",
    "sw t1, 0(t0)",
    "li t0, {retained}",
    "lw t1, 0(t0)",
    "li t2, {retained_keep}",
    "and t1, t1, t2",
    "beqz a0, 1f",
    "li t2, {service}",
    "or t1, t1, t2",
    "1:",
    "sw t1, 0(t0)",
    "li t0, {reset}",
    "lw t1, 0(t0)",
    "ori t1, t1, {request}",
    "sw t1, 0(t0)",
    "2:",
    "j 2b",
    boot_info = const BOOT_INFO_ADDRESS as i32,
    norc = const NORC_CONTROL as i32,
    norc_keep = const !NORC_PAUSE as i32,
    retained = const RETAINED as i32,
    retained_keep = const !SERVICE_ON_RESET as i32,
    service = const SERVICE_ON_RESET as i32,
    reset = const RESET_CONTROL as i32,
    request = const RESET_REQUEST as i32,
);

#[cfg(target_arch = "riscv32")]
unsafe extern "C" {
    fn kumi_chip_reset(service: u32) -> !;
    fn kumi_chip_fault_reset_normal() -> !;
    fn kumi_chip_fault_reset_service() -> !;
}

#[cfg(target_arch = "riscv32")]
pub fn reset(service: bool) -> ! {
    unsafe { kumi_chip_reset(u32::from(service)) }
}

#[cfg(target_arch = "riscv32")]
pub fn fault_vector(service: bool) -> usize {
    if service { kumi_chip_fault_reset_service as *const () as usize } else { kumi_chip_fault_reset_normal as *const () as usize }
}

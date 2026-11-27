// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

use ed25519_dalek::VerifyingKey;
use embedded_storage::nor_flash::{ErrorType, NorFlash, ReadNorFlash};
use kumi_boot_core::stage0::{BootRomReason, DiagRam, Memory, Outcome, Report, Stage0, encode_report};
use kumi_chip::scratch::no_op_pattern;
use kumi_chip::{BootRom, Error as FlashError, Identity, RomFlash, norc, regs, rom, watchdog};
use kumi_diag::{AREA_SIZE, Diag, Reason, Registers};
use kumi_layout::{
    BOOT_INFO_ADDRESS, DIAG_RECORD_ADDRESS, DISARM, FLASH_SIZE, FLASH_XIP_BASE, LAST_CRASH_ADDRESS, PAGE, SCRATCH, SECTOR,
    SPV_IDENTITY_LEN, STAGE0_CALL_FAULTED, STAGE0_CALL_PANICKED, STAGE0_REPORT_ADDRESS, STAGE0_VERIFY_MAGIC, SlotId, Writer,
    crc32,
};

const MODE_BOOT: u32 = 0;
const MODE_DRY: u32 = 1;
const MODE_VERIFY: u32 = 2;
const MODE_G2B: u32 = 3;
const G2B_PAUSED: u8 = 1;
const G2B_IDENTITY: u8 = 2;
const G2B_ERASED_BEFORE: u8 = 4;
const G2B_FIRST_ERASE: u8 = 8;
const G2B_PROGRAMMED: u8 = 16;
const G2B_SECOND_ERASE: u8 = 32;
const G2B_HEAD_UNCHANGED: u8 = 64;
const G2B_WRITES: u8 = G2B_FIRST_ERASE | G2B_PROGRAMMED | G2B_SECOND_ERASE;
const G2B_PASSED: u8 = 0;
const G2B_UNAVAILABLE: u8 = 15;
const G2B_NOT_ERASED: u8 = 18;
const G2B_WRITE_FAILED: u8 = 19;
const G2B_HEAD_CHANGED: u8 = 20;
const SAVED_MTVEC: usize = 17;
const SAVED_MODE: usize = 18;
const SAVED_MCACHE: usize = 19;
const REPORT_LEN: usize = 64;
const PANIC_TEXT: usize = 54;

static mut ENTRY_SNAPSHOT: [u8; AREA_SIZE] = [0; AREA_SIZE];

#[unsafe(no_mangle)]
#[unsafe(link_section = ".rodata.kumi_key")]
static KUMI_PUBLIC_KEY: [u8; 32] = include!(concat!(env!("OUT_DIR"), "/public_key.rs"));

global_asm!(
    ".option push",
    ".option norelax",
    ".section .text.xip, \"ax\"",
    ".global _start",
    "_start:",
    "lui t0, %hi(kumi_stage0_xip)",
    "addi t0, t0, %lo(kumi_stage0_xip)",
    "jr t0",
    "kumi_stage0_xip:",
    "csrci mstatus, 8",
    "lui t0, %hi(__ram_lma)",
    "addi t0, t0, %lo(__ram_lma)",
    "lui t1, %hi(__ram_start)",
    "addi t1, t1, %lo(__ram_start)",
    "lui t2, %hi(__ram_end)",
    "addi t2, t2, %lo(__ram_end)",
    "1:",
    "bgeu t1, t2, 2f",
    "lw t3, 0(t0)",
    "sw t3, 0(t1)",
    "addi t0, t0, 4",
    "addi t1, t1, 4",
    "j 1b",
    "2:",
    "fence.i",
    "lui t0, %hi(kumi_stage0_boot)",
    "addi t0, t0, %lo(kumi_stage0_boot)",
    "jr t0",
    ".section .text.vectors, \"ax\"",
    ".global kumi_stage0_vectors",
    ".global kumi_stage0_dry_vector",
    ".global kumi_stage0_verify_vector",
    ".global kumi_stage0_verify_magic",
    ".global kumi_stage0_g2b_vector",
    ".option push",
    ".option norvc",
    "kumi_stage0_vectors:",
    "j kumi_stage0_boot",
    "kumi_stage0_dry_vector:",
    "j kumi_stage0_dry",
    "kumi_stage0_verify_vector:",
    "j kumi_stage0_verify",
    "kumi_stage0_verify_magic:",
    ".word {magic}",
    "kumi_stage0_g2b_vector:",
    "j kumi_stage0_g2b",
    ".option pop",
    ".section .text.kumi_entry, \"ax\"",
    "kumi_stage0_zero_bss:",
    "lui t0, %hi(__bss_start)",
    "addi t0, t0, %lo(__bss_start)",
    "lui t1, %hi(__bss_end)",
    "addi t1, t1, %lo(__bss_end)",
    "1:",
    "bgeu t0, t1, 2f",
    "sw zero, 0(t0)",
    "addi t0, t0, 4",
    "j 1b",
    "2:",
    "jr t6",
    "kumi_stage0_boot:",
    "csrci mstatus, 8",
    "jal t6, kumi_stage0_zero_bss",
    "lui t0, %hi(kumi_stage0_saved)",
    "addi t0, t0, %lo(kumi_stage0_saved)",
    "csrr t1, mtvec",
    "sw t1, 68(t0)",
    "csrr t1, 0x7ca",
    "sw t1, 76(t0)",
    "fence.i",
    "ori t1, t1, 1",
    "csrw 0x7ca, t1",
    "lui t1, %hi(kumi_stage0_trap)",
    "addi t1, t1, %lo(kumi_stage0_trap)",
    "csrw mtvec, t1",
    "lui sp, %hi(__stack_top)",
    "addi sp, sp, %lo(__stack_top)",
    "call kumi_stage0_boot_main",
    "3:",
    "j 3b",
    "kumi_stage0_dry:",
    "li t5, {mode_dry}",
    "lui t4, %hi(kumi_stage0_dry_main)",
    "addi t4, t4, %lo(kumi_stage0_dry_main)",
    "j kumi_stage0_call",
    "kumi_stage0_g2b:",
    "li t5, {mode_g2b}",
    "lui t4, %hi(kumi_stage0_g2b_main)",
    "addi t4, t4, %lo(kumi_stage0_g2b_main)",
    "j kumi_stage0_call",
    "kumi_stage0_verify:",
    "li t5, {mode_verify}",
    "lui t4, %hi(kumi_stage0_verify_main)",
    "addi t4, t4, %lo(kumi_stage0_verify_main)",
    "kumi_stage0_call:",
    "csrrci t3, mstatus, 8",
    "jal t6, kumi_stage0_zero_bss",
    "lui t0, %hi(kumi_stage0_saved)",
    "addi t0, t0, %lo(kumi_stage0_saved)",
    "sw ra, 0(t0)",
    "sw sp, 4(t0)",
    "sw s0, 8(t0)",
    "sw s1, 12(t0)",
    "sw s2, 16(t0)",
    "sw s3, 20(t0)",
    "sw s4, 24(t0)",
    "sw s5, 28(t0)",
    "sw s6, 32(t0)",
    "sw s7, 36(t0)",
    "sw s8, 40(t0)",
    "sw s9, 44(t0)",
    "sw s10, 48(t0)",
    "sw s11, 52(t0)",
    "sw gp, 56(t0)",
    "sw tp, 60(t0)",
    "sw t3, 64(t0)",
    "csrr t1, mtvec",
    "sw t1, 68(t0)",
    "sw t5, 72(t0)",
    "csrr t1, 0x7ca",
    "sw t1, 76(t0)",
    "fence.i",
    "ori t1, t1, 1",
    "csrw 0x7ca, t1",
    "lui t1, %hi(kumi_stage0_trap)",
    "addi t1, t1, %lo(kumi_stage0_trap)",
    "csrw mtvec, t1",
    "lui sp, %hi(__stack_top)",
    "addi sp, sp, %lo(__stack_top)",
    "jalr t4",
    ".global kumi_stage0_call_exit",
    "kumi_stage0_call_exit:",
    "lui t0, %hi(kumi_stage0_saved)",
    "addi t0, t0, %lo(kumi_stage0_saved)",
    "lw t1, 76(t0)",
    "csrw 0x7ca, t1",
    "fence.i",
    "lw t1, 68(t0)",
    "csrw mtvec, t1",
    "lw ra, 0(t0)",
    "lw sp, 4(t0)",
    "lw s0, 8(t0)",
    "lw s1, 12(t0)",
    "lw s2, 16(t0)",
    "lw s3, 20(t0)",
    "lw s4, 24(t0)",
    "lw s5, 28(t0)",
    "lw s6, 32(t0)",
    "lw s7, 36(t0)",
    "lw s8, 40(t0)",
    "lw s9, 44(t0)",
    "lw s10, 48(t0)",
    "lw s11, 52(t0)",
    "lw gp, 56(t0)",
    "lw tp, 60(t0)",
    "lw t1, 64(t0)",
    "csrw mstatus, t1",
    "ret",
    ".balign 64",
    "kumi_stage0_trap:",
    "mv a4, sp",
    "mv a3, ra",
    "lui sp, %hi(__trap_stack_top)",
    "addi sp, sp, %lo(__trap_stack_top)",
    "csrr a0, mcause",
    "csrr a1, mepc",
    "csrr a2, mtval",
    "call kumi_stage0_trap_main",
    "4:",
    "j 4b",
    ".section .bss.kumi_saved, \"aw\", @nobits",
    ".balign 4",
    ".global kumi_stage0_saved",
    "kumi_stage0_saved:",
    ".space 80",
    ".option pop",
    magic = const u32::from_le_bytes(STAGE0_VERIFY_MAGIC),
    mode_dry = const MODE_DRY,
    mode_verify = const MODE_VERIFY,
    mode_g2b = const MODE_G2B,
);

unsafe extern "C" {
    fn kumi_stage0_call_exit(code: u32) -> !;
    static kumi_stage0_saved: [u32; 20];
}

fn saved(index: usize) -> u32 {
    unsafe { read_volatile((&raw const kumi_stage0_saved).cast::<u32>().add(index)) }
}

fn mode() -> u32 {
    saved(SAVED_MODE)
}

fn public_key() -> Option<VerifyingKey> {
    let key_bytes = unsafe { read_volatile(&raw const KUMI_PUBLIC_KEY) };
    VerifyingKey::from_bytes(&key_bytes).ok().filter(|key| !key.is_weak())
}

#[unsafe(no_mangle)]
extern "C" fn kumi_stage0_verify_main(slot: u32) -> u32 {
    let Some(slot) = SlotId::from_index(slot as usize) else { return 1 };
    let Some(key) = public_key() else { return 1 };
    let mut flash = BootFlash::Rom(RomFlash::new(unsafe { BootRom::prepared() }, None));
    u32::from(!kumi_boot_core::verify_slot(&mut flash, slot, &key).is_ok_and(|h| h.flags & kumi_layout::FLAG_LAB == 0))
}

fn arm_fault_reset() {
    let handler = kumi_chip::reset::fault_vector(true);
    unsafe { asm!("csrw mtvec, {0}", in(reg) handler, options(nomem, nostack)) };
}

struct Ram;

impl Memory for Ram {
    fn write(&mut self, address: u32, data: &[u8]) {
        unsafe {
            asm!(
                "beqz {n}, 2f",
                "1:",
                "lbu {t}, 0({s})",
                "sb {t}, 0({d})",
                "addi {s}, {s}, 1",
                "addi {d}, {d}, 1",
                "addi {n}, {n}, -1",
                "bnez {n}, 1b",
                "2:",
                d = inout(reg) address => _,
                s = inout(reg) data.as_ptr() => _,
                n = inout(reg) data.len() => _,
                t = out(reg) _,
                options(nostack),
            );
        }
    }

    fn zero(&mut self, address: u32, len: u32) {
        unsafe {
            asm!(
                "beqz {n}, 2f",
                "1:",
                "sb zero, 0({d})",
                "addi {d}, {d}, 1",
                "addi {n}, {n}, -1",
                "bnez {n}, 1b",
                "2:",
                d = inout(reg) address => _,
                n = inout(reg) len => _,
                options(nostack),
            );
        }
    }
}

#[inline(never)]
fn read_area(address: u32, area: &mut [u8; AREA_SIZE]) {
    let (words, _) = area.as_chunks_mut::<4>();
    for (i, word) in words.iter_mut().enumerate() {
        *word = unsafe { read_volatile((address as usize + 4 * i) as *const u32) }.to_le_bytes();
    }
}

#[inline(never)]
fn write_area(address: u32, area: &[u8; AREA_SIZE]) {
    let (words, _) = area.as_chunks::<4>();
    for (i, word) in words.iter().enumerate() {
        unsafe { write_volatile((address as usize + 4 * i) as *mut u32, u32::from_le_bytes(*word)) };
    }
}

struct RamAreas;

impl DiagRam for RamAreas {
    fn current(&mut self) -> [u8; AREA_SIZE] {
        let mut area = [0u8; AREA_SIZE];
        read_area(DIAG_RECORD_ADDRESS, &mut area);
        area
    }

    fn store_current(&mut self, area: &[u8; AREA_SIZE]) {
        write_area(DIAG_RECORD_ADDRESS, area);
    }

    fn store_last_crash(&mut self, area: &[u8; AREA_SIZE]) {
        write_area(LAST_CRASH_ADDRESS, area);
    }
}

#[inline(never)]
fn store_report(mut bytes: [u8; REPORT_LEN]) {
    let crc = crc32(&bytes[..REPORT_LEN - 4]);
    bytes[REPORT_LEN - 4..].copy_from_slice(&crc.to_le_bytes());
    for (i, byte) in bytes.iter().enumerate() {
        unsafe { write_volatile((STAGE0_REPORT_ADDRESS as usize + i) as *mut u8, *byte) };
    }
}

fn finish(outcome: Outcome, report: &Report, flash_head: Option<u32>) -> (Outcome, u8) {
    let mut encoded = encode_report(&outcome, report);
    if let Some(head) = flash_head {
        encoded[56..60].copy_from_slice(&head.to_le_bytes());
    }
    store_report(encoded);
    (outcome, encoded[4])
}

fn snapshot_entry() {
    let mut current = [0u8; AREA_SIZE];
    read_area(DIAG_RECORD_ADDRESS, &mut current);
    unsafe { (&raw mut ENTRY_SNAPSHOT).write(current) };
}

fn entry_untouched(current: &[u8; AREA_SIZE]) -> bool {
    let snapshot = unsafe { (&raw const ENTRY_SNAPSHOT).read() };
    current[..] == snapshot[..]
}

fn dry_run() -> (Outcome, u8) {
    let Some(key) = public_key() else {
        return finish(Outcome::BootRom(BootRomReason::NoKey), &Report::EMPTY, None);
    };
    let flash = BootFlash::Rom(RomFlash::new(unsafe { BootRom::prepared() }, None));
    let (outcome, report, mut flash) = Stage0 { key: Some(&key), dry_run: true }.run(flash, &mut Ram, &mut RamAreas);
    let mut head = [0u8; 4];
    flash.read(0, &mut head).ok();
    finish(outcome, &report, Some(u32::from_le_bytes(head)))
}

enum BootFlash {
    Xip { writer: Writer },
    Rom(RomFlash<BootRom>),
}

impl BootFlash {
    fn from_session(flash: RomFlash<BootRom>, writer: Writer, identity: &Identity) -> Self {
        flash.into_rom().end();
        if unsafe { norc::enter_quad_90mhz(360_000_000) }.is_some() {
            let mut check = [0u8; SPV_IDENTITY_LEN];
            unsafe { norc::read_xip(FLASH_XIP_BASE, 0, &mut check) };
            if &check == identity.bytes() {
                return Self::Xip { writer };
            }
            unsafe { norc::restore_default() };
        }
        Self::Rom(RomFlash::new(unsafe { BootRom::begin() }, Some(writer)))
    }

    #[inline(never)]
    fn ensure_rom(&mut self) -> Result<&mut RomFlash<BootRom>, FlashError> {
        if let Self::Xip { writer } = *self {
            unsafe { norc::restore_default() };
            let rom = unsafe { BootRom::begin() };
            if !rom.available() {
                return Err(FlashError::Unavailable);
            }
            *self = Self::Rom(RomFlash::new(rom, Some(writer)));
        }
        match self {
            Self::Rom(flash) => Ok(flash),
            Self::Xip { .. } => Err(FlashError::Unavailable),
        }
    }

    fn finish(self) {
        match self {
            Self::Xip { .. } => unsafe { norc::restore_default() },
            Self::Rom(flash) => flash.into_rom().end(),
        }
    }
}

impl ErrorType for BootFlash {
    type Error = FlashError;
}

impl ReadNorFlash for BootFlash {
    const READ_SIZE: usize = 1;

    #[inline(never)]
    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), FlashError> {
        match self {
            Self::Xip { .. } => {
                let len = u32::try_from(bytes.len()).map_err(|_| FlashError::OutOfBounds)?;
                match offset.checked_add(len) {
                    Some(end) if end <= FLASH_SIZE => {
                        if !bytes.is_empty() {
                            unsafe { norc::read_xip(FLASH_XIP_BASE, offset, bytes) };
                        }
                        Ok(())
                    }
                    _ => Err(FlashError::OutOfBounds),
                }
            }
            Self::Rom(flash) => flash.read(offset, bytes),
        }
    }

    fn capacity(&self) -> usize {
        FLASH_SIZE as usize
    }
}

impl NorFlash for BootFlash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), FlashError> {
        self.ensure_rom()?.erase(from, to)
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), FlashError> {
        self.ensure_rom()?.write(offset, bytes)
    }
}

#[inline(never)]
fn open_session(writer: Writer) -> Result<(RomFlash<BootRom>, Identity), BootRomReason> {
    let identity = unsafe { Identity::from_xip() };
    let rom = unsafe { BootRom::begin() };
    if !rom.available() {
        return Err(BootRomReason::FlashUnavailable);
    }
    let mut flash = RomFlash::new(rom, Some(writer));
    if flash.matches(0, identity.bytes()) != Ok(true) {
        flash.into_rom().end();
        return Err(BootRomReason::FlashMismatch);
    }
    Ok((flash, identity))
}

fn wait_cycles(cycles_to_wait: u32) {
    let start = cycles();
    while cycles().wrapping_sub(start) < cycles_to_wait {}
}

fn switch_turbo_dcdc() {
    use kumi_chip::power::*;
    unsafe {
        asm!("fence iorw, iorw", options(nostack));
        kumi_chip::regs::write(CMU_SYSCLK, SYSCLK_OSC_DEV);
        let _ = kumi_chip::regs::read(CMU_SYSCLK);
        asm!("fence iorw, iorw", options(nostack));
        kumi_chip::regs::update(AON_OSC_XTAL, 0, OSC_XTAL_ENABLE);
        kumi_chip::regs::update(AON_PMU_PWR_CTL, 0, EXT_DCDC_ENABLE);
        wait_cycles(DCDC_HANDOFF_US * 24);
        kumi_chip::regs::update(AON_PMU_PWR_CTL, INT_LDO_ENABLE, 0);
        kumi_chip::regs::update(AON_PMU_VOL_CTL, DVDD_VOL_MASK, DVDD_VOL_1_4V << DVDD_VOL_SHIFT);
        wait_cycles(VOLTAGE_SETTLE_US * 24);
        kumi_chip::regs::write(AON_PLL_CORE, PLL_CORE_360);
        wait_cycles(PLL_LOCK_US * 24);
        kumi_chip::regs::write(CMU_SYSCLK, CpuProfile::Turbo360.sysclk());
        let _ = kumi_chip::regs::read(CMU_SYSCLK);
        asm!("fence iorw, iorw", options(nostack));
    }
}

#[inline(never)]
fn restore_osc_ldo() {
    use kumi_chip::power::*;
    unsafe {
        asm!("fence iorw, iorw", options(nostack));
        kumi_chip::regs::update(RMU_RSTEN0, 0, AON_GATE_BIT);
        kumi_chip::regs::update(CMU_CLKEN0, 0, AON_GATE_BIT);
        kumi_chip::regs::write(CMU_SYSCLK, SYSCLK_OSC_DEV);
        let _ = kumi_chip::regs::read(CMU_SYSCLK);
        asm!("fence iorw, iorw", options(nostack));
        kumi_chip::regs::update(AON_PMU_PWR_CTL, 0, INT_LDO_ENABLE);
        wait_cycles(DCDC_HANDOFF_US * 24);
        kumi_chip::regs::update(AON_PMU_PWR_CTL, EXT_DCDC_ENABLE, 0);
        asm!("fence iorw, iorw", options(nostack));
    }
}

fn boot() -> Outcome {
    let key = public_key();
    let (flash, identity) = match open_session(Writer::Stage0) {
        Ok(pair) => pair,
        Err(reason) => return finish(Outcome::BootRom(reason), &Report::EMPTY, None).0,
    };
    let guard_qspi = unsafe { rom::has_quad_spi_init() };
    if guard_qspi {
        unsafe {
            regs::update(regs::RETAINED, 0, regs::SERVICE_ON_RESET);
            watchdog::arm_safe_reset();
        }
    }
    switch_turbo_dcdc();
    let boot_flash = BootFlash::from_session(flash, Writer::Stage0, &identity);
    let (outcome, report, boot_flash) = Stage0 { key: key.as_ref(), dry_run: false }.run(boot_flash, &mut Ram, &mut RamAreas);
    boot_flash.finish();
    restore_osc_ldo();
    if guard_qspi {
        unsafe {
            watchdog::disable();
            regs::update(regs::RETAINED, regs::SERVICE_ON_RESET, 0);
        }
    }
    finish(outcome, &report, None).0
}

fn cycles() -> u32 {
    let value: u32;
    unsafe { asm!("csrr {0}, mcycle", out(reg) value, options(nomem, nostack)) };
    value
}

fn reset_to_bootrom() -> ! {
    unsafe { asm!("csrci mstatus, 8", options(nomem, nostack)) };
    restore_osc_ldo();
    unsafe { asm!("csrci 0x7ca, 1", "fence.i", options(nostack)) };
    kumi_chip::reset::reset(true)
}

fn scratch_session(flash: &mut RomFlash<BootRom>, out: &mut [u8; REPORT_LEN]) -> u8 {
    let at = SCRATCH.offset;
    if flash.erased(at, SECTOR) != Ok(true) {
        return G2B_NOT_ERASED;
    }
    out[5] |= G2B_ERASED_BEFORE;
    if flash.erase(at, at + SECTOR).is_ok() {
        out[5] |= G2B_FIRST_ERASE;
    }
    let mut page = [0u8; PAGE as usize];
    if no_op_pattern(flash, at, &mut page) == Ok(true) && flash.write(at, &page).is_ok() {
        out[5] |= G2B_PROGRAMMED;
    }
    if flash.erase(at, at + SECTOR).is_ok() {
        out[5] |= G2B_SECOND_ERASE;
    }
    if out[5] & G2B_WRITES == G2B_WRITES { G2B_PASSED } else { G2B_WRITE_FAILED }
}

fn rom_head_crc(flash: &mut RomFlash<BootRom>) -> Option<u32> {
    let mut chunk = [0u8; PAGE as usize];
    let mut crc = 0xFFFF_FFFF;
    for at in (0..SECTOR).step_by(PAGE as usize) {
        flash.read(at, &mut chunk).ok()?;
        crc = kumi_layout::crc32_update(crc, &chunk);
    }
    Some(crc ^ 0xFFFF_FFFF)
}

fn g2b() -> u8 {
    let mut out = [0u8; REPORT_LEN];
    out[0..4].copy_from_slice(b"KS0W");
    let started = cycles();
    let rom = unsafe { BootRom::begin() };
    let (mut outcome, before, after) = if rom.available() {
        out[5] |= G2B_PAUSED | G2B_IDENTITY;
        let mut flash = RomFlash::new(rom, Some(Writer::Scratch));
        let before = rom_head_crc(&mut flash);
        let outcome = scratch_session(&mut flash, &mut out);
        let after = rom_head_crc(&mut flash);
        flash.into_rom().end();
        (outcome, before, after)
    } else {
        (G2B_UNAVAILABLE, None, None)
    };
    let elapsed = cycles().wrapping_sub(started);
    if before.is_some() && after == before {
        out[5] |= G2B_HEAD_UNCHANGED;
    } else if outcome == G2B_PASSED {
        outcome = G2B_HEAD_CHANGED;
    }
    out[4] = outcome;
    out[12..16].copy_from_slice(&before.unwrap_or(0).to_le_bytes());
    out[16..20].copy_from_slice(&after.unwrap_or(0).to_le_bytes());
    out[24..28].copy_from_slice(&elapsed.to_le_bytes());
    store_report(out);
    outcome
}

fn restore_bootrom_trap() {
    let mtvec = saved(SAVED_MTVEC);
    let mcache = saved(SAVED_MCACHE);
    unsafe {
        asm!(
            "csrw 0x7ca, {c}",
            "fence.i",
            "csrw mtvec, {v}",
            c = in(reg) mcache,
            v = in(reg) mtvec,
            options(nomem, nostack),
        )
    };
}

fn jump(entry: u32) -> ! {
    unsafe { asm!("fence.i", "jr {entry}", entry = in(reg) entry, in("a0") BOOT_INFO_ADDRESS, options(noreturn)) }
}

#[unsafe(no_mangle)]
extern "C" fn kumi_stage0_boot_main() -> ! {
    unsafe {
        kumi_chip::regs::update(0x2000_3000, 0, 1 << 27);
        kumi_chip::regs::update(0x2000_2000, 0, 1 << 27);
        let ready = kumi_chip::rom::usb_clock_source(kumi_chip::rom::USB_SOURCE_HZ) == 0;
        let _ = kumi_chip::rom::usb_pll(u32::from(ready));
    }
    snapshot_entry();
    let outcome = boot();
    restore_bootrom_trap();
    match outcome {
        Outcome::Jump { entry, .. } => jump(entry),
        _ => reset_to_bootrom(),
    }
}

#[unsafe(no_mangle)]
extern "C" fn kumi_stage0_dry_main() -> u32 {
    u32::from(dry_run().1)
}

#[unsafe(no_mangle)]
extern "C" fn kumi_stage0_g2b_main() -> u32 {
    u32::from(g2b())
}

fn record_crash(reason: Reason, registers: Registers, message: &str) {
    let mut current = [0u8; AREA_SIZE];
    read_area(DIAG_RECORD_ADDRESS, &mut current);
    if entry_untouched(&current) || kumi_diag::read(&current).is_none() {
        Diag::begin(&mut current, u32::MAX, 0);
    }
    if let Some(mut record) = Diag::attach(&mut current) {
        record.crash(reason, registers, 0, message);
    }
    write_area(DIAG_RECORD_ADDRESS, &current);
}

fn disarm_after_fault() {
    let rom = unsafe { BootRom::begin() };
    if rom.available() {
        let mut flash = RomFlash::new(rom, Some(Writer::Stage0));
        let _ = flash.write(DISARM.offset, &[0; DISARM.size as usize]);
    }
}

fn fail_boot(reason: Reason, registers: Registers, message: &str) -> ! {
    arm_fault_reset();
    unsafe { norc::restore_default() };
    restore_osc_ldo();
    record_crash(reason, registers, message);
    disarm_after_fault();
    reset_to_bootrom()
}

#[unsafe(no_mangle)]
extern "C" fn kumi_stage0_trap_main(mcause: u32, mepc: u32, mtval: u32, ra: u32, sp: u32) -> ! {
    let registers = Registers { mcause, mepc, mtval, ra, sp };
    match mode() {
        MODE_DRY | MODE_G2B => {
            let mut out = [0u8; REPORT_LEN];
            out[0..4].copy_from_slice(b"KS0T");
            for (i, value) in [mcause, mepc, mtval, ra, sp].iter().enumerate() {
                out[4 + 4 * i..8 + 4 * i].copy_from_slice(&value.to_le_bytes());
            }
            store_report(out);
            unsafe { kumi_stage0_call_exit(STAGE0_CALL_FAULTED) }
        }
        MODE_VERIFY => unsafe { kumi_stage0_call_exit(STAGE0_CALL_FAULTED) },
        MODE_BOOT => fail_boot(Reason::Trap, registers, "stage0 trap"),
        _ => reset_to_bootrom(),
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    let message = info.message().as_str().unwrap_or("stage0 panic");
    match mode() {
        MODE_DRY | MODE_G2B => {
            let mut len = message.len().min(PANIC_TEXT);
            while !message.is_char_boundary(len) {
                len -= 1;
            }
            let mut out = [0u8; REPORT_LEN];
            out[0..4].copy_from_slice(b"KS0P");
            out[4] = len as u8;
            for (dst, &src) in out[5..].iter_mut().zip(message.as_bytes().iter().take(len)) {
                *dst = src;
            }
            store_report(out);
            unsafe { kumi_stage0_call_exit(STAGE0_CALL_PANICKED) }
        }
        MODE_VERIFY => unsafe { kumi_stage0_call_exit(STAGE0_CALL_PANICKED) },
        MODE_BOOT => fail_boot(Reason::Panic, Registers::default(), message),
        _ => reset_to_bootrom(),
    }
}

// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![no_std]
#![no_main]

use core::arch::{asm, global_asm};
use core::panic::PanicInfo;
use core::ptr::{read_volatile, write_volatile};

const REPORT: usize = 0x8000;
const REPORT_WORDS: usize = 256;
const TRAP_FLAG: usize = REPORT + 4 * 127;
const MAGIC: u32 = u32::from_le_bytes(*b"KUM1");
const LAYOUT: u32 = 3;
const G4_TABLE_MAGIC: u32 = u32::from_le_bytes(*b"KG41");

const W_STATUS: usize = 2;
const W_SP: usize = 3;
const W_RA: usize = 4;
const W_MSTATUS: usize = 5;
const W_MTVEC: usize = 6;
const W_MIE: usize = 7;
const W_CSRS: usize = 8;
const W_CSR_TRAPS: usize = 23;
const W_CYCLES_PER_MS: usize = 24;
const W_MTIME_PER_MS: usize = 25;
const W_FRAMES: usize = 26;
const W_CYCLES: usize = 27;
const W_MTIME: usize = 28;
const W_AMO_CAUSE: usize = 29;
const W_LRSC_CAUSE: usize = 30;
const W_REG_COUNT: usize = 31;
const W_REGS: usize = 32;
const W_LOOP_CYCLES: usize = 120;
const W_LOOP_INSTRET: usize = 121;
const W_WAIT_INSTRET: usize = 122;
const W_INSTRET_TRAP: usize = 123;
const W_V3_STATUS: usize = 128;
const W_TIMER_CYCLES: usize = 129;
const W_TIMER_A: usize = 130;
const W_TIMER_B: usize = 163;
const W_TIMER_TRAPS: usize = 196;
const W_TIMER_TRAPS_HIGH: usize = 197;
const W_T0_SAVED: usize = 198;
const W_T0_SAMPLES: usize = 201;
const W_T0_RESTORED: usize = 207;
const W_PLIC: usize = 210;
const W_PLIC_PRIORITY: usize = 221;
const W_PLIC_TRAPS: usize = 255;

const STATUS_DONE: u32 = 1;
const STATUS_FRAME_TIMEOUT: u32 = 2;
const STATUS_FPU_PRESENT: u32 = 4;
const STATUS_FPU_TRAPPED: u32 = 8;
const STATUS_MCYCLE_STUCK: u32 = 16;
const STATUS_DLM_EXEC_OK: u32 = 32;
const STATUS_DLM_FETCH_TRAPPED: u32 = 64;
const STATUS_A_IN_MISA: u32 = 128;
const STATUS_AMO_TRAPPED: u32 = 256;
const STATUS_AMO_OK: u32 = 512;
const STATUS_LRSC_TRAPPED: u32 = 1024;
const STATUS_LRSC_OK: u32 = 2048;

const V3_TIMER_SAMPLED: u32 = 1;
const V3_TIMER_CHANGED: u32 = 2;
const V3_T0_RUN: u32 = 4;
const V3_T0_COUNTED: u32 = 8;
const V3_T0_RESTORED: u32 = 16;
const V3_PLIC_READ: u32 = 32;

const TIMER_BASE: usize = 0x2001_0000;
const TIMER_WORDS: usize = 33;
const TIMER_WINDOW_CYCLES: u64 = 120_000;
const T0_GLOBAL: usize = TIMER_BASE + 0x04;
const T0_CONTROL: usize = TIMER_BASE + 0x20;
const T0_RELOAD: usize = TIMER_BASE + 0x24;
const T0_COUNT: usize = TIMER_BASE + 0x28;
const T0_GLOBAL_ENABLE: u32 = 0x403;
const T0_START: u32 = 3;
const T0_RELOAD_VALUE: u32 = 120_000;
const T0_SAMPLE_CYCLES: u64 = 30_000;
const PLIC_WORDS: [usize; 11] = [
    0x1000_0000,
    0x1000_1000,
    0x1000_1004,
    0x1000_1080,
    0x1000_1084,
    0x1000_1100,
    0x1000_1104,
    0x1000_2000,
    0x1000_2004,
    0x1020_0000,
    0x1020_0400,
];
const PLIC_PRIORITY: usize = 0x1000_0000;
const PLIC_SOURCES: usize = 34;

const G4_REPORT: usize = 0x8400;
const BISECT: usize = 0x8800;
const BISECT_MAGIC: u32 = u32::from_le_bytes(*b"KS0Q");
const BISECT_RECORD: u32 = u32::from_le_bytes(*b"KS0B");
const BISECT_ROM_INTERRUPTS: u32 = 1;
const BISECT_XIP_UNPAUSED: u32 = 2;
const BISECT_XIP_AS_FOUND: u32 = 4;
const B_XIP: usize = 15;
const FLASH_XIP: usize = 0x4800_0000;
const BISECT_STEPS: u32 = 8;
const BISECT_WORDS: usize = 16;
const B_LIMIT: usize = 1;
const B_FLAGS: usize = 2;
const B_STEP: usize = 3;
const B_ERROR: usize = 4;
const B_JEDEC: usize = 5;
const B_HEAD: usize = 6;
const B_CYCLES: usize = 7;
const G4_WORDS: usize = 32;
const G4_MAGIC: u32 = u32::from_le_bytes(*b"KG4R");
const G4_REPORT_KIND: u32 = 1;
const G4_ARM: u32 = 2;
const G4_DISARM: u32 = 3;
const G4_RESET_KUMI: u32 = 4;
const G4_RESET_STOCK: u32 = 5;
const G4_RESET_NOT_TAKEN: u32 = 6;
const G4_W_KIND: usize = 1;
const G4_W_A0: usize = 2;
const G4_W_RA: usize = 3;
const G4_W_RETAINED_BEFORE: usize = 4;
const G4_W_RETAINED_AFTER: usize = 5;
const G4_W_RESET_CONTROL: usize = 6;
const G4_W_MSTATUS: usize = 7;
const G4_W_TRAPS: usize = 8;
const G4_W_REGS: usize = 9;
const G4_REGS: [usize; 21] = [
    0x2000_3000,
    0x2000_3020,
    0x2000_3024,
    0x2100_8000,
    0x2100_8004,
    0x2100_8008,
    0x2100_800C,
    0x2100_8010,
    0x2100_8014,
    0x2100_8018,
    0x2100_801C,
    0x2100_8020,
    0x2100_8024,
    0x2100_8028,
    0x2100_802C,
    0x2100_8030,
    0x2100_8034,
    0x2100_8038,
    0x2100_803C,
    0x200C_1000,
    0x200C_1004,
];
const RETAINED: usize = 0x2100_801C;
const SERVICE_ON_RESET: u32 = 1 << 31;
const RESET_CONTROL: usize = 0x2000_3024;
const RESET_REQUEST: u32 = 1 << 1;
const RESET_WAIT_CYCLES: u64 = 6_000_000;

const DLM_PROBE: usize = 0x0010_2000;
const DLM_PROBE_SENTINEL: u32 = 0x5A5;
const ATOMIC_PROBE: usize = 0x0010_2100;
const ATOMIC_SEED: u32 = 40;
const ATOMIC_ADD: u32 = 2;

const MUSB_FRAME: usize = 0x200D_000C;
const MUSB_BASE: usize = 0x200D_0000;
const MUSB_INDEX: usize = MUSB_BASE + 0x0E;
const MUSB_COMMON: [usize; 5] = [0x0C, 0x68, 0x6C, 0x78, 0x7C];
const MUSB_INDEXED: [(u8, usize); 9] =
    [(1, 0x10), (1, 0x14), (1, 0x60), (1, 0x64), (2, 0x10), (2, 0x14), (2, 0x60), (2, 0x64), (3, 0x10)];
const MTIME_LO: usize = 0x1200_0000;
const MTIME_HI: usize = 0x1200_0004;
const FRAMES_TO_MEASURE: u16 = 32;
const WAIT_LIMIT_CYCLES: u64 = 1 << 26;
const WAIT_LIMIT_ITERATIONS: u32 = 400_000;
const CPI_LOOP_ITERATIONS: u32 = 4096;

const MMIO: [usize; 30] = [
    0x2000_0000,
    0x2000_0010,
    0x2000_0024,
    0x2000_2000,
    0x2000_2010,
    0x2000_2020,
    0x2000_2028,
    0x2000_2030,
    0x2000_2060,
    0x2000_2064,
    0x2000_2078,
    0x2000_3000,
    0x2000_3020,
    0x2000_3024,
    0x2009_0000,
    0x2009_0004,
    0x2009_0008,
    0x2009_0030,
    0x2009_0034,
    0x2009_0038,
    0x2009_003C,
    0x200A_0000,
    0x200A_0004,
    0x200D_0000,
    0x200D_0060,
    0x2100_801C,
    0x2100_4010,
    0x1200_0008,
    0x1200_000C,
    0x2002_1000,
];

global_asm!(
    ".section .text.entry, \"ax\"",
    ".option push",
    ".option norvc",
    ".global _start",
    "_start:",
    "j kumi_probe_main",
    "j kumi_g4_report",
    "j kumi_g4_arm",
    "j kumi_g4_disarm",
    "j kumi_g4_reset_kumi",
    "j kumi_g4_reset_stock",
    ".4byte {magic}",
    "j kumi_bisect",
    ".option pop",
    magic = const G4_TABLE_MAGIC,
);

global_asm!(
    ".section .text.trap, \"ax\"",
    ".balign 64",
    ".global kumi_probe_trap",
    "kumi_probe_trap:",
    "csrr t6, mepc",
    "lhu t5, 0(t6)",
    "andi t5, t5, 3",
    "addi t6, t6, 2",
    "addi t5, t5, -3",
    "bnez t5, 1f",
    "addi t6, t6, 2",
    "1:",
    "csrw mepc, t6",
    "csrr t5, mcause",
    "andi t5, t5, 0xff",
    "ori t5, t5, 0x100",
    "li t6, {flag}",
    "sw t5, 0(t6)",
    "mret",
    flag = const TRAP_FLAG,
);

unsafe extern "C" {
    fn kumi_probe_trap();
}

fn put(word: usize, value: u32) {
    unsafe { write_volatile((REPORT + 4 * word) as *mut u32, value) }
}

fn clear_trap() {
    unsafe { write_volatile(TRAP_FLAG as *mut u32, 0) }
}

fn trap_word() -> u32 {
    unsafe { read_volatile(TRAP_FLAG as *const u32) }
}

fn trapped() -> bool {
    trap_word() != 0
}

macro_rules! csr {
    ($number:literal) => {{
        let value: u32;
        clear_trap();
        unsafe {
            asm!(
                "li {v}, 0",
                concat!("csrr {v}, ", stringify!($number)),
                v = out(reg) value,
                out("t5") _,
                out("t6") _,
            )
        };
        (value, trapped())
    }};
}

fn peek(address: usize) -> Option<u32> {
    let value: u32;
    clear_trap();
    unsafe {
        asm!(
            "li {v}, 0",
            "lw {v}, 0({a})",
            v = out(reg) value,
            a = in(reg) address,
            out("t5") _,
            out("t6") _,
        )
    };
    if trapped() { None } else { Some(value) }
}

fn peek_indexed(ep: u8, offset: usize) -> Option<u32> {
    let saved = unsafe { read_volatile(MUSB_INDEX as *const u8) };
    unsafe { write_volatile(MUSB_INDEX as *mut u8, ep) };
    let value = peek(MUSB_BASE + offset);
    unsafe { write_volatile(MUSB_INDEX as *mut u8, saved) };
    value
}

fn peek16(address: usize) -> Option<u16> {
    let value: u32;
    clear_trap();
    unsafe {
        asm!(
            "li {v}, 0",
            "lhu {v}, 0({a})",
            v = out(reg) value,
            a = in(reg) address,
            out("t5") _,
            out("t6") _,
        )
    };
    if trapped() { None } else { Some(value as u16) }
}

fn mcycle() -> u64 {
    loop {
        let (hi, _) = csr!(0xB80);
        let (lo, _) = csr!(0xB00);
        let (again, _) = csr!(0xB80);
        if hi == again {
            return (u64::from(hi) << 32) | u64::from(lo);
        }
    }
}

fn instret() -> Option<u32> {
    let (value, trapped) = csr!(0xB02);
    if trapped {
        put(W_INSTRET_TRAP, trap_word());
        None
    } else {
        Some(value)
    }
}

fn measure_cpi() {
    let (c0, i0, c1, i1): (u32, u32, u32, u32);
    clear_trap();
    unsafe {
        asm!(
            "csrr {c0}, mcycle",
            "csrr {i0}, minstret",
            "1:",
            "addi {n}, {n}, -1",
            "bnez {n}, 1b",
            "csrr {i1}, minstret",
            "csrr {c1}, mcycle",
            c0 = out(reg) c0,
            i0 = out(reg) i0,
            c1 = out(reg) c1,
            i1 = out(reg) i1,
            n = inout(reg) CPI_LOOP_ITERATIONS => _,
            out("t5") _,
            out("t6") _,
        )
    };
    if trapped() {
        put(W_INSTRET_TRAP, trap_word());
        return;
    }
    put(W_LOOP_CYCLES, c1.wrapping_sub(c0));
    put(W_LOOP_INSTRET, i1.wrapping_sub(i0));
}

fn mtime() -> u64 {
    loop {
        let hi = peek(MTIME_HI).unwrap_or(0);
        let lo = peek(MTIME_LO).unwrap_or(0);
        if peek(MTIME_HI).unwrap_or(0) == hi {
            return (u64::from(hi) << 32) | u64::from(lo);
        }
    }
}

fn frame() -> Option<u16> {
    peek16(MUSB_FRAME).map(|f| f & 0x7FF)
}

fn wait_frames(start: u16, frames: u16, deadline: u64) -> Option<u16> {
    for _ in 0..WAIT_LIMIT_ITERATIONS {
        let now = frame()?;
        let elapsed = now.wrapping_sub(start) & 0x7FF;
        if elapsed >= frames {
            return Some(elapsed);
        }
        if mcycle() > deadline {
            return None;
        }
    }
    None
}

fn measure_clocks(status: &mut u32) {
    let begin = mcycle();
    if mcycle() == begin && mcycle() == begin {
        *status |= STATUS_MCYCLE_STUCK;
    }
    let deadline = begin.wrapping_add(WAIT_LIMIT_CYCLES);
    let Some(first) = frame() else {
        *status |= STATUS_FRAME_TIMEOUT;
        return;
    };
    let Some(edge) = wait_frames(first, 1, deadline).map(|_| frame().unwrap_or(first)) else {
        *status |= STATUS_FRAME_TIMEOUT;
        return;
    };
    let c0 = mcycle();
    let i0 = instret();
    let t0 = mtime();
    let Some(frames) = wait_frames(edge, FRAMES_TO_MEASURE, deadline.wrapping_add(WAIT_LIMIT_CYCLES)) else {
        *status |= STATUS_FRAME_TIMEOUT;
        return;
    };
    let cycles = mcycle().wrapping_sub(c0);
    let ticks = mtime().wrapping_sub(t0);
    if let (Some(start), Some(end)) = (i0, instret()) {
        put(W_WAIT_INSTRET, end.wrapping_sub(start));
    }
    let frames = u64::from(frames.max(1));
    put(W_FRAMES, frames as u32);
    put(W_CYCLES, cycles as u32);
    put(W_MTIME, ticks as u32);
    put(W_CYCLES_PER_MS, (cycles / frames) as u32);
    put(W_MTIME_PER_MS, (ticks / frames) as u32);
}

fn probe_fpu(status: &mut u32) {
    let (misa, _) = csr!(0x301);
    if misa & (1 << 5) != 0 {
        *status |= STATUS_FPU_PRESENT;
    }
    clear_trap();
    unsafe {
        asm!(
            "li t5, 0x2000",
            "csrs mstatus, t5",
            ".4byte 0xF0000053",
            ".4byte 0x00007053",
            "li t5, 0x6000",
            "csrc mstatus, t5",
            out("t5") _,
            out("t6") _,
        )
    };
    if trapped() {
        *status |= STATUS_FPU_TRAPPED;
    }
}

fn probe_dlm_exec(status: &mut u32) {
    clear_trap();
    unsafe {
        write_volatile(DLM_PROBE as *mut u32, 0x5A50_0513);
        write_volatile((DLM_PROBE + 4) as *mut u32, 0x0003_0067);
    }
    if trapped() {
        return;
    }
    let result: u32;
    unsafe {
        asm!(
            "la t2, 3f",
            "csrw mtvec, t2",
            "fence.i",
            "li {res}, 0",
            "la t1, 2f",
            "li t0, 0x102000",
            "jr t0",
            "2:",
            "mv {res}, a0",
            "j 4f",
            ".balign 4",
            "3:",
            "li {res}, 0xbad",
            "4:",
            res = out(reg) result,
            out("a0") _,
            out("t0") _,
            out("t1") _,
            out("t2") _,
        );
        asm!("csrw mtvec, {t}", t = in(reg) kumi_probe_trap as *const () as usize);
    }
    if result == DLM_PROBE_SENTINEL {
        *status |= STATUS_DLM_EXEC_OK;
    } else if result == 0xBAD {
        *status |= STATUS_DLM_FETCH_TRAPPED;
    }
}

fn probe_atomics(status: &mut u32) {
    let (misa, _) = csr!(0x301);
    if misa & 1 != 0 {
        *status |= STATUS_A_IN_MISA;
    }
    clear_trap();
    unsafe { write_volatile(ATOMIC_PROBE as *mut u32, ATOMIC_SEED) };
    if trapped() {
        return;
    }
    let old: u32;
    unsafe {
        asm!(
            "li {old}, 0",
            ".option push",
            ".option arch, +a",
            "amoadd.w {old}, {add}, ({a})",
            ".option pop",
            old = out(reg) old,
            add = in(reg) ATOMIC_ADD,
            a = in(reg) ATOMIC_PROBE,
            out("t5") _,
            out("t6") _,
        )
    };
    let mut expected = ATOMIC_SEED;
    put(W_AMO_CAUSE, trap_word());
    if trapped() {
        *status |= STATUS_AMO_TRAPPED;
    } else if old == ATOMIC_SEED && unsafe { read_volatile(ATOMIC_PROBE as *const u32) } == ATOMIC_SEED + ATOMIC_ADD {
        *status |= STATUS_AMO_OK;
        expected = ATOMIC_SEED + ATOMIC_ADD;
    }
    clear_trap();
    let failed: u32;
    unsafe {
        asm!(
            "li {f}, 7",
            ".option push",
            ".option arch, +a",
            "lr.w t4, ({a})",
            "addi t4, t4, 1",
            "sc.w {f}, t4, ({a})",
            ".option pop",
            f = out(reg) failed,
            a = in(reg) ATOMIC_PROBE,
            out("t4") _,
            out("t5") _,
            out("t6") _,
        )
    };
    put(W_LRSC_CAUSE, trap_word());
    if trapped() {
        *status |= STATUS_LRSC_TRAPPED;
    } else if failed == 0 && unsafe { read_volatile(ATOMIC_PROBE as *const u32) } == expected + 1 {
        *status |= STATUS_LRSC_OK;
    }
}

fn poke(address: usize, value: u32) {
    unsafe { write_volatile(address as *mut u32, value) }
}

fn spin(cycles: u64) {
    let start = mcycle();
    while mcycle().wrapping_sub(start) < cycles {}
}

fn timer_snapshot(first: usize, traps: &mut u64) {
    for i in 0..TIMER_WORDS {
        match peek(TIMER_BASE + 4 * i) {
            Some(value) => put(first + i, value),
            None => *traps |= 1 << i,
        }
    }
}

fn sample_timer_block(v3: &mut u32) {
    let mut traps = 0u64;
    let start = mcycle();
    timer_snapshot(W_TIMER_A, &mut traps);
    spin(TIMER_WINDOW_CYCLES);
    timer_snapshot(W_TIMER_B, &mut traps);
    put(W_TIMER_CYCLES, mcycle().wrapping_sub(start) as u32);
    put(W_TIMER_TRAPS, traps as u32);
    put(W_TIMER_TRAPS_HIGH, (traps >> 32) as u32);
    *v3 |= V3_TIMER_SAMPLED;
    let changed = (0..TIMER_WORDS).any(|i| {
        let a = unsafe { read_volatile((REPORT + 4 * (W_TIMER_A + i)) as *const u32) };
        let b = unsafe { read_volatile((REPORT + 4 * (W_TIMER_B + i)) as *const u32) };
        traps & (1 << i) == 0 && a != b
    });
    if changed {
        *v3 |= V3_TIMER_CHANGED;
    }
}

fn timer0_experiment(v3: &mut u32) {
    let (Some(global), Some(control), Some(reload)) = (peek(T0_GLOBAL), peek(T0_CONTROL), peek(T0_RELOAD)) else {
        return;
    };
    put(W_T0_SAVED, global);
    put(W_T0_SAVED + 1, control);
    put(W_T0_SAVED + 2, reload);
    *v3 |= V3_T0_RUN;
    poke(T0_GLOBAL, global | T0_GLOBAL_ENABLE);
    poke(T0_RELOAD, T0_RELOAD_VALUE);
    poke(T0_CONTROL, T0_START);
    let mut first = None;
    let mut counted = false;
    for sample in 0..3 {
        if sample > 0 {
            spin(T0_SAMPLE_CYCLES);
        }
        let cycles = mcycle() as u32;
        let count = peek(T0_COUNT).unwrap_or(0xDEAD_0BAD);
        put(W_T0_SAMPLES + 2 * sample, cycles);
        put(W_T0_SAMPLES + 2 * sample + 1, count);
        match first {
            None => first = Some(count),
            Some(value) if value != count => counted = true,
            Some(_) => {}
        }
    }
    poke(T0_CONTROL, control);
    poke(T0_RELOAD, reload);
    poke(T0_GLOBAL, global);
    let restored = [peek(T0_GLOBAL), peek(T0_CONTROL), peek(T0_RELOAD)];
    for (i, value) in restored.iter().enumerate() {
        put(W_T0_RESTORED + i, value.unwrap_or(0xDEAD_0BAD));
    }
    if counted {
        *v3 |= V3_T0_COUNTED;
    }
    if restored == [Some(global), Some(control), Some(reload)] {
        *v3 |= V3_T0_RESTORED;
    }
}

fn read_plic(v3: &mut u32) {
    let mut traps = 0u32;
    for (i, address) in PLIC_WORDS.iter().enumerate() {
        match peek(*address) {
            Some(value) => put(W_PLIC + i, value),
            None => traps |= 1 << i,
        }
    }
    for source in 1..=PLIC_SOURCES {
        match peek(PLIC_PRIORITY + 4 * source) {
            Some(value) => put(W_PLIC_PRIORITY + source - 1, value),
            None => traps |= 1 << 16,
        }
    }
    put(W_PLIC_TRAPS, traps);
    *v3 |= V3_PLIC_READ;
}

fn g4_put(word: usize, value: u32) {
    unsafe { write_volatile((G4_REPORT + 4 * word) as *mut u32, value) }
}

fn g4_enter() -> (u32, u32) {
    let mstatus: u32;
    let mtvec: u32;
    unsafe {
        asm!(
            "csrrci {s}, mstatus, 8",
            "csrr {v}, mtvec",
            "csrw mtvec, {t}",
            s = out(reg) mstatus,
            v = out(reg) mtvec,
            t = in(reg) kumi_probe_trap as *const () as usize,
        )
    };
    (mstatus, mtvec)
}

fn g4_leave(mstatus: u32, mtvec: u32) {
    unsafe {
        asm!(
            "csrw mtvec, {v}",
            "csrw mstatus, {s}",
            v = in(reg) mtvec,
            s = in(reg) mstatus,
        )
    };
}

fn g4_record(kind: u32, a0: u32, ra: u32, mstatus: u32) {
    for word in 0..G4_WORDS {
        g4_put(word, 0);
    }
    g4_put(G4_W_KIND, kind);
    g4_put(G4_W_A0, a0);
    g4_put(G4_W_RA, ra);
    g4_put(G4_W_MSTATUS, mstatus);
    let mut traps = 0u32;
    for (i, address) in G4_REGS.iter().enumerate() {
        match peek(*address) {
            Some(value) => g4_put(G4_W_REGS + i, value),
            None => traps |= 1 << i,
        }
    }
    g4_put(G4_W_TRAPS, traps);
    let retained = unsafe { read_volatile(RETAINED as *const u32) };
    g4_put(G4_W_RETAINED_BEFORE, retained);
    g4_put(G4_W_RETAINED_AFTER, retained);
    g4_put(G4_W_RESET_CONTROL, unsafe { read_volatile(RESET_CONTROL as *const u32) });
    g4_put(0, G4_MAGIC);
}

fn g4_retained(value: u32) {
    poke(RETAINED, value);
    g4_put(G4_W_RETAINED_AFTER, unsafe { read_volatile(RETAINED as *const u32) });
}

fn g4_call(kind: u32, a0: u32, ra: u32) {
    let (mstatus, mtvec) = g4_enter();
    g4_record(kind, a0, ra, mstatus);
    let retained = unsafe { read_volatile(RETAINED as *const u32) };
    match kind {
        G4_ARM => g4_retained(retained | SERVICE_ON_RESET),
        G4_DISARM => g4_retained(retained & !SERVICE_ON_RESET),
        G4_RESET_KUMI | G4_RESET_STOCK => {
            let control = unsafe { read_volatile(RESET_CONTROL as *const u32) };
            let stock = kind == G4_RESET_STOCK;
            g4_retained(if stock { SERVICE_ON_RESET } else { retained | SERVICE_ON_RESET });
            unsafe { asm!("fence") };
            poke(RESET_CONTROL, if stock { RESET_REQUEST } else { control | RESET_REQUEST });
            spin(RESET_WAIT_CYCLES);
            poke(RESET_CONTROL, control);
            g4_retained(retained);
            g4_put(G4_W_KIND, G4_RESET_NOT_TAKEN);
        }
        _ => {}
    }
    g4_leave(mstatus, mtvec);
}

macro_rules! g4_entry {
    ($name:ident, $kind:expr) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn $name(a0: u32) {
            let ra: u32;
            unsafe { asm!("mv {0}, ra", out(reg) ra) };
            g4_call($kind, a0, ra);
        }
    };
}

g4_entry!(kumi_g4_report, G4_REPORT_KIND);
g4_entry!(kumi_g4_arm, G4_ARM);
g4_entry!(kumi_g4_disarm, G4_DISARM);
g4_entry!(kumi_g4_reset_kumi, G4_RESET_KUMI);
g4_entry!(kumi_g4_reset_stock, G4_RESET_STOCK);

fn bput(word: usize, value: u32) {
    unsafe { write_volatile((BISECT + 4 * word) as *mut u32, value) }
}

fn bget(word: usize) -> u32 {
    unsafe { read_volatile((BISECT + 4 * word) as *const u32) }
}

#[inline(never)]
fn rom_enter(rom_context: bool, mtvec: u32, mstatus: u32) {
    if rom_context {
        unsafe { asm!("csrw mtvec, {0}", "csrs mstatus, {1}", in(reg) mtvec, in(reg) mstatus & 8, options(nostack)) };
    }
}

#[inline(never)]
fn rom_leave(rom_context: bool) {
    if rom_context {
        unsafe {
            asm!("csrci mstatus, 8", "csrw mtvec, {t}", t = in(reg) kumi_probe_trap as *const () as usize, options(nostack))
        };
    }
}

#[inline(never)]
fn bisect_mark(step: &mut u32, started: u64, limit: u32) -> bool {
    bput(B_CYCLES + *step as usize, (mcycle() - started) as u32);
    *step += 1;
    bput(B_STEP, *step);
    *step >= limit
}

#[unsafe(no_mangle)]
pub extern "C" fn kumi_bisect() {
    use kumi_chip::{regs, rom};
    let request = [bget(0), bget(B_LIMIT), bget(B_FLAGS)];
    for word in 0..BISECT_WORDS {
        bput(word, 0);
    }
    if request[0] != BISECT_MAGIC {
        bput(B_ERROR, 0xBAD);
        return;
    }
    let limit = request[1].min(BISECT_STEPS);
    let ctx = request[2] & BISECT_ROM_INTERRUPTS != 0;
    bput(0, BISECT_RECORD);
    bput(B_LIMIT, limit);
    bput(B_FLAGS, request[2]);
    let (mstatus, _) = csr!(0x300);
    let (mtvec, _) = csr!(0x305);
    unsafe { asm!("csrci mstatus, 8", "csrw mtvec, {t}", t = in(reg) kumi_probe_trap as *const () as usize) };
    let started = mcycle();
    let paused_before = unsafe { regs::read(regs::NORC_CONTROL) } & regs::NORC_PAUSE != 0;
    if request[2] & BISECT_XIP_UNPAUSED != 0 {
        unsafe { regs::update(regs::NORC_CONTROL, regs::NORC_PAUSE, 0) };
        bput(B_XIP, unsafe { read_volatile(FLASH_XIP as *const u32) });
        if paused_before {
            unsafe { regs::update(regs::NORC_CONTROL, 0, regs::NORC_PAUSE) };
        }
    }
    if request[2] & BISECT_XIP_AS_FOUND != 0 {
        bput(B_XIP, unsafe { read_volatile(FLASH_XIP as *const u32) });
    }
    let mut step = 0;
    let mut initialised = false;
    'run: {
        if limit == 0 {
            break 'run;
        }
        unsafe { regs::update(regs::NORC_CONTROL, 0, regs::NORC_PAUSE) };
        let mut waited = 0;
        while unsafe { regs::read(regs::NORC_STATUS) } & regs::NORC_IDLE == 0 {
            waited += 1;
            if waited > regs::NORC_WAIT_LIMIT {
                bput(B_ERROR, 1);
                break 'run;
            }
        }
        if bisect_mark(&mut step, started, limit) {
            break 'run;
        }
        unsafe { regs::update(regs::PERIPHERAL_ENABLE, 0, regs::SPI_ENABLE) };
        for _ in 0..regs::SETTLE_LOOPS {
            unsafe { read_volatile(regs::NORC_STATUS as *const u32) };
        }
        if bisect_mark(&mut step, started, limit) {
            break 'run;
        }
        rom_enter(ctx, mtvec, mstatus);
        unsafe { rom::sf_init() };
        rom_leave(ctx);
        initialised = true;
        if bisect_mark(&mut step, started, limit) {
            break 'run;
        }
        rom_enter(ctx, mtvec, mstatus);
        let id = unsafe { rom::sf_jedec() };
        rom_leave(ctx);
        bput(B_JEDEC, id);
        if bisect_mark(&mut step, started, limit) {
            break 'run;
        }
        let mut head = [0u8; 4];
        rom_enter(ctx, mtvec, mstatus);
        unsafe { rom::sf_read(kumi_chip_scratch(), &mut head) };
        rom_leave(ctx);
        bput(B_HEAD, u32::from_le_bytes(head));
        if bisect_mark(&mut step, started, limit) {
            break 'run;
        }
        rom_enter(ctx, mtvec, mstatus);
        unsafe { rom::sf_erase(kumi_chip_scratch(), SCRATCH_SECTOR) };
        rom_leave(ctx);
        if bisect_mark(&mut step, started, limit) {
            break 'run;
        }
        let page = [0xFFu8; SCRATCH_PAGE];
        rom_enter(ctx, mtvec, mstatus);
        unsafe { rom::sf_write(kumi_chip_scratch(), &page) };
        rom_leave(ctx);
        if bisect_mark(&mut step, started, limit) {
            break 'run;
        }
        rom_enter(ctx, mtvec, mstatus);
        unsafe { rom::sf_erase(kumi_chip_scratch(), SCRATCH_SECTOR) };
        rom_leave(ctx);
        bisect_mark(&mut step, started, limit);
    }
    if initialised {
        rom_enter(ctx, mtvec, mstatus);
        unsafe { rom::sf_deinit() };
        rom_leave(ctx);
    }
    if !paused_before {
        unsafe { regs::update(regs::NORC_CONTROL, regs::NORC_PAUSE, 0) };
    }
    bput(B_ERROR, bget(B_ERROR) | (trap_word() << 8));
    unsafe { asm!("csrw mtvec, {v}", "csrw mstatus, {s}", v = in(reg) mtvec, s = in(reg) mstatus) };
}

const SCRATCH_OFFSET: u32 = 0x3F_0000;
const SCRATCH_SECTOR: u32 = 0x1000;
const SCRATCH_PAGE: usize = 256;

fn kumi_chip_scratch() -> u32 {
    SCRATCH_OFFSET
}

#[unsafe(no_mangle)]
pub extern "C" fn kumi_probe_main() {
    let sp: u32;
    let ra: u32;
    unsafe { asm!("mv {0}, sp", "mv {1}, ra", out(reg) sp, out(reg) ra) };
    for word in 0..REPORT_WORDS {
        put(word, 0);
    }
    put(0, MAGIC);
    put(1, LAYOUT);
    put(W_SP, sp);
    put(W_RA, ra);
    let (mstatus, _) = csr!(0x300);
    let (mtvec, _) = csr!(0x305);
    let (mie, _) = csr!(0x304);
    put(W_MSTATUS, mstatus);
    put(W_MTVEC, mtvec);
    put(W_MIE, mie);
    unsafe {
        asm!(
            "csrci mstatus, 8",
            "csrw mtvec, {t}",
            t = in(reg) kumi_probe_trap as *const () as usize,
        )
    };
    let mut status = 0u32;
    let mut traps = 0u32;
    let csrs = [
        csr!(0x301),
        csr!(0xF11),
        csr!(0xF12),
        csr!(0xF13),
        csr!(0xF14),
        csr!(0xFC0),
        csr!(0xFC1),
        csr!(0xFC2),
        csr!(0xFC3),
        csr!(0x7C0),
        csr!(0x7C1),
        csr!(0x7CA),
        csr!(0x7D0),
        csr!(0x7C4),
        csr!(0x7C9),
    ];
    for (i, (value, trap)) in csrs.iter().enumerate() {
        put(W_CSRS + i, *value);
        if *trap {
            traps |= 1 << i;
        }
    }
    put(W_CSR_TRAPS, traps);
    probe_fpu(&mut status);
    probe_dlm_exec(&mut status);
    probe_atomics(&mut status);
    measure_cpi();
    measure_clocks(&mut status);
    let mut count = 0;
    let common = MUSB_COMMON.iter().map(|offset| (MUSB_BASE + offset, peek(MUSB_BASE + offset)));
    let indexed = MUSB_INDEXED.iter().map(|&(ep, offset)| (MUSB_BASE | (ep as usize) << 12 | offset, peek_indexed(ep, offset)));
    for (address, value) in MMIO.iter().map(|&a| (a, peek(a))).chain(common).chain(indexed) {
        let index = W_REGS + 2 * count;
        if index + 1 >= W_LOOP_CYCLES {
            break;
        }
        put(index, address as u32);
        put(index + 1, value.unwrap_or(0xDEAD_0BAD));
        count += 1;
    }
    put(W_REG_COUNT, count as u32);
    let mut v3 = 0u32;
    sample_timer_block(&mut v3);
    timer0_experiment(&mut v3);
    read_plic(&mut v3);
    put(W_V3_STATUS, v3);
    put(W_STATUS, status | STATUS_DONE);
    unsafe {
        asm!(
            "csrw mtvec, {v}",
            "csrw mstatus, {s}",
            v = in(reg) mtvec,
            s = in(reg) mstatus,
        )
    };
}

#[panic_handler]
fn panic(_: &PanicInfo) -> ! {
    loop {
        core::hint::spin_loop();
    }
}

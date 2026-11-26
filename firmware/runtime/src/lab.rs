// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use core::ptr::{read_volatile, write_volatile};

use kumi_icd::StreamKind;

use crate::board;

pub const BOOT_MAGIC: u32 = u32::from_le_bytes(*b"KLBT");
pub const JOURNAL_MAGIC: u32 = u32::from_le_bytes(*b"KLJR");
pub const CONTROL_MAGIC: u32 = u32::from_le_bytes(*b"KLCT");

pub const LINK_BAD_LENGTH: u8 = 1;
pub const LINK_CHECKSUM: u8 = 2;

const BOOT_REGIONS: [(u32, u32); 26] = [
    (0x2000_0000, 1),
    (0x2000_0010, 1),
    (0x2000_0024, 1),
    (0x2000_2000, 1),
    (0x2000_2010, 1),
    (0x2000_2020, 1),
    (0x2000_2028, 1),
    (0x2000_2030, 1),
    (0x2000_2060, 2),
    (0x2000_2078, 1),
    (0x2000_2090, 1),
    (0x2000_20F8, 1),
    (0x2000_3000, 1),
    (0x2000_3020, 2),
    (0x2000_6000, 1),
    (0x2004_0000, 2),
    (0x2004_000C, 5),
    (0x2011_0000, 36),
    (0x2011_0110, 2),
    (0x2011_0220, 1),
    (0x2011_0228, 1),
    (0x2011_0300, 1),
    (0x2011_0310, 1),
    (0x2100_0010, 1),
    (0x2100_1000, 1),
    (0x2100_1010, 2),
];
const BOOT_ENTRIES: usize = 72;
const BOOT_ENTRY_CYCLES: usize = 1;
const BOOT_USB_CYCLES: usize = 2;
const BOOT_RUNNING_CYCLES: usize = 3;
const BOOT_CONFIGURED_CYCLES: usize = 4;
const BOOT_COUNT: usize = 5;
const BOOT_LOADED_BY_HOST: usize = 6;
const BOOT_ENTRY_COUNT: usize = 7;
const BOOT_HEADER: usize = 8;
const BOOT_WORDS: usize = BOOT_HEADER + 2 * BOOT_ENTRIES;

pub const JOURNAL_RECORDS: usize = 64;
const JOURNAL_WRITTEN: usize = 1;
const JOURNAL_DROPPED: usize = 2;
const JOURNAL_CPU_KHZ: usize = 3;
const JOURNAL_HEADER: usize = 4;
const JOURNAL_WORDS: usize = JOURNAL_HEADER + 4 * JOURNAL_RECORDS;

pub const WATCH_SLOTS: usize = 16;
const CONTROL_FAKE_STREAM_HZ: usize = 1;
const CONTROL_WATCH_BUDGET: usize = 2;
const CONTROL_HOLD_AWAKE_SECONDS: usize = 3;
const CONTROL_HEADER: usize = 4;
const CONTROL_WORDS: usize = CONTROL_HEADER + 2 * WATCH_SLOTS;
const DEFAULT_WATCH_BUDGET: u32 = 32;
const OFFLINE_TICK_STAMPS: u32 = 586;
const STAMPS_PER_SECOND: u32 = 58_594;
const MAX_HOLD_AWAKE_SECONDS: u32 = 600;
const DEFAULT_WATCH: [(u32, u32); 3] =
    [(0x2011_0110, !((1 << 6) | (1 << 7) | (1 << 14) | (1 << 15))), (0x2011_0114, u32::MAX), (0x2011_0228, u32::MAX)];
const WATCH_LIMIT: u32 = 0x4800_0000;
const FAKE_STREAM_BITS: u8 = 24;

const USB_POWER: usize = 0x200D_0001;
const USB_DEVCTL: usize = 0x200D_0060;
const CLOCK_WINDOW_MS: u32 = 1000;
const CLOCK_MIN_KHZ: u32 = 8_000;
const CLOCK_MAX_KHZ: u32 = 400_000;

pub mod kind {
    pub const BOOT: u8 = 1;
    pub const LINK_RX: u8 = 2;
    pub const LINK_TX: u8 = 3;
    pub const LINK_ERROR: u8 = 4;
    pub const USB: u8 = 5;
    pub const VBUS: u8 = 6;
    pub const WATCH: u8 = 7;
    pub const WATCH_START: u8 = 8;
    pub const CLOCK: u8 = 9;
    pub const MARK: u8 = 10;
    pub const KNOB: u8 = 11;
    pub const SLEEP_SNAP: u8 = 12;
}

pub mod usb_event {
    pub const RESET: u8 = 1;
    pub const SUSPEND: u8 = 2;
    pub const RESUME: u8 = 3;
    pub const CONFIGURED: u8 = 4;
    pub const DECONFIGURED: u8 = 5;
}

#[unsafe(no_mangle)]
static mut KUMI_LAB_BOOT: [u32; BOOT_WORDS] = [0; BOOT_WORDS];
#[unsafe(no_mangle)]
static mut KUMI_LAB_JOURNAL: [u32; JOURNAL_WORDS] = [0; JOURNAL_WORDS];
#[unsafe(no_mangle)]
static mut KUMI_LAB_CONTROL: [u32; CONTROL_WORDS] = [0; CONTROL_WORDS];

static mut CYCLES_HIGH: u32 = 0;
static mut CYCLES_LAST: u32 = 0;
static mut CLOCK_START: Option<(u64, u32)> = None;
static mut VBUS_LAST: u32 = u32::MAX;
static mut KNOB_LAST: u32 = 0;
static mut WATCH_ADDRESS: [u32; WATCH_SLOTS] = [0; WATCH_SLOTS];
static mut WATCH_LAST: [u32; WATCH_SLOTS] = [0; WATCH_SLOTS];
static mut WATCH_SECOND: u32 = 0;
static mut WATCH_SPENT: u32 = 0;
static mut TICK_LAST_MS: u32 = u32::MAX;
static mut TICK_LAST_STAMP: u32 = 0;
static mut HOLD_DEADLINE: u32 = 0;
static mut HOLD_ACTIVE: bool = false;

fn boot_words() -> *mut u32 {
    (&raw mut KUMI_LAB_BOOT).cast()
}

fn journal_words() -> *mut u32 {
    (&raw mut KUMI_LAB_JOURNAL).cast()
}

fn control_words() -> *mut u32 {
    (&raw mut KUMI_LAB_CONTROL).cast()
}

fn get(base: *mut u32, index: usize) -> u32 {
    unsafe { read_volatile(base.add(index)) }
}

fn set(base: *mut u32, index: usize, value: u32) {
    unsafe { write_volatile(base.add(index), value) }
}

fn read8(address: usize) -> u8 {
    unsafe { read_volatile(address as *const u8) }
}

fn cycles64() -> u64 {
    let now = board::cycles();
    unsafe {
        let mut high = (&raw const CYCLES_HIGH).read();
        if now < (&raw const CYCLES_LAST).read() {
            high = high.wrapping_add(1);
            (&raw mut CYCLES_HIGH).write(high);
        }
        (&raw mut CYCLES_LAST).write(now);
        (u64::from(high) << 32) | u64::from(now)
    }
}

fn stamp() -> u32 {
    (cycles64() >> 10) as u32
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn record(kind: u8, a: u8, b: u8, length: u8, first: u32, second: u32) {
    let journal = journal_words();
    let written = get(journal, JOURNAL_WRITTEN);
    let at = JOURNAL_HEADER + 4 * (written as usize % JOURNAL_RECORDS);
    set(journal, at, stamp());
    set(journal, at + 1, u32::from_le_bytes([kind, a, b, length]));
    set(journal, at + 2, first);
    set(journal, at + 3, second);
    set(journal, JOURNAL_WRITTEN, written.wrapping_add(1));
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn dropped() {
    let journal = journal_words();
    set(journal, JOURNAL_DROPPED, get(journal, JOURNAL_DROPPED).wrapping_add(1));
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn capture_boot() {
    let entry = board::cycles();
    let boot = boot_words();
    let mut at = BOOT_HEADER;
    for &(address, words) in &BOOT_REGIONS {
        for word in 0..words {
            let register = address + 4 * word;
            set(boot, at, register);
            set(boot, at + 1, board::read(register as usize));
            at += 2;
        }
    }
    set(boot, BOOT_ENTRY_CYCLES, entry);
    set(boot, BOOT_ENTRY_COUNT, ((at - BOOT_HEADER) / 2) as u32);
    set(boot, 0, BOOT_MAGIC);
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn start(loaded_by_host: bool, boot_count: u32) {
    let control = control_words();
    set(control, CONTROL_FAKE_STREAM_HZ, 0);
    set(control, CONTROL_WATCH_BUDGET, DEFAULT_WATCH_BUDGET);
    for (slot, &(address, mask)) in DEFAULT_WATCH.iter().enumerate() {
        set(control, CONTROL_HEADER + 2 * slot, address);
        set(control, CONTROL_HEADER + 2 * slot + 1, mask);
    }
    set(control, 0, CONTROL_MAGIC);
    let boot = boot_words();
    set(boot, BOOT_COUNT, boot_count);
    set(boot, BOOT_LOADED_BY_HOST, u32::from(loaded_by_host));
    set(journal_words(), 0, JOURNAL_MAGIC);
    record(kind::BOOT, u8::from(loaded_by_host), 0, 0, boot_count, get(boot, BOOT_ENTRY_CYCLES));
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn mark(stage: u16) {
    let now = board::cycles();
    let boot = boot_words();
    if stage == kumi_diag::stage::FW_USB {
        set(boot, BOOT_USB_CYCLES, now);
    } else if stage == kumi_diag::stage::FW_RUNNING || stage == kumi_diag::stage::FW_SAFE_MODE {
        set(boot, BOOT_RUNNING_CYCLES, now);
    }
    let [low, high] = stage.to_le_bytes();
    record(kind::MARK, low, high, 0, now, 0);
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn usb(event: u8) {
    let boot = boot_words();
    if event == usb_event::CONFIGURED && get(boot, BOOT_CONFIGURED_CYCLES) == 0 {
        set(boot, BOOT_CONFIGURED_CYCLES, board::cycles());
    }
    record(kind::USB, event, read8(USB_POWER), read8(USB_DEVCTL), 0, 0);
    if event == usb_event::SUSPEND {
        snapshot();
    }
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn snapshot() {
    let control = control_words();
    for slot in 0..WATCH_SLOTS {
        let address = get(control, CONTROL_HEADER + 2 * slot);
        if watchable(address) {
            record(kind::SLEEP_SNAP, slot as u8, 0, 0, address, board::read(address as usize));
        }
    }
}

fn body_words(body: &[u8]) -> (u32, u32) {
    let mut bytes = [0u8; 8];
    let kept = body.len().min(8);
    bytes[..kept].copy_from_slice(&body[..kept]);
    (u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]), u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]))
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn link(from_mcu: bool, group: u8, command: u8, body: &[u8]) {
    let (first, second) = body_words(body);
    let kind = if from_mcu { kind::LINK_RX } else { kind::LINK_TX };
    record(kind, group, command, body.len().min(usize::from(u8::MAX)) as u8, first, second);
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn link_error(code: u8, value: u32) {
    record(kind::LINK_ERROR, code, 0, 0, value, 0);
}

pub fn link_state(link: (u32, Option<StreamKind>, u8)) -> (u32, Option<StreamKind>, u8) {
    match get(control_words(), CONTROL_FAKE_STREAM_HZ) {
        0 => link,
        rate => (rate, Some(StreamKind::Pcm), FAKE_STREAM_BITS),
    }
}

pub fn control_pokeable(address: u32) -> bool {
    let first = control_words() as u32 + 4;
    let end = control_words() as u32 + 4 * CONTROL_WORDS as u32;
    address.is_multiple_of(4) && address >= first && address < end
}

fn watchable(address: u32) -> bool {
    address != 0 && address.is_multiple_of(4) && address < WATCH_LIMIT && board::peekable(address, 1)
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn clock(now: u64, milliseconds: u32) {
    let start = unsafe { (&raw const CLOCK_START).read() };
    let Some((cycles, at)) = start else {
        unsafe { (&raw mut CLOCK_START).write(Some((now, milliseconds))) };
        return;
    };
    let elapsed = milliseconds.wrapping_sub(at);
    if elapsed < CLOCK_WINDOW_MS {
        return;
    }
    unsafe { (&raw mut CLOCK_START).write(Some((now, milliseconds))) };
    let khz = ((now - cycles) / u64::from(elapsed)).min(u64::from(u32::MAX)) as u32;
    if !(CLOCK_MIN_KHZ..=CLOCK_MAX_KHZ).contains(&khz) {
        return;
    }
    let journal = journal_words();
    let previous = get(journal, JOURNAL_CPU_KHZ);
    if previous != 0 && khz.abs_diff(previous) > previous / 4 {
        return;
    }
    if previous == 0 || khz.abs_diff(previous) > previous / 200 {
        set(journal, JOURNAL_CPU_KHZ, khz);
        record(kind::CLOCK, 0, 0, 0, khz, elapsed);
    }
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn vbus() {
    let devctl = u32::from(read8(USB_DEVCTL));
    if devctl != unsafe { (&raw const VBUS_LAST).read() } {
        unsafe { (&raw mut VBUS_LAST).write(devctl) };
        record(kind::VBUS, devctl as u8, read8(USB_POWER), 0, 0, 0);
    }
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn knob() {
    let fake = get(control_words(), CONTROL_FAKE_STREAM_HZ);
    if fake != unsafe { (&raw const KNOB_LAST).read() } {
        unsafe { (&raw mut KNOB_LAST).write(fake) };
        record(kind::KNOB, 0, 0, 0, fake, 0);
    }
}

fn budget() -> bool {
    unsafe {
        let spent = (&raw const WATCH_SPENT).read();
        if spent >= get(control_words(), CONTROL_WATCH_BUDGET) {
            return false;
        }
        (&raw mut WATCH_SPENT).write(spent + 1);
    }
    true
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
fn watch() {
    let control = control_words();
    let armed = (&raw mut WATCH_ADDRESS).cast::<u32>();
    let last = (&raw mut WATCH_LAST).cast::<u32>();
    for slot in 0..WATCH_SLOTS {
        let address = get(control, CONTROL_HEADER + 2 * slot);
        let mask = get(control, CONTROL_HEADER + 2 * slot + 1);
        if !watchable(address) {
            if get(armed, slot) != 0 {
                set(armed, slot, 0);
                record(kind::WATCH_START, slot as u8, 1, 0, address, 0);
            }
            continue;
        }
        let value = board::read(address as usize);
        if get(armed, slot) != address {
            set(armed, slot, address);
            set(last, slot, value);
            record(kind::WATCH_START, slot as u8, 0, 0, address, value);
            continue;
        }
        let previous = get(last, slot);
        if (value ^ previous) & mask == 0 {
            continue;
        }
        set(last, slot, value);
        if budget() {
            record(kind::WATCH, slot as u8, 0, 0, previous, value);
        } else {
            dropped();
        }
    }
}

#[inline(never)]
pub fn hold_awake() -> bool {
    let control = control_words();
    let seconds = get(control, CONTROL_HOLD_AWAKE_SECONDS);
    if seconds == 0 {
        unsafe { (&raw mut HOLD_ACTIVE).write(false) };
        return false;
    }
    let now = stamp();
    let active = unsafe { (&raw const HOLD_ACTIVE).read() };
    if !active {
        let clamped = seconds.min(MAX_HOLD_AWAKE_SECONDS);
        let deadline = now.wrapping_add(clamped.saturating_mul(STAMPS_PER_SECOND));
        unsafe {
            (&raw mut HOLD_DEADLINE).write(deadline);
            (&raw mut HOLD_ACTIVE).write(true);
        }
        record(kind::KNOB, 1, 0, 0, clamped, 0);
        return true;
    }
    let deadline = unsafe { (&raw const HOLD_DEADLINE).read() };
    if deadline.wrapping_sub(now) < 0x8000_0000 {
        return true;
    }
    set(control, CONTROL_HOLD_AWAKE_SECONDS, 0);
    unsafe { (&raw mut HOLD_ACTIVE).write(false) };
    record(kind::KNOB, 2, 0, 0, 0, 0);
    false
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn tick(milliseconds: u32) {
    let now_cycles = cycles64();
    let now_stamp = (now_cycles >> 10) as u32;
    let last_ms = unsafe { (&raw const TICK_LAST_MS).read() };
    let last_stamp = unsafe { (&raw const TICK_LAST_STAMP).read() };
    if milliseconds == last_ms && now_stamp.wrapping_sub(last_stamp) < OFFLINE_TICK_STAMPS {
        return;
    }
    unsafe {
        (&raw mut TICK_LAST_MS).write(milliseconds);
        (&raw mut TICK_LAST_STAMP).write(now_stamp);
    }
    let second = now_stamp >> 16;
    if second != unsafe { (&raw const WATCH_SECOND).read() } {
        unsafe {
            (&raw mut WATCH_SECOND).write(second);
            (&raw mut WATCH_SPENT).write(0);
        }
    }
    if milliseconds != last_ms {
        clock(now_cycles, milliseconds);
    }
    vbus();
    knob();
    watch();
}

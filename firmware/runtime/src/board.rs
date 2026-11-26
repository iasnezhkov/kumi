// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use core::arch::asm;
use core::ptr::{read_volatile, write_volatile};
use kumi_boot_core::stage0::{BOOT_INFO_LEN, BootInfo};
use kumi_chip::power::CpuProfile;
use kumi_chip::{BootRom, RomFlash, regs, rom, scratch};
use kumi_diag::AREA_SIZE;
use kumi_icd::{FaultKind, RebootTarget};
use kumi_layout::{
    BOOT_INFO_ADDRESS, DIAG_RECORD_ADDRESS, LAST_CRASH_ADDRESS, STAGE0_VERIFY_ENTRY, STAGE0_VERIFY_MAGIC,
    STAGE0_VERIFY_MAGIC_ADDRESS, SlotId,
};
use kumi_runtime_core::Storage;

pub type Flash = RomFlash<BootRom>;

pub fn read(address: usize) -> u32 {
    unsafe { regs::read(address) }
}

fn write(address: usize, value: u32) {
    unsafe { regs::write(address, value) }
}

fn update(address: usize, clear: u32, set: u32) {
    unsafe { regs::update(address, clear, set) }
}

pub fn usb_init() -> bool {
    update(0x2000_3000, 0, 1 << 27);
    update(0x2000_2000, 0, 1 << 27);
    let source_ready = unsafe { rom::usb_clock_source(rom::USB_SOURCE_HZ) } == 0;
    if unsafe { rom::usb_pll(u32::from(source_ready)) } != 0 {
        return false;
    }
    update(0x2000_3000, 1 << 20, 0);
    update(0x2000_3000, 0, 1 << 20);
    update(0x2000_2000, 0, 1 << 20);
    write(0x2000_2090, 5);
    let phy = if source_ready { 0b1110 } else { 0b0110 };
    update(0x2000_6000, 0b1111 & !phy, phy);
    true
}

pub fn uart_init() {
    update(0x2000_2000, 0, 1 << 4);
    update(0x2000_3000, 1 << 9, 0);
    update(0x2000_2000, 0, 1 << 9);
    update(0x2000_3000, 0, 1 << 9);
    write(0x2000_2030, 1);
    for pin in [14, 15] {
        update(0x2011_0000 + 4 * pin, 0xf, 3);
    }
    update(0x2011_003c, 0, 0xc0);
    write(0x2004_0000, 0x1103);
    write(0x2004_0004, 0x00cf_00cf);
    write(0x2004_000c, 3);
    write(0x2004_0010, 2);
}

#[unsafe(link_section = ".text.hot")]
pub fn uart_rx() -> Option<u8> {
    if read(0x2004_0014) & 2 == 0 {
        return None;
    }
    let byte = read(0x2004_0024) as u8;
    write(0x2004_0014, 2);
    Some(byte)
}

#[unsafe(link_section = ".text.hot")]
pub fn uart_rx_ready() -> bool {
    read(0x2004_0014) & 2 != 0
}

pub const WAKE_TIMER_MS: u32 = 20;
const WAKE_SOURCES: [u32; 3] = [kumi_chip::plic::IRQ_TIMER3, kumi_chip::plic::IRQ_UART0, kumi_chip::plic::IRQ_USB_MAC];
const WAKE_ABORT_MASK: u64 = (1u64 << kumi_chip::plic::IRQ_UART0) | (1u64 << kumi_chip::plic::IRQ_USB_MAC);

#[repr(C)]
pub struct IdleStats {
    pub magic: u32,
    pub wfi_count: u32,
    pub timer_ticks: u32,
    pub wfi_cycles: u32,
}

#[unsafe(no_mangle)]
pub static mut KUMI_IDLE_STATS: IdleStats =
    IdleStats { magic: u32::from_le_bytes(*b"KWFI"), wfi_count: 0, timer_ticks: 0, wfi_cycles: 0 };

static mut WFI_US: u64 = 0;

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn enable_idle_wake() {
    unsafe {
        kumi_chip::timer::start_periodic(kumi_chip::timer::TimerId::T3, WAKE_TIMER_MS, true);
        kumi_chip::plic::enable_sources(&WAKE_SOURCES);
    }
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
pub fn enter_idle_wfi() -> (bool, u32) {
    unsafe {
        if !kumi_chip::timer::is_running(kumi_chip::timer::TimerId::T3) {
            kumi_chip::timer::start_periodic(kumi_chip::timer::TimerId::T3, WAKE_TIMER_MS, true);
            kumi_chip::plic::enable_sources(&WAKE_SOURCES);
        }
        if kumi_chip::timer::pending(kumi_chip::timer::TimerId::T3) {
            kumi_chip::timer::clear_pending(kumi_chip::timer::TimerId::T3);
        }
        if kumi_chip::plic::drain_stale_abort_on(WAKE_ABORT_MASK) {
            return (false, 0);
        }
        let before = cycles();
        kumi_chip::plic::wait_for_interrupt();
        let elapsed = cycles().wrapping_sub(before);
        let timer_fired = kumi_chip::timer::pending(kumi_chip::timer::TimerId::T3);
        if timer_fired {
            kumi_chip::timer::clear_pending(kumi_chip::timer::TimerId::T3);
        }
        let _ = kumi_chip::plic::drain_stale();
        let stats = &raw mut KUMI_IDLE_STATS;
        (*stats).wfi_count = (*stats).wfi_count.wrapping_add(1);
        if timer_fired {
            (*stats).timer_ticks = (*stats).timer_ticks.wrapping_add(1);
        }
        (*stats).wfi_cycles = (*stats).wfi_cycles.wrapping_add(elapsed);
        let total = &raw mut WFI_US;
        *total = (*total).saturating_add(u64::from(elapsed) / u64::from(cpu_mhz().max(1)));
        (timer_fired, elapsed)
    }
}

#[unsafe(link_section = ".text.hot")]
pub fn uart_tx(byte: u8) -> bool {
    if read(0x2004_0018) & 0x40 != 0 {
        return false;
    }
    write(0x2004_0020, u32::from(byte));
    true
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn set_awake(awake: bool) {
    update(0x2000_2000, 0, 1 << 4);
    let level = if awake { kumi_audio::board::PIN_HIGH } else { kumi_audio::board::PIN_LOW };
    write(kumi_audio::board::OUTPUT_ENABLE as usize, level | kumi_audio::board::OUTPUT_PIN_PULL);
}

pub fn keep_awake() {
    set_awake(true);
}

pub fn is_awake() -> bool {
    read(kumi_audio::board::OUTPUT_ENABLE as usize) & kumi_audio::board::PIN_HIGH == kumi_audio::board::PIN_HIGH
}

pub fn serial() -> Option<[u8; 8]> {
    unsafe { kumi_chip::efuse::read_serial() }
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn arm_watchdog() {
    unsafe { kumi_chip::watchdog::arm_safe_reset() };
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
pub fn kick_watchdog() {
    unsafe { kumi_chip::watchdog::kick() };
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn watchdog_armed() -> bool {
    unsafe { kumi_chip::watchdog::armed() }
}

pub fn open_flash() -> (Flash, [u8; 3]) {
    let mut rom = unsafe { BootRom::begin() };
    let jedec = rom.jedec().unwrap_or([0; 3]);
    (RomFlash::new(rom, None), jedec)
}

static mut BOOT_INFO: Option<BootInfo> = None;
static mut LOADED_BY_HOST: bool = false;

#[unsafe(link_section = ".text.cold")]
pub fn adopt_boot_info(boot_arg: u32) -> Option<BootInfo> {
    unsafe { (&raw mut LOADED_BY_HOST).write(boot_arg != BOOT_INFO_ADDRESS) };
    let info = if boot_arg == BOOT_INFO_ADDRESS {
        let mut raw = [0u8; BOOT_INFO_LEN];
        for (i, word) in raw.chunks_exact_mut(4).enumerate() {
            word.copy_from_slice(&read(BOOT_INFO_ADDRESS as usize + 4 * i).to_le_bytes());
        }
        BootInfo::decode(&raw)
    } else {
        None
    };
    unsafe { (&raw mut BOOT_INFO).write(info) };
    info
}

pub fn boot_info() -> Option<BootInfo> {
    unsafe { (&raw const BOOT_INFO).read() }
}

pub fn loaded_by_host() -> bool {
    let loaded = unsafe { (&raw const LOADED_BY_HOST).read() };
    loaded && boot_info().is_none()
}

pub const LAB: bool = cfg!(feature = "lab");

pub fn raw_access() -> bool {
    LAB || loaded_by_host()
}

#[unsafe(link_section = ".text.hot")]
pub fn cycles() -> u32 {
    let value: u32;
    unsafe { asm!("csrr {0}, mcycle", out(reg) value, options(nomem, nostack)) };
    value
}

struct MonotonicClock {
    high: u32,
    last: u32,
    micros: u64,
    rem: u64,
    initialized: bool,
}

static mut CLOCK: MonotonicClock = MonotonicClock { high: 0, last: 0, micros: 0, rem: 0, initialized: false };

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
fn update_clock() -> (u64, u64) {
    let now = cycles();
    let mhz = u64::from(cpu_mhz().max(1));
    unsafe {
        let clock = &raw mut CLOCK;
        if !(*clock).initialized {
            (*clock).initialized = true;
            (*clock).last = now;
            return (u64::from(now), (*clock).micros);
        }
        let delta = u64::from(now.wrapping_sub((*clock).last));
        if now < (*clock).last {
            (*clock).high = (*clock).high.wrapping_add(1);
        }
        (*clock).last = now;
        let total = delta.saturating_add((*clock).rem);
        (*clock).micros = (*clock).micros.saturating_add(total / mhz);
        (*clock).rem = total % mhz;
        ((u64::from((*clock).high) << 32) | u64::from(now), (*clock).micros)
    }
}

#[unsafe(link_section = ".text.hot")]
pub fn monotonic_us() -> u64 {
    update_clock().1
}

#[unsafe(link_section = ".text.hot")]
pub fn monotonic_cycles() -> u64 {
    update_clock().0
}

#[unsafe(link_section = ".text.hot")]
pub fn advance_monotonic_us(us: u64) {
    unsafe {
        let clock = &raw mut CLOCK;
        (*clock).micros = (*clock).micros.saturating_add(us);
    }
}

static mut CPU_PROFILE: CpuProfile = CpuProfile::Idle90;
static mut DCDC_INITIALIZED: bool = false;

#[unsafe(link_section = ".text.hot")]
pub fn cpu_profile() -> CpuProfile {
    unsafe { read_volatile(&raw const CPU_PROFILE) }
}

#[unsafe(link_section = ".text.hot")]
pub fn cpu_hz() -> u32 {
    if unsafe { read_volatile(&raw const DCDC_INITIALIZED) } { cpu_profile().hz() } else { 60_000_000 }
}

#[unsafe(link_section = ".text.hot")]
pub fn cpu_mhz() -> u32 {
    cpu_hz() / 1_000_000
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
pub fn delay_us_at_mhz(microseconds: u32, mhz: u32) {
    let start = cycles();
    let wait = microseconds.saturating_mul(mhz);
    while cycles().wrapping_sub(start) < wait {}
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
pub fn set_cpu_profile(target: CpuProfile) {
    let dcdc_ready = unsafe { read_volatile(&raw const DCDC_INITIALIZED) };
    if dcdc_ready && cpu_profile() == target {
        return;
    }
    let _ = update_clock();
    kick_watchdog();
    unsafe {
        asm!("fence iorw, iorw", options(nostack));
        kumi_chip::power::hardware_switch(target, delay_us_at_mhz);
        asm!("fence iorw, iorw", options(nostack));
        write_volatile(&raw mut CPU_PROFILE, target);
        write_volatile(&raw mut DCDC_INITIALIZED, true);
        let clock = &raw mut CLOCK;
        if (*clock).initialized {
            let now = cycles();
            if now < (*clock).last {
                (*clock).high = (*clock).high.wrapping_add(1);
            }
            (*clock).last = now;
            (*clock).rem = 0;
        }
    }
    kick_watchdog();
}

const PEEK_REGIONS: [(u32, u32); 5] = [
    (0x0000_0000, 0x0020_0000),
    (0x1000_0000, 0x1100_0000),
    (0x2000_0000, 0x2200_0000),
    (0x4000_0000, 0x4004_0000),
    (0x4800_0000, 0x4840_0000),
];
const POKE_REGIONS: [(u32, u32); 1] = [(0x2000_0000, 0x2200_0000)];
const POKE_HOLES: [(u32, u32); 4] =
    [(0x2004_0000, 0x2004_1000), (0x2009_0000, 0x2009_1000), (0x200A_0000, 0x200A_1000), (0x200D_0000, 0x200D_1000)];
const PEEK_HOLES: [(u32, u32); 2] = [(0x2004_0000, 0x2004_1000), (0x200D_0000, 0x200D_1000)];
const SLOT_POKE_HOLES: [(u32, u32); 12] = [
    (0x2000_0000, 0x2000_1000),
    (0x2000_2010, 0x2000_2014),
    (0x2000_2020, 0x2000_2024),
    (0x2000_2090, 0x2000_2094),
    (0x2000_3020, 0x2000_3024),
    (0x2000_3024, 0x2000_3028),
    (0x2000_6000, 0x2000_7000),
    (0x2002_0000, 0x2002_2000),
    (0x2100_1000, 0x2100_1004),
    (0x2100_4000, 0x2100_4008),
    (0x2100_4010, 0x2100_4014),
    (0x2100_801C, 0x2100_8020),
];
const SLOT_KEPT_BITS: u32 = regs::SPI_ENABLE | (1 << 20) | (1 << 27);
const SLOT_POKE_BITS: [(u32, u32); 2] = [(0x2000_2000, SLOT_KEPT_BITS), (0x2000_3000, SLOT_KEPT_BITS)];
const SLOT_I2C_WRITABLE: [(u8, u8); 2] = [(0, kumi_audio::dac::PRIMARY_ADDRESS), (0, kumi_audio::dac::SECONDARY_ADDRESS)];

pub struct Mmio;

impl kumi_audio::Registers for Mmio {
    fn read(&mut self, address: u32) -> u32 {
        let value = read(address as usize);
        unsafe { asm!("fence iorw, iorw", options(nostack)) };
        value
    }

    fn write(&mut self, address: u32, value: u32) {
        write(address as usize, value);
        unsafe { asm!("fence iorw, iorw", options(nostack)) };
    }

    fn write_pair(&mut self, first: (u32, u32), second: (u32, u32)) {
        unsafe {
            write_volatile(first.0 as usize as *mut u32, first.1);
            write_volatile(second.0 as usize as *mut u32, second.1);
            asm!("fence iorw, iorw", options(nostack));
        }
    }

    fn read_pair(&mut self, first: u32, second: u32) -> (u32, u32) {
        let pair = unsafe { (read_volatile(first as usize as *const u32), read_volatile(second as usize as *const u32)) };
        unsafe { asm!("fence iorw, iorw", options(nostack)) };
        pair
    }

    fn delay_us(&mut self, microseconds: u32) {
        delay_us_at_mhz(microseconds, cpu_mhz());
    }
}

fn inside(address: u32, length: u32, regions: &[(u32, u32)]) -> bool {
    let Some(end) = address.checked_add(length) else { return false };
    regions.iter().any(|&(start, stop)| address >= start && end <= stop)
}

fn overlaps(address: u32, length: u32, holes: &[(u32, u32)]) -> bool {
    let end = address.saturating_add(length);
    holes.iter().any(|&(start, stop)| address < stop && end > start)
}

pub fn peekable(address: u32, words: u8) -> bool {
    let length = 4 * u32::from(words);
    inside(address, length, &PEEK_REGIONS) && !overlaps(address, length, &PEEK_HOLES)
}

pub fn pokeable(address: u32, mask: u32) -> bool {
    inside(address, 4, &POKE_REGIONS)
        && !overlaps(address, 4, &POKE_HOLES)
        && (!LAB || loaded_by_host() || slot_pokeable(address, mask))
}

fn slot_pokeable(address: u32, mask: u32) -> bool {
    !overlaps(address, 4, &SLOT_POKE_HOLES)
        && SLOT_POKE_BITS.iter().all(|&(register, kept)| register != address || mask & kept == 0)
}

pub fn i2c_permitted(bus: u8, address: u8, write: &[u8], read: u8) -> bool {
    !LAB || loaded_by_host() || matches!((write.len(), read), (0, 0) | (1, 1)) || SLOT_I2C_WRITABLE.contains(&(bus, address))
}

pub fn peek(address: u32, words: u8) -> heapless::Vec<u32, { kumi_icd::WORD_BLOCK }> {
    (0..u32::from(words)).map(|i| kumi_audio::Registers::read(&mut Mmio, address + 4 * i)).collect()
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn peek_flash(
    flash: &mut Flash,
    offset: u32,
    words: u8,
) -> Result<heapless::Vec<u32, { kumi_icd::WORD_BLOCK }>, kumi_chip::Error> {
    let mut raw = [0u8; 4 * kumi_icd::WORD_BLOCK];
    let len = 4 * usize::from(words);
    kumi_chip::Rom::read(flash.rom(), offset, &mut raw[..len])?;
    Ok(raw[..len].chunks_exact(4).map(|word| u32::from_le_bytes([word[0], word[1], word[2], word[3]])).collect())
}

pub fn poke(address: u32, value: u32, mask: u32) -> u32 {
    let mut mmio = Mmio;
    if mask == u32::MAX {
        kumi_audio::Registers::write(&mut mmio, address, value);
    } else {
        kumi_audio::Registers::update(&mut mmio, address, mask, value & mask);
    }
    kumi_audio::Registers::read(&mut mmio, address)
}

pub fn i2c(bus: u8, address: u8, write: &[u8], buffer: &mut [u8]) -> Result<(), kumi_audio::i2c::I2cError> {
    use kumi_audio::i2c::{Controller, I2C0_BASE, I2C1_BASE, I2cBus, I2cError, MAX_PREFIX};
    let base = match bus {
        0 => I2C0_BASE,
        1 => I2C1_BASE,
        _ => return Err(I2cError::Invalid),
    };
    let mut controller = Controller::new(Mmio, base);
    match (write.is_empty(), buffer.is_empty()) {
        (true, true) => controller.probe(address),
        (false, true) => controller.write(address, write),
        _ if write.len() <= MAX_PREFIX => controller.write_then_read(address, write, buffer),
        _ => Err(I2cError::Invalid),
    }
}

#[unsafe(link_section = ".text.cold")]
pub fn flash_test(flash: &mut Flash) -> Result<scratch::Report, kumi_chip::Error> {
    kick_watchdog();
    let result = scratch::run(flash.rom(), cycles);
    kick_watchdog();
    result
}

#[unsafe(link_section = ".text.cold")]
pub fn fault(kind: FaultKind) -> ! {
    match kind {
        FaultKind::IllegalInstruction => unsafe {
            asm!(".option push", ".option norvc", "unimp", ".option pop", options(noreturn, nomem, nostack))
        },
        FaultKind::Breakpoint => unsafe {
            asm!(".option push", ".option norvc", "ebreak", ".option pop", options(noreturn, nomem, nostack))
        },
        FaultKind::Panic => panic!("developer fault"),
    }
}

fn stage0_verify_available() -> bool {
    read(STAGE0_VERIFY_MAGIC_ADDRESS as usize) == u32::from_le_bytes(STAGE0_VERIFY_MAGIC)
}

#[unsafe(link_section = ".text.cold")]
pub fn storage(flash: Flash) -> Storage<Flash> {
    Storage::open(flash, boot_info().filter(|_| stage0_verify_available()))
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
pub fn verify(flash: &mut Flash, slot: SlotId) -> bool {
    if !stage0_verify_available() || !flash.rom().available() {
        return false;
    }
    let previous = cpu_profile();
    set_cpu_profile(CpuProfile::Turbo360);
    kick_watchdog();
    unsafe { asm!("fence.i", options(nostack)) };
    let verify = unsafe { core::mem::transmute::<usize, extern "C" fn(u32) -> u32>(STAGE0_VERIFY_ENTRY as usize) };
    let ok = verify(slot.index() as u32) == 0;
    kick_watchdog();
    set_cpu_profile(previous);
    ok
}

static mut UPTIME_MS: u32 = 0;

pub fn set_uptime(milliseconds: u32) {
    unsafe { write_volatile(&raw mut UPTIME_MS, milliseconds) };
}

fn uptime() -> u32 {
    unsafe { read_volatile(&raw const UPTIME_MS) }
}

#[inline(never)]
fn read_area(address: u32, area: &mut [u8; AREA_SIZE]) {
    for (i, word) in area.chunks_exact_mut(4).enumerate() {
        word.copy_from_slice(&read(address as usize + 4 * i).to_le_bytes());
    }
}

#[inline(never)]
fn store_diag(bytes: &[u8; AREA_SIZE]) {
    for (i, word) in bytes.chunks_exact(4).enumerate() {
        write(DIAG_RECORD_ADDRESS as usize + 4 * i, u32::from_le_bytes([word[0], word[1], word[2], word[3]]));
    }
}

pub fn last_crash_area() -> [u8; AREA_SIZE] {
    let mut area = [0u8; AREA_SIZE];
    read_area(LAST_CRASH_ADDRESS, &mut area);
    area
}

#[unsafe(link_section = ".text.cold")]
pub fn mark(stage: u16) {
    let mut bytes = [0u8; AREA_SIZE];
    read_area(DIAG_RECORD_ADDRESS, &mut bytes);
    if kumi_diag::read(&bytes).is_none() {
        kumi_diag::Diag::begin(&mut bytes, boot_info().map_or(0, |info| info.boot_count), 0);
    }
    if let Some(mut record) = kumi_diag::Diag::attach(&mut bytes) {
        record.mark(stage);
    }
    store_diag(&bytes);
}

#[unsafe(link_section = ".text.cold")]
pub fn crash(reason: kumi_diag::Reason, registers: kumi_diag::Registers, message: &str) {
    let mut bytes = [0u8; AREA_SIZE];
    read_area(DIAG_RECORD_ADDRESS, &mut bytes);
    if kumi_diag::read(&bytes).is_none() {
        kumi_diag::Diag::begin(&mut bytes, boot_info().map_or(0, |info| info.boot_count), 0);
    }
    if let Some(mut record) = kumi_diag::Diag::attach(&mut bytes) {
        record.crash(reason, registers, uptime(), message);
    }
    store_diag(&bytes);
}

fn crash_report(area: &[u8; AREA_SIZE]) -> Option<kumi_icd::CrashReport> {
    let report = kumi_diag::read(area)?;
    let reason = match report.reason {
        kumi_diag::Reason::None => return None,
        kumi_diag::Reason::Panic => kumi_icd::CrashReason::Panic,
        kumi_diag::Reason::Trap => kumi_icd::CrashReason::Trap,
    };
    Some(kumi_icd::CrashReport {
        boot_count: report.boot_count,
        reason,
        stage: report.stage,
        mcause: report.registers.mcause,
        mepc: report.registers.mepc,
        mtval: report.registers.mtval,
        ra: report.registers.ra,
        sp: report.registers.sp,
        uptime_ms: report.uptime_ms,
        message: report.message().try_into().unwrap_or_default(),
    })
}

#[unsafe(link_section = ".text.cold")]
pub fn diagnostics(
    uptime_ms: u32,
    dropped_events: u32,
    logged: impl FnOnce() -> Option<[u8; AREA_SIZE]>,
) -> kumi_icd::Diagnostics {
    let info = boot_info();
    let mut current_bytes = [0u8; AREA_SIZE];
    read_area(DIAG_RECORD_ADDRESS, &mut current_bytes);
    let current = kumi_diag::read(&current_bytes);
    let last_crash = crash_report(&last_crash_area()).or_else(|| logged().and_then(|area| crash_report(&area)));
    kumi_icd::Diagnostics {
        boot_count: info.map_or(0, |i| i.boot_count),
        safe_mode: info.is_some_and(|i| i.safe_mode),
        stage: current.map_or(0, |r| r.stage),
        uptime_ms,
        dropped_events,
        watchdog_armed: watchdog_armed(),
        last_crash,
    }
}

static mut LAST_TELEMETRY_US: u64 = 0;
static mut LAST_WFI_US: u64 = 0;

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn telemetry(uptime_ms: u32) -> kumi_icd::SystemTelemetry {
    let mhz = cpu_mhz();
    let now_us = monotonic_us();
    let pmu_pwr = read(kumi_chip::power::AON_PMU_PWR_CTL);
    let pmu_vol = read(kumi_chip::power::AON_PMU_VOL_CTL);
    let dvdd_code = ((pmu_vol >> kumi_chip::power::DVDD_VOL_SHIFT) & 7) as u16;
    let audio_pll = read(kumi_audio::clock::AUDIO_PLL as usize);
    let analog_rails_on = (read(0x2000_20f8) | read(0x2011_0300) | read(0x2011_0310)) != 0;
    let jack_adc = (read(0x2101_0050) & 0x3ff) as u16;
    let (wfi_count, wfi_us) = unsafe {
        let stats = &raw const KUMI_IDLE_STATS;
        (read_volatile(&raw const (*stats).wfi_count), read_volatile(&raw const WFI_US))
    };
    let (prev_us, prev_wfi_us) = unsafe {
        let pu = read_volatile(&raw const LAST_TELEMETRY_US);
        let pw = read_volatile(&raw const LAST_WFI_US);
        write_volatile(&raw mut LAST_TELEMETRY_US, now_us);
        write_volatile(&raw mut LAST_WFI_US, wfi_us);
        (pu, pw)
    };
    let delta_us = now_us.saturating_sub(prev_us);
    let delta_wfi_us = wfi_us.saturating_sub(prev_wfi_us);
    let wfi_percent_tenths = delta_wfi_us
        .saturating_mul(1_000)
        .checked_div(delta_us)
        .or_else(|| wfi_us.saturating_mul(1_000).checked_div(now_us))
        .map_or(0, |v| v.min(1_000) as u16);
    kumi_icd::SystemTelemetry {
        cpu_mhz: mhz as u16,
        dcdc_active: pmu_pwr & kumi_chip::power::EXT_DCDC_ENABLE != 0,
        dvdd_mv: 800 + dvdd_code * 100,
        analog_rails_on,
        audio_pll_enabled: audio_pll & kumi_audio::clock::PLL_ENABLE != 0,
        wfi_percent_tenths,
        wfi_wakeups: wfi_count,
        jack_adc,
        uptime_ms,
    }
}

#[unsafe(link_section = ".text.hot")]
#[inline(never)]
pub fn reset(target: RebootTarget) -> ! {
    unsafe {
        asm!("csrci mstatus, 8", options(nomem, nostack));
        asm!("fence iorw, iorw", options(nostack));
        kumi_chip::power::hardware_prepare_reset(delay_us_at_mhz);
        asm!("fence iorw, iorw", options(nostack));
        asm!("csrci 0x7ca, 1", "fence.i", options(nostack));
    }
    kumi_chip::reset::reset(target == RebootTarget::BootRom)
}

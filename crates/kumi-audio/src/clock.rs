// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_chip::regs as hw;

use crate::regs::{MmioOp, Registers};

pub const SYSTEM_CLOCK: u32 = hw::CMU_SYSCLK as u32;
pub const CORE_PLL: u32 = hw::AON_PLL_CORE as u32;
pub const AUDIO_PLL: u32 = hw::AON_PLL_AUDIO as u32;
pub const AUDIO_PLL_FRACTION: u32 = hw::AON_PLL_AUDIO_FRAC as u32;
pub const AUDIO_PLL_TRIM: u32 = hw::AON_PLL_TRIM as u32;
pub const TRIM_MASK: u32 = 0x000f_0000;
pub const TRIM_SHIFT: u32 = 16;
pub const PERIPHERAL_GATE: u32 = hw::CMU_CLKEN0 as u32;
pub const PERIPHERAL_RESET: u32 = hw::RMU_RSTEN0 as u32;
pub const TIMER_BIT: u32 = 1 << 5;
pub const TIMER_CLOCK_SELECT: [u32; 2] = [hw::CMU_TIMER_CLK_BASE as u32, hw::CMU_TIMER_CLK_BASE as u32 + 4];
pub const TIMER_CLOCK_VALUES: [u32; 2] = [5, 11];
pub const TIMER_GLOBAL: u32 = hw::TIMER_BASE as u32 + 0x04;
pub const TIMER_CONTROL: [u32; 4] =
    [hw::TIMER_BASE as u32 + 0x20, hw::TIMER_BASE as u32 + 0x40, hw::TIMER_BASE as u32 + 0x60, hw::TIMER_BASE as u32 + 0x80];
pub const TIMER_REFERENCE_COUNT: u32 = hw::TIMER_BASE as u32 + 0x28;
pub const TIMER_MEASURED_COUNT: u32 = hw::TIMER_BASE as u32 + 0x48;
pub const PLL_ENABLE: u32 = 1;
pub const PLL_OFF_SETTLE_US: u32 = 60;
pub const TRIM_SETTLE_US: u32 = 1_000;
pub const LOCK_SETTLE_US: u32 = 2_000;
pub const GATE_US: u32 = 130;
pub const REFERENCE_HZ: u64 = 12_000_000;
pub const FRACTION_BITS: u32 = 20;
pub const ANALOG_SUPPLY: u32 = hw::AON_PMU_PWR_CTL as u32;
pub const ANALOG_SUPPLY_ENABLE: u32 = 1 << 12;
pub const ANALOG_POWER: [MmioOp; 4] = [
    MmioOp::Update { address: hw::AON_RMU_RESET as u32, clear: 0, set: 1 },
    MmioOp::Update { address: hw::AON_CMU_ENABLE as u32, clear: 0, set: 1 },
    MmioOp::Write { address: hw::AON_LRADC_CLK as u32, value: 0 },
    MmioOp::Update { address: ANALOG_SUPPLY, clear: 0, set: ANALOG_SUPPLY_ENABLE },
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Family {
    Rate44k1,
    Rate48k,
}

impl Family {
    pub const fn of_rate(hz: u32) -> Option<Self> {
        if hz != 0 && 45_158_400 % hz == 0 {
            Some(Self::Rate44k1)
        } else if hz != 0 && 49_152_000 % hz == 0 {
            Some(Self::Rate48k)
        } else {
            None
        }
    }

    pub const fn master_hz(self) -> u32 {
        match self {
            Self::Rate44k1 => 45_158_400,
            Self::Rate48k => 49_152_000,
        }
    }

    pub const fn pll(self) -> PllProfile {
        match self {
            Self::Rate44k1 => PllProfile::STOCK[3],
            Self::Rate48k => PllProfile::STOCK[1],
        }
    }

    const fn slot(self) -> usize {
        match self {
            Self::Rate48k => 0,
            Self::Rate44k1 => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct PllProfile {
    pub index: u8,
    pub integer: u32,
    pub fraction: u32,
    numerator: u64,
    shift: u32,
}

const fn stock(index: u8, integer: u32, fraction: u32, numerator: u64, shift: u32) -> PllProfile {
    PllProfile { index, integer, fraction, numerator, shift }
}

impl PllProfile {
    pub const STOCK: [Self; 4] = [
        stock(0, 6, 150_995, 6_442_451, 21),
        stock(1, 4, 100_663, 8_589_935, 22),
        stock(2, 5, 676_122, 2_959_501, 20),
        stock(3, 3, 800_273, 15_784_005, 23),
    ];

    pub const fn from_stock_index(index: u8) -> Option<Self> {
        if (index as usize) < Self::STOCK.len() { Some(Self::STOCK[index as usize]) } else { None }
    }

    pub const fn output_hz(self) -> u64 {
        REFERENCE_HZ * self.integer as u64 + ((REFERENCE_HZ * self.fraction as u64) >> FRACTION_BITS)
    }

    pub const fn bounds(self, reference: u32) -> Option<(u32, u32)> {
        if reference == 0 || reference > 65_535 {
            return None;
        }
        let scaled = reference as u64 * self.numerator;
        let low = (scaled * 8_353_661) >> (self.shift + 23);
        let high = (scaled * 8_423_555) >> (self.shift + 23);
        Some((low as u32, high as u32))
    }

    pub const fn accepts(self, reference: u32, measured: u32) -> Option<bool> {
        match self.bounds(reference) {
            Some((low, high)) => Some(measured >= low && measured <= high),
            None => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ClockError {
    NoTrimWindow,
    DisjointTrimWindows,
    InvalidReference { trim: u8, reference: u32 },
    TimerInUse,
    TimeWentBackwards,
    NotReady,
}

pub fn choose_trim(valid: u16) -> Result<u8, ClockError> {
    if valid == 0 {
        return Err(ClockError::NoTrimWindow);
    }
    let first = valid.trailing_zeros();
    let shifted = u32::from(valid) >> first;
    if shifted & (shifted + 1) != 0 {
        return Err(ClockError::DisjointTrimWindows);
    }
    let last = 15 - valid.leading_zeros();
    Ok((first + (last - first) / 2) as u8)
}

pub fn decode_cpu_hz(system_clock: u32, core_pll: u32) -> Option<u32> {
    let source = match system_clock & 7 {
        0 => 512_000u64,
        1 | 2 => 24_000_000,
        3 => 32_000_000,
        4 => {
            if core_pll & 1 == 0 {
                return None;
            }
            if core_pll & 8 == 0 {
                u64::from((core_pll >> 4) & 0x7f) * 6_000_000
            } else {
                let base = match core_pll & 6 {
                    0 | 2 => 24_000_000u64,
                    4 => 32_000_000,
                    _ => return None,
                };
                let n = u64::from((core_pll >> 16) & 0x1ff);
                let div = u64::from((core_pll >> 25) & 0x1f);
                let post = u64::from((core_pll >> 12) & 0xf);
                (base * n).checked_div(div)?.checked_div(post)?
            }
        }
        5 => 480_000_000,
        _ => return None,
    };
    let divisor = [1, 2, 4, 8, 12, 16, 24, 32][((system_clock >> 4) & 7) as usize];
    let hz = u32::try_from(source / divisor).ok()?;
    (hz != 0).then_some(hz)
}

pub fn read_cpu_hz<R: Registers>(registers: &mut R) -> Option<u32> {
    let system_clock = registers.read(SYSTEM_CLOCK);
    let core_pll = if system_clock & 7 == 4 { registers.read(CORE_PLL) } else { 0 };
    decode_cpu_hz(system_clock, core_pll)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleTime {
    hz: u32,
}

impl CycleTime {
    pub const fn new(hz: u32) -> Option<Self> {
        if hz == 0 { None } else { Some(Self { hz }) }
    }

    pub const fn hz(self) -> u32 {
        self.hz
    }

    pub const fn micros(self, cycles: u64) -> u64 {
        let hz = self.hz as u64;
        (cycles / hz) * 1_000_000 + (cycles % hz) * 1_000_000 / hz
    }

    pub const fn cycles(self, microseconds: u64) -> u64 {
        (self.hz as u64 * microseconds).div_ceil(1_000_000)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameClock {
    sample_cycles: u64,
    sample_frame: u16,
    cycles_per_ms: Option<u64>,
}

impl FrameClock {
    pub fn start(cycles: u64, frame: u16) -> Self {
        Self { sample_cycles: cycles, sample_frame: frame & 0x7ff, cycles_per_ms: None }
    }

    pub fn observe(&mut self, cycles: u64, frame: u16) {
        if self.cycles_per_ms.is_some() {
            return;
        }
        let frames = (frame & 0x7ff).wrapping_sub(self.sample_frame) & 0x7ff;
        if (32..=128).contains(&frames) {
            let rate = cycles.wrapping_sub(self.sample_cycles) / u64::from(frames);
            if (1_000..=1_000_000).contains(&rate) {
                self.cycles_per_ms = Some(rate);
            }
        } else if frames > 128 {
            self.sample_cycles = cycles;
            self.sample_frame = frame & 0x7ff;
        }
    }

    pub fn cpu_hz(&self) -> Option<u32> {
        self.cycles_per_ms.and_then(|rate| u32::try_from(rate * 1_000).ok())
    }
}

pub fn configure<R: Registers>(registers: &mut R, profile: PllProfile) {
    registers.update(AUDIO_PLL, 0x1f00, 0x200);
    registers.update(AUDIO_PLL, 0x1f_0000, profile.integer << 16);
    registers.write(AUDIO_PLL_FRACTION, profile.fraction);
    registers.update(AUDIO_PLL, 0xc, 4);
    registers.update(AUDIO_PLL, 0xf0, 0x40);
    registers.update(AUDIO_PLL, 0, 2);
}

pub fn set_trim<R: Registers>(registers: &mut R, trim: u8) {
    registers.update(AUDIO_PLL_TRIM, TRIM_MASK, u32::from(trim & 0xf) << TRIM_SHIFT);
}

pub fn quiesce_timers<R: Registers>(registers: &mut R) -> bool {
    registers.update(PERIPHERAL_RESET, TIMER_BIT, 0);
    registers.update(PERIPHERAL_GATE, 0, TIMER_BIT);
    registers.update(PERIPHERAL_RESET, 0, TIMER_BIT);
    registers.write(TIMER_GLOBAL, 0);
    for address in TIMER_CONTROL {
        registers.write(address, 0);
    }
    let stopped = TIMER_CONTROL.iter().all(|address| registers.read(*address) & 1 == 0);
    registers.update(PERIPHERAL_GATE, TIMER_BIT, 0);
    registers.update(PERIPHERAL_RESET, TIMER_BIT, 0);
    stopped
}

fn timers_enable<R: Registers>(registers: &mut R) {
    registers.update(PERIPHERAL_GATE, 0, TIMER_BIT);
    registers.update(PERIPHERAL_RESET, 0, TIMER_BIT);
    registers.write(TIMER_CLOCK_SELECT[0], TIMER_CLOCK_VALUES[0]);
    registers.write(TIMER_CLOCK_SELECT[1], TIMER_CLOCK_VALUES[1]);
}

fn timers_stop<R: Registers>(registers: &mut R) {
    registers.write(TIMER_CONTROL[0], 0);
    registers.write(TIMER_CONTROL[1], 0);
    registers.update(PERIPHERAL_GATE, TIMER_BIT, 0);
    registers.update(PERIPHERAL_RESET, TIMER_BIT, 0);
}

pub fn measure_trim<R: Registers>(registers: &mut R, profile: PllProfile, trim: u8) -> Result<bool, ClockError> {
    set_trim(registers, trim);
    registers.delay_us(TRIM_SETTLE_US);
    registers.update(AUDIO_PLL, 0, PLL_ENABLE);
    timers_enable(registers);
    registers.delay_us(LOCK_SETTLE_US);
    registers.write_pair((TIMER_CONTROL[0], 3), (TIMER_CONTROL[1], 3));
    registers.delay_us(GATE_US);
    let (reference, measured) = registers.read_pair(TIMER_REFERENCE_COUNT, TIMER_MEASURED_COUNT);
    registers.update(AUDIO_PLL, PLL_ENABLE, 0);
    registers.delay_us(PLL_OFF_SETTLE_US);
    timers_stop(registers);
    profile.accepts(reference, measured).ok_or(ClockError::InvalidReference { trim, reference })
}

pub fn sweep<R: Registers>(registers: &mut R, profile: PllProfile) -> Result<(u8, u16), ClockError> {
    registers.update(AUDIO_PLL, PLL_ENABLE, 0);
    registers.delay_us(PLL_OFF_SETTLE_US);
    configure(registers, profile);
    let mut valid = 0u16;
    for trim in 0..16u8 {
        if measure_trim(registers, profile, trim)? {
            valid |= 1 << trim;
        }
    }
    Ok((choose_trim(valid)?, valid))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Snapshot {
    control: u32,
    fraction: u32,
    trim: u32,
    timer_clocks: [u32; 2],
}

impl Snapshot {
    pub fn capture<R: Registers>(registers: &mut R) -> Self {
        Self {
            control: registers.read(AUDIO_PLL),
            fraction: registers.read(AUDIO_PLL_FRACTION),
            trim: registers.read(AUDIO_PLL_TRIM) & TRIM_MASK,
            timer_clocks: TIMER_CLOCK_SELECT.map(|address| registers.read(address)),
        }
    }

    pub fn restore_pll<R: Registers>(&self, registers: &mut R) {
        registers.write(AUDIO_PLL, self.control & !PLL_ENABLE);
        registers.write(AUDIO_PLL_FRACTION, self.fraction);
        registers.update(AUDIO_PLL_TRIM, TRIM_MASK, self.trim);
        registers.write(AUDIO_PLL, self.control);
    }

    fn restore_timer_clocks<R: Registers>(&self, registers: &mut R) {
        for (address, value) in TIMER_CLOCK_SELECT.into_iter().zip(self.timer_clocks) {
            registers.write(address, value);
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Calibration {
    pub trims: [u8; 2],
    pub windows: [u16; 2],
}

impl Calibration {
    pub fn run<R: Registers>(registers: &mut R) -> Result<Self, ClockError> {
        if (registers.read(PERIPHERAL_GATE) | registers.read(PERIPHERAL_RESET)) & TIMER_BIT != 0 {
            return Err(ClockError::TimerInUse);
        }
        crate::regs::apply(registers, &ANALOG_POWER);
        let snapshot = Snapshot::capture(registers);
        let result = (|| {
            let (trim48, window48) = sweep(registers, Family::Rate48k.pll())?;
            let (trim44, window44) = sweep(registers, Family::Rate44k1.pll())?;
            Ok(Self { trims: [trim48, trim44], windows: [window48, window44] })
        })();
        snapshot.restore_timer_clocks(registers);
        match result {
            Ok(calibration) => {
                calibration.apply(registers, Family::Rate48k);
                Ok(calibration)
            }
            Err(error) => {
                snapshot.restore_pll(registers);
                Err(error)
            }
        }
    }

    pub const fn trim(&self, family: Family) -> u8 {
        self.trims[family.slot()]
    }

    pub fn apply<R: Registers>(&self, registers: &mut R, family: Family) {
        registers.update(AUDIO_PLL, PLL_ENABLE, 0);
        registers.delay_us(PLL_OFF_SETTLE_US);
        configure(registers, family.pll());
        set_trim(registers, self.trim(family));
        registers.delay_us(TRIM_SETTLE_US);
        registers.update(AUDIO_PLL, 0, PLL_ENABLE);
        registers.delay_us(LOCK_SETTLE_US);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Off,
    Trim,
    Lock,
    Ready,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PllSwitch {
    family: Family,
    trim: u8,
    phase: Phase,
    wake_us: u64,
    last_us: u64,
    snapshot: Snapshot,
}

impl PllSwitch {
    pub fn begin<R: Registers>(registers: &mut R, calibration: &Calibration, family: Family, now_us: u64) -> Self {
        let snapshot = Snapshot::capture(registers);
        registers.update(AUDIO_PLL, PLL_ENABLE, 0);
        Self {
            family,
            trim: calibration.trim(family),
            phase: Phase::Off,
            wake_us: now_us.saturating_add(u64::from(PLL_OFF_SETTLE_US)),
            last_us: now_us,
            snapshot,
        }
    }

    pub const fn family(&self) -> Family {
        self.family
    }

    pub fn ready(&self) -> bool {
        self.phase == Phase::Ready
    }

    pub fn wake_us(&self) -> Option<u64> {
        (self.phase != Phase::Ready).then_some(self.wake_us)
    }

    pub fn poll<R: Registers>(&mut self, registers: &mut R, now_us: u64) -> Result<bool, ClockError> {
        if now_us < self.last_us {
            return Err(ClockError::TimeWentBackwards);
        }
        self.last_us = now_us;
        if self.phase == Phase::Ready || now_us < self.wake_us {
            return Ok(self.ready());
        }
        match self.phase {
            Phase::Off => {
                configure(registers, self.family.pll());
                set_trim(registers, self.trim);
                self.wake_us = now_us.saturating_add(u64::from(TRIM_SETTLE_US));
                self.phase = Phase::Trim;
            }
            Phase::Trim => {
                registers.update(AUDIO_PLL, 0, PLL_ENABLE);
                self.wake_us = now_us.saturating_add(u64::from(LOCK_SETTLE_US));
                self.phase = Phase::Lock;
            }
            Phase::Lock | Phase::Ready => self.phase = Phase::Ready,
        }
        Ok(self.ready())
    }

    pub fn abort<R: Registers>(self, registers: &mut R) {
        self.snapshot.restore_pll(registers);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::{Access, Recorder};

    #[test]
    fn bounds_match_stock_doubles_for_every_accepted_reference() {
        let bits = [0x4008_9374_c000_0000u64, 0x4000_624d_e000_0000, 0x4006_9446_8000_0000, 0x3ffe_1b08_a000_0000];
        let lower = f64::from_bits(0x3fef_dddf_4000_0000);
        let upper = f64::from_bits(0x3ff0_1110_6000_0000);
        for (index, ratio_bits) in bits.iter().enumerate() {
            let profile = PllProfile::from_stock_index(index as u8).unwrap();
            let ratio = f64::from_bits(*ratio_bits);
            for reference in 1..=65_535 {
                let expected = f64::from(reference) * ratio;
                assert_eq!(profile.bounds(reference), Some(((expected * lower) as u32, (expected * upper) as u32)));
            }
        }
    }

    #[test]
    fn profiles_program_the_family_master_clocks_from_a_12_mhz_reference() {
        let expected = [73_728_000u64, 49_152_000, 67_737_600, 45_158_400];
        for (profile, hz) in PllProfile::STOCK.into_iter().zip(expected) {
            assert!(profile.output_hz().abs_diff(hz) < 10, "{profile:?}");
            let ratio = hz as f64 / 24_000_000.0;
            let (low, high) = profile.bounds(24_000).unwrap();
            assert!(f64::from(low) < 24_000.0 * ratio && 24_000.0 * ratio < f64::from(high));
        }
        assert_eq!(Family::Rate48k.pll().output_hz().abs_diff(Family::Rate48k.master_hz().into()), 4);
        assert_eq!(Family::Rate44k1.pll().output_hz().abs_diff(Family::Rate44k1.master_hz().into()), 3);
    }

    #[test]
    fn families_follow_the_rate() {
        for hz in [44_100, 88_200, 176_400, 352_800, 705_600] {
            assert_eq!(Family::of_rate(hz), Some(Family::Rate44k1));
        }
        for hz in [32_000, 48_000, 96_000, 192_000, 384_000, 768_000] {
            assert_eq!(Family::of_rate(hz), Some(Family::Rate48k));
        }
        assert_eq!(Family::of_rate(0), None);
        assert_eq!(Family::of_rate(50_000), None);
    }

    #[test]
    fn invalid_counts_and_inclusive_thresholds() {
        let profile = PllProfile::from_stock_index(1).unwrap();
        assert_eq!(profile.bounds(0), None);
        assert_eq!(profile.bounds(65_536), None);
        assert_eq!(PllProfile::from_stock_index(4), None);
        assert_eq!(profile.bounds(3_120), Some((6_363, 6_416)));
        assert_eq!(profile.accepts(3_120, 6_362), Some(false));
        assert_eq!(profile.accepts(3_120, 6_363), Some(true));
        assert_eq!(profile.accepts(3_120, 6_416), Some(true));
        assert_eq!(profile.accepts(3_120, 6_417), Some(false));
        assert_eq!(PllProfile::from_stock_index(3).unwrap().bounds(3_120), Some((5_846, 5_895)));
    }

    #[test]
    fn all_trim_masks_are_checked() {
        for valid in 0..=u16::MAX {
            let ones: std::vec::Vec<u8> = (0..16u8).filter(|t| valid & (1 << t) != 0).collect();
            let expected = match (ones.first(), ones.last()) {
                (None, _) => Err(ClockError::NoTrimWindow),
                (Some(&first), Some(&last)) if usize::from(last - first) + 1 == ones.len() => Ok(first + (last - first) / 2),
                _ => Err(ClockError::DisjointTrimWindows),
            };
            assert_eq!(choose_trim(valid), expected);
        }
    }

    #[test]
    fn cpu_clock_decoding_and_the_measured_boot_rom_state() {
        for (code, divisor) in [1, 2, 4, 8, 12, 16, 24, 32].iter().enumerate() {
            assert_eq!(decode_cpu_hz(5 | ((code as u32) << 4), 0), Some(480_000_000 / divisor));
        }
        assert_eq!(decode_cpu_hz(4, 1 | 8 | (500 << 16) | (25 << 25) | (2 << 12)), Some(240_000_000));
        assert_eq!(decode_cpu_hz(4, 1 | (10 << 4)), Some(60_000_000));
        assert_eq!(decode_cpu_hz(4, 1 | 8 | (4 << 16)), None);
        assert_eq!(decode_cpu_hz(4, 0), None);
        assert_eq!(decode_cpu_hz(7, 0), None);
        let mut r = Recorder::default();
        r.set(SYSTEM_CLOCK, 0x0000_0004);
        r.set(CORE_PLL, 1 | (10 << 4));
        assert_eq!(read_cpu_hz(&mut r), Some(60_000_000));
        r.set(SYSTEM_CLOCK, 0x0000_0001);
        assert_eq!(read_cpu_hz(&mut r), Some(24_000_000));
        assert_eq!(r.log.iter().filter(|a| matches!(a, Access::Read(CORE_PLL, _))).count(), 1);
    }

    #[test]
    fn cycle_time_and_frame_clock() {
        let time = CycleTime::new(60_000_000).unwrap();
        assert_eq!(time.micros(60_000_000 * 3 + 30), 3_000_000);
        assert_eq!(time.micros(u64::MAX), u64::MAX / 60);
        assert_eq!(time.cycles(1), 60);
        assert_eq!(CycleTime::new(0), None);
        let mut clock = FrameClock::start(1_000, 2_040);
        clock.observe(1_000 + 31 * 60_000, (2_040 + 31) & 0x7ff);
        assert_eq!(clock.cpu_hz(), None);
        clock.observe(1_000 + 32 * 60_001, (2_040 + 32) & 0x7ff);
        assert_eq!(clock.cpu_hz(), Some(60_001_000));
        let mut stale = FrameClock::start(0, 0);
        stale.observe(10_000_000, 500);
        stale.observe(10_000_000 + 40 * 60_000, 540);
        assert_eq!(stale.cpu_hz(), Some(60_000_000));
    }

    fn accept_all(r: &mut Recorder, profile: PllProfile, windows: &[bool; 16]) {
        for good in windows {
            let reference = 3_120;
            let (low, high) = profile.bounds(reference).unwrap();
            r.script(TIMER_REFERENCE_COUNT, &[reference]);
            r.script(TIMER_MEASURED_COUNT, &[if *good { (low + high) / 2 } else { high + 1 }]);
        }
    }

    #[test]
    fn calibration_sweeps_both_families_and_leaves_the_48k_family_running() {
        let mut r = Recorder::default();
        r.set(TIMER_CLOCK_SELECT[0], 0x77);
        r.set(TIMER_CLOCK_SELECT[1], 0x88);
        let window48: [bool; 16] = core::array::from_fn(|t| (4..=9).contains(&t));
        let window44: [bool; 16] = core::array::from_fn(|t| (10..=15).contains(&t));
        accept_all(&mut r, Family::Rate48k.pll(), &window48);
        accept_all(&mut r, Family::Rate44k1.pll(), &window44);
        let calibration = Calibration::run(&mut r).unwrap();
        assert_eq!(calibration, Calibration { trims: [6, 12], windows: [0x03f0, 0xfc00] });
        assert_eq!(r.get(TIMER_CLOCK_SELECT[0]), 0x77);
        assert_eq!(r.get(TIMER_CLOCK_SELECT[1]), 0x88);
        assert_eq!(r.get(AUDIO_PLL_FRACTION), Family::Rate48k.pll().fraction);
        assert_eq!(r.get(AUDIO_PLL_TRIM) & TRIM_MASK, 6 << 16);
        assert_eq!(r.get(AUDIO_PLL) & 0x1f_0000, 4 << 16);
        assert_eq!(r.get(AUDIO_PLL) & 1, 1);
        assert_eq!(r.get(ANALOG_SUPPLY) & ANALOG_SUPPLY_ENABLE, ANALOG_SUPPLY_ENABLE);
        assert_eq!((r.get(0x2100_3000) & 1, r.get(0x2100_2000) & 1), (1, 1));
        let per_trim = TRIM_SETTLE_US + LOCK_SETTLE_US + GATE_US + PLL_OFF_SETTLE_US;
        let expected = 2 * (PLL_OFF_SETTLE_US + 16 * per_trim) + PLL_OFF_SETTLE_US + TRIM_SETTLE_US + LOCK_SETTLE_US;
        assert_eq!(r.elapsed_us, u64::from(expected));
    }

    #[test]
    fn a_failed_calibration_restores_the_pll_and_reports_the_reason() {
        let mut r = Recorder::default();
        r.set(AUDIO_PLL, 0x0004_0241);
        r.set(AUDIO_PLL_FRACTION, 1234);
        r.set(AUDIO_PLL_TRIM, 0x0003_0000);
        let window: [bool; 16] = core::array::from_fn(|t| t == 2 || t == 9);
        accept_all(&mut r, Family::Rate48k.pll(), &window);
        assert_eq!(Calibration::run(&mut r), Err(ClockError::DisjointTrimWindows));
        assert_eq!((r.get(AUDIO_PLL), r.get(AUDIO_PLL_FRACTION), r.get(AUDIO_PLL_TRIM)), (0x0004_0241, 1234, 0x0003_0000));
        let mut busy = Recorder::default();
        busy.set(PERIPHERAL_GATE, TIMER_BIT);
        assert_eq!(Calibration::run(&mut busy), Err(ClockError::TimerInUse));
        assert!(busy.writes().is_empty());
        let mut zero = Recorder::default();
        assert_eq!(Calibration::run(&mut zero), Err(ClockError::InvalidReference { trim: 0, reference: 0 }));
    }

    #[test]
    fn switch_waits_without_blocking_and_reaches_the_target_family() {
        let mut r = Recorder::default();
        let calibration = Calibration { trims: [6, 12], windows: [0, 0] };
        r.set(AUDIO_PLL, 0x0004_0243);
        let mut switch = PllSwitch::begin(&mut r, &calibration, Family::Rate44k1, 1_000);
        assert_eq!(r.get(AUDIO_PLL) & 1, 0);
        assert_eq!(switch.wake_us(), Some(1_060));
        let writes = r.writes().len();
        assert_eq!(switch.poll(&mut r, 1_059), Ok(false));
        assert_eq!(r.writes().len(), writes);
        assert_eq!(switch.poll(&mut r, 1_060), Ok(false));
        assert_eq!(r.get(AUDIO_PLL_FRACTION), Family::Rate44k1.pll().fraction);
        assert_eq!(r.get(AUDIO_PLL_TRIM) & TRIM_MASK, 12 << 16);
        assert_eq!(r.get(AUDIO_PLL) & 1, 0);
        assert_eq!(switch.poll(&mut r, 2_060), Ok(false));
        assert_eq!(r.get(AUDIO_PLL) & 1, 1);
        assert_eq!(switch.poll(&mut r, 4_059), Ok(false));
        assert_eq!(switch.poll(&mut r, 4_060), Ok(true));
        assert_eq!(switch.wake_us(), None);
        assert_eq!(switch.poll(&mut r, 4_000), Err(ClockError::TimeWentBackwards));
        assert!(r.delays().is_empty());
    }

    #[test]
    fn an_aborted_switch_restores_the_previous_pll_state() {
        let mut r = Recorder::default();
        r.set(AUDIO_PLL, 0x0004_0243);
        r.set(AUDIO_PLL_FRACTION, Family::Rate48k.pll().fraction);
        r.set(AUDIO_PLL_TRIM, 0x1006_0000);
        let calibration = Calibration { trims: [6, 12], windows: [0, 0] };
        let mut switch = PllSwitch::begin(&mut r, &calibration, Family::Rate44k1, 0);
        switch.poll(&mut r, 100).unwrap();
        switch.abort(&mut r);
        assert_eq!(r.get(AUDIO_PLL), 0x0004_0243);
        assert_eq!(r.get(AUDIO_PLL_FRACTION), Family::Rate48k.pll().fraction);
        assert_eq!(r.get(AUDIO_PLL_TRIM), 0x1006_0000);
        let writes = r.writes();
        assert_eq!(writes[writes.len() - 4], (AUDIO_PLL, 0x0004_0242));
    }

    #[test]
    fn timers_are_stopped_and_released_before_usb_starts() {
        let mut r = Recorder::default();
        r.set(TIMER_CONTROL[2], 1);
        r.script(TIMER_CONTROL[2], &[1]);
        assert!(!quiesce_timers(&mut r));
        assert_eq!(r.get(PERIPHERAL_GATE) & TIMER_BIT, 0);
        assert_eq!(r.get(PERIPHERAL_RESET) & TIMER_BIT, 0);
        assert!(TIMER_CONTROL.iter().all(|a| r.get(*a) == 0));
        assert!(quiesce_timers(&mut r));
    }
}

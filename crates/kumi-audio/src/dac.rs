// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::Vec;
use kumi_icd::{ClassHMode, DacExtras, DacFilter};

use crate::i2c::{I2cBus, I2cError};
use crate::pcm::{DsdRate, PcmProfile};

pub const PRIMARY_ADDRESS: u8 = 0x33;
pub const SECONDARY_ADDRESS: u8 = 0x31;
pub const NEGATIVE_CONTROL_ADDRESS: u8 = 0x32;
pub const REGISTER_PAD: u8 = 0x01;
pub const SETTLE_MS: u16 = 15;
pub const STANDBY_MS: u16 = 10;
pub const RESUME_MS: u16 = 12;
pub const MAX_OPS: usize = 128;
pub const MUTE: u8 = 0xff;
pub const MAX_ATTENUATION: u8 = 0xfe;
pub const STOCK_MAX_ATTENUATION: u8 = 119;
pub const IDENTITY: [u8; 2] = [0x43, 0x19];
pub const IDENTITY_E_HIGH: u8 = 0x80;

pub mod reg {
    pub const DEVICE_ID_AB: u32 = 0x01_0000;
    pub const DEVICE_ID_CD: u32 = 0x01_0001;
    pub const DEVICE_ID_E: u32 = 0x01_0002;
    pub const SYSTEM_CLOCKING: u32 = 0x01_0006;
    pub const SP_SAMPLE_RATE: u32 = 0x01_000b;
    pub const SP_BIT_SIZE: u32 = 0x01_000c;
    pub const DXD1: u32 = 0x01_0010;
    pub const POWER_DOWN: u32 = 0x02_0000;
    pub const CRYSTAL: u32 = 0x02_0052;
    pub const PLL_1: u32 = 0x03_0001;
    pub const PLL_2: u32 = 0x03_0002;
    pub const PLL_3: u32 = 0x03_0003;
    pub const PLL_4: u32 = 0x03_0004;
    pub const PLL_5: u32 = 0x03_0005;
    pub const PLL_6: u32 = 0x03_0008;
    pub const PLL_7: u32 = 0x03_000a;
    pub const PLL_8: u32 = 0x03_001b;
    pub const PLL_9: u32 = 0x04_0002;
    pub const ASP_NUM_1: u32 = 0x04_0010;
    pub const ASP_NUM_2: u32 = 0x04_0011;
    pub const ASP_DEN_1: u32 = 0x04_0012;
    pub const ASP_DEN_2: u32 = 0x04_0013;
    pub const ASP_LRCK_HIGH_1: u32 = 0x04_0014;
    pub const ASP_LRCK_HIGH_2: u32 = 0x04_0015;
    pub const ASP_LRCK_PERIOD_1: u32 = 0x04_0016;
    pub const ASP_LRCK_PERIOD_2: u32 = 0x04_0017;
    pub const ASP_CLOCK: u32 = 0x04_0018;
    pub const ASP_FRAME: u32 = 0x04_0019;
    pub const ASP_CH1_LOCATION: u32 = 0x05_0000;
    pub const ASP_CH2_LOCATION: u32 = 0x05_0001;
    pub const ASP_CH1_SIZE: u32 = 0x05_000a;
    pub const ASP_CH2_SIZE: u32 = 0x05_000b;
    pub const DSD_VOLUME_B: u32 = 0x07_0000;
    pub const DSD_VOLUME_A: u32 = 0x07_0001;
    pub const DSD_PATH_1: u32 = 0x07_0002;
    pub const DSD_INTERFACE: u32 = 0x07_0003;
    pub const DSD_PATH_2: u32 = 0x07_0004;
    pub const DSD_PCM_MIX: u32 = 0x07_0005;
    pub const HP_OUTPUT: u32 = 0x08_0000;
    pub const DXD13: u32 = 0x08_0032;
    pub const PCM_FILTER: u32 = 0x09_0000;
    pub const PCM_VOLUME_B: u32 = 0x09_0001;
    pub const PCM_VOLUME_A: u32 = 0x09_0002;
    pub const PCM_PATH_1: u32 = 0x09_0003;
    pub const PCM_PATH_2: u32 = 0x09_0004;
    pub const CLASS_H: u32 = 0x0b_0000;
    pub const HP_DETECT: u32 = 0x0d_0000;
    pub const HP_STATUS: u32 = 0x0d_0001;
    pub const HP_LOAD_1: u32 = 0x0e_0000;
    pub const HP_MEAS_LOAD_1: u32 = 0x0e_0003;
    pub const HP_MEAS_LOAD_2: u32 = 0x0e_0004;
    pub const HP_DC_STAT_1: u32 = 0x0e_000d;
    pub const HP_DC_STAT_2: u32 = 0x0e_000e;
    pub const HP_AC_STAT_1: u32 = 0x0e_0010;
    pub const HP_AC_STAT_2: u32 = 0x0e_0011;
    pub const HP_LOAD_STAT: u32 = 0x0e_001a;
    pub const INT_STATUS_1: u32 = 0x0f_0000;
    pub const INT_STATUS_4: u32 = 0x0f_0003;
    pub const INT_MASK_1: u32 = 0x0f_0010;
    pub const INT_MASK_2: u32 = 0x0f_0011;
    pub const INT_MASK_4: u32 = 0x0f_0013;
}

pub const HPLOAD_ON_INT: u8 = 1 << 0;
pub const HPLOAD_OFF_INT: u8 = 1 << 1;
pub const HPLOAD_DC_INT: u8 = 1 << 2;
pub const HPLOAD_AC_INT: u8 = 1 << 3;
pub const HPLOAD_OOR_INT: u8 = 1 << 4;
pub const HPLOAD_UNPLUG_INT: u8 = 1 << 6;
pub const HPLOAD_NO_DC_INT: u8 = 1 << 7;
pub const HPLOAD_FAULT_MASK: u8 = HPLOAD_NO_DC_INT | HPLOAD_UNPLUG_INT | HPLOAD_OOR_INT;
pub const HPLOAD_AC_1KHZ_CODE: u16 = 1_000 / 6;

pub const fn decode_hpload_ohms(stat1: u8, stat2: u8) -> u16 {
    ((stat1 as u16) >> 3) | ((stat2 as u16) << 5)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ImpedanceReading {
    pub left_ohms: Option<u16>,
    pub right_ohms: Option<u16>,
    pub left_ac_ohms: Option<u16>,
    pub right_ac_ohms: Option<u16>,
    pub left_status: u8,
    pub right_status: u8,
    pub hp_status: u8,
    pub load_stat: u8,
}

pub fn prepare_clocking(dac: Dac) -> Program {
    let mut program = Program::new();
    writes(&mut program, dac, &PREPARE);
    push(&mut program, Op::Delay { ms: SETTLE_MS });
    push(&mut program, Op::Write { dac, register: reg::SYSTEM_CLOCKING, value: 0x01 });
    program
}

pub fn measure_dc_impedance<B: I2cBus>(
    bus: &mut B,
    dac: Dac,
    mut delay_ms: impl FnMut(u16),
) -> Result<ImpedanceReading, DacError> {
    let mut wait_int4 = |bus: &mut B, target: u8| -> Result<u8, DacError> {
        let mut last = 0u8;
        for _ in 0..40 {
            let status = read(bus, dac, reg::INT_STATUS_4)?;
            last |= status;
            if last & (target | HPLOAD_FAULT_MASK) != 0 {
                return Ok(last);
            }
            delay_ms(10);
        }
        Ok(last)
    };

    let _ = read(bus, dac, reg::INT_STATUS_4)?;
    let hp_status = read(bus, dac, reg::HP_STATUS)?;

    let mut measure_channel = |bus: &mut B, ch_bits: u8| -> Result<(Option<u16>, Option<u16>, u8), DacError> {
        let mut active = false;
        let result = (|| {
            write(bus, dac, reg::INT_MASK_4, 0xff)?;
            write(bus, dac, reg::HP_MEAS_LOAD_1, 0x00)?;
            write(bus, dac, reg::HP_MEAS_LOAD_2, 0x00)?;
            write(bus, dac, reg::INT_MASK_4, 0x00)?;
            write(bus, dac, reg::HP_LOAD_1, 0x80)?;
            active = true;
            let on_stat = wait_int4(bus, HPLOAD_ON_INT)?;
            if on_stat & HPLOAD_ON_INT == 0 || on_stat & HPLOAD_FAULT_MASK != 0 {
                return Ok((None, None, on_stat));
            }
            write(bus, dac, reg::HP_LOAD_1, 0x80 | ch_bits)?;
            write(bus, dac, reg::HP_LOAD_1, 0x81 | ch_bits)?;
            let dc_stat = wait_int4(bus, HPLOAD_DC_INT)?;
            let dc_ohms = if dc_stat & HPLOAD_DC_INT != 0 && dc_stat & HPLOAD_FAULT_MASK == 0 {
                let lo = read(bus, dac, reg::HP_DC_STAT_1)?;
                let hi = read(bus, dac, reg::HP_DC_STAT_2)?;
                Some(decode_hpload_ohms(lo, hi))
            } else {
                None
            };
            let (ac_ohms, combined_stat) = if dc_ohms.is_some() {
                write(bus, dac, reg::HP_LOAD_1, 0x80 | ch_bits)?;
                write(bus, dac, reg::HP_MEAS_LOAD_1, (HPLOAD_AC_1KHZ_CODE & 0xff) as u8)?;
                write(bus, dac, reg::HP_MEAS_LOAD_2, (HPLOAD_AC_1KHZ_CODE >> 8) as u8)?;
                write(bus, dac, reg::HP_LOAD_1, 0x80 | ch_bits)?;
                write(bus, dac, reg::HP_LOAD_1, 0x82 | ch_bits)?;
                let ac_stat = wait_int4(bus, HPLOAD_AC_INT)?;
                let ac = if ac_stat & HPLOAD_AC_INT != 0 && ac_stat & HPLOAD_FAULT_MASK == 0 {
                    let lo = read(bus, dac, reg::HP_AC_STAT_1)?;
                    let hi = read(bus, dac, reg::HP_AC_STAT_2)?;
                    Some(decode_hpload_ohms(lo, hi))
                } else {
                    None
                };
                (ac, dc_stat | ac_stat)
            } else {
                (None, dc_stat)
            };
            Ok((dc_ohms, ac_ohms, combined_stat))
        })();
        if active {
            let _ = write(bus, dac, reg::HP_LOAD_1, 0x80);
            let _ = write(bus, dac, reg::HP_LOAD_1, 0x00);
            let _ = wait_int4(bus, HPLOAD_OFF_INT);
        }
        let _ = write(bus, dac, reg::HP_MEAS_LOAD_1, 0x00);
        let _ = write(bus, dac, reg::HP_MEAS_LOAD_2, 0x00);
        let _ = write(bus, dac, reg::INT_MASK_4, 0xff);
        result
    };

    let result = (|| {
        let (left_ohms, left_ac_ohms, left_status) = measure_channel(bus, 0x00)?;
        let (right_ohms, right_ac_ohms, right_status) = measure_channel(bus, 0x10)?;
        let load_stat = read(bus, dac, reg::HP_LOAD_STAT)?;
        Ok(ImpedanceReading {
            left_ohms,
            right_ohms,
            left_ac_ohms,
            right_ac_ohms,
            left_status,
            right_status,
            hp_status,
            load_stat,
        })
    })();
    let _ = write(bus, dac, reg::INT_MASK_4, 0xff);
    result
}

pub const PDN_ASP: u8 = 0x40;
pub const PDN_HP: u8 = 0x10;
pub const PDN_DIGITAL: u8 = 0xe2;
pub const POWER_DOWN_ALL: u8 = 0xfe;
pub const SYS_CLK_RCO: u8 = 0x06;
pub const CLOCK_SWITCH_MS: u16 = 1;
pub const DSD_PATH_1_MUTED: u8 = 0xab;
pub const DSD_PATH_2_OFF: u8 = 0x02;
pub const DSD_EN_BIT: u8 = 0x10;
pub const PCM_PATH_1_MUTED: u8 = 0xa3;
pub const PCM_PATH_1_SOFT_RAMP: u8 = 0xb0;
pub const PCM_PATH_1_SOFT_MUTED: u8 = 0xb3;
pub const FILTER_KEEP: u8 = 0x1f;
pub const FILTER_KEEP_EXTRAS: u8 = 0x1c;
pub const CLASS_H_ADAPTIVE_HV: u8 = 0x1e;
pub const CLASS_H_LOW_POWER_LV: u8 = 0x1c;
pub const CLASS_H_FIXED_HV: u8 = 0x06;
pub const HP_OUTPUT_HV: u8 = 0x31;
pub const HP_OUTPUT_LV: u8 = 0x11;
pub const VOLUME_LEFT: u32 = reg::PCM_VOLUME_B;
pub const VOLUME_RIGHT: u32 = reg::PCM_VOLUME_A;
pub const DSD_VOLUME_LEFT: u32 = reg::DSD_VOLUME_B;
pub const DSD_VOLUME_RIGHT: u32 = reg::DSD_VOLUME_A;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Dac {
    Primary,
    Secondary,
}

pub const PAIR: [Dac; 2] = [Dac::Primary, Dac::Secondary];

impl Dac {
    pub const fn address(self) -> u8 {
        match self {
            Self::Primary => PRIMARY_ADDRESS,
            Self::Secondary => SECONDARY_ADDRESS,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Filter {
    FastLowLatency,
    FastPhaseCompensated,
    SlowLowLatency,
    SlowPhaseCompensated,
    NonOversampling,
}

impl Filter {
    pub const fn bits(self) -> u8 {
        match self {
            Self::FastLowLatency => 0x00,
            Self::FastPhaseCompensated => 0x40,
            Self::SlowLowLatency => 0x80,
            Self::SlowPhaseCompensated => 0xc0,
            Self::NonOversampling => 0x20,
        }
    }
}

impl From<DacFilter> for Filter {
    fn from(filter: DacFilter) -> Self {
        match filter {
            DacFilter::FastLowLatency => Self::FastLowLatency,
            DacFilter::FastPhaseCompensated => Self::FastPhaseCompensated,
            DacFilter::SlowLowLatency => Self::SlowLowLatency,
            DacFilter::SlowPhaseCompensated => Self::SlowPhaseCompensated,
            DacFilter::NonOversampling => Self::NonOversampling,
        }
    }
}

pub const fn attenuation(centi_db: i32) -> u8 {
    if centi_db >= 0 {
        return 0;
    }
    let steps = (-(centi_db as i64) + 49) / 50;
    if steps >= MAX_ATTENUATION as i64 { MAX_ATTENUATION } else { steps as u8 }
}

pub const fn stock_attenuation(half_db_steps: u8, muted: bool) -> u8 {
    if muted || half_db_steps > STOCK_MAX_ATTENUATION { MUTE } else { half_db_steps }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Volume {
    pub left: u8,
    pub right: u8,
    pub muted: bool,
}

impl Volume {
    pub const MUTED: Self = Self { left: MUTE, right: MUTE, muted: true };
    pub const UNITY: Self = Self { left: 0, right: 0, muted: false };

    pub fn from_settings(host_centi_db: [i16; 2], host_muted: bool, limit_centi_db: i16, balance_centi_db: i16) -> Self {
        let level = |host: i16, balance: i32| attenuation(i32::from(host.min(limit_centi_db)) - balance);
        let balance = i32::from(balance_centi_db);
        Self {
            left: level(host_centi_db[0], balance.max(0)),
            right: level(host_centi_db[1], (-balance).max(0)),
            muted: host_muted,
        }
    }

    pub const fn registers(self) -> [u8; 2] {
        if self.muted { [MUTE, MUTE] } else { [self.left, self.right] }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    Write { dac: Dac, register: u32, value: u8 },
    Modify { dac: Dac, register: u32, keep: u8, set: u8 },
    Expect { dac: Dac, register: u32, mask: u8, value: u8 },
    Delay { ms: u16 },
}

pub type Program = Vec<Op, MAX_OPS>;

const PREPARE: [(u32, u8); 14] = [
    (reg::CRYSTAL, 0x04),
    (reg::INT_MASK_1, 0xe7),
    (reg::POWER_DOWN, 0xf2),
    (reg::PLL_9, 0x03),
    (reg::PLL_5, 0x40),
    (reg::PLL_2, 0x00),
    (reg::PLL_3, 0x00),
    (reg::PLL_4, 0x00),
    (reg::PLL_8, 0x01),
    (reg::PLL_6, 0x08),
    (reg::PLL_7, 0x83),
    (reg::INT_MASK_1, 0xe1),
    (reg::PLL_1, 0x01),
    (reg::SYSTEM_CLOCKING, 0x00),
];

const PCM_ROUTING: [(u32, u8); 5] = [
    (reg::PCM_PATH_1, 0xa0),
    (reg::PCM_PATH_2, 0x00),
    (reg::DSD_PATH_1, DSD_PATH_1_MUTED),
    (reg::DSD_PATH_2, DSD_PATH_2_OFF),
    (reg::DSD_PCM_MIX, 0x00),
];
const DSD_DISABLE_PCM: [(u32, u8); 2] = [(reg::PCM_PATH_1, PCM_PATH_1_MUTED), (reg::DSD_PCM_MIX, 0x00)];
const ENABLE: [(u32, u8); 4] =
    [(reg::CLASS_H, 0x1e), (reg::HP_OUTPUT, 0x31), (reg::INT_MASK_1, 0x11), (reg::SYSTEM_CLOCKING, 0x01)];
const PCM_POWER_UP: [(u32, u8); 2] = [(reg::INT_MASK_2, 0x07), (reg::POWER_DOWN, 0x01)];
const DSD_POWER_UP: [(u32, u8); 1] = [(reg::POWER_DOWN, 0x01)];
const SHUTDOWN_MUTE: [(u32, u8); 4] = [
    (reg::PCM_PATH_1, PCM_PATH_1_MUTED),
    (reg::DSD_PATH_1, DSD_PATH_1_MUTED),
    (reg::DSD_PATH_2, DSD_PATH_2_OFF),
    (reg::DSD_PCM_MIX, 0x00),
];

pub const fn pcm_timing(profile: PcmProfile) -> [(u32, u8); 16] {
    [
        (reg::SP_SAMPLE_RATE, profile.dac_rate_code()),
        (reg::SP_BIT_SIZE, 0),
        (reg::ASP_NUM_1, profile.asp_n() as u8),
        (reg::ASP_NUM_2, (profile.asp_n() >> 8) as u8),
        (reg::ASP_DEN_1, profile.asp_m() as u8),
        (reg::ASP_DEN_2, (profile.asp_m() >> 8) as u8),
        (reg::ASP_LRCK_HIGH_1, 0x1f),
        (reg::ASP_LRCK_HIGH_2, 0),
        (reg::ASP_LRCK_PERIOD_1, 0x3f),
        (reg::ASP_LRCK_PERIOD_2, 0),
        (reg::ASP_CLOCK, 0x0c),
        (reg::ASP_FRAME, 0x0a),
        (reg::ASP_CH1_LOCATION, 0),
        (reg::ASP_CH2_LOCATION, 0),
        (reg::ASP_CH1_SIZE, 7),
        (reg::ASP_CH2_SIZE, 15),
    ]
}

fn push(program: &mut Program, op: Op) {
    if program.push(op).is_err() {
        unreachable!()
    }
}

fn writes(program: &mut Program, dac: Dac, list: &[(u32, u8)]) {
    for &(register, value) in list {
        push(program, Op::Write { dac, register, value });
    }
}

fn prepare(program: &mut Program) {
    for dac in PAIR {
        writes(program, dac, &PREPARE);
        push(program, Op::Delay { ms: SETTLE_MS });
    }
}

fn enable(program: &mut Program, power_up: &[(u32, u8)]) {
    for dac in PAIR {
        writes(program, dac, &ENABLE);
        push(program, Op::Delay { ms: SETTLE_MS });
        writes(program, dac, power_up);
    }
}

fn volume_writes(program: &mut Program, volume: Volume, registers: [u32; 2]) {
    let values = volume.registers();
    for (register, value) in registers.into_iter().zip(values) {
        push(program, Op::Write { dac: Dac::Primary, register, value });
    }
}

fn volume_checks(program: &mut Program, volume: Volume, registers: [u32; 2]) {
    let values = volume.registers();
    for (register, value) in registers.into_iter().zip(values) {
        push(program, Op::Expect { dac: Dac::Primary, register, mask: 0xff, value });
    }
}

pub const fn class_h_value(mode: ClassHMode) -> u8 {
    match mode {
        ClassHMode::AdaptiveHv => CLASS_H_ADAPTIVE_HV,
        ClassHMode::LowPowerLv => CLASS_H_LOW_POWER_LV,
        ClassHMode::FixedHv => CLASS_H_FIXED_HV,
    }
}

pub const fn hp_output_value(mode: ClassHMode) -> u8 {
    match mode {
        ClassHMode::LowPowerLv => HP_OUTPUT_LV,
        ClassHMode::AdaptiveHv | ClassHMode::FixedHv => HP_OUTPUT_HV,
    }
}

pub const fn pcm_filter_bits(filter: Filter, extras: DacExtras) -> u8 {
    filter.bits() | if extras.high_pass_filter { 0x02 } else { 0 } | if extras.de_emphasis { 0x01 } else { 0 }
}

pub fn pcm_start(profile: PcmProfile, filter: Filter, volume: Volume) -> Program {
    let mut program = Program::new();
    prepare(&mut program);
    for dac in PAIR {
        writes(&mut program, dac, &pcm_timing(profile));
        push(&mut program, Op::Modify { dac, register: reg::PCM_FILTER, keep: FILTER_KEEP, set: filter.bits() });
        writes(&mut program, dac, &PCM_ROUTING);
    }
    volume_writes(&mut program, volume, [VOLUME_LEFT, VOLUME_RIGHT]);
    enable(&mut program, &PCM_POWER_UP);
    for dac in PAIR {
        for (register, value, mask) in pcm_verification(profile, filter) {
            push(&mut program, Op::Expect { dac, register, mask, value });
        }
    }
    program
}

pub fn pcm_start_with_extras(profile: PcmProfile, filter: Filter, extras: DacExtras, volume: Volume) -> Program {
    let mut program = Program::new();
    prepare(&mut program);
    let filter_bits = pcm_filter_bits(filter, extras);
    let routing = [
        (reg::PCM_PATH_1, PCM_PATH_1_SOFT_RAMP),
        (reg::PCM_PATH_2, 0x00),
        (reg::DSD_PATH_1, DSD_PATH_1_MUTED),
        (reg::DSD_PATH_2, DSD_PATH_2_OFF),
        (reg::DSD_PCM_MIX, 0x00),
    ];
    for dac in PAIR {
        writes(&mut program, dac, &pcm_timing(profile));
        push(&mut program, Op::Modify { dac, register: reg::PCM_FILTER, keep: FILTER_KEEP_EXTRAS, set: filter_bits });
        writes(&mut program, dac, &routing);
    }
    volume_writes(&mut program, volume, [VOLUME_LEFT, VOLUME_RIGHT]);
    let class_h = class_h_value(extras.class_h);
    let hp_out = hp_output_value(extras.class_h);
    let enable_ops = [(reg::CLASS_H, class_h), (reg::HP_OUTPUT, hp_out), (reg::INT_MASK_1, 0x11), (reg::SYSTEM_CLOCKING, 0x01)];
    for dac in PAIR {
        writes(&mut program, dac, &enable_ops);
        push(&mut program, Op::Delay { ms: SETTLE_MS });
        writes(&mut program, dac, &PCM_POWER_UP);
    }
    for dac in PAIR {
        for (register, value, mask) in pcm_verification(profile, filter) {
            let (value, mask) = if register == reg::PCM_FILTER { (filter_bits, 0xe3) } else { (value, mask) };
            push(&mut program, Op::Expect { dac, register, mask, value });
        }
        push(&mut program, Op::Expect { dac, register: reg::CLASS_H, mask: 0xff, value: class_h });
        push(&mut program, Op::Expect { dac, register: reg::HP_OUTPUT, mask: 0xff, value: hp_out });
    }
    program
}

pub fn only(program: &Program, dacs: &[Dac]) -> Program {
    program
        .iter()
        .copied()
        .filter(|op| match op {
            Op::Write { dac, .. } | Op::Modify { dac, .. } | Op::Expect { dac, .. } => dacs.contains(dac),
            Op::Delay { .. } => true,
        })
        .collect()
}

pub const fn mask(dacs: &[Dac]) -> u8 {
    let mut bits = 0;
    let mut i = 0;
    while i < dacs.len() {
        bits |= match dacs[i] {
            Dac::Primary => 1,
            Dac::Secondary => 2,
        };
        i += 1;
    }
    bits
}

pub fn answering<B: I2cBus>(bus: &mut B) -> heapless::Vec<Dac, 2> {
    PAIR.iter().copied().filter(|dac| identify(bus, *dac).is_ok()).collect()
}

pub fn pcm_verification(profile: PcmProfile, filter: Filter) -> [(u32, u8, u8); 12] {
    [
        (reg::SP_SAMPLE_RATE, profile.dac_rate_code(), 0xff),
        (reg::SP_BIT_SIZE, 0, 0xff),
        (reg::ASP_NUM_1, profile.asp_n() as u8, 0xff),
        (reg::ASP_NUM_2, (profile.asp_n() >> 8) as u8, 0xff),
        (reg::ASP_DEN_1, profile.asp_m() as u8, 0xff),
        (reg::ASP_DEN_2, (profile.asp_m() >> 8) as u8, 0xff),
        (reg::ASP_LRCK_PERIOD_1, 0x3f, 0xff),
        (reg::ASP_CH1_SIZE, 7, 0xff),
        (reg::ASP_CH2_SIZE, 15, 0xff),
        (reg::PCM_FILTER, filter.bits(), 0xe0),
        (reg::DSD_PATH_2, 0x00, DSD_EN_BIT),
        (reg::POWER_DOWN, 0x01, 0xff),
    ]
}

pub fn dsd_start(volume: Volume) -> Program {
    dsd_start_rate(DsdRate::Dsd64, volume)
}

pub fn dsd_start_rate(rate: DsdRate, volume: Volume) -> Program {
    let mut program = Program::new();
    prepare(&mut program);
    let path = [(reg::DSD_PATH_1, 0xe0), (reg::DSD_INTERFACE, 0x00), (reg::DSD_PATH_2, rate.dac_path2())];
    for dac in PAIR {
        writes(&mut program, dac, &DSD_DISABLE_PCM);
        writes(&mut program, dac, &path);
    }
    volume_writes(&mut program, volume, [DSD_VOLUME_LEFT, DSD_VOLUME_RIGHT]);
    enable(&mut program, &DSD_POWER_UP);
    for dac in PAIR {
        push(&mut program, Op::Expect { dac, register: reg::DSD_PATH_2, mask: 0xff, value: rate.dac_path2() });
        push(&mut program, Op::Expect { dac, register: reg::POWER_DOWN, mask: 0xff, value: 0x01 });
    }
    program
}

pub fn dsd_start_with_extras(rate: DsdRate, extras: DacExtras, volume: Volume) -> Program {
    let mut program = Program::new();
    prepare(&mut program);
    let path = [(reg::DSD_PATH_1, 0xe0), (reg::DSD_INTERFACE, 0x00), (reg::DSD_PATH_2, rate.dac_path2())];
    for dac in PAIR {
        writes(&mut program, dac, &DSD_DISABLE_PCM);
        writes(&mut program, dac, &path);
    }
    volume_writes(&mut program, volume, [DSD_VOLUME_LEFT, DSD_VOLUME_RIGHT]);
    let class_h = class_h_value(extras.class_h);
    let hp_out = hp_output_value(extras.class_h);
    let enable_ops = [(reg::CLASS_H, class_h), (reg::HP_OUTPUT, hp_out), (reg::INT_MASK_1, 0x11), (reg::SYSTEM_CLOCKING, 0x01)];
    for dac in PAIR {
        writes(&mut program, dac, &enable_ops);
        push(&mut program, Op::Delay { ms: SETTLE_MS });
        writes(&mut program, dac, &DSD_POWER_UP);
    }
    for dac in PAIR {
        push(&mut program, Op::Expect { dac, register: reg::DSD_PATH_2, mask: 0xff, value: rate.dac_path2() });
        push(&mut program, Op::Expect { dac, register: reg::CLASS_H, mask: 0xff, value: class_h });
        push(&mut program, Op::Expect { dac, register: reg::HP_OUTPUT, mask: 0xff, value: hp_out });
        push(&mut program, Op::Expect { dac, register: reg::POWER_DOWN, mask: 0xff, value: 0x01 });
    }
    program
}

pub fn power_down(dac: Dac) -> Program {
    let mut program = Program::new();
    push(&mut program, Op::Write { dac, register: reg::HP_LOAD_1, value: 0x00 });
    writes(&mut program, dac, &SHUTDOWN_MUTE);
    push(&mut program, Op::Modify { dac, register: reg::POWER_DOWN, keep: 0xff, set: PDN_HP });
    push(&mut program, Op::Delay { ms: STANDBY_MS });
    push(&mut program, Op::Modify { dac, register: reg::POWER_DOWN, keep: 0xff, set: PDN_DIGITAL });
    push(&mut program, Op::Delay { ms: CLOCK_SWITCH_MS });
    push(&mut program, Op::Write { dac, register: reg::SYSTEM_CLOCKING, value: SYS_CLK_RCO });
    push(&mut program, Op::Delay { ms: CLOCK_SWITCH_MS });
    push(&mut program, Op::Write { dac, register: reg::PLL_1, value: 0x00 });
    push(&mut program, Op::Write { dac, register: reg::POWER_DOWN, value: POWER_DOWN_ALL });
    program
}

pub fn volume(volume: Volume) -> Program {
    let mut program = Program::new();
    volume_writes(&mut program, volume, [VOLUME_LEFT, VOLUME_RIGHT]);
    volume_checks(&mut program, volume, [VOLUME_LEFT, VOLUME_RIGHT]);
    program
}

pub fn volume_fast(volume: Volume) -> Program {
    let mut program = Program::new();
    volume_writes(&mut program, volume, [VOLUME_LEFT, VOLUME_RIGHT]);
    program
}

pub fn dsd_volume(volume: Volume) -> Program {
    let mut program = Program::new();
    volume_writes(&mut program, volume, [DSD_VOLUME_LEFT, DSD_VOLUME_RIGHT]);
    volume_checks(&mut program, volume, [DSD_VOLUME_LEFT, DSD_VOLUME_RIGHT]);
    program
}

pub fn dsd_volume_fast(volume: Volume) -> Program {
    let mut program = Program::new();
    volume_writes(&mut program, volume, [DSD_VOLUME_LEFT, DSD_VOLUME_RIGHT]);
    program
}

pub fn mute() -> Program {
    volume(Volume::MUTED)
}

pub fn filter(filter: Filter) -> Program {
    let mut program = Program::new();
    for dac in PAIR {
        push(&mut program, Op::Expect { dac, register: reg::POWER_DOWN, mask: PDN_HP, value: PDN_HP });
    }
    for dac in PAIR {
        push(&mut program, Op::Modify { dac, register: reg::PCM_FILTER, keep: FILTER_KEEP, set: filter.bits() });
    }
    program
}

pub fn standby() -> Program {
    let mut program = Program::new();
    for dac in PAIR {
        push(&mut program, Op::Modify { dac, register: reg::INT_MASK_1, keep: 0xfe, set: 0 });
        push(&mut program, Op::Modify { dac, register: reg::POWER_DOWN, keep: 0xff, set: PDN_HP });
        push(&mut program, Op::Delay { ms: STANDBY_MS });
        push(&mut program, Op::Modify { dac, register: reg::POWER_DOWN, keep: 0xff, set: PDN_ASP });
    }
    program
}

pub fn resume() -> Program {
    let mut program = Program::new();
    for dac in PAIR {
        push(&mut program, Op::Write { dac, register: reg::DXD1, value: 0x99 });
        push(&mut program, Op::Write { dac, register: reg::DXD13, value: 0x20 });
        push(&mut program, Op::Modify { dac, register: reg::POWER_DOWN, keep: !PDN_ASP, set: 0 });
        push(&mut program, Op::Modify { dac, register: reg::POWER_DOWN, keep: !PDN_HP, set: 0 });
        push(&mut program, Op::Delay { ms: RESUME_MS });
        push(&mut program, Op::Write { dac, register: reg::DXD1, value: 0 });
        push(&mut program, Op::Write { dac, register: reg::DXD13, value: 0 });
    }
    program
}

pub const fn register_prefix(register: u32) -> [u8; 4] {
    [(register >> 16) as u8, (register >> 8) as u8, register as u8, REGISTER_PAD]
}

pub const fn register_write(register: u32, value: u8) -> [u8; 5] {
    let p = register_prefix(register);
    [p[0], p[1], p[2], p[3], value]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DacError {
    Bus { dac: Dac, register: u32, error: I2cError },
    Readback { dac: Dac, register: u32, expected: u8, actual: u8 },
    Identity { dac: Dac, bytes: [u8; 3] },
    IdentityChanged { dac: Dac, bytes: [u8; 3] },
    NegativeControlAcknowledged,
    NegativeControl(I2cError),
    TimeWentBackwards,
    Stopped,
}

pub fn read<B: I2cBus>(bus: &mut B, dac: Dac, register: u32) -> Result<u8, DacError> {
    let mut byte = [0];
    bus.write_then_read(dac.address(), &register_prefix(register), &mut byte).map_err(|error| DacError::Bus {
        dac,
        register,
        error,
    })?;
    Ok(byte[0])
}

pub fn write<B: I2cBus>(bus: &mut B, dac: Dac, register: u32, value: u8) -> Result<(), DacError> {
    bus.write(dac.address(), &register_write(register, value)).map_err(|error| DacError::Bus { dac, register, error })
}

fn check(dac: Dac, register: u32, mask: u8, expected: u8, actual: u8) -> Result<(), DacError> {
    if actual & mask == expected { Ok(()) } else { Err(DacError::Readback { dac, register, expected, actual: actual & mask }) }
}

pub fn run_blocking<B: I2cBus>(bus: &mut B, program: &[Op], mut delay_ms: impl FnMut(u16)) -> Result<(), DacError> {
    for op in program {
        match *op {
            Op::Write { dac, register, value } => write(bus, dac, register, value)?,
            Op::Modify { dac, register, keep, set } => {
                let current = read(bus, dac, register)?;
                write(bus, dac, register, (current & keep) | set)?;
            }
            Op::Expect { dac, register, mask, value } => check(dac, register, mask, value, read(bus, dac, register)?)?,
            Op::Delay { ms } => delay_ms(ms),
        }
    }
    Ok(())
}

pub fn identify<B: I2cBus>(bus: &mut B, dac: Dac) -> Result<[u8; 3], DacError> {
    let mut bytes = [0; 3];
    for (byte, register) in bytes.iter_mut().zip([reg::DEVICE_ID_AB, reg::DEVICE_ID_CD, reg::DEVICE_ID_E]) {
        *byte = read(bus, dac, register)?;
    }
    if bytes[..2] != IDENTITY || bytes[2] & 0xf0 != IDENTITY_E_HIGH {
        return Err(DacError::Identity { dac, bytes });
    }
    Ok(bytes)
}

pub fn validate_pair<B: I2cBus>(bus: &mut B) -> Result<[[u8; 3]; 2], DacError> {
    let mut identities = [[0; 3]; 2];
    for (identity, dac) in identities.iter_mut().zip(PAIR) {
        *identity = identify(bus, dac)?;
    }
    for _ in 0..2 {
        match bus.probe(NEGATIVE_CONTROL_ADDRESS) {
            Err(I2cError::Nack) => {}
            Ok(()) => return Err(DacError::NegativeControlAcknowledged),
            Err(error) => return Err(DacError::NegativeControl(error)),
        }
    }
    for (identity, dac) in identities.iter().zip(PAIR) {
        let bytes = identify(bus, dac)?;
        if bytes != *identity {
            return Err(DacError::IdentityChanged { dac, bytes });
        }
    }
    Ok(identities)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Progress {
    Pending { wake_us: Option<u64> },
    Done,
}

pub struct Runner {
    program: Program,
    index: usize,
    wake_us: Option<u64>,
    read_back: Option<u8>,
    last_us: Option<u64>,
    stopped: bool,
}

impl Runner {
    pub fn new(program: Program) -> Self {
        Self { program, index: 0, wake_us: None, read_back: None, last_us: None, stopped: false }
    }

    pub fn position(&self) -> usize {
        self.index
    }

    pub fn len(&self) -> usize {
        self.program.len()
    }

    pub fn is_empty(&self) -> bool {
        self.program.is_empty()
    }

    pub fn abort(&mut self) {
        self.stopped = true;
    }

    pub fn poll<B: I2cBus>(&mut self, bus: &mut B, now_us: u64) -> Result<Progress, DacError> {
        if self.stopped {
            return Err(DacError::Stopped);
        }
        if self.index >= self.program.len() {
            return Ok(Progress::Done);
        }
        if self.last_us.is_some_and(|last| now_us < last) {
            self.stopped = true;
            return Err(DacError::TimeWentBackwards);
        }
        self.last_us = Some(now_us);
        let result = self.step(bus, now_us);
        if result.is_err() {
            self.stopped = true;
        }
        result
    }

    fn step<B: I2cBus>(&mut self, bus: &mut B, now_us: u64) -> Result<Progress, DacError> {
        match self.program[self.index] {
            Op::Delay { ms } => {
                let wake = *self.wake_us.get_or_insert(now_us.saturating_add(u64::from(ms) * 1_000));
                if now_us < wake {
                    return Ok(Progress::Pending { wake_us: Some(wake) });
                }
                self.wake_us = None;
            }
            Op::Write { dac, register, value } => write(bus, dac, register, value)?,
            Op::Modify { dac, register, keep, set } => match self.read_back.take() {
                None => {
                    self.read_back = Some(read(bus, dac, register)?);
                    return Ok(Progress::Pending { wake_us: None });
                }
                Some(current) => write(bus, dac, register, (current & keep) | set)?,
            },
            Op::Expect { dac, register, mask, value } => check(dac, register, mask, value, read(bus, dac, register)?)?,
        }
        self.index += 1;
        Ok(if self.index >= self.program.len() { Progress::Done } else { Progress::Pending { wake_us: None } })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcm::PROFILES;
    use std::collections::{BTreeMap, BTreeSet};
    use std::vec::Vec as StdVec;

    #[test]
    fn a_start_program_can_skip_a_dac_that_does_not_answer() {
        let full = pcm_start(PcmProfile::from_hz(48_000).unwrap(), Filter::FastLowLatency, Volume::MUTED);
        let primary = only(&full, &[Dac::Primary]);
        let touches = |program: &Program, wanted: Dac| {
            program.iter().any(|op| match op {
                Op::Write { dac, .. } | Op::Modify { dac, .. } | Op::Expect { dac, .. } => *dac == wanted,
                Op::Delay { .. } => false,
            })
        };
        assert!(touches(&full, Dac::Secondary));
        assert!(!touches(&primary, Dac::Secondary));
        assert!(touches(&primary, Dac::Primary));
        let delays = |program: &Program| program.iter().filter(|op| matches!(op, Op::Delay { .. })).count();
        assert_eq!(delays(&primary), delays(&full));
        assert_eq!(only(&full, &PAIR), full);
        assert_eq!((mask(&[Dac::Primary]), mask(&PAIR), mask(&[])), (1, 3, 0));
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Event {
        W(u8, u32, u8),
        R(u8, u32),
        D(u16),
    }
    use Event::{D, W};

    const STOCK_PCM_44K1: [Event; 90] = [
        W(0x33, 0x20052, 0x04),
        W(0x33, 0xf0010, 0xe7),
        W(0x33, 0x20000, 0xf2),
        W(0x33, 0x40002, 0x03),
        W(0x33, 0x30005, 0x40),
        W(0x33, 0x30002, 0x00),
        W(0x33, 0x30003, 0x00),
        W(0x33, 0x30004, 0x00),
        W(0x33, 0x3001b, 0x01),
        W(0x33, 0x30008, 0x08),
        W(0x33, 0x3000a, 0x83),
        W(0x33, 0xf0010, 0xe1),
        W(0x33, 0x30001, 0x01),
        W(0x33, 0x10006, 0x00),
        D(15),
        W(0x31, 0x20052, 0x04),
        W(0x31, 0xf0010, 0xe7),
        W(0x31, 0x20000, 0xf2),
        W(0x31, 0x40002, 0x03),
        W(0x31, 0x30005, 0x40),
        W(0x31, 0x30002, 0x00),
        W(0x31, 0x30003, 0x00),
        W(0x31, 0x30004, 0x00),
        W(0x31, 0x3001b, 0x01),
        W(0x31, 0x30008, 0x08),
        W(0x31, 0x3000a, 0x83),
        W(0x31, 0xf0010, 0xe1),
        W(0x31, 0x30001, 0x01),
        W(0x31, 0x10006, 0x00),
        D(15),
        W(0x33, 0x1000b, 0x01),
        W(0x33, 0x1000c, 0x00),
        W(0x33, 0x40010, 0x93),
        W(0x33, 0x40011, 0x00),
        W(0x33, 0x40012, 0x00),
        W(0x33, 0x40013, 0x05),
        W(0x33, 0x40014, 0x1f),
        W(0x33, 0x40015, 0x00),
        W(0x33, 0x40016, 0x3f),
        W(0x33, 0x40017, 0x00),
        W(0x33, 0x40018, 0x0c),
        W(0x33, 0x40019, 0x0a),
        W(0x33, 0x50000, 0x00),
        W(0x33, 0x50001, 0x00),
        W(0x33, 0x5000a, 0x07),
        W(0x33, 0x5000b, 0x0f),
        W(0x33, 0x90000, 0x00),
        W(0x33, 0x90003, 0xa0),
        W(0x33, 0x90004, 0x00),
        W(0x33, 0x70002, 0xab),
        W(0x33, 0x70004, 0x02),
        W(0x33, 0x70005, 0x00),
        W(0x31, 0x1000b, 0x01),
        W(0x31, 0x1000c, 0x00),
        W(0x31, 0x40010, 0x93),
        W(0x31, 0x40011, 0x00),
        W(0x31, 0x40012, 0x00),
        W(0x31, 0x40013, 0x05),
        W(0x31, 0x40014, 0x1f),
        W(0x31, 0x40015, 0x00),
        W(0x31, 0x40016, 0x3f),
        W(0x31, 0x40017, 0x00),
        W(0x31, 0x40018, 0x0c),
        W(0x31, 0x40019, 0x0a),
        W(0x31, 0x50000, 0x00),
        W(0x31, 0x50001, 0x00),
        W(0x31, 0x5000a, 0x07),
        W(0x31, 0x5000b, 0x0f),
        W(0x31, 0x90000, 0x00),
        W(0x31, 0x90003, 0xa0),
        W(0x31, 0x90004, 0x00),
        W(0x31, 0x70002, 0xab),
        W(0x31, 0x70004, 0x02),
        W(0x31, 0x70005, 0x00),
        W(0x33, 0x90001, 0x00),
        W(0x33, 0x90002, 0x00),
        W(0x33, 0xb0000, 0x1e),
        W(0x33, 0x80000, 0x31),
        W(0x33, 0xf0010, 0x11),
        W(0x33, 0x10006, 0x01),
        D(15),
        W(0x33, 0xf0011, 0x07),
        W(0x33, 0x20000, 0x01),
        W(0x31, 0xb0000, 0x1e),
        W(0x31, 0x80000, 0x31),
        W(0x31, 0xf0010, 0x11),
        W(0x31, 0x10006, 0x01),
        D(15),
        W(0x31, 0xf0011, 0x07),
        W(0x31, 0x20000, 0x01),
    ];

    const STOCK_DSD: [Event; 54] = [
        W(0x33, 0x20052, 0x04),
        W(0x33, 0xf0010, 0xe7),
        W(0x33, 0x20000, 0xf2),
        W(0x33, 0x40002, 0x03),
        W(0x33, 0x30005, 0x40),
        W(0x33, 0x30002, 0x00),
        W(0x33, 0x30003, 0x00),
        W(0x33, 0x30004, 0x00),
        W(0x33, 0x3001b, 0x01),
        W(0x33, 0x30008, 0x08),
        W(0x33, 0x3000a, 0x83),
        W(0x33, 0xf0010, 0xe1),
        W(0x33, 0x30001, 0x01),
        W(0x33, 0x10006, 0x00),
        D(15),
        W(0x31, 0x20052, 0x04),
        W(0x31, 0xf0010, 0xe7),
        W(0x31, 0x20000, 0xf2),
        W(0x31, 0x40002, 0x03),
        W(0x31, 0x30005, 0x40),
        W(0x31, 0x30002, 0x00),
        W(0x31, 0x30003, 0x00),
        W(0x31, 0x30004, 0x00),
        W(0x31, 0x3001b, 0x01),
        W(0x31, 0x30008, 0x08),
        W(0x31, 0x3000a, 0x83),
        W(0x31, 0xf0010, 0xe1),
        W(0x31, 0x30001, 0x01),
        W(0x31, 0x10006, 0x00),
        D(15),
        W(0x33, 0x90003, 0xa3),
        W(0x33, 0x70005, 0x00),
        W(0x33, 0x70002, 0xe0),
        W(0x33, 0x70003, 0x00),
        W(0x33, 0x70004, 0x13),
        W(0x31, 0x90003, 0xa3),
        W(0x31, 0x70005, 0x00),
        W(0x31, 0x70002, 0xe0),
        W(0x31, 0x70003, 0x00),
        W(0x31, 0x70004, 0x13),
        W(0x33, 0x70000, 0x00),
        W(0x33, 0x70001, 0x00),
        W(0x33, 0xb0000, 0x1e),
        W(0x33, 0x80000, 0x31),
        W(0x33, 0xf0010, 0x11),
        W(0x33, 0x10006, 0x01),
        D(15),
        W(0x33, 0x20000, 0x01),
        W(0x31, 0xb0000, 0x1e),
        W(0x31, 0x80000, 0x31),
        W(0x31, 0xf0010, 0x11),
        W(0x31, 0x10006, 0x01),
        D(15),
        W(0x31, 0x20000, 0x01),
    ];

    fn simulate(program: &[Op]) -> StdVec<Event> {
        let mut registers = BTreeMap::new();
        let mut events = StdVec::new();
        for op in program {
            match *op {
                Op::Write { dac, register, value } => {
                    registers.insert((dac.address(), register), value);
                    events.push(W(dac.address(), register, value));
                }
                Op::Modify { dac, register, keep, set } => {
                    let value = (registers.get(&(dac.address(), register)).copied().unwrap_or(0) & keep) | set;
                    registers.insert((dac.address(), register), value);
                    events.push(W(dac.address(), register, value));
                }
                Op::Expect { .. } => {}
                Op::Delay { ms } => events.push(D(ms)),
            }
        }
        events
    }

    fn with_rate(trace: &[Event], code: u8, n: u16, m: u16) -> StdVec<Event> {
        trace
            .iter()
            .map(|event| match *event {
                W(a, 0x1000b, _) => W(a, 0x1000b, code),
                W(a, 0x40010, _) => W(a, 0x40010, n as u8),
                W(a, 0x40011, _) => W(a, 0x40011, (n >> 8) as u8),
                W(a, 0x40012, _) => W(a, 0x40012, m as u8),
                W(a, 0x40013, _) => W(a, 0x40013, (m >> 8) as u8),
                other => other,
            })
            .collect()
    }

    #[test]
    fn pcm_start_matches_the_stock_trace_at_every_rate() {
        let p44 = PcmProfile::from_hz(44_100).unwrap();
        let program = pcm_start(p44, Filter::FastLowLatency, Volume::UNITY);
        assert_eq!(program.len(), 114);
        assert_eq!(simulate(&program), STOCK_PCM_44K1);
        assert_eq!(
            simulate(&pcm_start(PcmProfile::from_hz(48_000).unwrap(), Filter::FastLowLatency, Volume::UNITY)),
            with_rate(&STOCK_PCM_44K1, 2, 1, 8)
        );
        assert_eq!(
            simulate(&pcm_start(PcmProfile::from_hz(192_000).unwrap(), Filter::FastLowLatency, Volume::UNITY)),
            with_rate(&STOCK_PCM_44K1, 6, 1, 2)
        );
        assert_eq!(
            simulate(&pcm_start(PcmProfile::from_hz(384_000).unwrap(), Filter::FastLowLatency, Volume::UNITY)),
            with_rate(&STOCK_PCM_44K1, 8, 1, 1)
        );
        for p in PROFILES {
            assert_eq!(
                simulate(&pcm_start(p, Filter::FastLowLatency, Volume::UNITY)),
                with_rate(&STOCK_PCM_44K1, p.dac_rate_code(), p.asp_n(), p.asp_m())
            );
        }
    }

    #[test]
    fn dsd_start_matches_the_stock_trace() {
        let program = dsd_start(Volume::UNITY);
        assert_eq!(simulate(&program), STOCK_DSD);
        assert_eq!(program.iter().filter(|op| matches!(op, Op::Expect { .. })).count(), 4);
        assert!(!simulate(&program).iter().any(|e| matches!(e, W(_, 0xf0011, _) | W(_, 0x1000b, _))));
        for (rate, code) in [(DsdRate::Dsd64, 0x13), (DsdRate::Dsd128, 0x17), (DsdRate::Dsd256, 0x1b)] {
            let p = dsd_start_rate(rate, Volume::UNITY);
            let trace = simulate(&p);
            assert!(trace.contains(&W(0x33, 0x70004, code)));
            assert!(trace.contains(&W(0x31, 0x70004, code)));
        }
    }

    #[test]
    fn filter_bits_follow_the_protocol_and_keep_the_low_bits() {
        let expected = [
            (DacFilter::FastLowLatency, 0x00),
            (DacFilter::FastPhaseCompensated, 0x40),
            (DacFilter::SlowLowLatency, 0x80),
            (DacFilter::SlowPhaseCompensated, 0xc0),
            (DacFilter::NonOversampling, 0x20),
        ];
        for (protocol, bits) in expected {
            assert_eq!(Filter::from(protocol).bits(), bits);
        }
        let mut bus = MockBus::default();
        bus.registers.insert((0x33, reg::POWER_DOWN), 0x13);
        bus.registers.insert((0x31, reg::POWER_DOWN), 0x10);
        bus.registers.insert((0x33, reg::PCM_FILTER), 0xe3);
        bus.registers.insert((0x31, reg::PCM_FILTER), 0x42);
        run_blocking(&mut bus, &filter(Filter::SlowLowLatency), |_| {}).unwrap();
        assert_eq!(bus.registers[&(0x33, reg::PCM_FILTER)], 0x83);
        assert_eq!(bus.registers[&(0x31, reg::PCM_FILTER)], 0x82);
        bus.registers.insert((0x31, reg::POWER_DOWN), 0x01);
        let before = bus.writes();
        assert_eq!(
            run_blocking(&mut bus, &filter(Filter::NonOversampling), |_| {}),
            Err(DacError::Readback { dac: Dac::Secondary, register: reg::POWER_DOWN, expected: PDN_HP, actual: 0 })
        );
        assert_eq!(bus.writes(), before);
    }

    #[test]
    fn volume_follows_host_level_limit_balance_and_mute() {
        assert_eq!(attenuation(0), 0);
        assert_eq!(attenuation(100), 0);
        assert_eq!(attenuation(-50), 1);
        assert_eq!(attenuation(-51), 2);
        assert_eq!(attenuation(-6_000), 120);
        assert_eq!(attenuation(-12_700), 254);
        assert_eq!(attenuation(i32::MIN), 254);
        assert_eq!(stock_attenuation(119, false), 119);
        assert_eq!(stock_attenuation(120, false), MUTE);
        assert_eq!(stock_attenuation(3, true), MUTE);
        let volume = Volume::from_settings([-1_200, -600], false, -1_000, 0);
        assert_eq!(volume, Volume { left: 24, right: 20, muted: false });
        assert_eq!(Volume::from_settings([0, 0], false, 0, 300), Volume { left: 6, right: 0, muted: false });
        assert_eq!(Volume::from_settings([0, 0], false, 0, -300), Volume { left: 0, right: 6, muted: false });
        assert_eq!(Volume::from_settings([-6_000, -6_000], false, 0, 600).registers(), [132, 120]);
        assert_eq!(Volume::from_settings([0, 0], true, 0, 0).registers(), [MUTE, MUTE]);
        let mut bus = MockBus::default();
        run_blocking(&mut bus, &volume_program(Volume { left: 12, right: 14, muted: false }), |_| {}).unwrap();
        assert_eq!(bus.writes(), [(0x33, VOLUME_LEFT, 12), (0x33, VOLUME_RIGHT, 14)]);
        run_blocking(&mut bus, &mute(), |_| {}).unwrap();
        assert_eq!(bus.registers[&(0x33, VOLUME_LEFT)], MUTE);
        bus.stuck.insert((0x33, VOLUME_RIGHT));
        assert_eq!(
            run_blocking(&mut bus, &volume_program(Volume::UNITY), |_| {}),
            Err(DacError::Readback { dac: Dac::Primary, register: VOLUME_RIGHT, expected: 0, actual: MUTE })
        );
        assert!(!bus.registers.contains_key(&(0x31, VOLUME_LEFT)));
        let dsd = dsd_volume(Volume { left: 2, right: 4, muted: false });
        assert_eq!(dsd[0], Op::Write { dac: Dac::Primary, register: reg::DSD_VOLUME_B, value: 2 });
        assert_eq!(dsd[1], Op::Write { dac: Dac::Primary, register: reg::DSD_VOLUME_A, value: 4 });
    }

    fn volume_program(v: Volume) -> Program {
        volume(v)
    }

    #[test]
    fn standby_and_resume_touch_only_their_bits() {
        let mut bus = MockBus::default();
        for dac in PAIR {
            bus.registers.insert((dac.address(), reg::POWER_DOWN), 0x83);
            bus.registers.insert((dac.address(), reg::INT_MASK_1), 0xa5);
        }
        let mut delays = StdVec::new();
        run_blocking(&mut bus, &standby(), |ms| delays.push(ms)).unwrap();
        assert_eq!(bus.registers[&(0x33, reg::POWER_DOWN)], 0xd3);
        assert_eq!(bus.registers[&(0x33, reg::INT_MASK_1)], 0xa4);
        run_blocking(&mut bus, &resume(), |ms| delays.push(ms)).unwrap();
        assert_eq!(bus.registers[&(0x31, reg::POWER_DOWN)], 0x83);
        assert_eq!(bus.registers[&(0x31, reg::DXD1)], 0);
        assert_eq!(bus.registers[&(0x31, reg::DXD13)], 0);
        assert_eq!(delays, [10, 10, 12, 12]);
    }

    #[test]
    fn frames_carry_a_24_bit_address_and_the_stock_pad_byte() {
        assert_eq!(register_prefix(reg::PCM_VOLUME_B), [0x09, 0x00, 0x01, 0x01]);
        assert_eq!(register_write(reg::SP_SAMPLE_RATE, 6), [0x01, 0x00, 0x0b, 0x01, 6]);
    }

    #[derive(Default)]
    struct MockBus {
        registers: BTreeMap<(u8, u32), u8>,
        events: StdVec<Event>,
        acknowledge: BTreeSet<u8>,
        stuck: BTreeSet<(u8, u32)>,
        fail_at: Option<usize>,
        transactions: usize,
        identity: Option<[u8; 3]>,
        reads: usize,
        change_identity_after: Option<usize>,
    }

    impl MockBus {
        fn transaction(&mut self) -> Result<(), I2cError> {
            let index = self.transactions;
            self.transactions += 1;
            if self.fail_at == Some(index) { Err(I2cError::Nack) } else { Ok(()) }
        }

        fn writes(&self) -> StdVec<(u8, u32, u8)> {
            self.events
                .iter()
                .filter_map(|e| match e {
                    W(a, r, v) => Some((*a, *r, *v)),
                    _ => None,
                })
                .collect()
        }
    }

    fn decode(bytes: &[u8]) -> u32 {
        assert_eq!(bytes[3], REGISTER_PAD);
        (u32::from(bytes[0]) << 16) | (u32::from(bytes[1]) << 8) | u32::from(bytes[2])
    }

    impl I2cBus for MockBus {
        fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), I2cError> {
            assert_eq!(bytes.len(), 5);
            self.transaction()?;
            let register = decode(bytes);
            self.events.push(W(address, register, bytes[4]));
            if !self.stuck.contains(&(address, register)) {
                self.registers.insert((address, register), bytes[4]);
            }
            Ok(())
        }

        fn write_then_read(&mut self, address: u8, prefix: &[u8], buffer: &mut [u8]) -> Result<(), I2cError> {
            self.transaction()?;
            let register = decode(prefix);
            self.events.push(Event::R(address, register));
            self.reads += 1;
            let mut identity = self.identity.unwrap_or([0x43, 0x19, 0x8a]);
            if self.change_identity_after.is_some_and(|n| self.reads > n) {
                identity[2] = 0x8b;
            }
            buffer[0] = match register {
                reg::DEVICE_ID_AB..=reg::DEVICE_ID_E => identity[(register - reg::DEVICE_ID_AB) as usize],
                _ => self.registers.get(&(address, register)).copied().unwrap_or(0),
            };
            Ok(())
        }

        fn probe(&mut self, address: u8) -> Result<(), I2cError> {
            self.transaction()?;
            if self.acknowledge.contains(&address) { Ok(()) } else { Err(I2cError::Nack) }
        }
    }

    fn run(runner: &mut Runner, bus: &mut MockBus, now: &mut u64) -> Result<StdVec<u64>, DacError> {
        let mut waits = StdVec::new();
        for _ in 0..1_000 {
            let before = bus.transactions;
            let progress = runner.poll(bus, *now);
            assert!(bus.transactions - before <= 1);
            match progress? {
                Progress::Done => return Ok(waits),
                Progress::Pending { wake_us: None } => *now += 40,
                Progress::Pending { wake_us: Some(wake) } => {
                    waits.push(wake - *now);
                    let count = bus.transactions;
                    assert_eq!(runner.poll(bus, wake - 1), Ok(Progress::Pending { wake_us: Some(wake) }));
                    assert_eq!(bus.transactions, count);
                    *now = wake;
                }
            }
        }
        panic!("runner did not finish");
    }

    #[test]
    fn the_runner_does_one_transaction_per_poll_and_matches_the_blocking_run() {
        for p in PROFILES {
            for filter in [Filter::FastLowLatency, Filter::SlowPhaseCompensated, Filter::NonOversampling] {
                let program = pcm_start(p, filter, Volume { left: 12, right: 14, muted: false });
                let mut blocking = MockBus::default();
                blocking.registers.insert((0x33, reg::PCM_FILTER), 0x03);
                let mut stepped = MockBus { registers: blocking.registers.clone(), ..MockBus::default() };
                let mut delays = StdVec::new();
                run_blocking(&mut blocking, &program, |ms| delays.push(ms)).unwrap();
                let mut runner = Runner::new(program);
                let mut now = 0;
                let waits = run(&mut runner, &mut stepped, &mut now).unwrap();
                assert_eq!(blocking.events, stepped.events);
                assert_eq!(waits, [15_000; 4]);
                assert_eq!(delays, [15; 4]);
                assert_eq!(stepped.registers[&(0x33, reg::PCM_FILTER)], filter.bits() | 0x03);
                assert_eq!(runner.poll(&mut stepped, now), Ok(Progress::Done));
            }
        }
    }

    #[test]
    fn every_failure_stops_the_runner_at_the_same_prefix() {
        let program = pcm_start(PcmProfile::from_hz(48_000).unwrap(), Filter::NonOversampling, Volume::UNITY);
        let transactions = program
            .iter()
            .map(|op| if matches!(op, Op::Modify { .. }) { 2 } else { usize::from(!matches!(op, Op::Delay { .. })) })
            .sum::<usize>();
        assert_eq!(transactions, 112);
        for fail_at in 0..transactions {
            let mut blocking = MockBus { fail_at: Some(fail_at), ..MockBus::default() };
            let mut stepped = MockBus { fail_at: Some(fail_at), ..MockBus::default() };
            let expected = run_blocking(&mut blocking, &program, |_| {});
            assert!(matches!(expected, Err(DacError::Bus { error: I2cError::Nack, .. })));
            let mut runner = Runner::new(program.clone());
            let mut now = 0;
            assert_eq!(run(&mut runner, &mut stepped, &mut now).map(|_| ()), expected);
            assert_eq!(blocking.events, stepped.events);
            let count = stepped.transactions;
            assert_eq!(runner.poll(&mut stepped, now), Err(DacError::Stopped));
            assert_eq!(stepped.transactions, count);
        }
    }

    #[test]
    fn readback_mismatch_and_backwards_time_are_terminal() {
        let program = pcm_start(PcmProfile::from_hz(48_000).unwrap(), Filter::NonOversampling, Volume::UNITY);
        let mut bus = MockBus::default();
        bus.stuck.insert((0x31, reg::ASP_DEN_1));
        let mut runner = Runner::new(program.clone());
        let mut now = 0;
        assert_eq!(
            run(&mut runner, &mut bus, &mut now).map(|_| ()),
            Err(DacError::Readback { dac: Dac::Secondary, register: reg::ASP_DEN_1, expected: 8, actual: 0 })
        );
        let mut runner = Runner::new(program);
        let mut bus = MockBus::default();
        runner.poll(&mut bus, 100).unwrap();
        let count = bus.transactions;
        assert_eq!(runner.poll(&mut bus, 99), Err(DacError::TimeWentBackwards));
        assert_eq!(bus.transactions, count);
        assert_eq!(runner.poll(&mut bus, 101), Err(DacError::Stopped));
    }

    #[test]
    fn the_pair_is_identified_and_the_negative_control_must_not_answer() {
        let mut bus = MockBus::default();
        assert_eq!(validate_pair(&mut bus), Ok([[0x43, 0x19, 0x8a]; 2]));
        assert_eq!(bus.events.iter().filter(|e| matches!(e, Event::R(..))).count(), 12);
        let mut bus = MockBus { acknowledge: [0x32].into_iter().collect(), ..MockBus::default() };
        assert_eq!(validate_pair(&mut bus), Err(DacError::NegativeControlAcknowledged));
        let mut bus = MockBus { identity: Some([0x43, 0x13, 0x10]), ..MockBus::default() };
        assert_eq!(validate_pair(&mut bus), Err(DacError::Identity { dac: Dac::Primary, bytes: [0x43, 0x13, 0x10] }));
        let mut bus = MockBus { change_identity_after: Some(7), ..MockBus::default() };
        assert_eq!(validate_pair(&mut bus), Err(DacError::IdentityChanged { dac: Dac::Primary, bytes: [0x43, 0x19, 0x8b] }));
        let mut bus = MockBus { fail_at: Some(0), ..MockBus::default() };
        assert_eq!(
            validate_pair(&mut bus),
            Err(DacError::Bus { dac: Dac::Primary, register: reg::DEVICE_ID_AB, error: I2cError::Nack })
        );
    }

    #[test]
    fn hpload_ohms_decodes_and_measures_both_channels() {
        assert_eq!(decode_hpload_ohms(0x00, 0x01), 32);
        assert_eq!(decode_hpload_ohms(0x80, 0x00), 16);
        assert_eq!(decode_hpload_ohms(0xf8, 0x1f), 1023);
        let mut bus = MockBus::default();
        bus.registers.insert((0x33, reg::INT_STATUS_4), HPLOAD_ON_INT | HPLOAD_DC_INT | HPLOAD_AC_INT | HPLOAD_OFF_INT);
        bus.registers.insert((0x33, reg::HP_DC_STAT_1), 0x00);
        bus.registers.insert((0x33, reg::HP_DC_STAT_2), 0x01);
        bus.registers.insert((0x33, reg::HP_AC_STAT_1), 0x20);
        bus.registers.insert((0x33, reg::HP_AC_STAT_2), 0x01);
        bus.registers.insert((0x33, reg::HP_STATUS), 0x02);
        bus.registers.insert((0x33, reg::HP_LOAD_STAT), 0x01);
        let reading = measure_dc_impedance(&mut bus, Dac::Primary, |_| {}).unwrap();
        assert_eq!(
            reading,
            ImpedanceReading {
                left_ohms: Some(32),
                right_ohms: Some(32),
                left_ac_ohms: Some(36),
                right_ac_ohms: Some(36),
                left_status: HPLOAD_ON_INT | HPLOAD_DC_INT | HPLOAD_AC_INT | HPLOAD_OFF_INT,
                right_status: HPLOAD_ON_INT | HPLOAD_DC_INT | HPLOAD_AC_INT | HPLOAD_OFF_INT,
                hp_status: 0x02,
                load_stat: 0x01,
            }
        );
        assert_eq!(bus.registers[&(0x33, reg::INT_MASK_4)], 0xff);
        assert_eq!(bus.registers[&(0x33, reg::HP_MEAS_LOAD_1)], 0x00);
        assert_eq!(bus.registers[&(0x33, reg::HP_MEAS_LOAD_2)], 0x00);
    }

    #[test]
    fn pcm_start_clears_stuck_dsd_path_and_power_down_sleeps_dac() {
        let mut bus = MockBus::default();
        run_blocking(&mut bus, &only(&dsd_start_rate(DsdRate::Dsd128, Volume::MUTED), &[Dac::Primary]), |_| {}).unwrap();
        assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_2)], 0x17);
        assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_1)], 0xe0);
        run_blocking(
            &mut bus,
            &only(&pcm_start(PcmProfile::from_hz(48_000).unwrap(), Filter::FastLowLatency, Volume::UNITY), &[Dac::Primary]),
            |_| {},
        )
        .unwrap();
        assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_2)], DSD_PATH_2_OFF);
        assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_1)], DSD_PATH_1_MUTED);
        assert_eq!(bus.registers[&(0x33, reg::PCM_PATH_1)], 0xa0);
        assert_eq!(bus.registers[&(0x33, reg::POWER_DOWN)], 0x01);
        assert_eq!(bus.registers[&(0x33, reg::SYSTEM_CLOCKING)], 0x01);
        let before_pdn = bus.events.len();
        let mut pdn_delays = StdVec::new();
        run_blocking(&mut bus, &power_down(Dac::Primary), |ms| pdn_delays.push(ms)).unwrap();
        let pdn_events = &bus.events[before_pdn..];
        let rco_pos = pdn_events.iter().position(|e| *e == W(0x33, reg::SYSTEM_CLOCKING, SYS_CLK_RCO)).unwrap();
        let all_off_pos = pdn_events.iter().position(|e| *e == W(0x33, reg::POWER_DOWN, POWER_DOWN_ALL)).unwrap();
        let pll_off_pos = pdn_events.iter().position(|e| *e == W(0x33, reg::PLL_1, 0x00)).unwrap();
        assert!(rco_pos < pll_off_pos && pll_off_pos < all_off_pos);
        assert_eq!(pdn_delays, [STANDBY_MS, CLOCK_SWITCH_MS, CLOCK_SWITCH_MS]);
        assert_eq!(bus.registers[&(0x33, reg::SYSTEM_CLOCKING)], SYS_CLK_RCO);
        assert_eq!(bus.registers[&(0x33, reg::PLL_1)], 0x00);
        assert_eq!(bus.registers[&(0x33, reg::POWER_DOWN)], POWER_DOWN_ALL);
        assert_eq!(bus.registers[&(0x33, reg::PCM_PATH_1)], PCM_PATH_1_MUTED);
        assert_eq!(bus.registers[&(0x33, reg::DSD_PATH_2)], DSD_PATH_2_OFF);
        let mut already_off = MockBus::default();
        already_off.registers.insert((0x33, reg::POWER_DOWN), POWER_DOWN_ALL);
        already_off.registers.insert((0x33, reg::SYSTEM_CLOCKING), SYS_CLK_RCO);
        run_blocking(&mut already_off, &power_down(Dac::Primary), |_| {}).unwrap();
        assert!(already_off.events.iter().all(|e| !matches!(*e, W(0x33, reg::POWER_DOWN, v) if v != POWER_DOWN_ALL)));
    }

    #[test]
    fn pcm_start_with_extras_programs_soft_ramp_hpf_deemp_and_class_h() {
        let extras = DacExtras {
            high_pass_filter: false,
            de_emphasis: true,
            line_out: false,
            class_h: ClassHMode::LowPowerLv,
            auto_uac_on_jack: true,
            stay_awake_while_active: false,
        };
        let mut bus = MockBus::default();
        bus.registers.insert((0x33, reg::PCM_FILTER), 0x0a);
        let program = only(
            &pcm_start_with_extras(PcmProfile::from_hz(48_000).unwrap(), Filter::SlowPhaseCompensated, extras, Volume::UNITY),
            &[Dac::Primary],
        );
        run_blocking(&mut bus, &program, |_| {}).unwrap();
        assert_eq!(bus.registers[&(0x33, reg::PCM_FILTER)], 0xc9);
        assert_eq!(bus.registers[&(0x33, reg::PCM_PATH_1)], PCM_PATH_1_SOFT_RAMP);
        assert_eq!(bus.registers[&(0x33, reg::CLASS_H)], CLASS_H_LOW_POWER_LV);
        assert_eq!(bus.registers[&(0x33, reg::HP_OUTPUT)], HP_OUTPUT_LV);
        let fast = volume_fast(Volume { left: 10, right: 12, muted: false });
        assert_eq!(fast.len(), 2);
        assert_eq!(fast[0], Op::Write { dac: Dac::Primary, register: VOLUME_LEFT, value: 10 });
        assert_eq!(fast[1], Op::Write { dac: Dac::Primary, register: VOLUME_RIGHT, value: 12 });
        let fast_dsd = dsd_volume_fast(Volume { left: 4, right: 6, muted: false });
        assert_eq!(fast_dsd.len(), 2);
        assert_eq!(fast_dsd[0], Op::Write { dac: Dac::Primary, register: DSD_VOLUME_LEFT, value: 4 });
        assert_eq!(fast_dsd[1], Op::Write { dac: Dac::Primary, register: DSD_VOLUME_RIGHT, value: 6 });
        let fixed_dsd = dsd_start_with_extras(
            DsdRate::Dsd128,
            DacExtras { class_h: ClassHMode::FixedHv, ..DacExtras::default() },
            Volume::MUTED,
        );
        let mut dsd_bus = MockBus::default();
        run_blocking(&mut dsd_bus, &only(&fixed_dsd, &[Dac::Primary]), |_| {}).unwrap();
        assert_eq!(dsd_bus.registers[&(0x33, reg::CLASS_H)], CLASS_H_FIXED_HV);
    }
}

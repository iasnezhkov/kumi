// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::regs;

pub const CMU_CLKEN0: usize = regs::CMU_CLKEN0;
pub const CMU_SYSCLK: usize = regs::CMU_SYSCLK;
pub const RMU_RSTEN0: usize = regs::RMU_RSTEN0;
pub const AON_OSC_XTAL: usize = regs::AON_OSC_XTAL;
pub const AON_PLL_CORE: usize = regs::AON_PLL_CORE;
pub const AON_PMU_PWR_CTL: usize = regs::AON_PMU_PWR_CTL;
pub const AON_PMU_VOL_CTL: usize = regs::AON_PMU_VOL_CTL;

pub const AON_GATE_BIT: u32 = 1 << 27;
pub const OSC_XTAL_ENABLE: u32 = 0x3;
pub const EXT_DCDC_ENABLE: u32 = 1 << 25;
pub const INT_LDO_ENABLE: u32 = 1 << 10;
pub const DVDD_VOL_SHIFT: u32 = 8;
pub const DVDD_VOL_MASK: u32 = 0x7 << DVDD_VOL_SHIFT;
pub const DVDD_VOL_1_4V: u32 = 6;
pub const PLL_CORE_ENABLE: u32 = 1;
pub const PLL_CORE_SRC_XTAL: u32 = 3;
pub const PLL_CORE_360: u32 = ((360 / 6) << 4) | PLL_CORE_SRC_XTAL;
pub const SYSCLK_OSC_DEV: u32 = 0x0000_0001;
pub const DCDC_HANDOFF_US: u32 = 20;
pub const VOLTAGE_SETTLE_US: u32 = 100;
pub const PLL_LOCK_US: u32 = 2_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CpuProfile {
    Suspend30,
    Idle90,
    Fast180,
    Turbo360,
}

impl CpuProfile {
    pub const fn hz(self) -> u32 {
        match self {
            Self::Suspend30 => 30_000_000,
            Self::Idle90 => 90_000_000,
            Self::Fast180 => 180_000_000,
            Self::Turbo360 => 360_000_000,
        }
    }

    pub const fn mhz(self) -> u32 {
        self.hz() / 1_000_000
    }

    pub const fn dvdd_vol(self) -> u32 {
        DVDD_VOL_1_4V
    }

    pub const fn sysclk(self) -> u32 {
        match self {
            Self::Suspend30 => 0x0000_0044,
            Self::Idle90 => 0x0000_0024,
            Self::Fast180 => 0x0000_0114,
            Self::Turbo360 => 0x0000_0204,
        }
    }
}

pub trait PowerIo {
    fn read(&mut self, address: usize) -> u32;
    fn write(&mut self, address: usize, value: u32);
    fn delay_us(&mut self, microseconds: u32, cpu_mhz: u32);
}

fn update<I: PowerIo>(io: &mut I, address: usize, clear: u32, set: u32) {
    let current = io.read(address);
    io.write(address, (current & !clear) | set);
}

pub fn enable_external_dcdc<I: PowerIo>(io: &mut I, dvdd_vol: u32, cpu_mhz: u32) {
    update(io, RMU_RSTEN0, 0, AON_GATE_BIT);
    update(io, CMU_CLKEN0, 0, AON_GATE_BIT);
    let pwr = io.read(AON_PMU_PWR_CTL);
    if pwr & EXT_DCDC_ENABLE == 0 {
        io.write(AON_PMU_PWR_CTL, pwr | EXT_DCDC_ENABLE);
        io.delay_us(DCDC_HANDOFF_US, cpu_mhz);
    }
    let after = io.read(AON_PMU_PWR_CTL);
    if after & INT_LDO_ENABLE != 0 {
        io.write(AON_PMU_PWR_CTL, after & !INT_LDO_ENABLE);
    }
    let vol = io.read(AON_PMU_VOL_CTL);
    let target = (vol & !DVDD_VOL_MASK) | ((dvdd_vol & 7) << DVDD_VOL_SHIFT);
    if target != vol {
        io.write(AON_PMU_VOL_CTL, target);
        io.delay_us(VOLTAGE_SETTLE_US, cpu_mhz);
    }
}

pub fn switch_profile<I: PowerIo>(io: &mut I, target: CpuProfile) {
    update(io, RMU_RSTEN0, 0, AON_GATE_BIT);
    update(io, CMU_CLKEN0, 0, AON_GATE_BIT);
    let pmu_ready = (io.read(AON_PMU_PWR_CTL) & (EXT_DCDC_ENABLE | INT_LDO_ENABLE)) == EXT_DCDC_ENABLE
        && ((io.read(AON_PMU_VOL_CTL) & DVDD_VOL_MASK) >> DVDD_VOL_SHIFT) == target.dvdd_vol();
    if pmu_ready && io.read(AON_PLL_CORE) == PLL_CORE_360 {
        io.write(CMU_SYSCLK, target.sysclk());
        let _ = io.read(CMU_SYSCLK);
        return;
    }
    io.write(CMU_SYSCLK, SYSCLK_OSC_DEV);
    let _ = io.read(CMU_SYSCLK);
    update(io, AON_OSC_XTAL, 0, OSC_XTAL_ENABLE);
    enable_external_dcdc(io, target.dvdd_vol(), 24);
    if io.read(AON_PLL_CORE) != PLL_CORE_360 {
        io.write(AON_PLL_CORE, PLL_CORE_360);
        io.delay_us(PLL_LOCK_US, 24);
    }
    io.write(CMU_SYSCLK, target.sysclk());
    let _ = io.read(CMU_SYSCLK);
}

pub fn prepare_for_reset<I: PowerIo>(io: &mut I) {
    update(io, RMU_RSTEN0, 0, AON_GATE_BIT);
    update(io, CMU_CLKEN0, 0, AON_GATE_BIT);
    io.write(CMU_SYSCLK, SYSCLK_OSC_DEV);
    let _ = io.read(CMU_SYSCLK);
    let pwr = io.read(AON_PMU_PWR_CTL);
    if pwr & INT_LDO_ENABLE == 0 {
        io.write(AON_PMU_PWR_CTL, pwr | INT_LDO_ENABLE);
        io.delay_us(DCDC_HANDOFF_US, 24);
    }
    let after = io.read(AON_PMU_PWR_CTL);
    if after & EXT_DCDC_ENABLE != 0 {
        io.write(AON_PMU_PWR_CTL, after & !EXT_DCDC_ENABLE);
    }
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn hardware_switch(target: CpuProfile, delay_at_mhz: fn(u32, u32)) {
    struct Mmio(fn(u32, u32));
    impl PowerIo for Mmio {
        fn read(&mut self, address: usize) -> u32 {
            unsafe { regs::read(address) }
        }

        fn write(&mut self, address: usize, value: u32) {
            unsafe { regs::write(address, value) }
        }

        fn delay_us(&mut self, microseconds: u32, cpu_mhz: u32) {
            (self.0)(microseconds, cpu_mhz);
        }
    }
    let mut io = Mmio(delay_at_mhz);
    switch_profile(&mut io, target);
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub unsafe fn hardware_prepare_reset(delay_at_mhz: fn(u32, u32)) {
    struct Mmio(fn(u32, u32));
    impl PowerIo for Mmio {
        fn read(&mut self, address: usize) -> u32 {
            unsafe { regs::read(address) }
        }

        fn write(&mut self, address: usize, value: u32) {
            unsafe { regs::write(address, value) }
        }

        fn delay_us(&mut self, microseconds: u32, cpu_mhz: u32) {
            (self.0)(microseconds, cpu_mhz);
        }
    }
    let mut io = Mmio(delay_at_mhz);
    prepare_for_reset(&mut io);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[derive(Default)]
    struct MockIo {
        regs: BTreeMap<usize, u32>,
        delays: Vec<(u32, u32)>,
    }

    impl PowerIo for MockIo {
        fn read(&mut self, address: usize) -> u32 {
            *self.regs.get(&address).unwrap_or(&0)
        }

        fn write(&mut self, address: usize, value: u32) {
            self.regs.insert(address, value);
        }

        fn delay_us(&mut self, microseconds: u32, cpu_mhz: u32) {
            self.delays.push((microseconds, cpu_mhz));
        }
    }

    #[test]
    fn turbo_enables_external_dcdc_sets_360_mhz_and_sys_div() {
        let mut io = MockIo::default();
        io.regs.insert(AON_PMU_PWR_CTL, 0x1700);
        io.regs.insert(AON_PMU_VOL_CTL, 0x012d_6425);
        switch_profile(&mut io, CpuProfile::Turbo360);
        assert_eq!(io.read(AON_PMU_PWR_CTL), (0x1700 | EXT_DCDC_ENABLE) & !INT_LDO_ENABLE);
        assert_eq!((io.read(AON_PMU_VOL_CTL) & DVDD_VOL_MASK) >> DVDD_VOL_SHIFT, DVDD_VOL_1_4V);
        assert_eq!(io.read(AON_PLL_CORE), PLL_CORE_360);
        assert_eq!(io.read(CMU_SYSCLK), 0x0000_0204);
        assert_eq!(io.delays, vec![(20, 24), (100, 24), (2_000, 24)]);
    }

    #[test]
    fn divider_switches_after_init_never_relock_pll_or_delay() {
        let mut io = MockIo::default();
        io.regs.insert(AON_PMU_PWR_CTL, 0x0200_1300);
        io.regs.insert(AON_PMU_VOL_CTL, 0x012d_6625);
        io.regs.insert(AON_PLL_CORE, PLL_CORE_360);
        switch_profile(&mut io, CpuProfile::Idle90);
        assert_eq!(io.read(CMU_SYSCLK), 0x0000_0024);
        switch_profile(&mut io, CpuProfile::Suspend30);
        assert_eq!(io.read(CMU_SYSCLK), 0x0000_0044);
        switch_profile(&mut io, CpuProfile::Turbo360);
        assert_eq!(io.read(CMU_SYSCLK), 0x0000_0204);
        assert_eq!(io.read(AON_PLL_CORE), PLL_CORE_360);
        assert!(io.delays.is_empty());
    }

    #[test]
    fn prepare_for_reset_restores_osc_dev_and_internal_ldo() {
        let mut io = MockIo::default();
        io.regs.insert(AON_PMU_PWR_CTL, 0x0200_1300);
        io.regs.insert(CMU_SYSCLK, 0x0000_0204);
        prepare_for_reset(&mut io);
        assert_eq!(io.read(CMU_SYSCLK), SYSCLK_OSC_DEV);
        assert_eq!(io.read(AON_PMU_PWR_CTL), 0x0000_1700);
        assert_eq!(io.delays, vec![(20, 24)]);
    }
}

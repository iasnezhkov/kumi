// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_chip::regs as hw;

use crate::regs::{Expect, MmioOp, Registers};

pub const I2C0_BASE: u32 = hw::I2C0_BASE as u32;
pub const I2C1_BASE: u32 = hw::I2C1_BASE as u32;
pub const CONFIG_100KHZ: u32 = 0x1_7800;
pub const CONFIG_400KHZ: u32 = 0x1_1e00;
pub const COMMAND: u32 = 0x04;
pub const PENDING: u32 = 0x0c;
pub const STATUS: u32 = 0x10;
pub const RECEIVED: u32 = 0x14;
pub const PENDING_STOP: u32 = 1;
pub const PENDING_BYTE: u32 = 2;
pub const PENDING_NACK: u32 = 4;
pub const PENDING_BUS_ERROR: u32 = 0x10;
pub const PENDING_MASK: u32 = 0x1f;
pub const STATUS_NACK: u32 = 2;
pub const STATUS_ADDRESS_NACK: u32 = 0x10;
pub const COMMAND_STOP: u32 = 0x21ff;
pub const COMMAND_RECEIVE_LAST: u32 = 0x100;
pub const MAX_WRITE: usize = 56;
pub const MAX_PREFIX: usize = 4;
pub const DEFAULT_BUDGET_US: u32 = 2_000;
pub const STOP_BUDGET_US: u32 = 200;
pub const POLL_STEP_US: u32 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2cError {
    Nack,
    Bus,
    Timeout,
    Invalid,
    Unrecovered,
}

pub trait I2cBus {
    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), I2cError>;
    fn write_then_read(&mut self, address: u8, prefix: &[u8], buffer: &mut [u8]) -> Result<(), I2cError>;
    fn probe(&mut self, address: u8) -> Result<(), I2cError>;
}

pub const fn command_start_write(address: u8) -> u32 {
    0x1100 | ((address as u32) << 1)
}

pub const fn command_start_read(address: u8) -> u32 {
    0x1001 | ((address as u32) << 1)
}

pub const fn command_byte(byte: u8) -> u32 {
    0x100 | byte as u32
}

pub const fn valid_address(address: u8) -> bool {
    address >= 0x08 && address <= 0x77
}

pub const SETUP: [MmioOp; 12] = [
    MmioOp::Update { address: hw::CMU_CLKEN0 as u32, clear: 0, set: 0x10 },
    MmioOp::Update { address: hw::GPIO_BASE as u32 + 0x18, clear: 0x0f, set: 0x04 },
    MmioOp::Update { address: hw::GPIO_BASE as u32 + 0x18, clear: 0xc0, set: 0xc0 },
    MmioOp::Update { address: hw::GPIO_BASE as u32 + 0x1c, clear: 0x0f, set: 0x04 },
    MmioOp::Update { address: hw::GPIO_BASE as u32 + 0x1c, clear: 0xc0, set: 0xc0 },
    MmioOp::Update { address: hw::RMU_RSTEN0 as u32, clear: 0x1000, set: 0 },
    MmioOp::Delay { microseconds: 1_000 },
    MmioOp::Update { address: hw::CMU_CLKEN0 as u32, clear: 0, set: 0x1000 },
    MmioOp::Update { address: hw::RMU_RSTEN0 as u32, clear: 0, set: 0x1000 },
    MmioOp::Update { address: hw::CMU_I2C0_CLK as u32, clear: 3, set: 0 },
    MmioOp::Write { address: hw::CMU_I2C0_CLK as u32, value: 1 },
    MmioOp::Write { address: I2C0_BASE, value: CONFIG_100KHZ },
];

pub const SETUP_EXPECT: [Expect; 6] = [
    Expect { address: I2C0_BASE, mask: u32::MAX, value: CONFIG_100KHZ },
    Expect { address: hw::CMU_I2C0_CLK as u32, mask: u32::MAX, value: 1 },
    Expect { address: hw::GPIO_BASE as u32 + 0x18, mask: 0xcf, value: 0xc4 },
    Expect { address: hw::GPIO_BASE as u32 + 0x1c, mask: 0xcf, value: 0xc4 },
    Expect { address: hw::CMU_CLKEN0 as u32, mask: 0x1010, value: 0x1010 },
    Expect { address: hw::RMU_RSTEN0 as u32, mask: 0x1000, value: 0x1000 },
];

pub const fn config_word(input_hz: u32, bus_hz: u32) -> u32 {
    0x1_0000 | ((input_hz / 2 / bus_hz) << 8)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Fault {
    pub error: Option<I2cError>,
    pub pending: u32,
    pub status: u32,
}

pub struct Controller<R> {
    registers: R,
    base: u32,
    budget_us: u32,
    healthy: bool,
    last: Fault,
}

impl<R: Registers> Controller<R> {
    pub fn new(registers: R, base: u32) -> Self {
        Self { registers, base, budget_us: DEFAULT_BUDGET_US, healthy: true, last: Fault::default() }
    }

    pub fn with_budget(mut self, budget_us: u32) -> Self {
        self.budget_us = budget_us;
        self
    }

    pub fn registers(&mut self) -> &mut R {
        &mut self.registers
    }

    pub fn into_registers(self) -> R {
        self.registers
    }

    pub fn healthy(&self) -> bool {
        self.healthy
    }

    pub fn last_fault(&self) -> Fault {
        self.last
    }

    pub fn recover(&mut self) {
        self.healthy = true;
        self.last = Fault::default();
    }

    fn step(&mut self, command: u32, stop: bool, check_ack: bool, budget: &mut u32) -> Result<(), I2cError> {
        let pending = self.registers.read(self.base + PENDING);
        self.registers.write(self.base + PENDING, pending & PENDING_MASK);
        self.registers.write(self.base + COMMAND, command);
        let complete = if stop { PENDING_STOP } else { PENDING_BYTE };
        loop {
            let pending = self.registers.read(self.base + PENDING);
            let status = self.registers.read(self.base + STATUS);
            self.last.pending = pending;
            self.last.status = status;
            if pending & PENDING_BUS_ERROR != 0 && status & STATUS_ADDRESS_NACK != 0 {
                return Err(I2cError::Nack);
            }
            if pending & PENDING_BUS_ERROR != 0 {
                return Err(I2cError::Bus);
            }
            if check_ack && (pending & PENDING_NACK != 0 || pending & complete != 0 && status & STATUS_NACK != 0) {
                return Err(I2cError::Nack);
            }
            if pending & complete != 0 {
                self.registers.write(self.base + PENDING, pending & PENDING_MASK);
                return Ok(());
            }
            if *budget == 0 {
                return Err(I2cError::Timeout);
            }
            *budget -= POLL_STEP_US.min(*budget);
            self.registers.delay_us(POLL_STEP_US);
        }
    }

    fn begin(&self, address: u8) -> Result<u32, I2cError> {
        if !self.healthy {
            return Err(I2cError::Unrecovered);
        }
        if !valid_address(address) || self.budget_us == 0 {
            return Err(I2cError::Invalid);
        }
        Ok(self.budget_us)
    }

    fn finish<T>(&mut self, result: Result<T, I2cError>, budget: &mut u32) -> Result<T, I2cError> {
        if let Err(error) = result.as_ref() {
            self.last.error = Some(*error);
            *budget = STOP_BUDGET_US;
        }
        let pending = self.last.pending;
        let status = self.last.status;
        let stopped = self.step(COMMAND_STOP, true, false, budget);
        if result.is_err() {
            self.last.pending = pending;
            self.last.status = status;
        }
        if stopped.is_err() {
            self.healthy = false;
            self.last.error = Some(I2cError::Unrecovered);
            return Err(I2cError::Unrecovered);
        }
        result
    }
}

impl<R: Registers> I2cBus for Controller<R> {
    fn write(&mut self, address: u8, bytes: &[u8]) -> Result<(), I2cError> {
        if bytes.is_empty() || bytes.len() > MAX_WRITE {
            return Err(I2cError::Invalid);
        }
        let mut budget = self.begin(address)?;
        let result = (|| {
            self.step(command_start_write(address), false, true, &mut budget)?;
            for &byte in bytes {
                self.step(command_byte(byte), false, true, &mut budget)?;
            }
            Ok(())
        })();
        self.finish(result, &mut budget)
    }

    fn write_then_read(&mut self, address: u8, prefix: &[u8], buffer: &mut [u8]) -> Result<(), I2cError> {
        if prefix.is_empty() || prefix.len() > MAX_PREFIX || buffer.len() != 1 {
            return Err(I2cError::Invalid);
        }
        let mut budget = self.begin(address)?;
        let result = (|| {
            self.step(command_start_write(address), false, true, &mut budget)?;
            for &byte in prefix {
                self.step(command_byte(byte), false, true, &mut budget)?;
            }
            self.step(COMMAND_STOP, true, false, &mut budget)?;
            self.step(command_start_read(address), false, true, &mut budget)?;
            self.step(COMMAND_RECEIVE_LAST, false, false, &mut budget)?;
            Ok(self.registers.read(self.base + RECEIVED) as u8)
        })();
        buffer[0] = self.finish(result, &mut budget)?;
        Ok(())
    }

    fn probe(&mut self, address: u8) -> Result<(), I2cError> {
        let mut budget = self.begin(address)?;
        let result = self.step(command_start_write(address), false, true, &mut budget);
        self.finish(result, &mut budget)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::regs::{apply, verify};
    use crate::testing::Recorder;
    use std::vec::Vec;

    struct Model {
        writes: Vec<(u32, u32)>,
        pending: u32,
        status: u32,
        fault: u32,
        stop_fails: bool,
        delays: u32,
    }

    impl Registers for Model {
        fn read(&mut self, offset: u32) -> u32 {
            match offset - I2C0_BASE {
                PENDING => self.pending,
                STATUS => self.status,
                RECEIVED => 0x43,
                _ => 0,
            }
        }

        fn write(&mut self, address: u32, value: u32) {
            let offset = address - I2C0_BASE;
            self.writes.push((offset, value));
            if offset == PENDING {
                self.pending &= !value;
            }
            if offset == COMMAND {
                self.pending = if value == COMMAND_STOP { if self.stop_fails { 0 } else { PENDING_STOP } } else { self.fault };
            }
        }

        fn delay_us(&mut self, microseconds: u32) {
            self.delays += microseconds;
        }
    }

    fn controller(fault: u32) -> Controller<Model> {
        Controller::new(Model { writes: Vec::new(), pending: 2, status: 0, fault, stop_fails: false, delays: 0 }, I2C0_BASE)
    }

    fn commands(c: &mut Controller<Model>) -> Vec<u32> {
        c.registers().writes.iter().filter(|w| w.0 == COMMAND).map(|w| w.1).collect()
    }

    #[test]
    fn dac_read_sends_the_prefix_stops_then_reads_one_byte() {
        let mut c = controller(PENDING_BYTE);
        let mut byte = [0];
        c.write_then_read(0x33, &[1, 0, 0, 1], &mut byte).unwrap();
        assert_eq!(byte, [0x43]);
        assert_eq!(&c.registers().writes[..2], &[(PENDING, 2), (COMMAND, 0x1166)]);
        assert_eq!(commands(&mut c), [0x1166, 0x101, 0x100, 0x100, 0x101, 0x21ff, 0x1067, 0x100, 0x21ff]);
    }

    #[test]
    fn write_frames_every_byte_and_ends_with_stop() {
        let mut c = controller(PENDING_BYTE);
        c.write(0x31, &[9, 0, 0x0b, 1, 2]).unwrap();
        assert_eq!(commands(&mut c), [0x1162, 0x109, 0x100, 0x10b, 0x101, 0x102, 0x21ff]);
        assert_eq!(c.write(0x31, &[]), Err(I2cError::Invalid));
        assert_eq!(c.write(0x31, &[0; 57]), Err(I2cError::Invalid));
        assert_eq!(c.write(0x07, &[1]), Err(I2cError::Invalid));
        assert_eq!(c.write(0x78, &[1]), Err(I2cError::Invalid));
        let mut two = [0; 2];
        assert_eq!(c.write_then_read(0x33, &[1], &mut two), Err(I2cError::Invalid));
    }

    #[test]
    fn nack_wins_over_completion_and_keeps_the_original_flags() {
        let mut c = controller(PENDING_NACK | PENDING_BYTE);
        assert_eq!(c.probe(0x32), Err(I2cError::Nack));
        assert_eq!(c.last_fault(), Fault { error: Some(I2cError::Nack), pending: 6, status: 0 });
        assert_eq!(commands(&mut c), [0x1164, 0x21ff]);
        let mut c = controller(PENDING_BYTE);
        c.registers().status = STATUS_NACK;
        assert_eq!(c.write(0x33, &[1]), Err(I2cError::Nack));
    }

    #[test]
    fn timeouts_are_bounded_and_a_failed_stop_disables_the_controller() {
        let mut c = controller(0).with_budget(20);
        assert_eq!(c.write(0x33, &[1]), Err(I2cError::Timeout));
        assert_eq!(c.registers().delays, 20);
        assert!(c.healthy());
        c.registers().stop_fails = true;
        assert_eq!(c.write(0x33, &[1]), Err(I2cError::Unrecovered));
        assert_eq!(c.registers().delays, 20 + 20 + STOP_BUDGET_US);
        let count = c.registers().writes.len();
        assert_eq!(c.write(0x33, &[1]), Err(I2cError::Unrecovered));
        assert_eq!(c.registers().writes.len(), count);
        c.registers().stop_fails = false;
        c.recover();
        c.registers().fault = PENDING_BYTE;
        assert_eq!(c.write(0x33, &[1]), Ok(()));
    }

    #[test]
    fn an_address_nobody_acknowledges_is_a_nack_as_measured_on_the_case() {
        let mut c = controller(PENDING_BUS_ERROR | PENDING_BYTE);
        c.registers().status = 0x18;
        assert_eq!(c.probe(0x31), Err(I2cError::Nack));
        assert_eq!(c.last_fault(), Fault { error: Some(I2cError::Nack), pending: 0x12, status: 0x18 });
    }

    #[test]
    fn bus_errors_are_reported_before_anything_else() {
        let mut c = controller(PENDING_BUS_ERROR | PENDING_BYTE);
        assert_eq!(c.write(0x33, &[1]), Err(I2cError::Bus));
        assert_eq!(c.last_fault().error, Some(I2cError::Bus));
    }

    #[test]
    fn setup_programs_100_khz_on_gpio6_and_gpio7_and_reads_back() {
        assert_eq!(config_word(24_000_000, 100_000), CONFIG_100KHZ);
        assert_eq!(config_word(24_000_000, 400_000), CONFIG_400KHZ);
        let mut r = Recorder::default();
        r.set(0x2011_0018, 0x3f);
        r.set(0x2011_001c, 0x01);
        r.set(0x2000_2028, 6);
        apply(&mut r, &SETUP);
        assert_eq!(verify(&mut r, &SETUP_EXPECT), Ok(()));
        assert_eq!(r.get(0x2011_0018), 0xf4);
        assert_eq!(r.delays(), [1_000]);
    }
}

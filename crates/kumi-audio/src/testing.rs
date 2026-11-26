// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::{BTreeMap, VecDeque};
use std::vec::Vec;

use crate::regs::Registers;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Access {
    Read(u32, u32),
    Write(u32, u32),
    Delay(u32),
}

#[derive(Default)]
pub struct Recorder {
    pub values: BTreeMap<u32, u32>,
    pub scripted: BTreeMap<u32, VecDeque<u32>>,
    pub log: Vec<Access>,
    pub elapsed_us: u64,
}

impl Recorder {
    pub fn set(&mut self, address: u32, value: u32) {
        self.values.insert(address, value);
    }

    pub fn get(&self, address: u32) -> u32 {
        self.values.get(&address).copied().unwrap_or(0)
    }

    pub fn script(&mut self, address: u32, values: &[u32]) {
        self.scripted.entry(address).or_default().extend(values.iter().copied());
    }

    pub fn writes(&self) -> Vec<(u32, u32)> {
        self.log
            .iter()
            .filter_map(|access| match access {
                Access::Write(address, value) => Some((*address, *value)),
                _ => None,
            })
            .collect()
    }

    pub fn delays(&self) -> Vec<u32> {
        self.log
            .iter()
            .filter_map(|access| match access {
                Access::Delay(us) => Some(*us),
                _ => None,
            })
            .collect()
    }
}

impl Registers for Recorder {
    fn read(&mut self, address: u32) -> u32 {
        let value = match self.scripted.get_mut(&address).and_then(VecDeque::pop_front) {
            Some(value) => value,
            None => self.get(address),
        };
        self.log.push(Access::Read(address, value));
        value
    }

    fn write(&mut self, address: u32, value: u32) {
        self.values.insert(address, value);
        self.log.push(Access::Write(address, value));
    }

    fn delay_us(&mut self, microseconds: u32) {
        self.elapsed_us += u64::from(microseconds);
        self.log.push(Access::Delay(microseconds));
    }
}

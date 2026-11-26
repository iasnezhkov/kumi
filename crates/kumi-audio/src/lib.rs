// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

pub mod board;
pub mod bringup;
pub mod clock;
pub mod dac;
pub mod flow;
pub mod i2c;
pub mod i2s;
pub mod jack;
pub mod pcm;
pub mod regs;
pub mod stream;
pub mod uac2;

#[cfg(test)]
mod testing;

pub use regs::{MmioOp, Registers};

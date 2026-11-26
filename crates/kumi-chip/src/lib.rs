// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]
#![allow(clippy::missing_safety_doc)]

pub mod bootrom;
pub mod efuse;
pub mod flash;
pub mod norc;
pub mod plic;
pub mod power;
pub mod regs;
pub mod reset;
pub mod rom;
pub mod scratch;
pub mod timer;
pub mod watchdog;

pub use bootrom::{BootRom, Identity};
pub use flash::{Error, Rom, RomFlash};

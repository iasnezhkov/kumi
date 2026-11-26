// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![cfg_attr(not(test), no_std)]

pub mod journal;
pub mod policy;
pub mod stage0;
pub mod verify;

pub use journal::{BootState, Journal, JournalError, SlotRecord, SlotStatus};
pub use policy::{
    Decision, Halt, PolicyError, begin_update, confirm, decide, finish_update, mark_healthy, prefer, request_boot_rom,
};
pub use verify::{VerifyError, image_digest, key_id, verify_header, verify_slot, verify_slot_raw};

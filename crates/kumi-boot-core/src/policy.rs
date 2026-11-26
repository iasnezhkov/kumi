// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_layout::SlotId;

use crate::journal::{BootState, SlotRecord, SlotStatus};

pub const TRIAL_ATTEMPTS: u8 = 3;
pub const SAFE_MODE_AFTER: u8 = 2;
pub const BOOTROM_AFTER: u8 = 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Halt {
    NoBootableSlot,
    CrashStreak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Decision {
    Boot { slot: SlotId, safe_mode: bool },
    BootRom(Halt),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PolicyError {
    NotBootable,
    NotBooted,
}

pub fn decide(state: &BootState, mut image: impl FnMut(SlotId) -> Option<u32>) -> (Decision, BootState) {
    let mut next = *state;
    next.boot_count = next.boot_count.wrapping_add(1);
    for slot in [state.preferred, state.preferred.other()] {
        let record = *next.slot(slot);
        let bootable = match record.status {
            SlotStatus::Confirmed => true,
            SlotStatus::Trial => record.attempts > 0,
            SlotStatus::Empty | SlotStatus::Invalid => continue,
        };
        if let Some(generation) = if bootable { image(slot) } else { None } {
            next.preferred = slot;
            if generation != record.generation {
                *next.slot_mut(slot) = SlotRecord { status: SlotStatus::Trial, attempts: TRIAL_ATTEMPTS, generation };
            }
            if next.slot(slot).status == SlotStatus::Trial {
                next.slot_mut(slot).attempts -= 1;
                return (Decision::Boot { slot, safe_mode: false }, next);
            }
            let unhealthy = next.unhealthy_starts;
            if unhealthy >= BOOTROM_AFTER {
                next.unhealthy_starts = SAFE_MODE_AFTER;
                return (Decision::BootRom(Halt::CrashStreak), next);
            }
            next.unhealthy_starts = unhealthy + 1;
            return (Decision::Boot { slot, safe_mode: unhealthy >= SAFE_MODE_AFTER }, next);
        }
        if record.status != SlotStatus::Confirmed {
            let record = next.slot_mut(slot);
            record.status = SlotStatus::Invalid;
            record.attempts = 0;
        }
        next.unhealthy_starts = 0;
    }
    (Decision::BootRom(Halt::NoBootableSlot), next)
}

pub fn begin_update(state: &BootState, booted: SlotId) -> (SlotId, BootState) {
    let target = booted.other();
    let mut next = *state;
    next.preferred = booted;
    let record = next.slot_mut(target);
    record.status = SlotStatus::Empty;
    record.attempts = 0;
    (target, next)
}

pub fn finish_update(state: &BootState, target: SlotId, generation: u32) -> BootState {
    let mut next = *state;
    let record = next.slot_mut(target);
    record.status = SlotStatus::Trial;
    record.attempts = TRIAL_ATTEMPTS;
    record.generation = generation;
    next.preferred = target;
    next.unhealthy_starts = 0;
    next
}

pub fn confirm(state: &BootState, booted: SlotId) -> Result<BootState, PolicyError> {
    let mut next = *state;
    let record = next.slot_mut(booted);
    match record.status {
        SlotStatus::Confirmed | SlotStatus::Trial => {
            record.status = SlotStatus::Confirmed;
            record.attempts = 0;
            next.preferred = booted;
            next.unhealthy_starts = 0;
            Ok(next)
        }
        _ => Err(PolicyError::NotBooted),
    }
}

pub fn mark_healthy(state: &BootState) -> BootState {
    let mut next = *state;
    next.unhealthy_starts = 0;
    next
}

pub fn prefer(state: &BootState, slot: SlotId) -> Result<BootState, PolicyError> {
    let record = state.slot(slot);
    match record.status {
        SlotStatus::Confirmed => {}
        SlotStatus::Trial if record.attempts > 0 => {}
        _ => return Err(PolicyError::NotBootable),
    }
    let mut next = *state;
    next.preferred = slot;
    next.unhealthy_starts = 0;
    Ok(next)
}

pub fn request_boot_rom(state: &BootState) -> BootState {
    let mut next = *state;
    next.bootrom_pending = true;
    next
}

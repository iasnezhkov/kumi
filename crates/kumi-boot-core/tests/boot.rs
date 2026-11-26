// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::cell::Cell;

use ed25519_dalek::{Signer, SigningKey};
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use kumi_boot_core::*;
use kumi_layout::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cut;

impl NorFlashError for Cut {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::Other
    }
}

struct MockFlash {
    data: Vec<u8>,
    budget: Option<usize>,
    scramble_on_cut: bool,
}

impl MockFlash {
    fn new() -> Self {
        Self { data: vec![0xFF; 0x24_0000], budget: None, scramble_on_cut: false }
    }

    fn spend(&mut self) -> bool {
        match &mut self.budget {
            None => true,
            Some(0) => false,
            Some(n) => {
                *n -= 1;
                true
            }
        }
    }
}

impl ErrorType for MockFlash {
    type Error = Cut;
}

impl ReadNorFlash for MockFlash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), Cut> {
        let start = offset as usize;
        bytes.copy_from_slice(&self.data[start..start + bytes.len()]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.data.len()
    }
}

impl NorFlash for MockFlash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = 4096;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), Cut> {
        for i in from as usize..to as usize {
            if !self.spend() {
                if self.scramble_on_cut {
                    for (k, byte) in self.data[i..to as usize].iter_mut().enumerate() {
                        *byte ^= (k as u8).wrapping_mul(37) | 1;
                    }
                }
                return Err(Cut);
            }
            self.data[i] = 0xFF;
        }
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), Cut> {
        for (i, b) in bytes.iter().enumerate() {
            if !self.spend() {
                if self.scramble_on_cut {
                    self.data[offset as usize + i] &= 0x5A;
                }
                return Err(Cut);
            }
            self.data[offset as usize + i] &= *b;
        }
        Ok(())
    }
}

fn state_with(boot_count: u32) -> BootState {
    let mut s = BootState::FRESH;
    s.boot_count = boot_count;
    s.slots[0] = SlotRecord { status: SlotStatus::Confirmed, attempts: 0, generation: boot_count };
    s
}

fn same_content(a: &BootState, b: &BootState) -> bool {
    BootState { sequence: 0, ..*a } == BootState { sequence: 0, ..*b }
}

#[test]
fn record_codec_round_trips_and_rejects_damage() {
    let mut s = state_with(42);
    s.sequence = 9;
    s.preferred = SlotId::B;
    s.unhealthy_starts = 3;
    s.bootrom_pending = true;
    s.slots[1] = SlotRecord { status: SlotStatus::Trial, attempts: 2, generation: 77 };
    let bytes = s.encode();
    assert_eq!(BootState::decode(&bytes), Some(s));
    for i in 0..journal::RECORD_SIZE {
        let mut damaged = bytes;
        damaged[i] ^= 0x10;
        assert_eq!(BootState::decode(&damaged), None, "byte {i}");
    }
}

#[test]
fn journal_appends_switches_sectors_and_survives_many_writes() {
    let mut j = Journal::new(MockFlash::new(), BOOT_JOURNAL).unwrap();
    assert_eq!(j.load().unwrap(), None);
    for n in 0..(5 * journal::RECORDS_PER_SECTOR + 3) {
        let stored = j.store(&state_with(n)).unwrap();
        assert_eq!(stored.sequence, n + 1);
        assert_eq!(j.load().unwrap(), Some(stored));
    }
}

#[test]
fn journal_recovers_from_garbage_only_flash() {
    let mut flash = MockFlash::new();
    flash.data[BOOT_JOURNAL.offset as usize..BOOT_JOURNAL.end() as usize].fill(0x00);
    let mut j = Journal::new(flash, BOOT_JOURNAL).unwrap();
    assert_eq!(j.load().unwrap(), None);
    let stored = j.store(&state_with(1)).unwrap();
    assert_eq!(j.load().unwrap(), Some(stored));
}

#[test]
fn power_cut_at_any_point_keeps_old_or_new_state() {
    let fills = [0, 1, journal::RECORDS_PER_SECTOR - 1, journal::RECORDS_PER_SECTOR, 2 * journal::RECORDS_PER_SECTOR - 1];
    let mut checked = 0;
    for &fill in &fills {
        for scramble in [false, true] {
            let mut budgets: Vec<usize> = (0..=40).collect();
            budgets.extend((41..4200).step_by(173));
            budgets.extend([4095, 4096, 4097, 4120, 4127, 4128, 4129]);
            for &budget in &budgets {
                let mut j = Journal::new(MockFlash::new(), BOOT_JOURNAL).unwrap();
                let mut old = None;
                for n in 0..=fill {
                    old = Some(j.store(&state_with(n)).unwrap());
                }
                let old = old.unwrap();
                let new = state_with(1_000);
                let mut flash = j.into_inner();
                flash.budget = Some(budget);
                flash.scramble_on_cut = scramble;
                let mut j = Journal::new(flash, BOOT_JOURNAL).unwrap();
                let finished = j.store(&new).is_ok();
                let mut flash = j.into_inner();
                flash.budget = None;
                let mut j = Journal::new(flash, BOOT_JOURNAL).unwrap();
                let loaded = j.load().unwrap().expect("a state must survive");
                if finished {
                    assert!(same_content(&loaded, &new));
                } else {
                    assert!(same_content(&loaded, &old) || same_content(&loaded, &new), "fill {fill} budget {budget}");
                }
                let after = j.store(&state_with(2_000)).unwrap();
                assert_eq!(j.load().unwrap(), Some(after));
                assert!(after.sequence > loaded.sequence);
                checked += 1;
            }
        }
    }
    assert!(checked > 400);
}

fn installed(slot: SlotId) -> BootState {
    let mut s = BootState::FRESH;
    s.slot_mut(slot).status = SlotStatus::Confirmed;
    s.preferred = slot;
    s
}

fn holding(a: Option<u32>, b: Option<u32>) -> impl Fn(SlotId) -> Option<u32> {
    move |slot| if slot == SlotId::A { a } else { b }
}

#[test]
fn update_trial_confirm_and_rollback() {
    let s = installed(SlotId::A);
    let (decision, s) = decide(&s, holding(Some(0), None));
    assert_eq!(decision, Decision::Boot { slot: SlotId::A, safe_mode: false });
    let (target, s) = begin_update(&s, SlotId::A);
    assert_eq!(target, SlotId::B);
    assert_eq!(s.slot(SlotId::B).status, SlotStatus::Empty);
    let s = finish_update(&s, target, 2);
    let both = holding(Some(0), Some(2));
    let (decision, trial) = decide(&s, &both);
    assert_eq!(decision, Decision::Boot { slot: SlotId::B, safe_mode: false });
    assert_eq!(trial.slot(SlotId::B).attempts, policy::TRIAL_ATTEMPTS - 1);
    let confirmed = confirm(&trial, SlotId::B).unwrap();
    assert_eq!(confirmed.slot(SlotId::B).status, SlotStatus::Confirmed);
    assert_eq!(decide(&confirmed, &both).0, Decision::Boot { slot: SlotId::B, safe_mode: false });
    let mut s = s;
    for _ in 0..policy::TRIAL_ATTEMPTS {
        let (decision, next) = decide(&s, &both);
        assert_eq!(decision, Decision::Boot { slot: SlotId::B, safe_mode: false });
        assert_eq!(next.unhealthy_starts, 0, "trial starts are counted by their attempts");
        s = next;
    }
    let (decision, s) = decide(&s, &both);
    assert_eq!(decision, Decision::Boot { slot: SlotId::A, safe_mode: false });
    assert_eq!(s.slot(SlotId::B).status, SlotStatus::Invalid);
    assert_eq!(s.preferred, SlotId::A);
    assert_eq!(s.unhealthy_starts, 1);
}

#[test]
fn corrupt_images_fall_back_and_both_bad_goes_to_bootrom() {
    let mut s = installed(SlotId::A);
    s.slot_mut(SlotId::B).status = SlotStatus::Confirmed;
    let (decision, next) = decide(&s, holding(None, Some(0)));
    assert_eq!(decision, Decision::Boot { slot: SlotId::B, safe_mode: false });
    assert_eq!(next.slot(SlotId::A).status, SlotStatus::Confirmed);
    assert_eq!(next.preferred, SlotId::B);
    let (decision, next) = decide(&next, holding(None, None));
    assert_eq!(decision, Decision::BootRom(Halt::NoBootableSlot));
    assert_eq!(next.slot(SlotId::B).status, SlotStatus::Confirmed);
    let (decision, _) = decide(&next, holding(Some(0), Some(0)));
    assert_eq!(
        decision,
        Decision::Boot { slot: SlotId::B, safe_mode: false },
        "a confirmed slot that failed one check is tried again"
    );
    let mut trial = installed(SlotId::A);
    *trial.slot_mut(SlotId::B) = SlotRecord { status: SlotStatus::Trial, attempts: 2, generation: 2 };
    trial.preferred = SlotId::B;
    let (decision, next) = decide(&trial, holding(Some(0), None));
    assert_eq!(decision, Decision::Boot { slot: SlotId::A, safe_mode: false });
    assert_eq!(next.slot(SlotId::B).status, SlotStatus::Invalid, "a trial image that fails its check is dropped");
    assert_eq!(decide(&BootState::FRESH, holding(Some(0), Some(0))).0, Decision::BootRom(Halt::NoBootableSlot));
}

#[test]
fn unhealthy_starts_enter_safe_mode_then_bootrom_and_recover() {
    let calls = Cell::new(0);
    let ok = |_| {
        calls.set(calls.get() + 1);
        Some(0)
    };
    let s = installed(SlotId::A);
    let (d, s) = decide(&s, ok);
    assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: false });
    let (d, s) = decide(&s, ok);
    assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: false });
    let (d, s) = decide(&s, ok);
    assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: true });
    let (d, s) = decide(&s, ok);
    assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: true });
    assert_eq!(s.unhealthy_starts, policy::BOOTROM_AFTER);
    let (d, s) = decide(&s, ok);
    assert_eq!(d, Decision::BootRom(Halt::CrashStreak));
    assert_eq!(s.slot(SlotId::A).status, SlotStatus::Confirmed);
    let (d, s) = decide(&s, ok);
    assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: true });
    let s = mark_healthy(&s);
    assert_eq!(decide(&s, ok).0, Decision::Boot { slot: SlotId::A, safe_mode: false });
    assert!(calls.get() >= 7);
}

#[test]
fn isolated_crashes_separated_by_healthy_runs_never_reach_safe_mode() {
    let mut s = installed(SlotId::A);
    for _ in 0..20 {
        let (d, next) = decide(&s, holding(Some(0), None));
        assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: false });
        s = mark_healthy(&next);
        let (d, next) = decide(&s, holding(Some(0), None));
        assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: false });
        s = next;
    }
    assert_eq!(s.unhealthy_starts, 1);
}

#[test]
fn switching_slots_or_installing_starts_the_new_slot_without_an_old_streak() {
    let mut s = installed(SlotId::A);
    s.slot_mut(SlotId::B).status = SlotStatus::Confirmed;
    s.unhealthy_starts = 3;
    let switched = prefer(&s, SlotId::B).unwrap();
    assert_eq!(switched.unhealthy_starts, 0);
    assert_eq!(decide(&switched, holding(Some(0), Some(0))).0, Decision::Boot { slot: SlotId::B, safe_mode: false });
    assert_eq!(finish_update(&s, SlotId::B, 9).unhealthy_starts, 0);
    assert_eq!(confirm(&s, SlotId::A).unwrap().unhealthy_starts, 0);
}

#[test]
fn a_slot_rewritten_with_another_generation_is_a_fresh_trial() {
    let mut s = installed(SlotId::A);
    s.slot_mut(SlotId::A).generation = 4;
    s.unhealthy_starts = 3;
    let (d, next) = decide(&s, holding(Some(5), None));
    assert_eq!(d, Decision::Boot { slot: SlotId::A, safe_mode: false });
    assert_eq!(
        *next.slot(SlotId::A),
        SlotRecord { status: SlotStatus::Trial, attempts: policy::TRIAL_ATTEMPTS - 1, generation: 5 }
    );
    let mut trial = BootState::FRESH;
    *trial.slot_mut(SlotId::B) = SlotRecord { status: SlotStatus::Trial, attempts: 1, generation: 2 };
    trial.preferred = SlotId::B;
    let (_, next) = decide(&trial, holding(None, Some(3)));
    assert_eq!(
        *next.slot(SlotId::B),
        SlotRecord { status: SlotStatus::Trial, attempts: policy::TRIAL_ATTEMPTS - 1, generation: 3 }
    );
    let (_, same) = decide(&trial, holding(None, Some(2)));
    assert_eq!(same.slot(SlotId::B).attempts, 0);
}

#[test]
fn prefer_and_confirm_guard_against_unbootable_slots() {
    let s = installed(SlotId::A);
    assert_eq!(prefer(&s, SlotId::B), Err(PolicyError::NotBootable));
    assert_eq!(confirm(&s, SlotId::B), Err(PolicyError::NotBooted));
    let mut both = s;
    both.slot_mut(SlotId::B).status = SlotStatus::Confirmed;
    assert_eq!(prefer(&both, SlotId::B).unwrap().preferred, SlotId::B);
    assert!(request_boot_rom(&s).bootrom_pending);
}

fn signed_image(key: &SigningKey, payload: &[u8], generation: u32) -> Vec<u8> {
    let mut header = ImageHeader {
        flags: FLAG_SAFE_MODE_CAPABLE,
        version: [0, 1, 0],
        generation,
        payload_size: payload.len() as u32,
        entry: 0x100,
        segments: [
            Some(Segment { kind: SegmentKind::Copy, destination: 0, offset: 0, size: payload.len() as u32 }),
            Some(Segment { kind: SegmentKind::Zero, destination: 0x0010_2000, offset: 0, size: 0x1000 }),
            None,
            None,
        ],
        key_id: key_id(&key.verifying_key()),
        digest: [0; 32],
        signature: [0; 64],
    };
    let mut signed = [0u8; SIGNED_HEADER_LEN];
    signed.copy_from_slice(&header.encode()[..SIGNED_HEADER_LEN]);
    header.digest = image_digest(&signed, payload);
    header.signature = key.sign(&header.digest).to_bytes();
    let mut image = header.encode().to_vec();
    image.extend_from_slice(payload);
    image
}

fn flash_with(slot: SlotId, image: &[u8]) -> MockFlash {
    let mut flash = MockFlash::new();
    let start = slot.region().offset as usize;
    flash.data[start..start + image.len()].copy_from_slice(image);
    flash
}

#[test]
fn signed_images_verify_and_any_tampering_is_caught() {
    let key = SigningKey::from_bytes(&[7; 32]);
    let payload: Vec<u8> = (0..5_000u32).map(|i| (i * 31 % 251) as u8).collect();
    let image = signed_image(&key, &payload, 5);
    let mut flash = flash_with(SlotId::B, &image);
    let header = verify_slot(&mut flash, SlotId::B, &key.verifying_key()).unwrap();
    assert_eq!(header.generation, 5);
    assert_eq!(verify_slot(&mut flash, SlotId::A, &key.verifying_key()), Err(VerifyError::Header(HeaderError::Magic)));
    let other = SigningKey::from_bytes(&[9; 32]);
    assert_eq!(verify_slot(&mut flash, SlotId::B, &other.verifying_key()), Err(VerifyError::Key));
    let mut tampered = image.clone();
    tampered[HEADER_SIZE + 4_321] ^= 1;
    assert_eq!(verify_slot(&mut flash_with(SlotId::B, &tampered), SlotId::B, &key.verifying_key()), Err(VerifyError::Digest));
    let mut forged = image.clone();
    forged[170] ^= 1;
    let crc = crc32(&forged[..252]);
    forged[252..256].copy_from_slice(&crc.to_le_bytes());
    assert_eq!(verify_slot(&mut flash_with(SlotId::B, &forged), SlotId::B, &key.verifying_key()), Err(VerifyError::Signature));
    let mut header_hit = image;
    header_hit[20] ^= 1;
    assert_eq!(
        verify_slot(&mut flash_with(SlotId::B, &header_hit), SlotId::B, &key.verifying_key()),
        Err(VerifyError::Header(HeaderError::Crc))
    );
}

#[test]
fn a_trial_without_a_fallback_ends_in_the_bootrom_and_never_uses_safe_mode() {
    let mut s = BootState::FRESH;
    *s.slot_mut(SlotId::A) = SlotRecord { status: SlotStatus::Trial, attempts: policy::TRIAL_ATTEMPTS, generation: 1 };
    for _ in 0..policy::TRIAL_ATTEMPTS {
        let (decision, next) = decide(&s, holding(Some(1), None));
        assert_eq!(decision, Decision::Boot { slot: SlotId::A, safe_mode: false });
        s = next;
    }
    let (decision, s) = decide(&s, holding(Some(1), None));
    assert_eq!(decision, Decision::BootRom(Halt::NoBootableSlot));
    assert_eq!(s.slot(SlotId::A).status, SlotStatus::Invalid);
}

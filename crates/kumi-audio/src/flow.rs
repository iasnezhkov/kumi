// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use crate::pcm::{SampleFormat, UsbSpeed};

pub const FRAME_BYTES: usize = 8;
pub const DOP_MARKER_A: u8 = 0x05;
pub const DOP_MARKER_B: u8 = 0xfa;
pub const DOP_LOCK_FRAMES: u16 = 32;
pub const DOP_UNLOCK_FRAMES: u16 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DopDetector {
    locked: bool,
    expected: u8,
    hits: u16,
    misses: u16,
}

impl Default for DopDetector {
    fn default() -> Self {
        Self::new()
    }
}

impl DopDetector {
    pub const fn new() -> Self {
        Self { locked: false, expected: 0, hits: 0, misses: 0 }
    }

    pub const fn locked(&self) -> bool {
        self.locked
    }

    pub fn reset(&mut self) {
        self.locked = false;
        self.expected = 0;
        self.hits = 0;
        self.misses = 0;
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn observe_packet(&mut self, format: SampleFormat, packet: &[u8]) -> Option<bool> {
        let before = self.locked;
        let mut locked = self.locked;
        let mut expected = self.expected;
        let mut hits = self.hits;
        let mut misses = self.misses;
        match format {
            SampleFormat::S16 => return None,
            SampleFormat::S24 => {
                if !packet.len().is_multiple_of(6) {
                    return None;
                }
                for frame in packet.chunks_exact(6) {
                    let left = frame[2];
                    let right = frame[5];
                    if expected != 0 && left == expected && right == expected {
                        expected ^= 0xff;
                        misses = 0;
                        if !locked {
                            hits = hits.saturating_add(1);
                            if hits >= DOP_LOCK_FRAMES {
                                locked = true;
                            }
                        }
                        continue;
                    }
                    if expected == 0 && left == right && (left == DOP_MARKER_A || left == DOP_MARKER_B) {
                        expected = left ^ 0xff;
                        misses = 0;
                        if !locked {
                            hits = hits.saturating_add(1);
                            if hits >= DOP_LOCK_FRAMES {
                                locked = true;
                            }
                        }
                        continue;
                    }
                    if left == 0 && right == 0 && (frame[0] | frame[1] | frame[3] | frame[4]) == 0 {
                        continue;
                    }
                    expected = 0;
                    hits = 0;
                    if locked {
                        misses = misses.saturating_add(1);
                        if misses >= DOP_UNLOCK_FRAMES {
                            locked = false;
                        }
                    }
                }
            }
            SampleFormat::S32 => {
                if !packet.len().is_multiple_of(8) {
                    return None;
                }
                for frame in packet.chunks_exact(8) {
                    let left = frame[3];
                    let right = frame[7];
                    if expected != 0 && left == expected && right == expected {
                        expected ^= 0xff;
                        misses = 0;
                        if !locked {
                            hits = hits.saturating_add(1);
                            if hits >= DOP_LOCK_FRAMES {
                                locked = true;
                            }
                        }
                        continue;
                    }
                    if expected == 0 && left == right && (left == DOP_MARKER_A || left == DOP_MARKER_B) {
                        expected = left ^ 0xff;
                        misses = 0;
                        if !locked {
                            hits = hits.saturating_add(1);
                            if hits >= DOP_LOCK_FRAMES {
                                locked = true;
                            }
                        }
                        continue;
                    }
                    if left == 0 && right == 0 && (frame[0] | frame[1] | frame[2] | frame[4] | frame[5] | frame[6]) == 0 {
                        continue;
                    }
                    expected = 0;
                    hits = 0;
                    if locked {
                        misses = misses.saturating_add(1);
                        if misses >= DOP_UNLOCK_FRAMES {
                            locked = false;
                        }
                    }
                }
            }
        }
        self.locked = locked;
        self.expected = expected;
        self.hits = hits;
        self.misses = misses;
        (locked != before).then_some(locked)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlowError {
    Malformed,
    Overrun { frames: usize, free: usize },
    NotReady,
    Stopped,
    InvalidCursor,
}

pub trait FrameStore {
    fn capacity(&self) -> usize;
    fn store(&mut self, index: usize, frame: [i32; 2]);
}

impl FrameStore for [[i32; 2]] {
    fn capacity(&self) -> usize {
        self.len()
    }

    fn store(&mut self, index: usize, frame: [i32; 2]) {
        self[index] = frame;
    }
}

pub fn sample(format: SampleFormat, bytes: &[u8]) -> i32 {
    match format {
        SampleFormat::S16 => i32::from(i16::from_le_bytes([bytes[0], bytes[1]])) << 16,
        SampleFormat::S24 => i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]),
        SampleFormat::S32 => i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]),
    }
}

pub fn frame(format: SampleFormat, bytes: &[u8]) -> [i32; 2] {
    let split = usize::from(format.subslot_bytes());
    [sample(format, &bytes[..split]), sample(format, &bytes[split..])]
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub fn unpack(format: SampleFormat, packet: &[u8], out: &mut [i32]) -> Result<usize, FlowError> {
    let frame_bytes = format.frame_bytes();
    if !packet.len().is_multiple_of(frame_bytes) || out.len() < packet.len() / frame_bytes * 2 {
        return Err(FlowError::Malformed);
    }
    let frames = packet.len() / frame_bytes;
    match format {
        SampleFormat::S16 => {
            for (bytes, slot) in packet.chunks_exact(4).zip(out.chunks_exact_mut(2)) {
                slot[0] = i32::from(i16::from_le_bytes([bytes[0], bytes[1]])) << 16;
                slot[1] = i32::from(i16::from_le_bytes([bytes[2], bytes[3]])) << 16;
            }
        }
        SampleFormat::S24 => {
            for (bytes, slot) in packet.chunks_exact(6).zip(out.chunks_exact_mut(2)) {
                slot[0] = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]);
                slot[1] = i32::from_le_bytes([0, bytes[3], bytes[4], bytes[5]]);
            }
        }
        SampleFormat::S32 => {
            for (bytes, slot) in packet.chunks_exact(8).zip(out.chunks_exact_mut(2)) {
                slot[0] = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                slot[1] = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            }
        }
    }
    Ok(frames)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaCursor {
    pub frame: usize,
    pub wraps: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Observation {
    Advanced { frames: usize },
    Underrun { missing: u64 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Priming,
    Running,
    Faulted,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counters {
    pub written_frames: u64,
    pub played_frames: u64,
    pub dropped_frames: u64,
    pub underrun_frames: u64,
    pub overruns: u32,
    pub underruns: u32,
    pub malformed: u32,
    pub feedback_corrections: u32,
}

impl Counters {
    pub const fn icd(&self, dac_faults: u32) -> kumi_icd::AudioCounters {
        kumi_icd::AudioCounters {
            underruns: self.underruns,
            overruns: self.overruns,
            feedback_corrections: self.feedback_corrections,
            dac_faults,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Flow {
    capacity: usize,
    prefill: usize,
    write: usize,
    read: usize,
    wraps: u32,
    queued: usize,
    phase: Phase,
    counters: Counters,
    last_feedback: Option<u32>,
    silence: [i32; 2],
}

impl Flow {
    pub const fn new(capacity: usize, prefill: usize) -> Self {
        let prefill = if prefill > capacity { capacity } else { prefill };
        Self {
            capacity,
            prefill,
            write: 0,
            read: 0,
            wraps: 0,
            queued: 0,
            phase: Phase::Priming,
            counters: Counters {
                written_frames: 0,
                played_frames: 0,
                dropped_frames: 0,
                underrun_frames: 0,
                overruns: 0,
                underruns: 0,
                malformed: 0,
                feedback_corrections: 0,
            },
            last_feedback: None,
            silence: [0, 0],
        }
    }

    pub fn set_silence(&mut self, silence: [i32; 2]) {
        self.silence = silence;
    }

    pub const fn capacity(&self) -> usize {
        self.capacity
    }

    pub const fn prefill(&self) -> usize {
        self.prefill
    }

    pub const fn queued(&self) -> usize {
        self.queued
    }

    pub const fn free(&self) -> usize {
        self.capacity - self.queued
    }

    pub const fn phase(&self) -> Phase {
        self.phase
    }

    pub const fn counters(&self) -> Counters {
        self.counters
    }

    pub const fn ready_to_start(&self) -> bool {
        matches!(self.phase, Phase::Priming) && self.queued >= self.prefill && self.prefill > 0
    }

    fn admit(&mut self, frames: usize) -> Result<(), FlowError> {
        if self.phase == Phase::Faulted {
            return Err(FlowError::Stopped);
        }
        if frames > self.free() {
            self.counters.overruns = self.counters.overruns.saturating_add(1);
            self.counters.dropped_frames = self.counters.dropped_frames.saturating_add(frames as u64);
            return Err(FlowError::Overrun { frames, free: self.free() });
        }
        Ok(())
    }

    fn advance_write(&mut self, frames: usize) {
        self.queued += frames;
        self.counters.written_frames = self.counters.written_frames.saturating_add(frames as u64);
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn push<S: FrameStore + ?Sized>(&mut self, store: &mut S, samples: &[i32]) -> Result<usize, FlowError> {
        if !samples.len().is_multiple_of(2) {
            self.counters.malformed = self.counters.malformed.saturating_add(1);
            return Err(FlowError::Malformed);
        }
        let frames = samples.len() / 2;
        self.admit(frames)?;
        let capacity = self.capacity;
        let mut write = self.write;
        for pair in samples.chunks_exact(2) {
            store.store(write, [pair[0], pair[1]]);
            write += 1;
            if write == capacity {
                write = 0;
            }
        }
        self.write = write;
        self.advance_write(frames);
        Ok(frames)
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn push_packet<S: FrameStore + ?Sized>(
        &mut self,
        store: &mut S,
        format: SampleFormat,
        packet: &[u8],
    ) -> Result<usize, FlowError> {
        let frame_bytes = format.frame_bytes();
        if !packet.len().is_multiple_of(frame_bytes) {
            self.counters.malformed = self.counters.malformed.saturating_add(1);
            return Err(FlowError::Malformed);
        }
        let frames = packet.len() / frame_bytes;
        self.admit(frames)?;
        let capacity = self.capacity;
        let mut write = self.write;
        match format {
            SampleFormat::S16 => {
                for bytes in packet.chunks_exact(4) {
                    let left = i32::from(i16::from_le_bytes([bytes[0], bytes[1]])) << 16;
                    let right = i32::from(i16::from_le_bytes([bytes[2], bytes[3]])) << 16;
                    store.store(write, [left, right]);
                    write += 1;
                    if write == capacity {
                        write = 0;
                    }
                }
            }
            SampleFormat::S24 => {
                for bytes in packet.chunks_exact(6) {
                    let left = i32::from_le_bytes([0, bytes[0], bytes[1], bytes[2]]);
                    let right = i32::from_le_bytes([0, bytes[3], bytes[4], bytes[5]]);
                    store.store(write, [left, right]);
                    write += 1;
                    if write == capacity {
                        write = 0;
                    }
                }
            }
            SampleFormat::S32 => {
                for bytes in packet.chunks_exact(8) {
                    let left = i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
                    let right = i32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
                    store.store(write, [left, right]);
                    write += 1;
                    if write == capacity {
                        write = 0;
                    }
                }
            }
        }
        self.write = write;
        self.advance_write(frames);
        Ok(frames)
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn push_dop_packet<S: FrameStore + ?Sized>(
        &mut self,
        store: &mut S,
        format: SampleFormat,
        packet: &[u8],
        next_marker: &mut u8,
    ) -> Result<usize, FlowError> {
        let frame_bytes = format.frame_bytes();
        if !packet.len().is_multiple_of(frame_bytes) {
            self.counters.malformed = self.counters.malformed.saturating_add(1);
            return Err(FlowError::Malformed);
        }
        let frames = packet.len() / frame_bytes;
        self.admit(frames)?;
        let capacity = self.capacity;
        let mut write = self.write;
        let mut marker = if *next_marker == DOP_MARKER_B { DOP_MARKER_B } else { DOP_MARKER_A };
        match format {
            SampleFormat::S24 => {
                for bytes in packet.chunks_exact(6) {
                    let lm = bytes[2];
                    let rm = bytes[5];
                    let frame = if lm == rm && (lm == DOP_MARKER_A || lm == DOP_MARKER_B) {
                        marker = lm ^ 0xff;
                        [i32::from_le_bytes([0, bytes[0], bytes[1], lm]), i32::from_le_bytes([0, bytes[3], bytes[4], rm])]
                    } else {
                        let word = (((marker as u32) << 24) | 0x0069_6900) as i32;
                        marker ^= 0xff;
                        [word, word]
                    };
                    store.store(write, frame);
                    write += 1;
                    if write == capacity {
                        write = 0;
                    }
                }
            }
            SampleFormat::S32 => {
                for bytes in packet.chunks_exact(8) {
                    let lm = bytes[3];
                    let rm = bytes[7];
                    let frame = if lm == rm && (lm == DOP_MARKER_A || lm == DOP_MARKER_B) {
                        marker = lm ^ 0xff;
                        [
                            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], lm]),
                            i32::from_le_bytes([bytes[4], bytes[5], bytes[6], rm]),
                        ]
                    } else {
                        let word = (((marker as u32) << 24) | 0x0069_6900) as i32;
                        marker ^= 0xff;
                        [word, word]
                    };
                    store.store(write, frame);
                    write += 1;
                    if write == capacity {
                        write = 0;
                    }
                }
            }
            SampleFormat::S16 => {
                for _ in 0..frames {
                    let word = (((marker as u32) << 24) | 0x0069_6900) as i32;
                    marker ^= 0xff;
                    store.store(write, [word, word]);
                    write += 1;
                    if write == capacity {
                        write = 0;
                    }
                }
            }
        }
        *next_marker = marker;
        self.write = write;
        self.advance_write(frames);
        Ok(frames)
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn push_native_dsd_packet<S: FrameStore + ?Sized>(&mut self, store: &mut S, packet: &[u8]) -> Result<usize, FlowError> {
        if !packet.len().is_multiple_of(FRAME_BYTES) {
            self.counters.malformed = self.counters.malformed.saturating_add(1);
            return Err(FlowError::Malformed);
        }
        let frames = packet.len() / FRAME_BYTES;
        self.admit(frames)?;
        let capacity = self.capacity;
        let mut write = self.write;
        for bytes in packet.chunks_exact(FRAME_BYTES) {
            let left = i32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
            let right = i32::from_be_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
            store.store(write, [left, right]);
            write += 1;
            if write == capacity {
                write = 0;
            }
        }
        self.write = write;
        self.advance_write(frames);
        Ok(frames)
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn push_native_dsd_words<S: FrameStore + ?Sized>(&mut self, store: &mut S, words: &[u32]) -> Result<usize, FlowError> {
        if !words.len().is_multiple_of(2) {
            self.counters.malformed = self.counters.malformed.saturating_add(1);
            return Err(FlowError::Malformed);
        }
        let frames = words.len() / 2;
        self.admit(frames)?;
        let capacity = self.capacity;
        let mut write = self.write;
        for pair in words.chunks_exact(2) {
            let left = u32::from_be_bytes(pair[0].to_le_bytes()) as i32;
            let right = u32::from_be_bytes(pair[1].to_le_bytes()) as i32;
            store.store(write, [left, right]);
            write += 1;
            if write == capacity {
                write = 0;
            }
        }
        self.write = write;
        self.advance_write(frames);
        Ok(frames)
    }

    pub fn start(&mut self) -> Result<(), FlowError> {
        if !self.ready_to_start() || self.read != 0 {
            return Err(FlowError::NotReady);
        }
        self.phase = Phase::Running;
        Ok(())
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn observe<S: FrameStore + ?Sized>(&mut self, store: &mut S, cursor: DmaCursor) -> Result<Observation, FlowError> {
        if self.phase != Phase::Running {
            return Err(FlowError::NotReady);
        }
        if cursor.frame >= self.capacity {
            return Err(FlowError::InvalidCursor);
        }
        let laps = u64::from(cursor.wraps.wrapping_sub(self.wraps));
        let position = laps * self.capacity as u64 + cursor.frame as u64;
        if position < self.read as u64 {
            return Err(FlowError::InvalidCursor);
        }
        let advanced = position - self.read as u64;
        self.counters.played_frames = self.counters.played_frames.saturating_add(advanced);
        if advanced > self.queued as u64 {
            let missing = advanced - self.queued as u64;
            self.counters.underruns = self.counters.underruns.saturating_add(1);
            self.counters.underrun_frames = self.counters.underrun_frames.saturating_add(missing);
            self.queued = 0;
            self.phase = Phase::Faulted;
            return Ok(Observation::Underrun { missing });
        }
        let capacity = self.capacity;
        let silence = self.silence;
        let mut at = self.read;
        for _ in 0..advanced as usize {
            store.store(at, silence);
            at += 1;
            if at == capacity {
                at = 0;
            }
        }
        self.queued -= advanced as usize;
        self.read = cursor.frame;
        self.wraps = cursor.wraps;
        Ok(Observation::Advanced { frames: advanced as usize })
    }

    pub fn reset<S: FrameStore + ?Sized>(&mut self, store: &mut S) {
        let silence = self.silence;
        for index in 0..self.capacity.min(store.capacity()) {
            store.store(index, silence);
        }
        self.write = 0;
        self.read = 0;
        self.wraps = 0;
        self.queued = 0;
        self.phase = Phase::Priming;
        self.last_feedback = None;
    }

    pub fn resume<S: FrameStore + ?Sized>(&mut self, store: &mut S, cursor: DmaCursor) -> Result<(), FlowError> {
        if cursor.frame >= self.capacity || self.prefill == 0 {
            return Err(FlowError::InvalidCursor);
        }
        let silence = self.silence;
        for index in 0..self.capacity.min(store.capacity()) {
            store.store(index, silence);
        }
        self.read = cursor.frame;
        self.wraps = cursor.wraps;
        self.write = (cursor.frame + self.prefill) % self.capacity;
        self.queued = self.prefill;
        self.phase = Phase::Running;
        self.last_feedback = None;
        Ok(())
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn feedback(&mut self, nominal_q16: u32) -> u32 {
        if self.phase != Phase::Running || self.prefill == 0 {
            return nominal_q16;
        }
        let maximum = nominal_q16 / 1_000;
        let delta = self.queued.abs_diff(self.prefill);
        let correction = if delta >= self.prefill {
            maximum
        } else if let Some(product) = (delta as u32).checked_mul(maximum) {
            product / self.prefill as u32
        } else {
            (delta as u64 * u64::from(maximum) / self.prefill as u64) as u32
        };
        let value = if self.queued >= self.prefill { nominal_q16 - correction } else { nominal_q16 + correction };
        if self.last_feedback.is_some_and(|last| last != value) {
            self.counters.feedback_corrections = self.counters.feedback_corrections.saturating_add(1);
        }
        self.last_feedback = Some(value);
        value
    }
}

pub fn measured_feedback(frames: u64, intervals: u32) -> Option<u32> {
    if intervals == 0 {
        return None;
    }
    u32::try_from((frames << 16) / u64::from(intervals)).ok()
}

#[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
pub const fn encode_feedback(value_q16: u32, speed: UsbSpeed) -> ([u8; 4], usize) {
    match speed {
        UsbSpeed::Full => {
            let q14 = (value_q16 >> 2).to_le_bytes();
            ([q14[0], q14[1], q14[2], 0], 3)
        }
        UsbSpeed::High => (value_q16.to_le_bytes(), 4),
    }
}

pub const fn feedback_bytes(speed: UsbSpeed) -> u16 {
    match speed {
        UsbSpeed::Full => 3,
        UsbSpeed::High => 4,
    }
}

pub struct Queue<T: Copy, const N: usize> {
    items: [T; N],
    head: usize,
    used: usize,
    dropped: u32,
}

impl<T: Copy, const N: usize> Queue<T, N> {
    pub const fn new(empty: T) -> Self {
        Self { items: [empty; N], head: 0, used: 0, dropped: 0 }
    }

    pub fn push(&mut self, item: T) -> Result<(), T> {
        if self.used == N {
            self.dropped = self.dropped.saturating_add(1);
            return Err(item);
        }
        self.items[(self.head + self.used) % N] = item;
        self.used += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<T> {
        if self.used == 0 {
            return None;
        }
        let value = self.items[self.head];
        self.head = (self.head + 1) % N;
        self.used -= 1;
        Some(value)
    }

    pub fn front(&self) -> Option<T> {
        (self.used != 0).then(|| self.items[self.head])
    }

    pub fn clear(&mut self) {
        self.head = 0;
        self.used = 0;
    }

    pub fn dropped(&self) -> u32 {
        self.dropped
    }

    pub fn len(&self) -> usize {
        self.used
    }

    pub fn is_empty(&self) -> bool {
        self.used == 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pcm::PcmProfile;
    use std::vec;
    use std::vec::Vec;

    const N: usize = 2_400;

    fn store() -> Vec<[i32; 2]> {
        vec![[0; 2]; N]
    }

    #[test]
    fn unpack_keeps_sign_and_left_justifies() {
        let mut out = [0; 4];
        assert_eq!(unpack(SampleFormat::S16, &[0, 128, 255, 127], &mut out), Ok(1));
        assert_eq!(out[..2], [i32::MIN, 0x7fff_0000]);
        assert_eq!(unpack(SampleFormat::S24, &[1, 2, 128, 255, 255, 127], &mut out), Ok(1));
        assert_eq!(out[..2], [0x8002_0100u32 as i32, 0x7fff_ff00]);
        assert_eq!(unpack(SampleFormat::S32, &[0, 0, 0, 128, 255, 255, 255, 127, 1, 0, 0, 0, 2, 0, 0, 0], &mut out), Ok(2));
        assert_eq!(out, [i32::MIN, i32::MAX, 1, 2]);
        assert_eq!(unpack(SampleFormat::S24, &[0; 5], &mut out), Err(FlowError::Malformed));
        assert_eq!(unpack(SampleFormat::S16, &[0; 12], &mut out), Err(FlowError::Malformed));
    }

    #[test]
    fn rejection_is_atomic_and_counted() {
        let mut s = store();
        let mut flow = Flow::new(N, N / 2);
        assert_eq!(flow.start(), Err(FlowError::NotReady));
        assert_eq!(flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[0; 3]), Err(FlowError::Malformed));
        flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[1; N * 4]).unwrap();
        assert_eq!(
            flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[2; 4]),
            Err(FlowError::Overrun { frames: 1, free: 0 })
        );
        assert_eq!(flow.push(s.as_mut_slice(), &[1]), Err(FlowError::Malformed));
        assert_eq!(s[0], [0x0101_0000; 2]);
        assert_eq!(flow.queued(), N);
        let c = flow.counters();
        assert_eq!((c.overruns, c.dropped_frames, c.malformed, c.written_frames), (1, 1, 2, N as u64));
        assert_eq!(c.icd(3), kumi_icd::AudioCounters { underruns: 0, overruns: 1, feedback_corrections: 0, dac_faults: 3 });
    }

    #[test]
    fn a_full_lap_without_new_data_is_an_underrun() {
        let mut s = store();
        let mut flow = Flow::new(N, N / 2);
        flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[1; N * 4]).unwrap();
        flow.start().unwrap();
        assert_eq!(flow.observe(s.as_mut_slice(), DmaCursor { frame: 0, wraps: 0 }), Ok(Observation::Advanced { frames: 0 }));
        assert_eq!(flow.observe(s.as_mut_slice(), DmaCursor { frame: 0, wraps: 1 }), Ok(Observation::Advanced { frames: N }));
        assert!(s.iter().all(|f| *f == [0, 0]));
        assert_eq!(
            flow.observe(s.as_mut_slice(), DmaCursor { frame: 0, wraps: 2 }),
            Ok(Observation::Underrun { missing: N as u64 })
        );
        assert_eq!(flow.phase(), Phase::Faulted);
        assert_eq!(flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[0; 4]), Err(FlowError::Stopped));
        flow.reset(s.as_mut_slice());
        assert_eq!(flow.phase(), Phase::Priming);
        assert_eq!(flow.counters().underruns, 1);
        assert_eq!(flow.counters().underrun_frames, N as u64);
    }

    #[test]
    fn resume_after_an_underrun_puts_a_prefill_of_silence_ahead_of_the_running_dma() {
        let mut s = store();
        let mut flow = Flow::new(N, N / 2);
        flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[1; N * 2]).unwrap();
        flow.start().unwrap();
        assert!(matches!(flow.observe(s.as_mut_slice(), DmaCursor { frame: 1_300, wraps: 0 }), Ok(Observation::Underrun { .. })));
        s[5] = [9, 9];
        assert_eq!(flow.resume(s.as_mut_slice(), DmaCursor { frame: N, wraps: 0 }), Err(FlowError::InvalidCursor));
        flow.resume(s.as_mut_slice(), DmaCursor { frame: 1_400, wraps: 0 }).unwrap();
        assert!(s.iter().all(|f| *f == [0, 0]));
        assert_eq!((flow.phase(), flow.queued()), (Phase::Running, N / 2));
        flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[2; 4]).unwrap();
        assert_eq!(s[(1_400 + N / 2) % N], [0x0202_0000; 2]);
        assert_eq!(
            flow.observe(s.as_mut_slice(), DmaCursor { frame: 100, wraps: 1 }),
            Ok(Observation::Advanced { frames: N - 1_300 })
        );
        assert_eq!(flow.queued(), N / 2 + 1 - (N - 1_300));
        assert_eq!(flow.counters().underruns, 1);
        let mut empty = Flow::new(N, 0);
        assert_eq!(empty.resume(s.as_mut_slice(), DmaCursor { frame: 0, wraps: 0 }), Err(FlowError::InvalidCursor));
    }

    #[test]
    fn consumed_space_is_reused_across_the_wrap_and_cursors_are_checked() {
        let mut s = store();
        let mut flow = Flow::new(N, N / 2);
        flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[1; N * 4]).unwrap();
        flow.start().unwrap();
        flow.observe(s.as_mut_slice(), DmaCursor { frame: 100, wraps: 0 }).unwrap();
        flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[2; 400]).unwrap();
        assert_eq!(flow.queued(), N);
        assert_eq!(s[0], [0x0202_0000; 2]);
        assert_eq!(s[100], [0x0101_0000; 2]);
        assert_eq!(flow.observe(s.as_mut_slice(), DmaCursor { frame: 0, wraps: 0 }), Err(FlowError::InvalidCursor));
        assert_eq!(flow.observe(s.as_mut_slice(), DmaCursor { frame: N, wraps: 1 }), Err(FlowError::InvalidCursor));
        assert_eq!(flow.counters().played_frames, 100);
    }

    #[test]
    fn feedback_is_nominal_at_the_target_and_bounded_around_it() {
        let nominal = PcmProfile::from_hz(48_000).unwrap().nominal_frames_per_interval_q16(UsbSpeed::High);
        let mut s = store();
        let mut flow = Flow::new(N, N / 2);
        assert_eq!(flow.feedback(nominal), nominal);
        flow.push_packet(s.as_mut_slice(), SampleFormat::S16, &[0; N * 4]).unwrap();
        flow.start().unwrap();
        assert_eq!(flow.feedback(nominal), nominal - nominal / 1_000);
        flow.observe(s.as_mut_slice(), DmaCursor { frame: N / 2, wraps: 0 }).unwrap();
        assert_eq!(flow.feedback(nominal), nominal);
        flow.observe(s.as_mut_slice(), DmaCursor { frame: 0, wraps: 1 }).unwrap();
        assert_eq!(flow.feedback(nominal), nominal + nominal / 1_000);
        assert_eq!(flow.counters().feedback_corrections, 2);
    }

    #[test]
    fn feedback_encodings_follow_the_bus_speed() {
        let fs = PcmProfile::from_hz(48_000).unwrap().nominal_frames_per_interval_q16(UsbSpeed::Full);
        assert_eq!(encode_feedback(fs, UsbSpeed::Full), ([0, 0, 0x0c, 0], 3));
        let fs44 = PcmProfile::from_hz(44_100).unwrap().nominal_frames_per_interval_q16(UsbSpeed::Full);
        assert_eq!(encode_feedback(fs44, UsbSpeed::Full), ([0x66, 0x06, 0x0b, 0], 3));
        let hs = PcmProfile::from_hz(192_000).unwrap().nominal_frames_per_interval_q16(UsbSpeed::High);
        assert_eq!(encode_feedback(hs, UsbSpeed::High), ([0, 0, 0x18, 0], 4));
        assert_eq!(measured_feedback(48_000, 1_000), Some(48 << 16));
        assert_eq!(measured_feedback(44_100, 8_000), Some(0x0005_8333));
        assert_eq!(measured_feedback(1, 0), None);
        assert_eq!(measured_feedback(u64::MAX >> 16, 1), None);
        assert_eq!((feedback_bytes(UsbSpeed::Full), feedback_bytes(UsbSpeed::High)), (3, 4));
    }

    #[test]
    fn queue_is_bounded_and_counts_drops() {
        let mut q: Queue<u16, 3> = Queue::new(0);
        assert!(q.is_empty());
        for v in 1..=3 {
            q.push(v).unwrap();
        }
        assert_eq!(q.push(4), Err(4));
        assert_eq!(q.dropped(), 1);
        assert_eq!(q.front(), Some(1));
        assert_eq!(q.pop(), Some(1));
        q.push(5).unwrap();
        assert_eq!([q.pop(), q.pop(), q.pop(), q.pop()], [Some(2), Some(3), Some(5), None]);
        q.push(6).unwrap();
        q.clear();
        assert_eq!(q.len(), 0);
    }

    fn dop_s32_packet(frames: usize, start_marker: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(frames * 8);
        let mut marker = start_marker;
        for i in 0..frames {
            let b = (i as u8).wrapping_mul(3).wrapping_add(0x69);
            out.extend_from_slice(&[0x00, b, b ^ 0x55, marker, 0x00, b ^ 0xaa, b, marker]);
            marker ^= 0xff;
        }
        out
    }

    fn dop_s24_packet(frames: usize, start_marker: u8) -> Vec<u8> {
        let mut out = Vec::with_capacity(frames * 6);
        let mut marker = start_marker;
        for i in 0..frames {
            let b = (i as u8).wrapping_add(0x69);
            out.extend_from_slice(&[b, b ^ 0x55, marker, b ^ 0xaa, b, marker]);
            marker ^= 0xff;
        }
        out
    }

    #[test]
    fn dop_detector_locks_on_alternating_markers_ignores_silence_and_unlocks_on_pcm() {
        let mut det = DopDetector::new();
        assert!(!det.locked());
        assert_eq!(det.observe_packet(SampleFormat::S16, &[0x05; 128]), None);
        let short = dop_s32_packet(16, DOP_MARKER_A);
        assert_eq!(det.observe_packet(SampleFormat::S32, &short), None);
        assert!(!det.locked());
        assert_eq!(det.observe_packet(SampleFormat::S32, &[0; 64]), None);
        assert_eq!(det.observe_packet(SampleFormat::S32, &dop_s32_packet(16, DOP_MARKER_A)), Some(true));
        assert!(det.locked());
        assert_eq!(det.observe_packet(SampleFormat::S32, &[0; 128]), None);
        assert!(det.locked());
        let pcm: Vec<u8> = (0..16 * 8).map(|i| ((i * 37) & 0xff) as u8 | 0x10).collect();
        assert_eq!(det.observe_packet(SampleFormat::S32, &pcm), Some(false));
        assert!(!det.locked());

        det.reset();
        assert_eq!(det.observe_packet(SampleFormat::S24, &dop_s24_packet(32, DOP_MARKER_B)), Some(true));
        assert!(det.locked());
    }

    #[test]
    fn native_dsd_packet_byte_swaps_u32_be_and_uses_configured_silence() {
        let mut s = store();
        let mut flow = Flow::new(N, N / 2);
        flow.set_silence([crate::pcm::DSD_SILENCE_WORD; 2]);
        flow.reset(s.as_mut_slice());
        assert_eq!(s[0], [0x6969_6969; 2]);
        let packet = [0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0];
        assert_eq!(flow.push_native_dsd_packet(s.as_mut_slice(), &packet), Ok(1));
        assert_eq!(s[0], [0x1234_5678, 0x9abc_def0u32 as i32]);
        assert_eq!(flow.push_native_dsd_packet(s.as_mut_slice(), &[0; 7]), Err(FlowError::Malformed));
        let words = [u32::from_le_bytes([0x12, 0x34, 0x56, 0x78]), u32::from_le_bytes([0x9a, 0xbc, 0xde, 0xf0])];
        assert_eq!(flow.push_native_dsd_words(s.as_mut_slice(), &words), Ok(1));
        assert_eq!(s[1], [0x1234_5678, 0x9abc_def0u32 as i32]);
        assert_eq!(flow.push_native_dsd_words(s.as_mut_slice(), &[0]), Err(FlowError::Malformed));
    }

    #[test]
    fn dop_packet_substitutes_dsd_silence_for_zero_frames() {
        let mut s = store();
        let mut flow = Flow::new(N, N / 2);
        let mut marker = DOP_MARKER_A;
        let zeros = [0u8; 16];
        assert_eq!(flow.push_dop_packet(s.as_mut_slice(), SampleFormat::S32, &zeros, &mut marker), Ok(2));
        assert_eq!(s[0], [0x0569_6900u32 as i32; 2]);
        assert_eq!(s[1], [0xfa69_6900u32 as i32; 2]);
        assert_eq!(marker, DOP_MARKER_A);
        let valid = dop_s32_packet(2, DOP_MARKER_A);
        assert_eq!(flow.push_dop_packet(s.as_mut_slice(), SampleFormat::S32, &valid, &mut marker), Ok(2));
        assert_eq!((s[2][0] as u32) >> 24, u32::from(DOP_MARKER_A));
        assert_eq!((s[3][0] as u32) >> 24, u32::from(DOP_MARKER_B));
    }
}

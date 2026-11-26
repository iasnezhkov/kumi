// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_chip::regs as hw;

use crate::flow::{DmaCursor, FRAME_BYTES};
use crate::pcm::{DsdMode, DsdRate, I2S_CONTROL, PcmProfile};
use crate::regs::{MmioOp, Registers, apply};

pub const PERIPHERAL_GATE: u32 = hw::CMU_CLKEN0 as u32;
pub const PERIPHERAL_RESET: u32 = hw::RMU_RSTEN0 as u32;
pub const AUDIO_BLOCK_BITS: u32 = (1 << 14) | 1;
pub const I2S_BLOCK_BIT: u32 = 1 << 14;
pub const I2S_CLOCK: u32 = hw::CMU_I2S_CLK as u32;
pub const I2S_BASE: u32 = hw::I2S_BASE as u32;
pub const I2S_ENABLE: u32 = I2S_BASE + 8;
pub const I2S_TX_ENABLE: u32 = 1;
pub const I2S_TX_DMA: u32 = 1 << 2;
pub const I2S_TX_FROM_SRC: u32 = 1 << 4;
pub const I2S_CONTROL_FIFO_DMA: u32 = 0x0003_0300 | I2S_TX_DMA;
pub const I2S_TX_FIFO: u32 = I2S_BASE + 0x20;
pub const DMA_CHANNEL: u32 = hw::DMA_CH2_BASE as u32;
pub const DMA_SOURCE: u32 = DMA_CHANNEL;
pub const DMA_SOURCE_CONFIG: u32 = DMA_CHANNEL + 4;
pub const DMA_DESTINATION: u32 = DMA_CHANNEL + 8;
pub const DMA_DESTINATION_CONFIG: u32 = DMA_CHANNEL + 12;
pub const DMA_LENGTH: u32 = DMA_CHANNEL + 16;
pub const DMA_CONTROL: u32 = DMA_CHANNEL + 20;
pub const DMA_REMAINING: u32 = DMA_CHANNEL + 24;
pub const DMA_CONTROL_CIRCULAR: u32 = 0x0c55_fc06;
pub const DMA_ENABLE: u32 = hw::DMA_ENABLE as u32;
pub const DMA_INTERRUPT_ENABLE: u32 = hw::DMA_INT_ENABLE as u32;
pub const DMA_STATUS: u32 = hw::DMA_STATUS as u32;
pub const DMA_REQUEST: u32 = hw::DMA_REQUEST as u32;
pub const CHANNEL_BIT: u32 = 1 << 2;
pub const I2S_PINS: [u32; 3] = [hw::GPIO_BASE as u32, hw::GPIO_BASE as u32 + 4, hw::GPIO_BASE as u32 + 8];
pub const I2S_PIN_FUNCTION: u32 = 0x46;
pub const MAX_BUFFER_BYTES: u32 = 0x1_0000;
pub const HALT_POLLS: u32 = 4_096;
pub const HALT_STABLE_READS: u32 = 32;
pub const CURSOR_ATTEMPTS: u32 = 16;
pub const COUNT_MASK: u32 = 0xffff;
pub const PROBE_SAMPLES: usize = 64;
pub const PROBE_STEP_US: u32 = 50;
pub const PROBE_UNITS: [u8; 3] = [1, 4, 8];
pub const PROBE_TOLERANCE: u64 = 4;
pub const PROBE_NOISE: u64 = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum I2sError {
    Buffer,
    StopTimeout,
    ResetStuck,
    StartFailed,
    Cursor,
    NotRunning,
    Probe(ProbeFault),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Down,
    Up,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Decoding {
    pub direction: Direction,
    pub unit_bytes: u8,
    pub bias: u8,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProbeFault {
    Stalled,
    Erratic,
    Pace,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Probe {
    pub result: Result<Decoding, ProbeFault>,
    pub counts_per_second: u32,
}

impl Decoding {
    pub const REMAINING_BYTES: Self = Self { direction: Direction::Down, unit_bytes: 1, bias: 1 };
    pub const LEARNED: u32 = 1 << 31;

    #[inline]
    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub const fn offset(self, raw: u32, ring_bytes: u32) -> Option<u32> {
        let count = (raw & COUNT_MASK) + self.bias as u32;
        let count = match self.direction {
            Direction::Down => count,
            Direction::Up => count & COUNT_MASK,
        };
        let bytes = count * self.unit_bytes as u32;
        if !bytes.is_multiple_of(4) || bytes > ring_bytes {
            return None;
        }
        match self.direction {
            Direction::Down if bytes != 0 => Some(ring_bytes - bytes),
            Direction::Down => None,
            Direction::Up => Some(bytes % ring_bytes),
        }
    }

    pub const fn word(self) -> u32 {
        let up = match self.direction {
            Direction::Down => 0,
            Direction::Up => 1,
        };
        Self::LEARNED | up | (self.unit_bytes as u32) << 8 | (self.bias as u32) << 16
    }

    pub const fn from_word(word: u32) -> Option<Self> {
        if word & Self::LEARNED == 0 {
            return None;
        }
        let direction = if word & 1 == 0 { Direction::Down } else { Direction::Up };
        let unit_bytes = (word >> 8) as u8;
        if unit_bytes == 0 {
            return None;
        }
        Some(Self { direction, unit_bytes, bias: (word >> 16) as u8 })
    }
}

impl ProbeFault {
    pub const fn code(self) -> u32 {
        match self {
            Self::Stalled => 1,
            Self::Erratic => 2,
            Self::Pace => 3,
        }
    }

    pub const fn from_code(code: u32) -> Option<Self> {
        match code {
            1 => Some(Self::Stalled),
            2 => Some(Self::Erratic),
            3 => Some(Self::Pace),
            _ => None,
        }
    }
}

impl Probe {
    pub const fn word(&self) -> u32 {
        match self.result {
            Ok(decoding) => decoding.word(),
            Err(fault) => fault.code() << 24,
        }
    }
}

fn unwrapped(from: u32, to: u32, modulus: i64) -> i64 {
    let delta = (i64::from(to & COUNT_MASK) - i64::from(from & COUNT_MASK)).rem_euclid(modulus);
    if delta > modulus / 2 { delta - modulus } else { delta }
}

fn residue_bias(samples: &[u32]) -> Option<u8> {
    let mut votes = [0u64; 4];
    for sample in samples {
        votes[(sample & 3) as usize] += 1;
    }
    let (residue, count) = votes.iter().copied().enumerate().max_by_key(|(_, count)| *count)?;
    let total = samples.len() as u64;
    if (total - count) * PROBE_NOISE > total {
        return None;
    }
    Some(((4 - residue) & 3) as u8)
}

pub fn infer(samples: &[u32], step_us: u32, rate_hz: u32, ring_bytes: u32) -> Probe {
    let modulus = i64::from(ring_bytes.clamp(8, COUNT_MASK + 1));
    let (mut down, mut up) = (0u64, 0u64);
    for pair in samples.windows(2) {
        let delta = unwrapped(pair[0], pair[1], modulus);
        if delta < 0 {
            down += delta.unsigned_abs();
        } else {
            up += delta as u64;
        }
    }
    let elapsed_us = u64::from(step_us) * (samples.len().max(1) as u64 - 1);
    let (major, minor, direction) = if down >= up { (down, up, Direction::Down) } else { (up, down, Direction::Up) };
    let counts_per_second = ((major - minor) * 1_000_000).checked_div(elapsed_us).unwrap_or(0).min(u64::from(u32::MAX)) as u32;
    let result = if major == 0 || elapsed_us == 0 {
        Err(ProbeFault::Stalled)
    } else if minor * PROBE_NOISE > major {
        Err(ProbeFault::Erratic)
    } else {
        let nominal = u64::from(rate_hz) * FRAME_BYTES as u64;
        let rate = u64::from(counts_per_second);
        match PROBE_UNITS.iter().copied().find(|unit| (rate * u64::from(*unit)).abs_diff(nominal) * PROBE_TOLERANCE <= nominal) {
            None => Err(ProbeFault::Pace),
            Some(1) => residue_bias(samples).map(|bias| Decoding { direction, unit_bytes: 1, bias }).ok_or(ProbeFault::Erratic),
            Some(unit_bytes) => Ok(Decoding { direction, unit_bytes, bias: 1 }),
        }
    };
    Probe { result, counts_per_second }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DmaBuffer {
    address: u32,
    bytes: u32,
}

impl DmaBuffer {
    pub const fn new(address: u32, bytes: u32) -> Result<Self, I2sError> {
        if !address.is_multiple_of(16) || bytes == 0 || !bytes.is_multiple_of(FRAME_BYTES as u32) || bytes > MAX_BUFFER_BYTES {
            return Err(I2sError::Buffer);
        }
        Ok(Self { address, bytes })
    }

    pub const fn address(self) -> u32 {
        self.address
    }

    pub const fn bytes(self) -> u32 {
        self.bytes
    }

    pub const fn frames(self) -> usize {
        self.bytes as usize / FRAME_BYTES
    }
}

pub const fn reset_ops() -> [MmioOp; 3] {
    [
        MmioOp::Update { address: PERIPHERAL_RESET, clear: (1 << 14) | 1, set: 0 },
        MmioOp::Update { address: PERIPHERAL_GATE, clear: 0, set: (1 << 14) | 1 },
        MmioOp::Update { address: PERIPHERAL_RESET, clear: 0, set: (1 << 14) | 1 },
    ]
}

pub const fn start_ops(profile: PcmProfile, buffer: DmaBuffer) -> [MmioOp; 20] {
    start_with(profile.i2s_clock_word(), I2S_CONTROL, buffer)
}

pub const fn start_dsd_ops(mode: DsdMode, rate: DsdRate, buffer: DmaBuffer) -> [MmioOp; 20] {
    start_with(rate.i2s_clock_word(), mode.i2s_control(), buffer)
}

const fn start_with(clock_word: u32, control_word: u32, buffer: DmaBuffer) -> [MmioOp; 20] {
    [
        MmioOp::Update { address: PERIPHERAL_RESET, clear: 0, set: AUDIO_BLOCK_BITS },
        MmioOp::Update { address: PERIPHERAL_GATE, clear: 0, set: AUDIO_BLOCK_BITS },
        MmioOp::Update { address: I2S_PINS[0], clear: 0xcf, set: I2S_PIN_FUNCTION },
        MmioOp::Update { address: I2S_PINS[1], clear: 0xcf, set: I2S_PIN_FUNCTION },
        MmioOp::Update { address: I2S_PINS[2], clear: 0xcf, set: I2S_PIN_FUNCTION },
        MmioOp::Write { address: I2S_CLOCK, value: clock_word },
        MmioOp::Write { address: I2S_BASE, value: control_word },
        MmioOp::Write { address: I2S_ENABLE, value: I2S_CONTROL_FIFO_DMA },
        MmioOp::Write { address: I2S_BASE + 12, value: 6 },
        MmioOp::Write { address: DMA_SOURCE, value: buffer.address },
        MmioOp::Write { address: DMA_SOURCE_CONFIG, value: 3 },
        MmioOp::Write { address: DMA_DESTINATION, value: I2S_TX_FIFO },
        MmioOp::Write { address: DMA_DESTINATION_CONFIG, value: 3 },
        MmioOp::Write { address: DMA_CONTROL, value: DMA_CONTROL_CIRCULAR },
        MmioOp::Write { address: DMA_LENGTH, value: buffer.bytes - 1 },
        MmioOp::Update { address: DMA_REQUEST, clear: 0, set: CHANNEL_BIT },
        MmioOp::Update { address: DMA_INTERRUPT_ENABLE, clear: 0, set: CHANNEL_BIT },
        MmioOp::Write { address: DMA_STATUS, value: CHANNEL_BIT },
        MmioOp::Write { address: DMA_ENABLE, value: (CHANNEL_BIT << 16) | CHANNEL_BIT },
        MmioOp::Update { address: I2S_ENABLE, clear: 0, set: I2S_TX_ENABLE },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct I2sDma {
    buffer: DmaBuffer,
    running: bool,
    last_offset: u32,
    wraps: u32,
    decoding: Decoding,
    completions: u32,
}

impl I2sDma {
    pub const fn new(buffer: DmaBuffer) -> Self {
        Self { buffer, running: false, last_offset: 0, wraps: 0, decoding: Decoding::REMAINING_BYTES, completions: 0 }
    }

    pub const fn decoding(&self) -> Decoding {
        self.decoding
    }

    pub fn set_decoding(&mut self, decoding: Decoding) {
        self.decoding = decoding;
    }

    pub const fn completions(&self) -> u32 {
        self.completions
    }

    pub const fn buffer(&self) -> DmaBuffer {
        self.buffer
    }

    pub const fn running(&self) -> bool {
        self.running
    }

    pub fn reset_blocks<R: Registers>(&mut self, registers: &mut R) -> Result<(), I2sError> {
        apply(registers, &reset_ops());
        self.halt(registers)
    }

    pub fn halt<R: Registers>(&mut self, registers: &mut R) -> Result<(), I2sError> {
        registers.update(I2S_ENABLE, 1, 0);
        registers.write(DMA_ENABLE, CHANNEL_BIT << 16);
        let mut previous = registers.read(DMA_REMAINING);
        let mut stable = 0;
        for _ in 0..HALT_POLLS {
            let remaining = registers.read(DMA_REMAINING);
            if registers.read(DMA_ENABLE) & CHANNEL_BIT == 0 && remaining == previous {
                stable += 1;
                if stable == HALT_STABLE_READS {
                    self.running = false;
                    return Ok(());
                }
            } else {
                stable = 0;
            }
            previous = remaining;
        }
        Err(I2sError::StopTimeout)
    }

    pub fn start<R: Registers>(&mut self, registers: &mut R, profile: PcmProfile) -> Result<(), I2sError> {
        self.start_program(registers, &start_ops(profile, self.buffer))
    }

    pub fn start_dsd<R: Registers>(&mut self, registers: &mut R, mode: DsdMode, rate: DsdRate) -> Result<(), I2sError> {
        self.start_program(registers, &start_dsd_ops(mode, rate, self.buffer))
    }

    fn start_program<R: Registers>(&mut self, registers: &mut R, ops: &[MmioOp; 20]) -> Result<(), I2sError> {
        registers.update(PERIPHERAL_GATE, 0, I2S_BLOCK_BIT);
        self.halt(registers)?;
        registers.update(PERIPHERAL_RESET, I2S_BLOCK_BIT, 0);
        registers.update(PERIPHERAL_GATE, I2S_BLOCK_BIT, 0);
        if registers.read(PERIPHERAL_RESET) & I2S_BLOCK_BIT != 0 {
            return Err(I2sError::ResetStuck);
        }
        self.last_offset = 0;
        self.wraps = 0;
        self.completions = 0;
        apply(registers, ops);
        self.running = true;
        if registers.read(DMA_ENABLE) & CHANNEL_BIT == 0 || registers.read(I2S_ENABLE) & 1 == 0 {
            let _ = self.halt(registers);
            return Err(I2sError::StartFailed);
        }
        Ok(())
    }

    pub fn active<R: Registers>(&self, registers: &mut R) -> bool {
        registers.read(DMA_ENABLE) & CHANNEL_BIT != 0
    }

    pub fn learn<R: Registers>(&mut self, registers: &mut R, rate_hz: u32) -> Result<Probe, I2sError> {
        let mut samples = [0u32; PROBE_SAMPLES];
        self.learn_into(registers, rate_hz, &mut samples)
    }

    pub fn learn_into<R: Registers>(
        &mut self,
        registers: &mut R,
        rate_hz: u32,
        samples: &mut [u32; PROBE_SAMPLES],
    ) -> Result<Probe, I2sError> {
        if !self.running {
            return Err(I2sError::NotRunning);
        }
        for (index, sample) in samples.iter_mut().enumerate() {
            if index != 0 {
                registers.delay_us(PROBE_STEP_US);
            }
            *sample = registers.read(DMA_REMAINING);
        }
        let probe = infer(samples, PROBE_STEP_US, rate_hz, self.buffer.bytes);
        let decoding = probe.result.map_err(I2sError::Probe)?;
        self.decoding = decoding;
        self.wraps = 0;
        self.last_offset = samples.iter().rev().find_map(|raw| decoding.offset(*raw, self.buffer.bytes)).unwrap_or(0);
        Ok(probe)
    }

    #[cfg_attr(target_arch = "riscv32", unsafe(link_section = ".text.hot"))]
    pub fn cursor<R: Registers>(&mut self, registers: &mut R) -> Result<DmaCursor, I2sError> {
        if !self.running {
            return Err(I2sError::NotRunning);
        }
        for _ in 0..CURSOR_ATTEMPTS {
            let before = registers.read(DMA_STATUS) & CHANNEL_BIT;
            let raw = registers.read(DMA_REMAINING);
            let after = registers.read(DMA_STATUS) & CHANNEL_BIT;
            if before != after {
                continue;
            }
            if after != 0 {
                registers.write(DMA_STATUS, CHANNEL_BIT);
                self.completions = self.completions.wrapping_add(1);
            }
            let Some(offset) = self.decoding.offset(raw, self.buffer.bytes) else { continue };
            if offset < self.last_offset {
                self.wraps = self.wraps.wrapping_add(1);
            }
            self.last_offset = offset;
            return Ok(DmaCursor { frame: offset as usize / FRAME_BYTES, wraps: self.wraps });
        }
        Err(I2sError::Cursor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::Recorder;

    impl Counter {
        fn read_and_wait(&mut self) -> u32 {
            let raw = self.raw();
            self.delay_us(PROBE_STEP_US);
            raw
        }
    }

    fn buffer() -> DmaBuffer {
        DmaBuffer::new(0x0010_4000, 19_200).unwrap()
    }

    #[test]
    fn buffers_must_be_aligned_whole_frames_within_the_length_field() {
        assert_eq!(DmaBuffer::new(0x0010_4008, 64), Err(I2sError::Buffer));
        assert_eq!(DmaBuffer::new(0x0010_4000, 0), Err(I2sError::Buffer));
        assert_eq!(DmaBuffer::new(0x0010_4000, 12), Err(I2sError::Buffer));
        assert_eq!(DmaBuffer::new(0x0010_4000, 0x1_0008), Err(I2sError::Buffer));
        assert_eq!(DmaBuffer::new(0x0010_4000, 0x1_0000).unwrap().frames(), 8_192);
        assert_eq!(buffer().frames(), 2_400);
    }

    #[test]
    fn start_feeds_the_tx_fifo_from_memory_and_enables_tx_after_the_dma() {
        let mut r = Recorder::default();
        r.set(PERIPHERAL_RESET, I2S_BLOCK_BIT | 0x0810_008f);
        r.set(DMA_ENABLE, 0);
        let mut dma = I2sDma::new(buffer());
        r.script(DMA_ENABLE, &[0; HALT_STABLE_READS as usize]);
        dma.start(&mut r, PcmProfile::from_hz(44_100).unwrap()).unwrap();
        r.set(DMA_ENABLE, CHANNEL_BIT);
        assert!(dma.running());
        assert_eq!(r.get(I2S_CLOCK), 0xf3);
        assert_eq!(r.get(I2S_BASE), 0x0808_0d94);
        assert_eq!(r.get(I2S_ENABLE), 0x0003_0305);
        assert_eq!(r.get(I2S_ENABLE) & I2S_TX_FROM_SRC, 0);
        assert_eq!(r.get(DMA_SOURCE), 0x0010_4000);
        assert_eq!(r.get(DMA_DESTINATION), 0x2014_0020);
        assert_eq!(r.get(DMA_LENGTH), 19_199);
        assert_eq!(r.get(DMA_CONTROL), 0x0c55_fc06);
        assert_eq!(r.get(PERIPHERAL_RESET) & AUDIO_BLOCK_BITS, AUDIO_BLOCK_BITS);
        for pin in I2S_PINS {
            assert_eq!(r.get(pin), 0x46);
        }
        let writes = r.writes();
        let position = |address: u32| writes.iter().position(|w| w.0 == address).unwrap();
        assert!(position(I2S_CLOCK) < position(DMA_SOURCE));
        assert!(position(DMA_CONTROL) < position(DMA_LENGTH));
        assert!(!writes.iter().any(|w| w.0 == I2S_BASE + 16));
        assert!(position(DMA_ENABLE) < writes.len() - 1);
        assert_eq!(writes.last(), Some(&(I2S_ENABLE, 0x0003_0305)));
    }

    #[test]
    fn a_start_that_does_not_take_is_reported_and_halted() {
        let mut r = Recorder::default();
        let mut dma = I2sDma::new(buffer());
        r.script(DMA_ENABLE, &[0; HALT_STABLE_READS as usize]);
        r.script(DMA_ENABLE, &[0]);
        r.values.insert(DMA_ENABLE, 0);
        assert_eq!(dma.start(&mut r, PcmProfile::from_hz(48_000).unwrap()), Err(I2sError::StartFailed));
        assert!(!dma.running());
        let mut stuck = Recorder::default();
        stuck.set(PERIPHERAL_RESET, I2S_BLOCK_BIT);
        stuck.script(PERIPHERAL_RESET, &[I2S_BLOCK_BIT, I2S_BLOCK_BIT]);
        assert_eq!(I2sDma::new(buffer()).start(&mut stuck, PcmProfile::from_hz(48_000).unwrap()), Err(I2sError::ResetStuck));
    }

    #[test]
    fn halt_waits_for_a_quiet_channel_and_gives_up_after_a_bounded_poll() {
        let mut r = Recorder::default();
        r.set(DMA_ENABLE, CHANNEL_BIT);
        let mut dma = I2sDma::new(buffer());
        assert_eq!(dma.halt(&mut r), Ok(()));
        assert_eq!(r.writes()[1], (DMA_ENABLE, CHANNEL_BIT << 16));
        let mut busy = Recorder::default();
        busy.script(DMA_ENABLE, &[CHANNEL_BIT; HALT_POLLS as usize]);
        assert_eq!(dma.halt(&mut busy), Err(I2sError::StopTimeout));
        let mut moving = Recorder::default();
        let remaining: std::vec::Vec<u32> = (0..=HALT_POLLS).collect();
        moving.script(DMA_REMAINING, &remaining);
        assert_eq!(dma.halt(&mut moving), Err(I2sError::StopTimeout));
    }

    #[test]
    fn cursor_counts_wraps_and_skips_torn_reads() {
        let mut r = Recorder::default();
        let mut dma = I2sDma::new(buffer());
        assert_eq!(dma.cursor(&mut r), Err(I2sError::NotRunning));
        dma.running = true;
        r.set(DMA_REMAINING, 19_200 - 800 - 1);
        assert_eq!(dma.cursor(&mut r), Ok(DmaCursor { frame: 100, wraps: 0 }));
        r.script(DMA_STATUS, &[0, CHANNEL_BIT, CHANNEL_BIT, CHANNEL_BIT]);
        r.set(DMA_REMAINING, 19_200 - 80 - 1);
        assert_eq!(dma.cursor(&mut r), Ok(DmaCursor { frame: 10, wraps: 1 }));
        assert_eq!(r.writes().last(), Some(&(DMA_STATUS, CHANNEL_BIT)));
        r.set(DMA_REMAINING, 0xffff);
        assert_eq!(dma.cursor(&mut r), Err(I2sError::Cursor));
        r.set(DMA_REMAINING, 19_200 - 82 - 1);
        assert_eq!(dma.cursor(&mut r), Err(I2sError::Cursor));
    }

    struct Counter {
        decoding: Decoding,
        bytes_per_second: u64,
        ring: u32,
        elapsed_ns: u64,
    }

    impl Counter {
        fn new(decoding: Decoding, bytes_per_second: u64) -> Self {
            Self { decoding, bytes_per_second, ring: 16_384, elapsed_ns: 0 }
        }

        fn offset(&self) -> u32 {
            let transferred = self.elapsed_ns * self.bytes_per_second / 1_000_000_000;
            ((transferred / 4 * 4) % u64::from(self.ring)) as u32
        }

        fn raw(&self) -> u32 {
            let unit = u32::from(self.decoding.unit_bytes);
            let units = match self.decoding.direction {
                Direction::Down => (self.ring - self.offset()).div_ceil(unit),
                Direction::Up => self.offset() / unit,
            };
            units.wrapping_sub(u32::from(self.decoding.bias)) & COUNT_MASK
        }
    }

    impl Registers for Counter {
        fn read(&mut self, address: u32) -> u32 {
            if address == DMA_REMAINING { self.raw() } else { 0 }
        }

        fn write(&mut self, _: u32, _: u32) {}

        fn delay_us(&mut self, microseconds: u32) {
            self.elapsed_ns += u64::from(microseconds) * 1_000;
        }
    }

    fn running(bytes: u32) -> I2sDma {
        let mut dma = I2sDma::new(DmaBuffer::new(0x0010_4000, bytes).unwrap());
        dma.running = true;
        dma
    }

    fn played(dma: &mut I2sDma, counter: &mut Counter, polls: u32, poll_us: u32) -> u64 {
        let mut position = 0;
        for _ in 0..polls {
            counter.delay_us(poll_us);
            if let Ok(cursor) = dma.cursor(counter) {
                position = u64::from(cursor.wraps) * 2_048 + cursor.frame as u64;
            }
        }
        position
    }

    #[test]
    fn the_remaining_count_is_bytes_minus_one_counting_down_and_all_ones_when_idle() {
        let d = Decoding::REMAINING_BYTES;
        assert_eq!([16_383, 16_379, 3].map(|raw| d.offset(raw, 16_384)), [Some(0), Some(4), Some(16_380)]);
        for invalid in [0xffff, 0xffff_ffff, 16_382, 16_384, 20_000] {
            assert_eq!(d.offset(invalid, 16_384), None, "{invalid:#x}");
        }
        let up = Decoding { direction: Direction::Up, unit_bytes: 1, bias: 1 };
        assert_eq!([0xffff, 3, 16_379, 16_383].map(|raw| up.offset(raw, 16_384)), [Some(0), Some(4), Some(16_380), Some(0)]);
        let words = Decoding { direction: Direction::Down, unit_bytes: 4, bias: 1 };
        assert_eq!([4_095, 0, 4_096].map(|raw| words.offset(raw, 16_384)), [Some(0), Some(16_380), None]);
        assert_eq!(d.offset(0xffff, MAX_BUFFER_BYTES), Some(0));
        for decoding in [d, up, words, Decoding { direction: Direction::Up, unit_bytes: 8, bias: 0 }] {
            assert_eq!(Decoding::from_word(decoding.word()), Some(decoding));
        }
        assert_eq!(Decoding::from_word(0), None);
        let failed = Probe { result: Err(ProbeFault::Pace), counts_per_second: 7 };
        assert_eq!(
            (Decoding::from_word(failed.word()), ProbeFault::from_code(failed.word() >> 24)),
            (None, Some(ProbeFault::Pace))
        );
    }

    #[test]
    fn the_probe_learns_direction_unit_and_bias_from_the_rate_and_the_cursor_then_plays_at_the_rate() {
        let hypotheses = [
            Decoding::REMAINING_BYTES,
            Decoding { direction: Direction::Up, unit_bytes: 1, bias: 1 },
            Decoding { direction: Direction::Up, unit_bytes: 1, bias: 0 },
            Decoding { direction: Direction::Down, unit_bytes: 4, bias: 1 },
            Decoding { direction: Direction::Down, unit_bytes: 8, bias: 1 },
            Decoding { direction: Direction::Up, unit_bytes: 8, bias: 1 },
        ];
        for rate in [44_100u32, 48_000, 96_000] {
            for truth in hypotheses {
                let mut counter = Counter::new(truth, u64::from(rate) * 8);
                let mut dma = running(16_384);
                let probe = dma.learn(&mut counter, rate).unwrap();
                assert_eq!(probe.result, Ok(truth), "{rate} {truth:?}");
                let expected = u64::from(rate) * 8 / u64::from(truth.unit_bytes);
                assert!(u64::from(probe.counts_per_second).abs_diff(expected) * 20 < expected, "{probe:?}");
                let probe_us = u64::from(PROBE_STEP_US) * (PROBE_SAMPLES as u64 - 1);
                let position = played(&mut dma, &mut counter, 2_551, 392);
                let frames = u64::from(rate) * (probe_us + 2_551 * 392) / 1_000_000;
                assert!(position.abs_diff(frames) <= 2, "{rate} {truth:?}: {position} against {frames}");
                assert_eq!(dma.decoding(), truth);
            }
        }
    }

    #[test]
    fn the_05_11_wrap_on_almost_every_poll_comes_from_a_free_running_dma_or_an_up_counter_and_the_probe_tells_them_apart() {
        let unpaced = Counter::new(Decoding::REMAINING_BYTES, 39_800_000);
        let up = Counter::new(Decoding { direction: Direction::Up, unit_bytes: 1, bias: 1 }, 384_000);
        for (mut counter, learned) in
            [(unpaced, Err(ProbeFault::Pace)), (up, Ok(Decoding { direction: Direction::Up, unit_bytes: 1, bias: 1 }))]
        {
            let mut stock = running(16_384);
            let counted = played(&mut stock, &mut counter, 5_103, 389);
            assert!(stock.wraps > 4_600 && stock.wraps < 5_103, "{}", stock.wraps);
            assert!(counted > 9_000_000 && counted < 10_500_000, "{counted}");
            let mut probed = running(16_384);
            assert_eq!(probed.learn(&mut counter, 48_000).map(|probe| probe.result), learned.map_err(I2sError::Probe).map(Ok));
        }
        let mut fast = Counter::new(Decoding::REMAINING_BYTES, 39_800_000);
        let probe =
            infer(&core::array::from_fn::<u32, PROBE_SAMPLES, _>(|_| fast.read_and_wait()), PROBE_STEP_US, 48_000, 16_384);
        assert_eq!(probe.result, Err(ProbeFault::Pace));
        assert!(probe.counts_per_second.abs_diff(39_800_000) < 400_000, "{probe:?}");
    }

    #[test]
    fn a_halted_channel_a_noisy_counter_and_an_unknown_unit_are_refused() {
        let mut dma = running(16_384);
        let mut halted = Recorder::default();
        halted.set(DMA_REMAINING, 0xffff_ffff);
        assert_eq!(dma.learn(&mut halted, 48_000), Err(I2sError::Probe(ProbeFault::Stalled)));
        let noisy: std::vec::Vec<u32> = (0..PROBE_SAMPLES as u32).map(|i| 8_003 + if i % 2 == 0 { 40 } else { 0 }).collect();
        assert_eq!(infer(&noisy, PROBE_STEP_US, 48_000, 16_384).result, Err(ProbeFault::Erratic));
        let slow: std::vec::Vec<u32> = (0..PROBE_SAMPLES as u32).map(|i| 16_383 - 4 * (i / 4)).collect();
        assert_eq!(infer(&slow, PROBE_STEP_US, 48_000, 16_384).result, Err(ProbeFault::Pace));
        let torn: std::vec::Vec<u32> = (0..PROBE_SAMPLES as u32).map(|i| 16_383 - 19 * i).collect();
        assert_eq!(infer(&torn, PROBE_STEP_US, 48_000, 16_384).result, Err(ProbeFault::Erratic));
        assert_eq!(infer(&[], PROBE_STEP_US, 48_000, 16_384).result, Err(ProbeFault::Stalled));
        assert_eq!(I2sDma::new(DmaBuffer::new(0x0010_4000, 64).unwrap()).learn(&mut halted, 48_000), Err(I2sError::NotRunning));
    }

    #[test]
    fn transfer_completions_are_counted_when_the_cursor_clears_them() {
        let mut r = Recorder::default();
        let mut dma = running(19_200);
        r.set(DMA_REMAINING, 19_200 - 800 - 1);
        r.script(DMA_STATUS, &[CHANNEL_BIT, CHANNEL_BIT, 0, 0]);
        dma.cursor(&mut r).unwrap();
        dma.cursor(&mut r).unwrap();
        assert_eq!(dma.completions(), 1);
    }

    #[test]
    fn start_dsd_programs_hardware_dsd_control_and_clock_dividers() {
        for (mode, rate, control, clock) in [
            (DsdMode::Dop, DsdRate::Dsd64, 0x0888_0e94, 0xf3),
            (DsdMode::Dop, DsdRate::Dsd128, 0x0888_0e94, 0x73),
            (DsdMode::Native, DsdRate::Dsd64, 0x08c8_0f94, 0xf3),
            (DsdMode::Native, DsdRate::Dsd128, 0x08c8_0f94, 0x73),
            (DsdMode::Native, DsdRate::Dsd256, 0x08c8_0f94, 0x33),
        ] {
            let mut r = Recorder::default();
            r.set(PERIPHERAL_RESET, I2S_BLOCK_BIT);
            r.set(DMA_ENABLE, 0);
            r.script(DMA_ENABLE, &[0; HALT_STABLE_READS as usize]);
            let mut dma = I2sDma::new(buffer());
            dma.start_dsd(&mut r, mode, rate).unwrap();
            assert_eq!(r.get(I2S_BASE), control);
            assert_eq!(r.get(I2S_CLOCK), clock);
            assert_eq!(r.get(I2S_ENABLE), 0x0003_0305);
        }
    }
}

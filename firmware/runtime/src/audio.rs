// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use core::ptr::{read_volatile, write_volatile};
use kumi_audio::bringup::{self, CountTap, Tone};
use kumi_audio::clock::{self, Calibration, FrameClock};
use kumi_audio::dac::{self, Dac, Filter, Progress, Volume};
use kumi_audio::flow::{DmaCursor, FRAME_BYTES, FrameStore};
use kumi_audio::i2c::{self, Controller, I2C0_BASE};
use kumi_audio::i2s::{self, DmaBuffer, I2sDma, Probe};
use kumi_audio::pcm::PcmProfile;
use kumi_audio::stream::{self, Start};
use kumi_audio::{Registers, board as pins, regs};
use kumi_chip::power::CpuProfile;
use kumi_icd::{
    AUDIO_MONITOR_COMPLETIONS, AUDIO_MONITOR_CYCLES, AUDIO_MONITOR_DECODING, AUDIO_MONITOR_ERRORS, AUDIO_MONITOR_FRAME,
    AUDIO_MONITOR_POLLS, AUDIO_MONITOR_PROBE_RATE, AUDIO_MONITOR_WORDS, AUDIO_MONITOR_WRAPS, Ack, AudioBench, AudioBenchRequest,
    AudioBenchResult, AudioBenchState, AudioCalibration, AudioCursorMap, AudioFailure, AudioFault, AudioHalt, AudioPhase,
    AudioStart, AudioStream, CaseError, ImpedanceReport, ImpedanceResult, TestToneRequest,
};

use crate::board::{self, Mmio};

pub const RING_FRAMES: usize = 2_048;
pub const RING_BYTES: u32 = (RING_FRAMES * FRAME_BYTES) as u32;
const FILTER: Filter = Filter::FastLowLatency;
const TONE_VOLUME_CENTI_DB: i16 = -4_000;
const HALT_SETTLE_US: u32 = 100;
pub const STREAM_I2C_BUDGET_US: u32 = 50_000;
const I2S_STATUS: u32 = i2s::I2S_BASE + 0x10;
const MONITOR_WORDS: usize = AUDIO_MONITOR_WORDS as usize;

#[repr(C, align(16))]
struct Ring([[i32; 2]; RING_FRAMES]);

static mut RING: Ring = Ring([[0; 2]; RING_FRAMES]);
static mut MONITOR: [u32; MONITOR_WORDS] = [0; MONITOR_WORDS];
#[unsafe(no_mangle)]
static mut KUMI_PROBE_TRACE: [u32; i2s::PROBE_SAMPLES] = [0; i2s::PROBE_SAMPLES];

pub struct RingStore;

impl FrameStore for RingStore {
    fn capacity(&self) -> usize {
        RING_FRAMES
    }

    #[inline(always)]
    fn store(&mut self, index: usize, frame: [i32; 2]) {
        if index < RING_FRAMES {
            unsafe {
                let ptr = (&raw mut RING.0).cast::<i32>().add(index * 2);
                write_volatile(ptr, frame[0]);
                write_volatile(ptr.add(1), frame[1]);
            }
        }
    }
}

pub fn ring_address() -> u32 {
    (&raw const RING) as u32
}

fn monitor_address() -> u32 {
    (&raw const MONITOR) as u32
}

pub fn monitor(index: usize) -> u32 {
    unsafe { read_volatile((&raw const MONITOR).cast::<u32>().add(index)) }
}

pub fn record(index: usize, value: u32) {
    unsafe { write_volatile((&raw mut MONITOR).cast::<u32>().add(index), value) };
}

pub fn clear_monitor() {
    for index in 0..MONITOR_WORDS {
        record(index, 0);
    }
}

pub fn record_cursor(dma: &I2sDma, cursor: Result<DmaCursor, i2s::I2sError>) {
    match cursor {
        Ok(cursor) => {
            record(AUDIO_MONITOR_CYCLES, board::cycles());
            record(AUDIO_MONITOR_FRAME, cursor.frame as u32);
            record(AUDIO_MONITOR_WRAPS, cursor.wraps);
            record(AUDIO_MONITOR_POLLS, monitor(AUDIO_MONITOR_POLLS).wrapping_add(1));
        }
        Err(_) => record(AUDIO_MONITOR_ERRORS, monitor(AUDIO_MONITOR_ERRORS).wrapping_add(1)),
    }
    record(AUDIO_MONITOR_COMPLETIONS, dma.completions());
}

static mut CACHED_DECODING: Option<i2s::Decoding> = None;

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn learn(dma: &mut I2sDma, rate_hz: u32) -> Result<Probe, i2s::I2sError> {
    let trace = &raw mut KUMI_PROBE_TRACE;
    let probe = dma.learn_into(&mut Mmio, rate_hz, unsafe { &mut *trace });
    match probe {
        Ok(probe) => {
            if let Ok(decoding) = probe.result {
                unsafe { write_volatile(&raw mut CACHED_DECODING, Some(decoding)) };
            }
            record(AUDIO_MONITOR_DECODING, probe.word());
            record(AUDIO_MONITOR_PROBE_RATE, probe.counts_per_second);
        }
        Err(i2s::I2sError::Probe(fault)) => record(AUDIO_MONITOR_DECODING, fault.code() << 24),
        Err(_) => {}
    }
    probe
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn learn_cached(dma: &mut I2sDma, rate_hz: u32) -> Result<(), i2s::I2sError> {
    if let Some(decoding) = unsafe { read_volatile(&raw const CACHED_DECODING) } {
        dma.set_decoding(decoding);
        return Ok(());
    }
    learn(dma, rate_hz).map(|_| ())
}

pub fn cursor_map() -> AudioCursorMap {
    AudioCursorMap {
        remaining: i2s::DMA_REMAINING,
        status: i2s::DMA_STATUS,
        enable: i2s::DMA_ENABLE,
        i2s_status: I2S_STATUS,
        monitor: monitor_address(),
    }
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn power_down_dacs() {
    let mut mmio = Mmio;
    regs::apply(&mut mmio, &i2c::SETUP);
    let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(STREAM_I2C_BUDGET_US);
    let dacs = dac::answering(&mut bus);
    for dac in dacs {
        let _ = dac::run_blocking(&mut bus, &dac::power_down(dac), |ms| {
            board::kick_watchdog();
            mmio.delay_us(1_000 * u32::from(ms));
        });
    }
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn prepare_dacs() -> Result<heapless::Vec<Dac, 2>, AudioFault> {
    let mut mmio = Mmio;
    regs::apply(&mut mmio, &pins::POWER_ON);
    regs::apply(&mut mmio, &i2c::SETUP);
    if let Err(mismatch) = regs::verify(&mut mmio, &i2c::SETUP_EXPECT) {
        regs::apply(&mut mmio, &pins::QUIET);
        regs::apply(&mut mmio, &pins::POWER_OFF);
        board::keep_awake();
        return Err(mismatch.into());
    }
    regs::apply(&mut mmio, &pins::DAC_RESET_PULSE);
    let dacs = dac::answering(&mut Controller::new(Mmio, I2C0_BASE));
    if !dacs.contains(&Dac::Primary) {
        regs::apply(&mut mmio, &pins::QUIET);
        regs::apply(&mut mmio, &pins::POWER_OFF);
        board::keep_awake();
        return Err(AudioFault::NoDac);
    }
    Ok(dacs)
}

enum State {
    Idle,
    Starting { profile: PcmProfile, tone: bool, began_us: u64 },
    Running { dma: I2sDma },
    Failed,
}

pub struct Bench {
    last_profile: CpuProfile,
    frame_seen: Option<u16>,
    frame_clock: Option<FrameClock>,
    calibration: Option<Calibration>,
    report: Option<AudioCalibration>,
    state: State,
    starter: Option<Start>,
    stream: Option<AudioStream>,
    failure: Option<AudioFailure>,
    halt: Option<AudioHalt>,
    usb: Option<UsbView>,
    test_tone_until_ms: Option<u32>,
}

#[derive(Clone, Copy)]
pub struct UsbView {
    pub state: AudioBenchState,
    pub stream: AudioStream,
    pub failure: Option<AudioFailure>,
}

impl Bench {
    pub fn new() -> Self {
        Self {
            last_profile: board::cpu_profile(),
            frame_seen: None,
            frame_clock: None,
            calibration: None,
            report: None,
            state: State::Idle,
            starter: None,
            stream: None,
            failure: None,
            halt: None,
            usb: None,
            test_tone_until_ms: None,
        }
    }

    pub fn set_usb(&mut self, usb: Option<UsbView>) {
        self.usb = usb;
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    pub fn calibration(&mut self) -> Result<Calibration, AudioFault> {
        if let Some(calibration) = self.calibration {
            return Ok(calibration);
        }
        board::kick_watchdog();
        let mut mmio = Mmio;
        clock::quiesce_timers(&mut mmio);
        let calibration = Calibration::run(&mut mmio).map_err(|error| AudioFault::Clock(error.into()))?;
        board::kick_watchdog();
        self.calibration = Some(calibration);
        Ok(calibration)
    }

    pub fn poll(&mut self, unlocked: bool, now_ms: u32) {
        let profile = board::cpu_profile();
        if profile != self.last_profile {
            self.last_profile = profile;
            self.frame_clock = None;
        }
        let cycles = board::monotonic_cycles();
        let frame = kumi_chip::regs::usb_frame();
        if self.frame_seen.replace(frame).is_some_and(|seen| seen != frame) {
            match self.frame_clock.as_mut() {
                Some(clock) => clock.observe(cycles, frame),
                None => self.frame_clock = Some(FrameClock::start(cycles, frame)),
            }
        }
        if let Some(until) = self.test_tone_until_ms {
            if now_ms.wrapping_sub(until) as i32 >= 0 {
                self.test_tone_until_ms = None;
                let _ = self.stop();
                return;
            }
        } else if !unlocked && self.busy() {
            let _ = self.stop();
            return;
        }
        match &mut self.state {
            State::Starting { .. } => self.advance(),
            State::Running { dma } => {
                let cursor = dma.cursor(&mut Mmio);
                record_cursor(dma, cursor);
            }
            State::Idle | State::Failed => {
                self.test_tone_until_ms = None;
            }
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    pub fn measure_impedance(&mut self) -> ImpedanceResult {
        if self.busy() || self.usb.is_some_and(|usb| usb.state != AudioBenchState::Failed) {
            return Err(CaseError::Busy);
        }
        let mut mmio = Mmio;
        let cal = self.calibration().map_err(|_| CaseError::HardwareFault)?;
        if mmio.read(clock::AUDIO_PLL) & clock::PLL_ENABLE == 0 {
            cal.apply(&mut mmio, clock::Family::Rate48k);
        }
        let dacs = match prepare_dacs() {
            Ok(dacs) if dacs.contains(&Dac::Primary) => dacs,
            _ => {
                regs::apply(&mut mmio, &pins::QUIET);
                regs::apply(&mut mmio, &pins::POWER_OFF);
                board::keep_awake();
                return Err(CaseError::HardwareFault);
            }
        };
        let _ = dacs;
        let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(STREAM_I2C_BUDGET_US);
        let reading = dac::run_blocking(&mut bus, &dac::prepare_clocking(Dac::Primary), |ms| {
            board::kick_watchdog();
            mmio.delay_us(1_000 * u32::from(ms));
        })
        .and_then(|()| {
            dac::measure_dc_impedance(&mut bus, Dac::Primary, |ms| {
                board::kick_watchdog();
                mmio.delay_us(1_000 * u32::from(ms));
            })
        });
        let _ = dac::run_blocking(&mut bus, &dac::power_down(Dac::Primary), |ms| {
            board::kick_watchdog();
            mmio.delay_us(1_000 * u32::from(ms));
        });
        regs::apply(&mut mmio, &pins::QUIET);
        regs::apply(&mut mmio, &pins::POWER_OFF);
        board::keep_awake();
        match reading {
            Ok(meas) => Ok(ImpedanceReport {
                left_ohms: meas.left_ohms,
                right_ohms: meas.right_ohms,
                left_ac_ohms: meas.left_ac_ohms,
                right_ac_ohms: meas.right_ac_ohms,
                left_status: meas.left_status,
                right_status: meas.right_status,
                hp_status: meas.hp_status,
                load_stat: meas.load_stat,
            }),
            Err(_) => Err(CaseError::HardwareFault),
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    pub fn start_test_tone(&mut self, request: TestToneRequest, now_ms: u32) -> Ack {
        if request.frequency_hz == 0 {
            if self.test_tone_until_ms.take().is_some() {
                let _ = self.stop();
            }
            return Ok(());
        }
        if self.usb.is_some_and(|usb| usb.state != AudioBenchState::Failed) || (self.busy() && self.test_tone_until_ms.is_none())
        {
            return Err(CaseError::Busy);
        }
        let rate_hz = 48_000u32;
        let tone = Tone::sine(rate_hz, u32::from(request.frequency_hz), -2_000, request.channel.slots(), RING_FRAMES)
            .ok_or(CaseError::InvalidArgument)?
            .with_inverted_right(request.invert_right);
        let profile = PcmProfile::from_hz(rate_hz).ok_or(CaseError::InvalidArgument)?;
        if self.test_tone_until_ms.take().is_some() {
            let _ = self.stop();
        }
        let calibration = self.calibration().map_err(|_| CaseError::HardwareFault)?;
        let start =
            AudioStart { rate_hz, channel: request.channel, level_centi_dbfs: -2_000, tone_hz: u32::from(request.frequency_hz) };
        self.begin_tone(start, profile, tone, calibration)?;
        self.test_tone_until_ms = Some(now_ms.wrapping_add(u32::from(request.duration_ms)));
        Ok(())
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    pub fn request(&mut self, request: AudioBenchRequest) -> AudioBenchResult {
        if self.usb.is_some_and(|usb| usb.state != AudioBenchState::Failed) && request != AudioBenchRequest::Status {
            return Err(CaseError::Busy);
        }
        match request {
            AudioBenchRequest::Calibrate => self.calibrate(),
            AudioBenchRequest::Start(start) => self.start(start),
            AudioBenchRequest::Stop => self.stop(),
            AudioBenchRequest::Status => Ok(self.snapshot()),
        }
    }

    pub fn busy(&self) -> bool {
        matches!(self.state, State::Starting { .. } | State::Running { .. })
    }

    fn cpu_hz(&self) -> Option<u32> {
        self.frame_clock.as_ref().and_then(FrameClock::cpu_hz)
    }

    fn fail(&mut self, phase: AudioPhase, fault: AudioFault) {
        self.state = State::Failed;
        self.failure = Some(AudioFailure { phase, fault });
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn snapshot(&self) -> AudioBench {
        let usb = self.usb.filter(|_| matches!(self.state, State::Idle | State::Failed));
        AudioBench {
            state: match (&self.state, usb) {
                (_, Some(usb)) => usb.state,
                (State::Idle, None) => AudioBenchState::Idle,
                (State::Starting { .. }, _) => AudioBenchState::Starting,
                (State::Running { .. }, _) => AudioBenchState::Running,
                (State::Failed, None) => AudioBenchState::Failed,
            },
            cpu_hz: self.cpu_hz(),
            calibration: self.report.clone(),
            stream: usb.map(|usb| usb.stream).or(self.stream),
            failure: usb.map_or(self.failure, |usb| usb.failure),
            halt: self.halt,
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn calibrate(&mut self) -> AudioBenchResult {
        if self.busy() {
            return Err(CaseError::Busy);
        }
        let mut mmio = Mmio;
        let system_clock = mmio.read(clock::SYSTEM_CLOCK);
        let core_pll = mmio.read(clock::CORE_PLL);
        let decoded_cpu_hz = clock::read_cpu_hz(&mut mmio);
        let timers_quiesced = clock::quiesce_timers(&mut mmio);
        let began = board::cycles();
        board::kick_watchdog();
        let mut tap = CountTap::new(&mut mmio);
        let result = Calibration::run(&mut tap);
        board::kick_watchdog();
        let counts = tap.into_counts();
        let cycles = board::cycles().wrapping_sub(began);
        let pll = [clock::AUDIO_PLL, clock::AUDIO_PLL_FRACTION, clock::AUDIO_PLL_TRIM].map(|address| mmio.read(address));
        self.calibration = result.ok();
        let (fault, trims, windows) = match result {
            Ok(calibration) => (None, calibration.trims, calibration.windows),
            Err(error) => (Some(error.into()), [0; 2], [0; 2]),
        };
        self.report = Some(AudioCalibration {
            system_clock,
            core_pll,
            decoded_cpu_hz,
            frame_clock_hz: self.cpu_hz(),
            timers_quiesced,
            fault,
            trims,
            windows,
            counts,
            cycles,
            pll,
        });
        if fault.is_some() { Err(CaseError::HardwareFault) } else { Ok(self.snapshot()) }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn begin_tone(&mut self, request: AudioStart, profile: PcmProfile, tone: Tone, calibration: Calibration) -> Ack {
        board::set_cpu_profile(if profile.hz() >= 88_200 { CpuProfile::Turbo360 } else { CpuProfile::Idle90 });
        let began_us = board::monotonic_us();
        tone.fill(&mut RingStore);
        clear_monitor();
        self.failure = None;
        self.halt = None;
        self.stream = Some(AudioStream {
            start: request,
            amplitude: tone.amplitude(),
            ring_address: ring_address(),
            ring_bytes: (tone.frames() * FRAME_BYTES) as u32,
            periods: tone.periods(),
            started_us: None,
            dacs: 0,
            cursor: cursor_map(),
        });
        let mut mmio = Mmio;
        let dacs = match prepare_dacs() {
            Ok(dacs) => dacs,
            Err(fault) => {
                board::set_cpu_profile(CpuProfile::Idle90);
                self.fail(AudioPhase::Setup, fault);
                return Err(CaseError::HardwareFault);
            }
        };
        if let Some(stream) = self.stream.as_mut() {
            stream.dacs = dac::mask(&dacs);
        }
        let now = board::monotonic_us();
        self.starter = Some(Start::pcm_on(&mut mmio, &calibration, profile, FILTER, Volume::MUTED, now, &dacs));
        self.state = State::Starting { profile, tone: request.tone_hz != 0, began_us };
        Ok(())
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn start(&mut self, request: AudioStart) -> AudioBenchResult {
        if self.busy() {
            return Err(CaseError::Busy);
        }
        let profile = PcmProfile::from_hz(request.rate_hz).ok_or(CaseError::InvalidArgument)?;
        let tone = match request.tone_hz {
            0 => Tone::silence(RING_FRAMES),
            hz => Tone::sine(request.rate_hz, hz, request.level_centi_dbfs, request.channel.slots(), RING_FRAMES),
        }
        .ok_or(CaseError::InvalidArgument)?;
        let calibration = self.calibration.ok_or(CaseError::WrongState)?;
        self.begin_tone(request, profile, tone, calibration)?;
        Ok(self.snapshot())
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn advance(&mut self) {
        let now = board::monotonic_us();
        let Some(start) = self.starter.as_mut() else { return };
        let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(STREAM_I2C_BUDGET_US);
        let outcome = start.poll(&mut Mmio, &mut bus, now);
        let stage = start.stage();
        match outcome {
            Ok(Progress::Pending { .. }) => {}
            Ok(Progress::Done) => {
                self.starter = None;
                self.launch();
            }
            Err(error) => {
                if let Some(start) = self.starter.take() {
                    start.abort(&mut Mmio);
                }
                power_down_dacs();
                regs::apply(&mut Mmio, &pins::POWER_OFF);
                board::set_cpu_profile(CpuProfile::Idle90);
                board::keep_awake();
                self.fail(bringup::phase(stage), error.into());
            }
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn launch(&mut self) {
        let State::Starting { profile, tone, began_us, .. } = core::mem::replace(&mut self.state, State::Failed) else {
            return;
        };
        let mut mmio = Mmio;
        let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(STREAM_I2C_BUDGET_US);
        let bytes = self.stream.map_or(0, |stream| stream.ring_bytes);
        let mut dma = match DmaBuffer::new(ring_address(), bytes) {
            Ok(buffer) => I2sDma::new(buffer),
            Err(error) => {
                regs::apply(&mut mmio, &pins::QUIET);
                regs::apply(&mut mmio, &pins::POWER_OFF);
                board::set_cpu_profile(CpuProfile::Idle90);
                board::keep_awake();
                return self.fail(AudioPhase::Dma, AudioFault::Dma(error.into()));
            }
        };
        if let Err(error) = dma.start(&mut mmio, profile) {
            let _ = stream::stop(&mut mmio, &mut bus, &mut dma);
            regs::apply(&mut mmio, &pins::POWER_OFF);
            board::set_cpu_profile(CpuProfile::Idle90);
            board::keep_awake();
            return self.fail(AudioPhase::Dma, AudioFault::Dma(error.into()));
        }
        let _ = learn(&mut dma, profile.hz());
        board::keep_awake();
        if tone {
            regs::apply_one(&mut mmio, pins::output(true));
            let unmuted =
                dac::run_blocking(&mut bus, &dac::volume(Volume::from_settings([TONE_VOLUME_CENTI_DB; 2], false, 0, 0)), |ms| {
                    mmio.delay_us(1_000 * u32::from(ms))
                });
            if let Err(error) = unmuted {
                let _ = stream::stop(&mut mmio, &mut bus, &mut dma);
                regs::apply(&mut mmio, &pins::POWER_OFF);
                board::set_cpu_profile(CpuProfile::Idle90);
                board::keep_awake();
                return self.fail(AudioPhase::Unmute, AudioFault::Dac(error.into()));
            }
        }
        let elapsed = board::monotonic_us().saturating_sub(began_us);
        if let Some(stream) = self.stream.as_mut() {
            stream.started_us = Some(u32::try_from(elapsed).unwrap_or(u32::MAX));
        }
        self.state = State::Running { dma };
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn stop(&mut self) -> AudioBenchResult {
        self.test_tone_until_ms = None;
        let mut mmio = Mmio;
        let began = board::cycles();
        let result = match core::mem::replace(&mut self.state, State::Idle) {
            State::Running { mut dma } => {
                let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(STREAM_I2C_BUDGET_US);
                let stopped = stream::stop(&mut mmio, &mut bus, &mut dma);
                regs::apply(&mut mmio, &pins::QUIET);
                regs::apply(&mut mmio, &pins::POWER_OFF);
                let first = mmio.read(i2s::DMA_REMAINING);
                mmio.delay_us(HALT_SETTLE_US);
                let remaining = [first, mmio.read(i2s::DMA_REMAINING)];
                let enable = mmio.read(i2s::DMA_ENABLE);
                self.halt = Some(AudioHalt { cycles: board::cycles().wrapping_sub(began), enable, remaining });
                stopped
            }
            State::Starting { .. } => {
                if let Some(start) = self.starter.take() {
                    start.abort(&mut mmio);
                }
                power_down_dacs();
                regs::apply(&mut mmio, &pins::POWER_OFF);
                Ok(())
            }
            State::Idle | State::Failed => {
                power_down_dacs();
                regs::apply(&mut mmio, &pins::QUIET);
                regs::apply(&mut mmio, &pins::POWER_OFF);
                Ok(())
            }
        };
        board::set_cpu_profile(CpuProfile::Idle90);
        board::keep_awake();
        match result {
            Ok(()) => Ok(self.snapshot()),
            Err(error) => {
                self.fail(AudioPhase::Stop, error.into());
                Err(CaseError::HardwareFault)
            }
        }
    }
}

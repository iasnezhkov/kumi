// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use embassy_usb::Builder;
use embassy_usb::control::{InResponse, OutResponse, Request};
use embassy_usb::descriptor::{SynchronizationType, UsageType};
use embassy_usb::types::InterfaceNumber;
use embassy_usb_driver::{Direction, Endpoint as _, EndpointAddress, EndpointInfo, EndpointType};
use heapless::Vec;
use kumi_audio::bringup;
use kumi_audio::dac::{self, Dac, Progress, Runner, Volume};
use kumi_audio::flow::{self, DOP_MARKER_A, DopDetector, Flow, Observation};
use kumi_audio::i2c::{self, CONFIG_100KHZ, CONFIG_400KHZ, Controller, I2C0_BASE};
use kumi_audio::i2s::{DmaBuffer, I2sDma, I2sError};
use kumi_audio::pcm::{DSD_SILENCE_WORD, DsdMode, DsdRate, PcmProfile, SampleFormat, UsbSpeed};
use kumi_audio::stream::{self, Start};
use kumi_audio::uac2::{self, Change, Config, Control, RequestError, Setup, Walk};
use kumi_audio::{board as pins, regs};
use kumi_chip::power::CpuProfile;
use kumi_dsp::Equalizer;
use kumi_icd::{
    AUDIO_MONITOR_FEEDBACK, AUDIO_MONITOR_LOST_PACKETS, AUDIO_MONITOR_OVERRUNS, AUDIO_MONITOR_PACKETS, AUDIO_MONITOR_PAUSES,
    AUDIO_MONITOR_QUEUED, AUDIO_MONITOR_UNDERRUNS, AudioBenchState, AudioChannel, AudioCounters, AudioFailure, AudioFault,
    AudioMeters, AudioPhase, AudioSpectrum, AudioStart, AudioStatus, AudioStream, DacExtras, DspExtras, DspSettings,
    EqBaseSettings, Jack, OutputSettings, StreamFormat, StreamKind, ToneEqSettings,
};

use crate::app::Shared;
use crate::audio::{self, RING_BYTES, RING_FRAMES, RingStore, UsbView};
use crate::board::{self, Mmio};
use crate::usb;

pub const CONTROL_INTERFACE: u8 = 2;
pub const OUT_ENDPOINT: u8 = 0x07;
pub const FEEDBACK_ENDPOINT: u8 = 0x84;
pub const MAX_RATE_HZ: u32 = 384_000;
const HIGH_SPEED_DATA_INTERVAL: u8 = 2;
const PACKET_BUFFER: usize = 776;
const MAX_PACKET_FRAMES: usize = PACKET_BUFFER / 4;
const PACKETS_PER_POLL: usize = 4;
const HOST_PAUSE_US: u32 = 8_000;
const PUBLISH_INTERVAL_MS: u32 = 4;
const METER_INTERVAL_MS: u32 = 30;
const SPECTRUM_INTERVAL_MS: u32 = 25;
const SPECTRUM_MIN_INTERVAL_MS: u32 = 18;

const fn max_eq_bands(_rate_hz: u32) -> usize {
    kumi_dsp::MAX_BANDS
}

const fn stream_cpu_profile(rate_hz: u32, transport: Transport, dsp_active: bool) -> CpuProfile {
    match transport {
        Transport::NativeDsd(_) => CpuProfile::Turbo360,
        Transport::Dop(_, _) => CpuProfile::Fast180,
        Transport::Pcm(_) => {
            if rate_hz >= 352_800 || (dsp_active && rate_hz >= 176_400) {
                CpuProfile::Turbo360
            } else if dsp_active || rate_hz >= 88_200 {
                CpuProfile::Fast180
            } else {
                CpuProfile::Idle90
            }
        }
    }
}

pub const fn config(speed: usb::Speed) -> Config {
    let (speed, data_interval, raw_alternate) = match speed {
        usb::Speed::High => (UsbSpeed::High, HIGH_SPEED_DATA_INTERVAL, true),
        usb::Speed::Full => (UsbSpeed::Full, 1, false),
    };
    Config {
        speed,
        control_interface: CONTROL_INTERFACE,
        out_endpoint: OUT_ENDPOINT,
        feedback_endpoint: FEEDBACK_ENDPOINT,
        raw_alternate,
        data_interval,
        max_rate_hz: MAX_RATE_HZ,
    }
}

#[unsafe(link_section = ".text.cold")]
#[inline(never)]
pub fn build<'d>(builder: &mut Builder<'d, usb::Driver<'d>>, config: &Config) -> Option<(usb::Endpoint<'d>, usb::Endpoint<'d>)> {
    let mut bytes = [0u8; uac2::MAX_FUNCTION_BYTES];
    let length = uac2::function_descriptor(config, &mut bytes).ok()?;
    let mut function = builder.function(uac2::CLASS_AUDIO, 0, uac2::PROTOCOL_V2);
    let mut interface = None;
    let mut alt = None;
    let mut out: Option<usb::Endpoint<'d>> = None;
    let mut feedback: Option<usb::Endpoint<'d>> = None;
    for part in Walk::new(&bytes[..length]) {
        match part[1] {
            uac2::INTERFACE_ASSOCIATION => {}
            uac2::INTERFACE => {
                if part[3] == 0 {
                    interface = Some(function.interface());
                    if interface.as_ref()?.interface_number().0 != part[2] {
                        return None;
                    }
                }
                alt = interface.as_mut().map(|i| i.alt_setting(part[5], part[6], part[7], None));
            }
            uac2::ENDPOINT => {
                let alt = alt.as_mut()?;
                let address = EndpointAddress::from(part[2]);
                let packet = u16::from_le_bytes([part[4], part[5]]);
                let (slot, size, sync, usage) = if address.is_in() {
                    (&mut feedback, packet, SynchronizationType::NoSynchronization, UsageType::FeedbackEndpoint)
                } else {
                    (&mut out, config.largest_packet(), SynchronizationType::Asynchronous, UsageType::DataEndpoint)
                };
                if slot.is_none() {
                    *slot = Some(if address.is_in() {
                        alt.alloc_endpoint_in(EndpointType::Isochronous, Some(address), size, part[6])
                    } else {
                        alt.alloc_endpoint_out(EndpointType::Isochronous, Some(address), size, part[6])
                    });
                }
                let info = *slot.as_ref()?.info();
                alt.endpoint_descriptor(&EndpointInfo { max_packet_size: packet, ..info }, sync, usage, &[]);
            }
            kind => alt.as_mut()?.descriptor(kind, &part[2..]),
        }
    }
    Some((out?, feedback?))
}

pub struct Function {
    present: bool,
    jack: Jack,
    control: Control,
    alternate: Option<u8>,
    restart: bool,
    volume: bool,
    local_mute: bool,
    ota_silence: bool,
    speed_ok: bool,
    calibration_failed: bool,
    stream: Option<StreamFormat>,
    eq_active: Option<u8>,
    counters: AudioCounters,
    meters: AudioMeters,
    meters_consumed: bool,
    spectrum: AudioSpectrum,
    spectrum_consumed: bool,
    spectrum_until_ms: Option<u32>,
}

impl Function {
    pub fn new(config: Config) -> Self {
        Self {
            present: false,
            jack: Jack::Unknown,
            control: Control::new(config),
            alternate: None,
            restart: false,
            volume: false,
            local_mute: false,
            ota_silence: false,
            speed_ok: true,
            calibration_failed: false,
            stream: None,
            eq_active: None,
            counters: AudioCounters::default(),
            meters: AudioMeters::default(),
            meters_consumed: false,
            spectrum: AudioSpectrum::default(),
            spectrum_consumed: false,
            spectrum_until_ms: None,
        }
    }

    pub fn toggle_local_mute(&mut self) -> bool {
        self.local_mute = !self.local_mute;
        self.volume = true;
        self.local_mute
    }

    pub fn set_ota_silence(&mut self, silence: bool) {
        self.ota_silence = silence;
        self.volume = true;
    }

    pub fn take_meters(&mut self) -> AudioMeters {
        self.meters_consumed = true;
        self.meters
    }

    pub fn take_spectrum(&mut self, now_ms: u32) -> AudioSpectrum {
        self.spectrum_consumed = true;
        self.spectrum_until_ms = Some(now_ms.wrapping_add(1_500));
        self.spectrum
    }

    fn spectrum_wanted(&self, now_ms: u32) -> bool {
        self.spectrum_until_ms.is_some_and(|until| until.wrapping_sub(now_ms) < 0x8000_0000)
    }

    pub fn set_present(&mut self) {
        self.present = true;
    }

    pub fn present(&self) -> bool {
        self.present
    }

    pub fn set_jack(&mut self, jack: Jack) {
        self.jack = jack;
    }

    pub fn jack(&self) -> Jack {
        self.jack
    }

    fn refresh_clock(&mut self) {
        self.control.set_clock_valid(self.speed_ok && !self.calibration_failed);
    }

    fn take(&mut self) -> (Option<u8>, bool, bool) {
        (self.alternate.take(), core::mem::take(&mut self.restart), core::mem::take(&mut self.volume))
    }

    pub fn audio_status(&self) -> AudioStatus {
        AudioStatus {
            jack: self.jack,
            usb_audio_present: self.present,
            stream: self.stream,
            host_volume_centi_db: if self.present { self.control.host_volume_centi_db() } else { [0; 2] },
            host_muted: self.control.state().mute || self.local_mute,
            eq_active: self.eq_active,
            counters: self.counters,
        }
    }

    pub fn set_eq_active(&mut self, eq_active: Option<u8>) {
        self.eq_active = eq_active;
    }

    pub fn eq_profile(&self) -> u8 {
        self.eq_active.map_or(0, |slot| slot + 1)
    }

    pub fn link(&self) -> (u32, Option<StreamKind>, u8) {
        if !self.present {
            return (0, None, 0);
        }
        let bits = self.stream.map_or(0, |stream| stream.bits);
        (self.control.state().rate, self.stream.map(|stream| stream.kind), bits)
    }
}

pub struct Requests<'a>(pub &'a Shared, pub &'a usb::State);

fn setup(request: &Request) -> Setup {
    let direction = match request.direction {
        Direction::In => 0x80,
        Direction::Out => 0,
    };
    Setup {
        request_type: direction | (request.request_type as u8) << 5 | request.recipient as u8,
        request: request.request,
        value: request.value,
        index: request.index,
        length: request.length,
    }
}

impl embassy_usb::Handler for Requests<'_> {
    #[unsafe(link_section = ".text.cold")]
    fn reset(&mut self) {
        let mut function = self.0.uac.borrow_mut();
        function.speed_ok = self.1.high_speed() == (function.control.config().speed == UsbSpeed::High);
        function.calibration_failed = false;
        function.refresh_clock();
        function.control.reset();
        function.alternate = Some(0);
    }

    #[unsafe(link_section = ".text.cold")]
    fn set_alternate_setting(&mut self, iface: InterfaceNumber, alternate_setting: u8) {
        let mut function = self.0.uac.borrow_mut();
        if let Ok(Some(Change::Alternate(alternate))) = function.control.set_alternate(iface.0, alternate_setting) {
            function.alternate = Some(alternate);
            for address in [OUT_ENDPOINT, FEEDBACK_ENDPOINT] {
                self.1.set_enabled(EndpointAddress::from(address), alternate != 0);
            }
        }
    }

    #[unsafe(link_section = ".text.cold")]
    fn control_out(&mut self, req: Request, data: &[u8]) -> Option<OutResponse> {
        let mut function = self.0.uac.borrow_mut();
        match function.control.control_out(&setup(&req), data) {
            Ok(Change::Rate(_)) => function.restart = true,
            Ok(Change::Mute(_) | Change::Volume { .. }) => function.volume = true,
            Ok(Change::Alternate(_)) => {}
            Err(RequestError::Stall) => return Some(OutResponse::Rejected),
            Err(RequestError::NotAudio) => return None,
        }
        Some(OutResponse::Accepted)
    }

    #[unsafe(link_section = ".text.cold")]
    fn control_in<'a>(&'a mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        match self.0.uac.borrow().control.control_in(&setup(&req), buf) {
            Ok(length) => Some(InResponse::Accepted(&buf[..length])),
            Err(RequestError::Stall) => Some(InResponse::Rejected),
            Err(RequestError::NotAudio) => None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Transport {
    Pcm(SampleFormat),
    Dop(SampleFormat, DsdRate),
    NativeDsd(DsdRate),
}

impl Transport {
    const fn is_dsd(self) -> bool {
        !matches!(self, Self::Pcm(_))
    }

    const fn dsd(self) -> Option<(DsdMode, DsdRate)> {
        match self {
            Self::Pcm(_) => None,
            Self::Dop(_, rate) => Some((DsdMode::Dop, rate)),
            Self::NativeDsd(rate) => Some((DsdMode::Native, rate)),
        }
    }

    const fn stream_format(self, profile: PcmProfile) -> StreamFormat {
        match self {
            Self::Pcm(format) => profile.stream_format(format),
            Self::Dop(_, rate) => rate.stream_format(DsdMode::Dop),
            Self::NativeDsd(rate) => rate.stream_format(DsdMode::Native),
        }
    }
}

enum Phase {
    Idle,
    Starting { profile: PcmProfile, transport: Transport },
    Priming { profile: PcmProfile, transport: Transport },
    Running { dma: I2sDma, profile: PcmProfile, transport: Transport, waiting: bool },
    Failed,
}

pub struct Stream<'d> {
    out: usb::Endpoint<'d>,
    feedback: usb::Endpoint<'d>,
    speed: UsbSpeed,
    phase: Phase,
    starter: Option<Start>,
    flow: Flow,
    dop: DopDetector,
    pending_transport: Option<Transport>,
    equalizer: Equalizer,
    settings_rev: u32,
    last_output: Option<OutputSettings>,
    last_extras: Option<DacExtras>,
    frame_buf: [i32; MAX_PACKET_FRAMES * 2],
    dacs: Vec<Dac, 2>,
    volume: Option<Runner>,
    applied_volume: Option<Volume>,
    volume_dirty: bool,
    volume_retries: u8,
    alternate: u8,
    suspended_alternate: Option<u8>,
    ota_suspended: Option<u8>,
    dop_marker: u8,
    packets: u32,
    lost: u32,
    dac_faults: u32,
    requested_us: u64,
    started_us: Option<u32>,
    failure: Option<AudioFailure>,
    last_packet_cycles: u32,
    publish_ms: u32,
    meter_ms: u32,
    spectrum_ms: u32,
    pauses: u32,
}

impl<'d> Stream<'d> {
    pub fn new(out: usb::Endpoint<'d>, feedback: usb::Endpoint<'d>, config: &Config) -> Self {
        Self {
            out,
            feedback,
            speed: config.speed,
            phase: Phase::Idle,
            starter: None,
            flow: Flow::new(RING_FRAMES, RING_FRAMES / 2),
            dop: DopDetector::new(),
            pending_transport: None,
            equalizer: Equalizer::new(),
            settings_rev: 0,
            last_output: None,
            last_extras: None,
            frame_buf: [0; MAX_PACKET_FRAMES * 2],
            dacs: Vec::new(),
            volume: None,
            applied_volume: None,
            volume_dirty: false,
            volume_retries: 0,
            alternate: 0,
            suspended_alternate: None,
            ota_suspended: None,
            dop_marker: DOP_MARKER_A,
            packets: 0,
            lost: 0,
            dac_faults: 0,
            requested_us: 0,
            started_us: None,
            failure: None,
            last_packet_cycles: 0,
            publish_ms: u32::MAX,
            meter_ms: 0,
            spectrum_ms: 0,
            pauses: 0,
        }
    }

    fn is_dsd(&self) -> bool {
        match &self.phase {
            Phase::Starting { transport, .. } | Phase::Priming { transport, .. } | Phase::Running { transport, .. } => {
                transport.is_dsd()
            }
            Phase::Idle | Phase::Failed => false,
        }
    }

    fn active_transport(&self) -> Option<(u32, Transport)> {
        match &self.phase {
            Phase::Starting { profile, transport }
            | Phase::Priming { profile, transport }
            | Phase::Running { profile, transport, .. } => Some((profile.hz(), *transport)),
            Phase::Idle | Phase::Failed => None,
        }
    }

    fn update_stream_cpu(&self) {
        if let Some((rate, transport)) = self.active_transport() {
            let need_compute = !self.equalizer.is_bypassed() || self.equalizer.spectrum_enabled();
            board::set_cpu_profile(stream_cpu_profile(rate, transport, need_compute));
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn sync_settings(&mut self, shared: &Shared, rate: u32, transport: Transport) -> bool {
        let dsd = transport.is_dsd();
        let rev = shared.settings_revision.get();
        let (output, dsp, dsp_extras, extras, active_eq, profile, eq_base, tone_eq) = {
            let controls = shared.controls.borrow();
            let active = controls.active_eq();
            let profile = active.and_then(|slot| controls.eq(slot).ok().flatten());
            (
                controls.output(),
                controls.dsp(),
                controls.dsp_extras(),
                controls.dac_extras(),
                active,
                profile,
                controls.eq_base(),
                controls.tone_eq(),
            )
        };
        let host_vol = if dsd || extras.line_out {
            0
        } else {
            let v = shared.uac.borrow().control.host_volume_centi_db();
            ((i32::from(v[0]) + i32::from(v[1])) / 2) as i16
        };
        let mut hardware_changed = false;
        if let Some(previous) = self.last_output {
            if previous.volume_limit_centi_db != output.volume_limit_centi_db
                || previous.balance_centi_db != output.balance_centi_db
                || (dsd && previous.dsd_direct != output.dsd_direct)
            {
                self.volume_dirty = true;
                self.volume_retries = 0;
            }
            if previous.filter != output.filter && self.active() && !dsd {
                hardware_changed = true;
            }
        }
        if let Some(previous) = self.last_extras {
            if previous.line_out != extras.line_out {
                self.volume_dirty = true;
                self.volume_retries = 0;
            }
            if self.active()
                && (dac::effective_class_h(previous) != dac::effective_class_h(extras)
                    || (!dsd
                        && (previous.high_pass_filter != extras.high_pass_filter || previous.de_emphasis != extras.de_emphasis)))
            {
                hardware_changed = true;
            }
        }
        self.last_output = Some(output);
        self.last_extras = Some(extras);
        let budget = if dsd { 0 } else { max_eq_bands(rate) };
        let applied = match (active_eq, profile.as_ref()) {
            (Some(slot), Some(profile))
                if budget > 0
                    && self.equalizer.configure_profile(profile, rate).is_ok()
                    && self.equalizer.band_count() <= budget =>
            {
                Some(slot)
            }
            _ => {
                self.equalizer.bypass_eq();
                None
            }
        };
        let effective_dsp = if dsd { DspSettings::default() } else { dsp };
        let effective_dsp_extras = if dsd { DspExtras::default() } else { dsp_extras };
        let effective_base = if dsd { EqBaseSettings::default() } else { eq_base };
        let effective_tone = if dsd { ToneEqSettings::default() } else { tone_eq };
        let _ = self.equalizer.configure_dsp(effective_dsp, rate);
        let _ = self.equalizer.configure_dsp_extras(effective_dsp_extras, rate);
        let _ = self.equalizer.configure_base_eq(&effective_base, rate);
        let _ = self.equalizer.configure_tone_eq(effective_tone, rate);
        self.equalizer.set_loudness_volume(host_vol);
        self.settings_rev = rev;
        shared.uac.borrow_mut().eq_active = applied;
        if self.active() {
            let need_compute = !self.equalizer.is_bypassed() || self.equalizer.spectrum_enabled();
            board::set_cpu_profile(stream_cpu_profile(rate, transport, need_compute));
        }
        hardware_changed
    }

    #[unsafe(link_section = ".text.hot")]
    pub fn service_iso(&mut self) {
        if self.alternate != 0 && !matches!(self.phase, Phase::Idle | Phase::Failed) {
            self.receive(board::cycles());
        }
    }

    #[unsafe(link_section = ".text.hot")]
    #[inline(never)]
    pub fn poll(&mut self, shared: &Shared) {
        if shared.uac.borrow().ota_silence {
            if self.alternate != 0 || self.active() {
                let alt = self.alternate;
                self.stop(shared);
                if alt != 0 && self.ota_suspended.is_none() {
                    self.ota_suspended = Some(alt);
                }
            }
            return;
        }
        if self.alternate == 0
            && let Some(alt) = self.ota_suspended.take()
        {
            self.select(shared, alt);
        }
        let (alternate, mut restart, volume) = shared.uac.borrow_mut().take();
        if self.settings_rev != shared.settings_revision.get() {
            let (rate, transport) = self
                .active_transport()
                .unwrap_or_else(|| (shared.uac.borrow().control.state().rate, Transport::Pcm(SampleFormat::S24)));
            restart |= self.sync_settings(shared, rate, transport);
        }
        if let Some(alternate) = alternate {
            self.select(shared, alternate);
        } else if restart && self.alternate != 0 {
            self.select(shared, self.alternate);
        } else if let Some(transport) = self.pending_transport.take()
            && self.alternate != 0
        {
            self.start_transport(shared, self.alternate, transport);
        }
        if self.alternate == 0 {
            return;
        }
        if volume {
            self.volume_dirty = true;
            self.volume_retries = 0;
        }
        let now_cycles = board::cycles();
        self.receive(now_cycles);
        self.advance(shared, now_cycles);
        self.send_feedback(shared);
        let ms = shared.milliseconds.get();
        if ms.wrapping_sub(self.publish_ms) >= PUBLISH_INTERVAL_MS || !matches!(self.phase, Phase::Running { .. }) {
            self.publish_ms = ms;
            self.publish(shared);
            self.receive(board::cycles());
        } else if self.equalizer.spectrum_busy() && !self.out.readable() {
            if let Some(spectrum) = self.equalizer.step_spectrum() {
                let mut function = shared.uac.borrow_mut();
                function.spectrum = spectrum;
                function.spectrum_consumed = false;
                self.spectrum_ms = shared.milliseconds.get();
            }
            self.receive(board::cycles());
        }
    }

    pub fn active(&self) -> bool {
        matches!(self.phase, Phase::Starting { .. } | Phase::Priming { .. } | Phase::Running { .. })
    }

    pub fn streaming(&self) -> bool {
        matches!(self.phase, Phase::Priming { .. } | Phase::Running { waiting: false, .. })
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    pub fn suspend(&mut self, shared: &Shared) {
        let alt = self.alternate;
        if alt != 0 || self.active() {
            self.stop(shared);
            if alt != 0 {
                self.suspended_alternate = Some(alt);
            }
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    pub fn resume(&mut self, shared: &Shared) {
        if self.alternate == 0
            && let Some(alt) = self.suspended_alternate.take()
        {
            self.select(shared, alt);
        }
    }

    fn fail(&mut self, phase: AudioPhase, fault: AudioFault) {
        self.phase = Phase::Failed;
        self.failure = Some(AudioFailure { phase, fault });
        if matches!(phase, AudioPhase::Setup | AudioPhase::Clock | AudioPhase::ResetLow | AudioPhase::ResetHigh | AudioPhase::Dac)
        {
            self.dac_faults = self.dac_faults.wrapping_add(1);
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn select(&mut self, shared: &Shared, alternate: u8) {
        self.suspended_alternate = None;
        self.ota_suspended = None;
        self.dop.reset();
        self.dop_marker = DOP_MARKER_A;
        if alternate == uac2::RAW_ALTERNATE {
            let rate = shared.uac.borrow().control.state().rate;
            let Some(dsd_rate) = DsdRate::from_native_hz(rate) else {
                self.stop(shared);
                self.alternate = alternate;
                self.phase = Phase::Failed;
                return;
            };
            self.start_transport(shared, alternate, Transport::NativeDsd(dsd_rate));
        } else if let Some(format) = SampleFormat::from_alternate(alternate) {
            self.start_transport(shared, alternate, Transport::Pcm(format));
        } else {
            self.stop(shared);
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn start_transport(&mut self, shared: &Shared, alternate: u8, transport: Transport) {
        self.stop(shared);
        self.alternate = alternate;
        self.phase = Phase::Failed;
        let rate = shared.uac.borrow().control.state().rate;
        let Some(profile) = PcmProfile::from_hz(rate) else { return };
        if !shared.uac.borrow().speed_ok || shared.audio.borrow().busy() {
            return;
        }
        let calibration = shared.audio.borrow_mut().calibration();
        let calibration = match calibration {
            Ok(calibration) => calibration,
            Err(fault) => {
                let mut function = shared.uac.borrow_mut();
                function.calibration_failed = true;
                function.refresh_clock();
                self.failure = Some(AudioFailure { phase: AudioPhase::Calibrate, fault });
                return;
            }
        };
        let dacs = match audio::prepare_dacs() {
            Ok(dacs) => dacs,
            Err(fault) => return self.fail(AudioPhase::Setup, fault),
        };
        let dsd = transport.is_dsd();
        self.flow.set_silence(if dsd { [DSD_SILENCE_WORD; 2] } else { [0, 0] });
        let (filter, extras) = {
            let controls = shared.controls.borrow();
            (controls.output().filter.into(), controls.dac_extras())
        };
        let _ = self.sync_settings(shared, profile.hz(), transport);
        let dsp_active = !self.equalizer.is_bypassed() || self.equalizer.spectrum_enabled();
        board::set_cpu_profile(stream_cpu_profile(profile.hz(), transport, dsp_active));
        let now = board::monotonic_us();
        audio::clear_monitor();
        let start = match transport.dsd() {
            Some((_, dsd_rate)) => {
                Start::dsd_on_with_extras(&mut Mmio, &calibration, dsd_rate, extras, Volume::MUTED, now, &dacs)
            }
            None => Start::pcm_on_with_extras(&mut Mmio, &calibration, profile, filter, extras, Volume::MUTED, now, &dacs),
        };
        self.dacs = dacs;
        self.applied_volume = None;
        self.requested_us = now;
        self.started_us = None;
        self.starter = Some(start);
        self.phase = Phase::Starting { profile, transport };
        self.publish(shared);
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn stop(&mut self, shared: &Shared) {
        let mut mmio = Mmio;
        let owned = self.active();
        regs::apply_one(&mut mmio, regs::MmioOp::Write { address: I2C0_BASE, value: CONFIG_100KHZ });
        match core::mem::replace(&mut self.phase, Phase::Idle) {
            Phase::Running { mut dma, transport, .. } => {
                let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(audio::STREAM_I2C_BUDGET_US);
                let res = if transport.is_dsd() {
                    stream::stop_dsd(&mut mmio, &mut bus, &mut dma)
                } else {
                    stream::stop(&mut mmio, &mut bus, &mut dma)
                };
                if res.is_err() {
                    self.dac_faults = self.dac_faults.wrapping_add(1);
                }
                regs::apply(&mut mmio, &pins::QUIET);
                regs::apply(&mut mmio, &pins::POWER_OFF);
            }
            Phase::Priming { .. } => {
                audio::power_down_dacs();
                regs::apply(&mut mmio, &pins::QUIET);
                regs::apply(&mut mmio, &pins::POWER_OFF);
            }
            Phase::Starting { .. } | Phase::Idle | Phase::Failed => {}
        }
        if let Some(start) = self.starter.take() {
            start.abort(&mut mmio);
            audio::power_down_dacs();
            regs::apply(&mut mmio, &pins::POWER_OFF);
        }
        board::set_cpu_profile(CpuProfile::Idle90);
        if owned {
            self.flow.reset(&mut RingStore);
        }
        self.pending_transport = None;
        self.failure = None;
        self.volume = None;
        self.applied_volume = None;
        self.equalizer.reset_stream();
        self.alternate = 0;
        {
            let mut function = shared.uac.borrow_mut();
            function.stream = None;
            function.meters = AudioMeters::default();
            function.meters_consumed = false;
            function.spectrum = AudioSpectrum::default();
            function.spectrum_consumed = false;
            function.spectrum_until_ms = None;
        }
        self.equalizer.set_spectrum_enabled(false);
        shared.audio.borrow_mut().set_usb(None);
    }

    #[unsafe(link_section = ".text.hot")]
    fn receive(&mut self, now_cycles: u32) {
        if !self.out.readable() {
            return;
        }
        let mut packet = [0u32; PACKET_BUFFER / 4];
        for _ in 0..PACKETS_PER_POLL {
            let bytes = unsafe { core::slice::from_raw_parts_mut(packet.as_mut_ptr().cast::<u8>(), PACKET_BUFFER) };
            let Some((length, flags)) = self.out.receive(bytes) else { break };
            if flags != 0 && matches!(self.phase, Phase::Running { .. }) {
                self.lost = self.lost.wrapping_add(1);
            }
            if length == 0 || flags & usb::DAMAGED != 0 {
                continue;
            }
            self.packets = self.packets.wrapping_add(1);
            self.last_packet_cycles = now_cycles;
            let bytes = unsafe { core::slice::from_raw_parts(packet.as_ptr().cast::<u8>(), length) };
            let words = &packet[..length / 4];
            self.accept(bytes, words);
            if self.pending_transport.is_some() {
                break;
            }
        }
    }

    #[unsafe(link_section = ".text.hot")]
    #[inline(always)]
    fn push_audio(&mut self, transport: Transport, packet: &[u8], words: &[u32]) {
        match transport {
            Transport::Pcm(format) => {
                if self.equalizer.is_bypassed() {
                    if format == SampleFormat::S32 && packet.len().is_multiple_of(8) {
                        let samples = unsafe { core::slice::from_raw_parts(words.as_ptr().cast::<i32>(), words.len()) };
                        self.equalizer.observe_stereo(samples);
                        let _ = self.flow.push(&mut RingStore, samples);
                    } else if let Ok(frames) = flow::unpack(format, packet, &mut self.frame_buf) {
                        let samples = &self.frame_buf[..frames * 2];
                        self.equalizer.observe_stereo(samples);
                        let _ = self.flow.push(&mut RingStore, samples);
                    }
                } else if let Ok(frames) = flow::unpack(format, packet, &mut self.frame_buf) {
                    let was_transitioning = self.equalizer.is_transitioning();
                    let samples = &mut self.frame_buf[..frames * 2];
                    self.equalizer.process_stereo(samples);
                    let _ = self.flow.push(&mut RingStore, samples);
                    if was_transitioning && self.equalizer.is_bypassed() {
                        self.update_stream_cpu();
                    }
                }
            }
            Transport::Dop(format, _) => {
                let _ = self.flow.push_dop_packet(&mut RingStore, format, packet, &mut self.dop_marker);
            }
            Transport::NativeDsd(_) => {
                if packet.len().is_multiple_of(8) {
                    let _ = self.flow.push_native_dsd_words(&mut RingStore, words);
                } else {
                    let _ = self.flow.push_native_dsd_packet(&mut RingStore, packet);
                }
            }
        }
    }

    #[unsafe(link_section = ".text.hot")]
    #[inline(always)]
    fn check_dop(&mut self, profile: PcmProfile, transport: Transport, packet: &[u8]) -> bool {
        match transport {
            Transport::Pcm(format) => {
                if let Some(dsd_rate) = DsdRate::from_dop_hz(profile.hz())
                    && self.dop.observe_packet(format, packet) == Some(true)
                {
                    self.pending_transport = Some(Transport::Dop(format, dsd_rate));
                    return true;
                }
            }
            Transport::Dop(format, _) => {
                if self.dop.observe_packet(format, packet) == Some(false) {
                    self.pending_transport = Some(Transport::Pcm(format));
                    return true;
                }
            }
            Transport::NativeDsd(_) => {}
        }
        false
    }

    #[unsafe(link_section = ".text.hot")]
    #[inline(never)]
    fn accept(&mut self, packet: &[u8], words: &[u32]) {
        let ready = match &mut self.phase {
            Phase::Priming { profile, transport } => {
                let (profile, transport) = (*profile, *transport);
                if self.check_dop(profile, transport, packet) {
                    return;
                }
                self.push_audio(transport, packet, words);
                self.flow.ready_to_start()
            }
            Phase::Running { dma, profile, transport, waiting } => {
                let (profile, transport) = (*profile, *transport);
                if *waiting {
                    let Ok(cursor) = dma.cursor(&mut Mmio) else { return };
                    if self.flow.resume(&mut RingStore, cursor).is_err() {
                        return;
                    }
                    *waiting = false;
                }
                if self.check_dop(profile, transport, packet) {
                    return;
                }
                self.push_audio(transport, packet, words);
                false
            }
            _ => false,
        };
        if ready {
            self.launch();
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn launch(&mut self) {
        let Phase::Priming { profile, transport } = self.phase else { return };
        self.phase = Phase::Failed;
        let mut mmio = Mmio;
        let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(audio::STREAM_I2C_BUDGET_US);
        let Ok(buffer) = DmaBuffer::new(audio::ring_address(), RING_BYTES) else { return };
        let mut dma = I2sDma::new(buffer);
        let dma_start = match transport.dsd() {
            Some((mode, dsd_rate)) => dma.start_dsd(&mut mmio, mode, dsd_rate),
            None => dma.start(&mut mmio, profile),
        };
        let started = dma_start.and_then(|()| audio::learn_cached(&mut dma, profile.hz()));
        if let Err(error) = started.and_then(|()| self.flow.start().map_err(|_| I2sError::NotRunning)) {
            regs::apply_one(&mut mmio, regs::MmioOp::Write { address: I2C0_BASE, value: CONFIG_100KHZ });
            let _ = if transport.is_dsd() {
                stream::stop_dsd(&mut mmio, &mut bus, &mut dma)
            } else {
                stream::stop(&mut mmio, &mut bus, &mut dma)
            };
            regs::apply(&mut mmio, &pins::QUIET);
            regs::apply(&mut mmio, &pins::POWER_OFF);
            board::set_cpu_profile(CpuProfile::Idle90);
            return self.fail(AudioPhase::Dma, AudioFault::Dma(error.into()));
        }
        let now_cycles = board::cycles();
        self.last_packet_cycles = now_cycles;
        regs::apply_one(&mut mmio, regs::MmioOp::Write { address: I2C0_BASE, value: CONFIG_400KHZ });
        regs::apply_one(&mut mmio, pins::output(true));
        self.volume_dirty = true;
        self.volume_retries = 0;
        let elapsed = board::monotonic_us().saturating_sub(self.requested_us);
        self.started_us = Some(u32::try_from(elapsed).unwrap_or(u32::MAX));
        self.phase = Phase::Running { dma, profile, transport, waiting: false };
        self.receive(now_cycles);
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn advance_starting(&mut self, profile: PcmProfile, transport: Transport) {
        let Some(start) = self.starter.as_mut() else { return };
        let now = board::monotonic_us();
        let mut bus = Controller::new(Mmio, I2C0_BASE).with_budget(audio::STREAM_I2C_BUDGET_US);
        let outcome = start.poll(&mut Mmio, &mut bus, now);
        let stage = start.stage();
        match outcome {
            Ok(Progress::Pending { .. }) => {}
            Ok(Progress::Done) => {
                self.starter = None;
                self.flow.reset(&mut RingStore);
                board::keep_awake();
                self.phase = Phase::Priming { profile, transport };
            }
            Err(error) => {
                if let Some(start) = self.starter.take() {
                    start.abort(&mut Mmio);
                }
                audio::power_down_dacs();
                regs::apply(&mut Mmio, &pins::POWER_OFF);
                board::set_cpu_profile(CpuProfile::Idle90);
                self.fail(bringup::phase(stage), error.into());
            }
        }
    }

    #[unsafe(link_section = ".text.hot")]
    #[inline(never)]
    fn advance(&mut self, shared: &Shared, now_cycles: u32) {
        match &mut self.phase {
            Phase::Starting { profile, transport } => {
                let (profile, transport) = (*profile, *transport);
                self.advance_starting(profile, transport);
            }
            Phase::Running { dma, waiting, .. } => {
                let pause_cycles = HOST_PAUSE_US.saturating_mul(board::cpu_mhz());
                if !*waiting && now_cycles.wrapping_sub(self.last_packet_cycles) > pause_cycles {
                    *waiting = true;
                    self.flow.reset(&mut RingStore);
                    self.pauses = self.pauses.wrapping_add(1);
                }
                if !*waiting {
                    let cursor = dma.cursor(&mut Mmio);
                    audio::record_cursor(dma, cursor);
                    if let Ok(cursor) = cursor
                        && !matches!(self.flow.observe(&mut RingStore, cursor), Ok(Observation::Advanced { .. }))
                    {
                        *waiting = true;
                        self.flow.reset(&mut RingStore);
                    }
                }
                if self.volume.is_some() || self.volume_dirty {
                    let now = board::monotonic_us();
                    self.step_volume(shared, now);
                }
            }
            Phase::Idle | Phase::Priming { .. } | Phase::Failed => {}
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn step_volume(&mut self, shared: &Shared, now: u64) {
        if self.volume.is_none() && self.volume_dirty {
            self.volume_dirty = false;
            let (centi_db, muted) = {
                let function = shared.uac.borrow();
                (function.control.host_volume_centi_db(), function.control.state().mute || function.local_mute)
            };
            let (output, line_out) = {
                let controls = shared.controls.borrow();
                (controls.output(), controls.dac_extras().line_out)
            };
            let dsd = self.is_dsd();
            let loudness_vol = if dsd || line_out { 0 } else { ((i32::from(centi_db[0]) + i32::from(centi_db[1])) / 2) as i16 };
            let was_bypassed = self.equalizer.is_bypassed();
            self.equalizer.set_loudness_volume(loudness_vol);
            if self.equalizer.is_bypassed() != was_bypassed {
                self.update_stream_cpu();
            }
            let min_host_centi_db = (i32::from(uac2::VOLUME_MIN) * 100 / 256) as i16;
            let host_silent = muted || (centi_db[0] <= min_host_centi_db && centi_db[1] <= min_host_centi_db);
            let volume = if line_out || (dsd && output.dsd_direct) {
                if host_silent {
                    Volume::MUTED
                } else if output.volume_limit_centi_db < 0 {
                    Volume::from_settings([0, 0], false, output.volume_limit_centi_db, 0)
                } else {
                    Volume::UNITY
                }
            } else {
                Volume::from_settings(centi_db, muted, output.volume_limit_centi_db, output.balance_centi_db)
            };
            if self.applied_volume == Some(volume) {
                return;
            }
            self.applied_volume = Some(volume);
            let program = if dsd { dac::dsd_volume_fast(volume) } else { dac::volume_fast(volume) };
            self.volume = Some(Runner::new(dac::only(&program, &self.dacs)));
        }
        let Some(mut runner) = self.volume.take() else { return };
        self.receive(board::cycles());
        let outcome = runner.poll(&mut Controller::new(Mmio, I2C0_BASE), now);
        self.receive(board::cycles());
        match outcome {
            Ok(Progress::Pending { .. }) => self.volume = Some(runner),
            Ok(Progress::Done) => {
                self.volume_retries = 0;
            }
            Err(_) => {
                self.dac_faults = self.dac_faults.wrapping_add(1);
                self.applied_volume = None;
                regs::apply(&mut Mmio, &i2c::SETUP);
                regs::apply_one(&mut Mmio, regs::MmioOp::Write { address: I2C0_BASE, value: CONFIG_400KHZ });
                if self.volume_retries < 3 {
                    self.volume_retries += 1;
                    self.volume_dirty = true;
                }
            }
        }
    }

    #[unsafe(link_section = ".text.hot")]
    fn send_feedback(&mut self, shared: &Shared) {
        if !self.feedback.writable() {
            return;
        }
        let rate = match &self.phase {
            Phase::Starting { profile, .. } | Phase::Priming { profile, .. } | Phase::Running { profile, .. } => profile.hz(),
            Phase::Idle | Phase::Failed => shared.uac.borrow().control.state().rate,
        };
        let nominal = PcmProfile::from_hz(rate).map_or(0, |profile| profile.nominal_frames_per_interval_q16(self.speed));
        let value = match self.phase {
            Phase::Running { waiting: false, .. } => self.flow.feedback(nominal),
            _ => nominal,
        };
        let (bytes, length) = flow::encode_feedback(value, self.speed);
        if self.feedback.offer(&bytes[..length]).is_some() {
            audio::record(AUDIO_MONITOR_FEEDBACK, value);
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn publish(&mut self, shared: &Shared) {
        let counters = self.flow.counters();
        let (state, rate, stream) = match &self.phase {
            Phase::Running { profile, transport, .. } => {
                (AudioBenchState::Running, profile.hz(), Some(transport.stream_format(*profile)))
            }
            Phase::Starting { profile, .. } | Phase::Priming { profile, .. } => (AudioBenchState::Starting, profile.hz(), None),
            Phase::Idle | Phase::Failed => {
                let mut function = shared.uac.borrow_mut();
                function.stream = None;
                function.counters = counters.icd(self.dac_faults);
                drop(function);
                let failed = self.failure.map(|failure| UsbView {
                    state: AudioBenchState::Failed,
                    stream: self.description(0),
                    failure: Some(failure),
                });
                shared.audio.borrow_mut().set_usb(failed);
                return;
            }
        };
        audio::record(AUDIO_MONITOR_QUEUED, self.flow.queued() as u32);
        audio::record(AUDIO_MONITOR_PACKETS, self.packets);
        audio::record(AUDIO_MONITOR_UNDERRUNS, counters.underruns);
        audio::record(AUDIO_MONITOR_OVERRUNS, counters.overruns);
        audio::record(AUDIO_MONITOR_LOST_PACKETS, self.lost);
        audio::record(AUDIO_MONITOR_PAUSES, self.pauses);
        let view = UsbView { state, stream: self.description(rate), failure: None };
        shared.audio.borrow_mut().set_usb(Some(view));
        let mut function = shared.uac.borrow_mut();
        function.stream = stream;
        function.counters = counters.icd(self.dac_faults);
        let running = matches!(self.phase, Phase::Running { waiting: false, .. });
        let ms = shared.milliseconds.get();
        let want_spectrum = running && !self.is_dsd() && function.spectrum_wanted(ms);
        let prev_spectrum = self.equalizer.spectrum_enabled();
        self.equalizer.set_spectrum_enabled(want_spectrum);
        if !running {
            self.equalizer.reset_meters();
            function.meters = AudioMeters::default();
            function.meters_consumed = false;
            function.spectrum = AudioSpectrum::default();
            function.spectrum_consumed = false;
        } else {
            let mut meters_updated = false;
            if function.meters_consumed || ms.wrapping_sub(self.meter_ms) >= METER_INTERVAL_MS {
                function.meters = self.equalizer.take_meters(true);
                function.meters_consumed = false;
                self.meter_ms = ms;
                meters_updated = true;
            }
            if !want_spectrum {
                function.spectrum = AudioSpectrum::default();
                function.spectrum_consumed = false;
            } else if !meters_updated && !self.equalizer.spectrum_busy() {
                let elapsed = ms.wrapping_sub(self.spectrum_ms);
                let due = elapsed >= SPECTRUM_INTERVAL_MS || (function.spectrum_consumed && elapsed >= SPECTRUM_MIN_INTERVAL_MS);
                if due && let Some(spectrum) = self.equalizer.start_spectrum(true) {
                    function.spectrum = spectrum;
                    function.spectrum_consumed = false;
                    self.spectrum_ms = ms;
                }
            }
        }
        drop(function);
        if want_spectrum != prev_spectrum {
            self.update_stream_cpu();
        }
    }

    fn description(&self, rate_hz: u32) -> AudioStream {
        AudioStream {
            start: AudioStart { rate_hz, channel: AudioChannel::Both, level_centi_dbfs: 0, tone_hz: 0 },
            amplitude: 0,
            ring_address: audio::ring_address(),
            ring_bytes: RING_BYTES,
            periods: 0,
            started_us: self.started_us,
            dacs: dac::mask(&self.dacs),
            cursor: audio::cursor_map(),
        }
    }
}

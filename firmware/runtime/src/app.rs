// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use core::cell::{Cell, RefCell};
use core::future::Future;
use core::future::poll_fn;
use core::pin::pin;
use core::task::Poll;
use embassy_usb::class::hid::{self, HidProtocolMode, ReportId, RequestHandler};
use embassy_usb::control::{InResponse, OutResponse, Request, RequestType};
use embassy_usb_driver::{Endpoint, EndpointIn, EndpointOut};
use heapless::Deque;
use kumi_control::{AutoSave, Controls, SaveTrigger};
use kumi_icd::*;
use kumi_keymap::{KeyboardReport, MouseReport, Output};
use kumi_runtime_core::{SETTINGS_SETTLE_MS, Storage};
use kumi_service::Provider;
use kumi_settings::StoredSettings;
use postcard_rpc::Topic;

use crate::{audio, board, uac, usb};

const MIRROR_QUEUE: usize = 8;
const RAM_BUILD: &str = if board::LAB { "ram-lab" } else { "ram" };
const SLOT_BUILD: &str = if board::LAB { "slot-lab" } else { "slot" };

pub struct Shared {
    pub controls: RefCell<Controls>,
    pub status: Cell<KeyboardStatus>,
    keyboard: Cell<KeyboardReport>,
    consumer: Cell<u16>,
    mouse: Cell<MouseReport>,
    reports: RefCell<Deque<(KeyboardReport, u16, MouseReport), 32>>,
    protocol: Cell<HidProtocolMode>,
    pub leds: Cell<u8>,
    idle: [Cell<u32>; 2],
    pub milliseconds: Cell<u32>,
    events: RefCell<Deque<KeyEvent, 32>>,
    subscribed: Cell<bool>,
    startup_log: Cell<bool>,
    dropped_events: Cell<u32>,
    bus_reset: Cell<bool>,
    pub usb_high_speed: Cell<bool>,
    pub storage: RefCell<Storage<board::Flash>>,
    autosave: RefCell<AutoSave>,
    explicit_save: Cell<bool>,
    pub settings_revision: Cell<u32>,
    pub configured: Cell<bool>,
    pub suspended: Cell<bool>,
    pub remote_wakeup_enabled: Cell<bool>,
    pub last_activity_ms: Cell<u32>,
    pub rpc_activity: Cell<u32>,
    flush: Cell<Option<SaveTrigger>>,
    safe_mode: bool,
    unlocked_until: Cell<Option<u32>>,
    fault: Cell<Option<FaultKind>>,
    mirror: Cell<bool>,
    mirror_batch: RefCell<heapless::Vec<u8, BYTE_BLOCK>>,
    mirror_at: Cell<u32>,
    mirrored: RefCell<Deque<ByteBlock, MIRROR_QUEUE>>,
    uart_tx: RefCell<Deque<u8, 128>>,
    pub audio: RefCell<audio::Bench>,
    pub uac: RefCell<uac::Function>,
}

impl Shared {
    pub fn new(
        settings: StoredSettings,
        storage: Storage<board::Flash>,
        safe_mode: bool,
        audio: kumi_audio::uac2::Config,
    ) -> Self {
        let initial_eq = settings.active_eq;
        let mut uac = uac::Function::new(audio);
        uac.set_eq_active(initial_eq);
        Self {
            controls: RefCell::new(Controls::new(settings)),
            status: Cell::new(KeyboardStatus::default()),
            keyboard: Cell::new(KeyboardReport::default()),
            consumer: Cell::new(0),
            mouse: Cell::new(MouseReport::default()),
            reports: RefCell::new(Deque::new()),
            protocol: Cell::new(HidProtocolMode::Report),
            leds: Cell::new(0),
            idle: [Cell::new(u32::MAX), Cell::new(u32::MAX)],
            milliseconds: Cell::new(0),
            events: RefCell::new(Deque::new()),
            subscribed: Cell::new(false),
            startup_log: Cell::new(false),
            dropped_events: Cell::new(0),
            bus_reset: Cell::new(false),
            usb_high_speed: Cell::new(false),
            storage: RefCell::new(storage),
            autosave: RefCell::new(AutoSave::new(SETTINGS_SETTLE_MS)),
            explicit_save: Cell::new(false),
            settings_revision: Cell::new(1),
            configured: Cell::new(false),
            suspended: Cell::new(false),
            remote_wakeup_enabled: Cell::new(false),
            last_activity_ms: Cell::new(0),
            rpc_activity: Cell::new(0),
            flush: Cell::new(None),
            safe_mode,
            unlocked_until: Cell::new(None),
            fault: Cell::new(None),
            mirror: Cell::new(false),
            mirror_batch: RefCell::new(heapless::Vec::new()),
            mirror_at: Cell::new(0),
            mirrored: RefCell::new(Deque::new()),
            uart_tx: RefCell::new(Deque::new()),
            audio: RefCell::new(audio::Bench::new()),
            uac: RefCell::new(uac),
        }
    }

    pub fn safe_mode(&self) -> bool {
        self.safe_mode
    }

    pub fn touch_activity(&self) {
        self.last_activity_ms.set(self.milliseconds.get());
    }

    pub fn has_pending_work(&self) -> bool {
        self.startup_log.get()
            || !self.uart_tx.borrow().is_empty()
            || !self.reports.borrow().is_empty()
            || !self.events.borrow().is_empty()
            || !self.mirrored.borrow().is_empty()
    }

    pub fn developer(&self) -> Ack {
        match self.unlocked_until.get() {
            Some(until) if until.wrapping_sub(self.milliseconds.get()) as i32 > 0 => Ok(()),
            _ => Err(CaseError::NotUnlocked),
        }
    }

    fn unlock(&self, seconds: u16) {
        self.unlocked_until.set(Some(self.milliseconds.get().wrapping_add(1000 * u32::from(seconds))));
    }

    pub fn mirror_byte(&self, byte: u8) {
        if !self.mirror.get() {
            return;
        }
        let full = {
            let mut batch = self.mirror_batch.borrow_mut();
            let _ = batch.push(byte);
            batch.is_full()
        };
        self.mirror_at.set(self.milliseconds.get());
        if full {
            self.flush_mirror();
        }
    }

    pub fn flush_mirror(&self) {
        let bytes = core::mem::take(&mut *self.mirror_batch.borrow_mut());
        if !bytes.is_empty() && self.mirrored.borrow_mut().push_back(ByteBlock { bytes }).is_err() {
            self.dropped_events.set(self.dropped_events.get().saturating_add(1));
        }
    }

    pub fn tick_mirror(&self) {
        if self.milliseconds.get() != self.mirror_at.get() {
            self.flush_mirror();
        }
        if self.mirror.get() && self.developer().is_err() {
            self.flush_mirror();
            self.mirror.set(false);
        }
    }

    fn stop_mirror(&self) {
        self.mirror.set(false);
        self.mirror_batch.borrow_mut().clear();
        self.mirrored.borrow_mut().clear();
    }

    pub fn pop_uart_tx(&self) -> Option<u8> {
        self.uart_tx.borrow_mut().pop_front()
    }

    pub fn note_changes(&self) -> (bool, bool) {
        let explicit = self.explicit_save.replace(false);
        let mut controls = self.controls.borrow_mut();
        let dirty = controls.dirty();
        let keymap_dirty = controls.take_keymap_dirty();
        if !dirty && !explicit && !keymap_dirty {
            return (false, false);
        }
        if dirty {
            controls.clear_dirty();
            self.autosave.borrow_mut().record(self.milliseconds.get());
        }
        let active_eq = controls.active_eq();
        drop(controls);
        self.settings_revision.set(self.settings_revision.get().wrapping_add(1));
        let mut uac = self.uac.borrow_mut();
        if !uac.present() {
            uac.set_eq_active(active_eq);
        }
        (true, keymap_dirty)
    }

    pub fn autosave_pending(&self) -> bool {
        self.autosave.borrow().pending()
    }

    pub fn take_flush(&self) -> SaveTrigger {
        self.flush.take().unwrap_or(SaveTrigger::Tick)
    }

    pub fn persist_settings(&self, trigger: SaveTrigger, streaming: bool) -> bool {
        if self.safe_mode
            || (streaming && matches!(trigger, SaveTrigger::Tick))
            || !self.autosave.borrow().due(self.milliseconds.get(), trigger)
        {
            return false;
        }
        let controls = self.controls.borrow();
        if self.storage.borrow_mut().save(controls.settings()) != Err(CaseError::Busy) {
            self.autosave.borrow_mut().saved();
            return true;
        }
        false
    }

    pub fn apply_case_hotkeys(&self, hotkeys: &[CaseHotkey]) {
        for &hotkey in hotkeys {
            match hotkey {
                CaseHotkey::ToggleMute => {
                    self.uac.borrow_mut().toggle_local_mute();
                }
                CaseHotkey::CycleEq => {
                    let mut controls = self.controls.borrow_mut();
                    let current = controls.active_eq();
                    let start = current.map_or(0, |slot| slot + 1);
                    let mut next = None;
                    for slot in start..EQ_SLOTS {
                        if controls.eq(slot).ok().flatten().is_some() {
                            next = Some(slot);
                            break;
                        }
                    }
                    let _ = controls.select_eq(next);
                }
                CaseHotkey::CycleFilter => {
                    let mut controls = self.controls.borrow_mut();
                    let mut out = controls.output();
                    out.filter = match out.filter {
                        DacFilter::FastLowLatency => DacFilter::FastPhaseCompensated,
                        DacFilter::FastPhaseCompensated => DacFilter::SlowLowLatency,
                        DacFilter::SlowLowLatency => DacFilter::SlowPhaseCompensated,
                        DacFilter::SlowPhaseCompensated => DacFilter::NonOversampling,
                        DacFilter::NonOversampling => DacFilter::FastLowLatency,
                    };
                    let _ = controls.set_output(out);
                }
                CaseHotkey::CycleCrossfeed => {
                    let mut controls = self.controls.borrow_mut();
                    let mut dsp = controls.dsp();
                    dsp.crossfeed = match dsp.crossfeed {
                        CrossfeedMode::Off => CrossfeedMode::Subtle,
                        CrossfeedMode::Subtle => CrossfeedMode::Natural,
                        CrossfeedMode::Natural => CrossfeedMode::Strong,
                        CrossfeedMode::Strong => CrossfeedMode::Off,
                    };
                    let _ = controls.set_dsp(dsp);
                }
                CaseHotkey::ToggleLimiter => {
                    let mut controls = self.controls.borrow_mut();
                    let mut dsp = controls.dsp();
                    dsp.limiter = !dsp.limiter;
                    let _ = controls.set_dsp(dsp);
                }
                CaseHotkey::ToggleMono => {
                    let mut controls = self.controls.borrow_mut();
                    let mut dsp = controls.dsp();
                    dsp.mono = !dsp.mono;
                    let _ = controls.set_dsp(dsp);
                }
                CaseHotkey::ToggleLineOut => {
                    let mut controls = self.controls.borrow_mut();
                    let mut extras = controls.dac_extras();
                    if extras.line_out || controls.output().volume_limit_centi_db >= 0 {
                        extras.line_out = !extras.line_out;
                        let _ = controls.set_dac_extras(extras);
                    }
                }
                CaseHotkey::ToggleLoudness => {
                    let mut controls = self.controls.borrow_mut();
                    let mut dsp = controls.dsp();
                    dsp.loudness = !dsp.loudness;
                    let _ = controls.set_dsp(dsp);
                }
                CaseHotkey::ToggleCapsWord => {}
            }
        }
    }

    pub fn report(&self, output: &Output) {
        if self.subscribed.get() {
            for event in &output.events {
                if self.events.borrow_mut().push_back(*event).is_err() {
                    self.dropped_events.set(self.dropped_events.get().saturating_add(1));
                }
            }
        }
        let mouse_changed = output.mouse.buttons != self.mouse.get().buttons || output.mouse.has_motion();
        if output.keyboard == self.keyboard.get() && output.consumer == self.consumer.get() && !mouse_changed {
            return;
        }
        self.keyboard.set(output.keyboard);
        self.consumer.set(output.consumer);
        self.mouse.set(MouseReport { buttons: output.mouse.buttons, dx: 0, dy: 0, wheel: 0 });
        let mut reports = self.reports.borrow_mut();
        if reports.push_back((output.keyboard, output.consumer, output.mouse)).is_err() {
            reports.clear();
            let _ = reports.push_back((KeyboardReport::default(), 0, MouseReport::default()));
            let _ = reports.push_back((output.keyboard, output.consumer, output.mouse));
            let mut status = self.status.get();
            status.bad_frames = status.bad_frames.wrapping_add(1);
            self.status.set(status);
        }
    }
}

pub struct HidControl<'a>(pub &'a Shared, pub &'a usb::State, pub bool);

pub struct Lifecycle<'a>(pub &'a Shared);

impl embassy_usb::Handler for Lifecycle<'_> {
    fn reset(&mut self) {
        self.0.protocol.set(HidProtocolMode::Report);
        self.0.leds.set(0);
        self.0.idle.iter().for_each(|idle| idle.set(u32::MAX));
        self.0.reports.borrow_mut().clear();
        self.0.subscribed.set(false);
        self.0.events.borrow_mut().clear();
        self.0.startup_log.set(false);
        self.0.bus_reset.set(true);
        self.0.configured.set(false);
        self.0.suspended.set(false);
        self.0.remote_wakeup_enabled.set(false);
        self.0.flush.set(Some(SaveTrigger::LinkDown));
        self.0.unlocked_until.set(None);
        self.0.stop_mirror();
        self.0.uart_tx.borrow_mut().clear();
        #[cfg(feature = "lab")]
        crate::lab::usb(crate::lab::usb_event::RESET);
    }

    fn configured(&mut self, configured: bool) {
        self.0.configured.set(configured);
        #[cfg(feature = "lab")]
        crate::lab::usb(if configured { crate::lab::usb_event::CONFIGURED } else { crate::lab::usb_event::DECONFIGURED });
    }

    fn suspended(&mut self, suspended: bool) {
        self.0.suspended.set(suspended);
        #[cfg(feature = "lab")]
        crate::lab::usb(if suspended { crate::lab::usb_event::SUSPEND } else { crate::lab::usb_event::RESUME });
        if suspended {
            self.0.flush.set(Some(SaveTrigger::Sleeping));
        } else {
            self.0.touch_activity();
        }
    }

    fn remote_wakeup_enabled(&mut self, enabled: bool) {
        self.0.remote_wakeup_enabled.set(enabled);
    }

    #[unsafe(link_section = ".text.cold")]
    fn control_in<'a>(&mut self, req: Request, buf: &'a mut [u8]) -> Option<InResponse<'a>> {
        const WEBUSB_URL: &[u8] = b"\x1b\x03\x01nonbiri.dev/mindone/case";
        if req.request_type == RequestType::Vendor && req.request == 0x21 && req.index == 2 && req.value == 1 {
            let n = WEBUSB_URL.len().min(usize::from(req.length)).min(buf.len());
            buf[..n].copy_from_slice(&WEBUSB_URL[..n]);
            return Some(InResponse::Accepted(&buf[..n]));
        }
        None
    }
}

impl RequestHandler for HidControl<'_> {
    #[unsafe(link_section = ".text.cold")]
    fn get_report(&mut self, id: ReportId, buf: &mut [u8]) -> Option<usize> {
        let keyboard = self.0.keyboard.get();
        let report = kumi_keymap::hid::keyboard_report(&keyboard);
        let boot = kumi_keymap::hid::boot_keyboard_report(&keyboard);
        let consumer = kumi_keymap::hid::consumer_report(self.0.consumer.get());
        let mouse = kumi_keymap::hid::mouse_report(&self.0.mouse.get());
        let leds = [1, self.0.leds.get()];
        let data: &[u8] = match id {
            ReportId::In(0 | 1) if self.0.protocol.get() == HidProtocolMode::Boot => &boot,
            ReportId::In(1) => &report,
            ReportId::In(2) => &consumer,
            ReportId::In(3) if self.2 => &mouse,
            ReportId::Out(0 | 1) if self.0.protocol.get() == HidProtocolMode::Boot => &leds[1..],
            ReportId::Out(1) => &leds,
            _ => return None,
        };
        buf.get_mut(..data.len())?.copy_from_slice(data);
        Some(data.len())
    }

    #[unsafe(link_section = ".text.cold")]
    fn set_report(&mut self, id: ReportId, data: &[u8]) -> OutResponse {
        let leds = match (self.0.protocol.get(), id, data) {
            (HidProtocolMode::Boot, ReportId::Out(0 | 1), [leds]) => *leds,
            (HidProtocolMode::Report, ReportId::Out(1), [1, leds]) => *leds,
            _ => return OutResponse::Rejected,
        };
        if leds & !0x1f == 0 {
            self.0.leds.set(leds);
            OutResponse::Accepted
        } else {
            OutResponse::Rejected
        }
    }

    fn get_protocol(&self) -> HidProtocolMode {
        self.0.protocol.get()
    }

    fn set_protocol(&mut self, mode: HidProtocolMode) -> OutResponse {
        if mode != self.0.protocol.get() {
            self.1.cancel_endpoint(0x81.into());
        }
        self.0.protocol.set(mode);
        OutResponse::Accepted
    }

    fn get_idle_ms(&mut self, id: Option<ReportId>) -> Option<u32> {
        match id {
            None | Some(ReportId::In(0 | 1)) => Some(self.0.idle[0].get()),
            Some(ReportId::In(2)) => Some(self.0.idle[1].get()),
            _ => None,
        }
    }

    fn set_idle_ms(&mut self, id: Option<ReportId>, duration_ms: u32) {
        match id {
            None => self.0.idle.iter().for_each(|idle| idle.set(duration_ms)),
            Some(ReportId::In(0 | 1)) => self.0.idle[0].set(duration_ms),
            Some(ReportId::In(2)) => self.0.idle[1].set(duration_ms),
            _ => {}
        }
    }
}

struct Services<'a> {
    shared: &'a Shared,
    jedec: [u8; 3],
}

impl Services<'_> {
    fn storage(&self) -> core::cell::RefMut<'_, Storage<board::Flash>> {
        self.shared.storage.borrow_mut()
    }
}

impl Provider for Services<'_> {
    fn diagnostics(&mut self) -> CaseResult<Diagnostics> {
        Ok(board::diagnostics(self.shared.milliseconds.get(), self.shared.dropped_events.get(), || self.storage().logged_crash()))
    }

    fn telemetry(&mut self) -> TelemetryResult {
        Ok(board::telemetry(self.shared.milliseconds.get()))
    }

    fn measure_impedance(&mut self) -> ImpedanceResult {
        self.shared.audio.borrow_mut().measure_impedance()
    }

    fn audio_meters(&mut self) -> AudioMeters {
        self.shared.uac.borrow_mut().take_meters()
    }

    fn play_test_tone(&mut self, request: TestToneRequest) -> Ack {
        let now_ms = (board::monotonic_us() / 1_000) as u32;
        self.shared.audio.borrow_mut().start_test_tone(request, now_ms)
    }

    fn subscribe(&mut self, events: EventMask) -> EventsResult {
        let available = (EventMask::KEYS | EventMask::LOG).with(EventMask::UART, self.shared.developer().is_ok());
        let granted = events & available;
        self.shared.subscribed.set(granted.contains(EventMask::KEYS));
        self.shared.startup_log.set(granted.contains(EventMask::LOG));
        self.shared.events.borrow_mut().clear();
        self.shared.stop_mirror();
        self.shared.mirror.set(granted.contains(EventMask::UART));
        Ok(granted)
    }
    fn device_info(&mut self) -> DeviceInfo {
        let storage = self.storage();
        let table = storage.slots();
        let firmware = table
            .booted
            .and_then(|slot| table.slots.iter().find(|s| s.slot == slot).and_then(|s| s.version))
            .unwrap_or(Version { major: 0, minor: 1, patch: 0 });
        let audio_present = self.shared.uac.borrow().present();
        DeviceInfo {
            protocol: PROTOCOL_VERSION,
            firmware,
            build: (if board::loaded_by_host() { RAM_BUILD } else { SLOT_BUILD }).try_into().unwrap(),
            chip_id: board::read(kumi_chip::regs::CHIP_ID),
            flash_jedec: self.jedec,
            serial: board::serial(),
            running_from_ram: board::loaded_by_host(),
            capabilities: (Capabilities::KEYMAP | Capabilities::LAYERS | Capabilities::DEVELOPER)
                .with(Capabilities::AUDIO_OUT | Capabilities::PARAMETRIC_EQ | Capabilities::DSD, audio_present)
                .with(Capabilities::PERSISTENT_SETTINGS | Capabilities::FIRMWARE_UPDATE, storage.writable())
                .with(Capabilities::LAB, board::LAB),
        }
    }

    fn audio_supported(&self) -> bool {
        true
    }

    fn audio_status(&mut self) -> AudioStatus {
        self.shared.uac.borrow().audio_status()
    }

    fn keyboard_status(&mut self) -> KeyboardStatus {
        let mut status = self.shared.status.get();
        status.host_leds = self.shared.leds.get();
        status
    }

    fn slots(&mut self) -> SlotTable {
        self.storage().slots()
    }

    fn save(&mut self, settings: &StoredSettings) -> Ack {
        let saved = self.storage().save(settings);
        if saved.is_ok() {
            self.shared.autosave.borrow_mut().saved();
            self.shared.explicit_save.set(true);
        }
        saved
    }

    fn reboot(&mut self, target: RebootTarget) -> Ack {
        self.storage().request_reboot(target, board::verify)
    }

    fn confirm_boot(&mut self) -> Ack {
        self.storage().confirm()
    }

    fn update_begin(&mut self, manifest: UpdateManifest) -> Ack {
        self.storage().update_begin(manifest)
    }

    fn update_chunk(&mut self, block: UpdateBlock) -> CountResult {
        self.storage().update_chunk(block)
    }

    fn update_finish(&mut self) -> SlotResult {
        self.storage().update_finish(board::verify)
    }

    fn dev_unlock(&mut self, request: DevUnlockRequest) -> Ack {
        self.shared.unlock(request.seconds);
        Ok(())
    }

    fn dev_peek(&mut self, request: PeekRequest) -> WordsResult {
        self.shared.developer()?;
        if !board::peekable(request.address, request.words) {
            return Err(CaseError::InvalidArgument);
        }
        let words = if request.address >= kumi_layout::FLASH_XIP_BASE {
            board::peek_flash(self.storage().flash(), request.address - kumi_layout::FLASH_XIP_BASE, request.words)
                .map_err(|_| CaseError::HardwareFault)?
        } else {
            board::peek(request.address, request.words)
        };
        Ok(WordBlock { address: request.address, words })
    }

    fn dev_poke(&mut self, request: PokeRequest) -> CountResult {
        if !board::raw_access() {
            return Err(CaseError::WrongState);
        }
        self.shared.developer()?;
        #[cfg(feature = "lab")]
        if crate::lab::control_pokeable(request.address) {
            return Ok(board::poke(request.address, request.value, request.mask));
        }
        if !board::pokeable(request.address, request.mask) {
            return Err(CaseError::InvalidArgument);
        }
        Ok(board::poke(request.address, request.value, request.mask))
    }

    fn dev_i2c(&mut self, request: I2cRequest) -> I2cResult {
        if !board::raw_access() {
            return Err(CaseError::WrongState);
        }
        self.shared.developer()?;
        if !board::i2c_permitted(request.bus, request.address, &request.write, request.read) {
            return Err(CaseError::InvalidArgument);
        }
        let mut data = heapless::Vec::<u8, BYTE_BLOCK>::new();
        data.resize(usize::from(request.read), 0).map_err(|_| CaseError::InvalidArgument)?;
        match board::i2c(request.bus, request.address, &request.write, &mut data) {
            Ok(()) => Ok(I2cReply { acked: true, data }),
            Err(kumi_audio::i2c::I2cError::Nack) => Ok(I2cReply { acked: false, data: heapless::Vec::new() }),
            Err(kumi_audio::i2c::I2cError::Invalid) => Err(CaseError::InvalidArgument),
            Err(_) => Err(CaseError::HardwareFault),
        }
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn dev_uart(&mut self, block: ByteBlock) -> Ack {
        if !board::LAB {
            return Err(CaseError::Unsupported);
        }
        self.shared.developer()?;
        if block.bytes.is_empty() {
            return Err(CaseError::InvalidArgument);
        }
        let mut queue = self.shared.uart_tx.borrow_mut();
        if queue.capacity() - queue.len() < block.bytes.len() {
            return Err(CaseError::Busy);
        }
        for byte in block.bytes {
            let _ = queue.push_back(byte);
        }
        Ok(())
    }

    #[unsafe(link_section = ".text.cold")]
    #[inline(never)]
    fn dev_audio(&mut self, request: AudioBenchRequest) -> AudioBenchResult {
        if !board::raw_access() {
            return Err(CaseError::WrongState);
        }
        self.shared.developer()?;
        self.shared.audio.borrow_mut().request(request)
    }

    fn dev_fault(&mut self, kind: FaultKind) -> Ack {
        self.shared.developer()?;
        self.shared.fault.set(Some(kind));
        Ok(())
    }

    fn dev_flash_test(&mut self) -> FlashTestResult {
        if !board::loaded_by_host() {
            return Err(CaseError::WrongState);
        }
        self.shared.developer()?;
        if self.jedec != kumi_chip::scratch::JEDEC {
            return Err(CaseError::HardwareFault);
        }
        let report = board::flash_test(self.storage().flash()).map_err(|_| CaseError::HardwareFault)?;
        Ok(FlashTestReport {
            window_was_erased: report.window_was_erased,
            page_matches: report.page_matches,
            unaligned_matches: report.unaligned_matches,
            sector_erased: report.sector_erased,
            block_erased: report.block_erased,
            page_program_cycles: report.page_program_cycles,
            sector_erase_cycles: report.sector_erase_cycles,
            block_erase_cycles: report.block_erase_cycles,
            stock_head_crc_before: report.head_crc_before,
            stock_head_crc_after: report.head_crc_after,
        })
    }
}

struct Reply {
    bytes: heapless::Vec<u8, { MAX_PAYLOAD + 16 }>,
    reboot: Option<RebootTarget>,
    fault: Option<FaultKind>,
}

#[unsafe(link_section = ".text.cold")]
pub async fn rpc<'d>(input: usb::Endpoint<'d>, output: usb::Endpoint<'d>, shared: &Shared, jedec: [u8; 3]) -> ! {
    let queue = RefCell::new(Deque::<Reply, 4>::new());
    let mut reader = pin!(requests(input, shared, jedec, &queue));
    let mut writer = pin!(responses(output, shared, &queue));
    poll_fn(|cx| {
        let _ = reader.as_mut().poll(cx);
        let _ = writer.as_mut().poll(cx);
        Poll::<()>::Pending
    })
    .await;
    unreachable!()
}

async fn requests<'d>(mut input: usb::Endpoint<'d>, shared: &Shared, jedec: [u8; 3], queue: &RefCell<Deque<Reply, 4>>) -> ! {
    let mut provider = Services { shared, jedec };
    let mut frame = [0u8; MAX_PAYLOAD + 16];
    let mut response = [0u8; MAX_PAYLOAD + 16];
    loop {
        poll_fn(|_| if queue.borrow().is_full() { Poll::Pending } else { Poll::Ready(()) }).await;
        input.wait_enabled().await;
        let mut used = 0;
        let mut overflow = false;
        let max_packet = usize::from(embassy_usb_driver::Endpoint::info(&input).max_packet_size);
        loop {
            let mut packet = [0u8; 512];
            let count = match input.read(&mut packet).await {
                Ok(n) => n,
                Err(_) => {
                    used = 0;
                    break;
                }
            };
            if let Some(dst) = frame.get_mut(used..used + count) {
                dst.copy_from_slice(&packet[..count]);
                used += count;
            } else {
                overflow = true;
            }
            if count < max_packet {
                break;
            }
        }
        if overflow || used == 0 {
            continue;
        }
        if shared.bus_reset.replace(false) {
            shared.storage.borrow_mut().abort_update();
        }
        shared.touch_activity();
        shared.rpc_activity.set(shared.rpc_activity.get().wrapping_add(1));
        shared.flush_mirror();
        let (size, active_eq) = {
            let mut controls = shared.controls.borrow_mut();
            let size = kumi_service::serve_frame(&mut controls, &mut provider, &frame[..used], &mut response);
            (size, controls.active_eq())
        };
        {
            let mut uac = shared.uac.borrow_mut();
            if !uac.present() {
                uac.set_eq_active(active_eq);
            }
        }
        if let Some(size) = size {
            let reply = Reply {
                bytes: heapless::Vec::from_slice(&response[..size]).unwrap(),
                reboot: shared.storage.borrow_mut().take_reboot(),
                fault: shared.fault.take(),
            };
            let _ = queue.borrow_mut().push_back(reply);
        }
    }
}

fn topic<T: Topic>(value: &T::Message) -> Option<Reply>
where
    T::Message: serde::Serialize,
{
    let mut bytes = [0; MAX_PAYLOAD + 16];
    let length = frame::encode(T::TOPIC_KEY, frame::TOPIC_SEQUENCE, value, &mut bytes)?;
    Some(Reply { bytes: heapless::Vec::from_slice(&bytes[..length]).ok()?, reboot: None, fault: None })
}

async fn responses<'d>(mut output: usb::Endpoint<'d>, shared: &Shared, queue: &RefCell<Deque<Reply, 4>>) -> ! {
    loop {
        output.wait_enabled().await;
        let reply = poll_fn(|_| {
            if shared.startup_log.replace(false) {
                return topic::<LogTopic>(&LogLine {
                    uptime_ms: shared.milliseconds.get(),
                    level: LogLevel::Info,
                    module: "runtime".try_into().unwrap(),
                    text: if shared.usb_high_speed.get() {
                        "Event stream started at USB high speed"
                    } else {
                        "Event stream started at USB full speed"
                    }
                    .try_into()
                    .unwrap(),
                })
                .map_or(Poll::Pending, Poll::Ready);
            }
            if let Some(event) = shared.events.borrow_mut().pop_front() {
                return topic::<KeyEventTopic>(&event).map_or(Poll::Pending, Poll::Ready);
            }
            if let Some(block) = shared.mirrored.borrow_mut().pop_front() {
                return topic::<UartMirrorTopic>(&block).map_or(Poll::Pending, Poll::Ready);
            }
            if let Some(reply) = queue.borrow_mut().pop_front() {
                return Poll::Ready(reply);
            }
            Poll::Pending
        })
        .await;
        if output.write_transfer(&reply.bytes, true).await.is_ok() {
            if let Some(kind) = reply.fault {
                board::fault(kind);
            }
            if let Some(target) = reply.reboot {
                let _ = shared.note_changes();
                shared.persist_settings(SaveTrigger::LinkDown, false);
                board::reset(target);
            }
        } else {
            shared.events.borrow_mut().clear();
            shared.mirrored.borrow_mut().clear();
            queue.borrow_mut().clear();
        }
    }
}

pub async fn hid_reports<'d>(
    mut writer: hid::HidWriter<'d, usb::Driver<'d>, 16>,
    shared: &Shared,
    bus: &usb::State,
    mouse_enabled: bool,
) -> ! {
    let mut epoch = u32::MAX;
    let mut protocol = HidProtocolMode::Report;
    let mut sent_at = [0u32; 2];
    let mut last_mouse_buttons = 0u8;
    loop {
        writer.ready().await;
        let (keyboard, consumer, mouse) = poll_fn(|_| {
            let current = (shared.keyboard.get(), shared.consumer.get(), shared.mouse.get());
            if epoch != bus.epoch() || protocol != shared.protocol.get() {
                shared.reports.borrow_mut().clear();
                return Poll::Ready(current);
            }
            if let Some(next) = shared.reports.borrow_mut().pop_front() {
                return Poll::Ready(next);
            }
            let now = shared.milliseconds.get();
            let count = if shared.protocol.get() == HidProtocolMode::Boot { 1 } else { 2 };
            if shared
                .idle
                .iter()
                .zip(sent_at)
                .take(count)
                .any(|(idle, at)| idle.get() != u32::MAX && now.wrapping_sub(at) >= idle.get())
            {
                Poll::Ready(current)
            } else {
                Poll::Pending
            }
        })
        .await;
        epoch = bus.epoch();
        protocol = shared.protocol.get();
        let keyboard_result = if protocol == HidProtocolMode::Boot {
            writer.write(&kumi_keymap::hid::boot_keyboard_report(&keyboard)).await
        } else {
            writer.write(&kumi_keymap::hid::keyboard_report(&keyboard)).await
        };
        if keyboard_result.is_err() {
            epoch = u32::MAX;
            continue;
        }
        sent_at[0] = shared.milliseconds.get();
        if protocol == HidProtocolMode::Report {
            if writer.write(&kumi_keymap::hid::consumer_report(consumer)).await.is_err() {
                epoch = u32::MAX;
                continue;
            }
            sent_at[1] = shared.milliseconds.get();
            if mouse_enabled && (mouse.buttons != last_mouse_buttons || mouse.has_motion()) {
                if writer.write(&kumi_keymap::hid::mouse_report(&mouse)).await.is_err() {
                    epoch = u32::MAX;
                    continue;
                }
                last_mouse_buttons = mouse.buttons;
            }
        }
    }
}

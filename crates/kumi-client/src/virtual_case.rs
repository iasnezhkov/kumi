// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::io;

use kumi_control::Controls;
use kumi_icd::{
    Ack, AudioCounters, AudioStatus, BYTE_BLOCK, Capabilities, CaseError, CountResult, CrashReason, CrashReport,
    DevUnlockRequest, DeviceInfo, Diagnostics, DiagnosticsResult, EventMask, EventsResult, FaultKind, I2cReply, I2cRequest,
    I2cResult, Jack, KeyEvent, KeyEventTopic, KeyboardStatus, LogLevel, LogLine, LogTopic, MAX_PAYLOAD, PROTOCOL_VERSION,
    PeekRequest, PokeRequest, RebootTarget, Slot, SlotInfo, SlotState, SlotTable, SystemTelemetry, TelemetryResult, Version,
    WordBlock, WordsResult, frame, stage,
};
use kumi_keymap::{CONSUMER_SOURCE_BASE, Engine, Input, Keymap, MODIFIER_FIRST, MODIFIER_LAST, ROLLOVER};
use kumi_service::{Provider, serve_frame};
use kumi_settings::StoredSettings;
use postcard_rpc::Topic;
use serde::Serialize;

pub const CHIP_ID: u32 = 0x4901_4480;
pub const CHIP_ID_ADDRESS: u32 = 0x2000_0000;
pub const FLASH_JEDEC: [u8; 3] = [0xC8, 0x65, 0x16];
pub const CONSUMER_DEFAULTS: [u16; 8] = kumi_keymap::STOCK_CONSUMER_USAGES;
pub const I2C_BUSES: u8 = 2;
pub const BUILD: &str = "virtual";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VirtualError {
    NoFrame,
}

impl fmt::Display for VirtualError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VirtualError::NoFrame => f.write_str("no frame from the virtual case"),
        }
    }
}

impl std::error::Error for VirtualError {}

impl From<VirtualError> for io::Error {
    fn from(error: VirtualError) -> Self {
        io::Error::new(io::ErrorKind::TimedOut, error)
    }
}

struct Firmware {
    uptime_ms: u32,
    boot_count: u32,
    unlocked_until: Option<u32>,
    startup_log: bool,
    keys: bool,
    memory: BTreeMap<u32, u32>,
    i2c: BTreeMap<(u8, u8), I2cDevice>,
    keyboard: KeyboardStatus,
    last_crash: Option<CrashReport>,
    restart: Option<Restart>,
    reboots: Vec<RebootTarget>,
}

struct I2cDevice {
    pointer: u8,
    registers: [u8; 256],
}

enum Restart {
    Reboot,
    Fault(FaultKind),
}

impl Firmware {
    fn new() -> Self {
        Self {
            uptime_ms: 0,
            boot_count: 1,
            unlocked_until: None,
            startup_log: false,
            keys: false,
            memory: BTreeMap::from([(CHIP_ID_ADDRESS, CHIP_ID)]),
            i2c: BTreeMap::new(),
            keyboard: KeyboardStatus::default(),
            last_crash: None,
            restart: None,
            reboots: Vec::new(),
        }
    }

    fn developer(&self) -> Ack {
        match self.unlocked_until {
            Some(until) if until.wrapping_sub(self.uptime_ms) as i32 > 0 => Ok(()),
            _ => Err(CaseError::NotUnlocked),
        }
    }

    fn restarted(&mut self) {
        self.boot_count = self.boot_count.wrapping_add(1);
        self.uptime_ms = 0;
        self.unlocked_until = None;
        self.startup_log = false;
        self.keys = false;
        self.keyboard = KeyboardStatus::default();
    }
}

impl Provider for Firmware {
    fn audio_supported(&self) -> bool {
        false
    }

    fn device_info(&mut self) -> DeviceInfo {
        DeviceInfo {
            protocol: PROTOCOL_VERSION,
            firmware: Version { major: 0, minor: 1, patch: 0 },
            build: BUILD.try_into().unwrap_or_default(),
            chip_id: CHIP_ID,
            flash_jedec: FLASH_JEDEC,
            serial: None,
            running_from_ram: true,
            capabilities: Capabilities::KEYMAP | Capabilities::LAYERS | Capabilities::DEVELOPER,
        }
    }

    fn audio_status(&mut self) -> AudioStatus {
        AudioStatus {
            jack: Jack::Unknown,
            usb_audio_present: false,
            stream: None,
            host_volume_centi_db: [0; 2],
            host_muted: false,
            eq_active: None,
            counters: AudioCounters::default(),
        }
    }

    fn keyboard_status(&mut self) -> KeyboardStatus {
        self.keyboard
    }

    fn slots(&mut self) -> SlotTable {
        let empty = |slot| SlotInfo { slot, state: SlotState::Empty, version: None, size: 0 };
        SlotTable { booted: None, slots: [empty(Slot::A), empty(Slot::B)] }
    }

    fn save(&mut self, _settings: &StoredSettings) -> Ack {
        Err(CaseError::Unsupported)
    }

    fn reboot(&mut self, target: RebootTarget) -> Ack {
        if target == RebootTarget::OtherSlot {
            return Err(CaseError::Unsupported);
        }
        self.reboots.push(target);
        self.restart = Some(Restart::Reboot);
        Ok(())
    }

    fn confirm_boot(&mut self) -> Ack {
        Err(CaseError::Unsupported)
    }

    fn diagnostics(&mut self) -> DiagnosticsResult {
        Ok(Diagnostics {
            boot_count: self.boot_count,
            safe_mode: false,
            stage: stage::FW_RUNNING,
            uptime_ms: self.uptime_ms,
            dropped_events: 0,
            watchdog_armed: false,
            last_crash: self.last_crash.clone(),
        })
    }

    fn telemetry(&mut self) -> TelemetryResult {
        Ok(SystemTelemetry {
            cpu_mhz: 90,
            dcdc_active: true,
            dvdd_mv: 1_400,
            analog_rails_on: false,
            audio_pll_enabled: false,
            wfi_percent_tenths: 990,
            wfi_wakeups: self.uptime_ms / 20,
            jack_adc: 1_020,
            uptime_ms: self.uptime_ms,
        })
    }

    fn subscribe(&mut self, events: EventMask) -> EventsResult {
        let available = (EventMask::KEYS | EventMask::LOG).with(EventMask::UART, self.developer().is_ok());
        let granted = events & available;
        self.keys = granted.contains(EventMask::KEYS);
        self.startup_log = granted.contains(EventMask::LOG);
        Ok(granted)
    }

    fn dev_unlock(&mut self, request: DevUnlockRequest) -> Ack {
        self.unlocked_until = Some(self.uptime_ms.wrapping_add(1000 * u32::from(request.seconds)));
        Ok(())
    }

    fn dev_peek(&mut self, request: PeekRequest) -> WordsResult {
        self.developer()?;
        let words = (0..u32::from(request.words))
            .map(|i| self.memory.get(&(request.address + 4 * i)).copied().unwrap_or(0))
            .collect::<heapless::Vec<u32, { kumi_icd::WORD_BLOCK }>>();
        Ok(WordBlock { address: request.address, words })
    }

    fn dev_poke(&mut self, request: PokeRequest) -> CountResult {
        self.developer()?;
        let word = self.memory.entry(request.address).or_insert(0);
        *word = (*word & !request.mask) | (request.value & request.mask);
        Ok(*word)
    }

    fn dev_i2c(&mut self, request: I2cRequest) -> I2cResult {
        self.developer()?;
        if request.bus >= I2C_BUSES {
            return Err(CaseError::InvalidArgument);
        }
        let Some(device) = self.i2c.get_mut(&(request.bus, request.address)) else {
            return Ok(I2cReply { acked: false, data: heapless::Vec::new() });
        };
        if let Some((register, data)) = request.write.split_first() {
            device.pointer = *register;
            for byte in data {
                device.registers[usize::from(device.pointer)] = *byte;
                device.pointer = device.pointer.wrapping_add(1);
            }
        }
        let mut data = heapless::Vec::<u8, BYTE_BLOCK>::new();
        for _ in 0..request.read {
            let _ = data.push(device.registers[usize::from(device.pointer)]);
            device.pointer = device.pointer.wrapping_add(1);
        }
        Ok(I2cReply { acked: true, data })
    }

    fn dev_fault(&mut self, kind: FaultKind) -> Ack {
        self.developer()?;
        self.restart = Some(Restart::Fault(kind));
        Ok(())
    }
}

pub struct VirtualCase {
    controls: Controls,
    firmware: Firmware,
    keymap: Box<Keymap>,
    engine: Engine,
    input: Input,
    outgoing: VecDeque<Vec<u8>>,
}

impl Default for VirtualCase {
    fn default() -> Self {
        Self::new()
    }
}

impl VirtualCase {
    pub fn new() -> Self {
        Self {
            controls: Controls::new(StoredSettings::default()),
            firmware: Firmware::new(),
            keymap: Box::new(Keymap::new(CONSUMER_DEFAULTS)),
            engine: Engine::default(),
            input: Input::default(),
            outgoing: VecDeque::new(),
        }
    }

    pub fn settings(&self) -> &StoredSettings {
        self.controls.settings()
    }

    pub fn reboots(&self) -> &[RebootTarget] {
        &self.firmware.reboots
    }

    pub fn boot_count(&self) -> u32 {
        self.firmware.boot_count
    }

    pub fn advance(&mut self, milliseconds: u32) {
        self.firmware.uptime_ms = self.firmware.uptime_ms.wrapping_add(milliseconds);
    }

    pub fn add_i2c_device(&mut self, bus: u8, address: u8) {
        self.firmware.i2c.insert((bus, address), I2cDevice { pointer: 0, registers: [0; 256] });
    }

    pub fn press(&mut self, source: u8) {
        self.key(source, true);
    }

    pub fn release(&mut self, source: u8) {
        self.key(source, false);
    }

    pub fn tap(&mut self, source: u8) {
        self.press(source);
        self.release(source);
    }

    fn key(&mut self, source: u8, pressed: bool) {
        match source {
            MODIFIER_FIRST..=MODIFIER_LAST => {
                let bit = 1 << (source - MODIFIER_FIRST);
                self.input.keyboard.modifiers =
                    if pressed { self.input.keyboard.modifiers | bit } else { self.input.keyboard.modifiers & !bit };
            }
            s if s >= CONSUMER_SOURCE_BASE && s - CONSUMER_SOURCE_BASE < 8 => {
                let bit = 1 << (s - CONSUMER_SOURCE_BASE);
                self.input.consumer_bits = if pressed { self.input.consumer_bits | bit } else { self.input.consumer_bits & !bit };
            }
            _ => {
                let keys = &mut self.input.keyboard.keys;
                if pressed && !keys.contains(&source) {
                    if let Some(slot) = keys.iter_mut().find(|k| **k == 0) {
                        *slot = source;
                    }
                } else if !pressed {
                    let kept: Vec<u8> = keys.iter().copied().filter(|k| *k != source && *k != 0).collect();
                    *keys = [0; ROLLOVER];
                    keys[..kept.len()].copy_from_slice(&kept);
                }
            }
        }
        let output = self.engine.process_at(&self.keymap, &self.input, self.firmware.uptime_ms);
        let status = &mut self.firmware.keyboard;
        status.link_up = true;
        status.frames = status.frames.wrapping_add(1);
        status.active_layer = self.engine.top_layer();
        status.caps_word = self.engine.caps_word();
        if self.firmware.keys {
            for event in output.events.iter() {
                self.topic::<KeyEventTopic, KeyEvent>(event);
            }
        }
    }

    fn topic<T: Topic<Message = M>, M: Serialize>(&mut self, message: &M) {
        let mut out = [0u8; frame::MAX_FRAME];
        if let Some(used) = frame::encode(T::TOPIC_KEY, frame::TOPIC_SEQUENCE, message, &mut out) {
            self.outgoing.push_back(out[..used].to_vec());
        }
    }

    fn rebuild_keymap(&mut self) {
        self.keymap.clear();
        self.keymap.clear_macros();
        self.keymap.clear_combos();
        self.keymap.set_config(self.controls.settings().key_config);
        for (slot, program) in self.controls.settings().macros.iter().enumerate() {
            let _ = self.keymap.set_macro(slot as u8, program.clone());
        }
        for (slot, combo) in self.controls.settings().combos.iter().enumerate() {
            let _ = self.keymap.set_combo(slot as u8, *combo);
        }
        for entry in &self.controls.settings().keymap {
            let _ = self.keymap.set(entry.layer, entry.source, entry.action);
        }
    }

    fn restart(&mut self, reason: Restart) {
        if let Restart::Fault(kind) = reason {
            let (reason, mcause) = match kind {
                FaultKind::IllegalInstruction => (CrashReason::Trap, 2),
                FaultKind::Breakpoint => (CrashReason::Trap, 3),
                FaultKind::Panic => (CrashReason::Panic, 0),
            };
            self.firmware.last_crash = Some(CrashReport {
                boot_count: self.firmware.boot_count,
                reason,
                stage: stage::FW_RUNNING,
                mcause,
                mepc: 0,
                mtval: 0,
                ra: 0,
                sp: 0,
                uptime_ms: self.firmware.uptime_ms,
                message: "virtual case fault".try_into().unwrap_or_default(),
            });
        }
        self.firmware.restarted();
        self.controls = Controls::new(StoredSettings::default());
        self.rebuild_keymap();
        self.engine.reset();
        self.input = Input::default();
    }
}

impl crate::wire::FrameLink for VirtualCase {
    type Error = VirtualError;

    fn send(&mut self, request: &[u8]) -> Result<(), VirtualError> {
        let mut out = [0u8; MAX_PAYLOAD + frame::MAX_HEADER];
        let reply = serve_frame(&mut self.controls, &mut self.firmware, request, &mut out);
        if self.controls.dirty() {
            self.controls.clear_dirty();
            self.rebuild_keymap();
        }
        if std::mem::take(&mut self.firmware.startup_log) {
            let line = LogLine {
                uptime_ms: self.firmware.uptime_ms,
                level: LogLevel::Info,
                module: "runtime".try_into().unwrap_or_default(),
                text: "Event stream started on the virtual case".try_into().unwrap_or_default(),
            };
            self.topic::<LogTopic, LogLine>(&line);
        }
        if let Some(used) = reply {
            self.outgoing.push_back(out[..used].to_vec());
        }
        if let Some(reason) = self.firmware.restart.take() {
            self.restart(reason);
        }
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>, VirtualError> {
        self.outgoing.pop_front().ok_or(VirtualError::NoFrame)
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, VirtualError> {
        Ok(self.outgoing.pop_front())
    }
}

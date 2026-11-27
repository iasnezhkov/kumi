// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::VecDeque;

use ed25519_dalek::SigningKey;
use embedded_storage::nor_flash::{ErrorType, NorFlash, NorFlashError, NorFlashErrorKind, ReadNorFlash};
use heapless::String;
use kumi_boot_core::stage0::{BootInfo, DiagAreas, Memory, Outcome, Stage0};
use kumi_boot_core::verify_slot;
use kumi_buxx::Link;
use kumi_buxx::sim::SimBrom;
use kumi_control::Controls;
use kumi_icd::*;
use kumi_install::spv::{self, Verdict};
use kumi_install::{bundle, sample};
use kumi_layout::{SECTOR, STAGE0, SlotId, WriteGuard, Writer, region_for_write};
use kumi_runtime_core::Storage;
use kumi_service::{Provider, serve_frame};
use kumi_settings::StoredSettings;
use postcard_rpc::Topic;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Stock = 0,
    Service = 1,
    Kumi = 2,
}

impl Mode {
    pub fn from_index(index: i32) -> Option<Self> {
        match index {
            0 => Some(Mode::Stock),
            1 => Some(Mode::Service),
            2 => Some(Mode::Kumi),
            _ => None,
        }
    }
}

pub fn demo_key() -> SigningKey {
    SigningKey::from_bytes(&[0x4B; 32])
}

pub fn demo_public_hex() -> std::string::String {
    crate::api::hex(demo_key().verifying_key().as_bytes())
}

pub fn demo_package(version: [u8; 3], generation: u32) -> Vec<u8> {
    let signer = demo_key();
    let stage0 = sample::stage0(&signer.verifying_key().to_bytes());
    let image = sample::image(&signer, version, generation);
    let name = format!("demo-{}.{}.{}", version[0], version[1], version[2]);
    bundle::build(&stage0, &image, &name, &signer).expect("the demo parts always form a package")
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OutOfBounds;

impl NorFlashError for OutOfBounds {
    fn kind(&self) -> NorFlashErrorKind {
        NorFlashErrorKind::OutOfBounds
    }
}

pub struct RamFlash(Vec<u8>, Option<Writer>);

impl RamFlash {
    fn writable(&self, offset: u32, len: usize) -> Result<core::ops::Range<usize>, OutOfBounds> {
        let writer = self.1.ok_or(OutOfBounds)?;
        region_for_write(offset, u32::try_from(len).map_err(|_| OutOfBounds)?, writer).ok_or(OutOfBounds)?;
        self.range(offset, len)
    }

    fn range(&self, offset: u32, len: usize) -> Result<core::ops::Range<usize>, OutOfBounds> {
        let start = offset as usize;
        let end = start.checked_add(len).filter(|end| *end <= self.0.len()).ok_or(OutOfBounds)?;
        Ok(start..end)
    }
}

impl ErrorType for RamFlash {
    type Error = OutOfBounds;
}

impl ReadNorFlash for RamFlash {
    const READ_SIZE: usize = 1;

    fn read(&mut self, offset: u32, bytes: &mut [u8]) -> Result<(), OutOfBounds> {
        let range = self.range(offset, bytes.len())?;
        bytes.copy_from_slice(&self.0[range]);
        Ok(())
    }

    fn capacity(&self) -> usize {
        self.0.len()
    }
}

impl NorFlash for RamFlash {
    const WRITE_SIZE: usize = 1;
    const ERASE_SIZE: usize = SECTOR as usize;

    fn erase(&mut self, from: u32, to: u32) -> Result<(), OutOfBounds> {
        let range = self.writable(from, to.checked_sub(from).ok_or(OutOfBounds)? as usize)?;
        self.0[range].fill(0xFF);
        Ok(())
    }

    fn write(&mut self, offset: u32, bytes: &[u8]) -> Result<(), OutOfBounds> {
        let range = self.writable(offset, bytes.len())?;
        self.0[range].iter_mut().zip(bytes).for_each(|(cell, byte)| *cell &= byte);
        Ok(())
    }
}

impl WriteGuard for RamFlash {
    fn set_writer(&mut self, writer: Option<Writer>) {
        self.1 = writer;
    }
}

struct NoMemory;

impl Memory for NoMemory {
    fn write(&mut self, _address: u32, _data: &[u8]) {}
    fn zero(&mut self, _address: u32, _len: u32) {}
}

fn verify(flash: &mut RamFlash, slot: SlotId) -> bool {
    verify_slot(flash, slot, &demo_key().verifying_key()).is_ok()
}

struct Firmware {
    storage: Storage<RamFlash>,
    info: Option<BootInfo>,
    events: EventMask,
    pending_log: bool,
    uptime_ms: u32,
    unlocked: bool,
    active_eq: Option<u8>,
}

impl Provider for Firmware {
    fn device_info(&mut self) -> DeviceInfo {
        let table = self.storage.slots();
        let firmware = table
            .booted
            .and_then(|slot| table.slots.iter().find(|s| s.slot == slot).and_then(|s| s.version))
            .unwrap_or(Version { major: 0, minor: 0, patch: 0 });
        DeviceInfo {
            protocol: PROTOCOL_VERSION,
            firmware,
            build: String::try_from("simulated").unwrap_or_default(),
            chip_id: kumi_buxx::EXPECTED_CHIP,
            flash_jedec: [0xC8, 0x65, 0x16],
            serial: Some(*b"SIMCASE1"),
            running_from_ram: self.info.is_none(),
            capabilities: (Capabilities::AUDIO_OUT
                | Capabilities::DSD
                | Capabilities::PARAMETRIC_EQ
                | Capabilities::KEYMAP
                | Capabilities::LAYERS
                | Capabilities::DEVELOPER)
                .with(Capabilities::PERSISTENT_SETTINGS | Capabilities::FIRMWARE_UPDATE, self.storage.writable()),
        }
    }

    fn audio_status(&mut self) -> AudioStatus {
        AudioStatus {
            jack: Jack::Headphones,
            usb_audio_present: true,
            stream: Some(StreamFormat { kind: StreamKind::Pcm, rate_hz: 96_000, bits: 24, channels: 2 }),
            host_volume_centi_db: [-1_200, -1_200],
            host_muted: false,
            eq_active: self.active_eq,
            counters: AudioCounters::default(),
        }
    }

    fn keyboard_status(&mut self) -> KeyboardStatus {
        KeyboardStatus {
            link_up: true,
            frames: 1_024,
            bad_frames: 0,
            controller_commands: 3,
            active_layer: 0,
            caps_word: false,
            host_leds: 0,
        }
    }

    fn slots(&mut self) -> SlotTable {
        self.storage.slots()
    }

    fn save(&mut self, settings: &StoredSettings) -> Ack {
        self.storage.save(settings)
    }

    fn reboot(&mut self, target: RebootTarget) -> Ack {
        self.storage.request_reboot(target, verify)
    }

    fn confirm_boot(&mut self) -> Ack {
        self.storage.confirm()
    }

    fn diagnostics(&mut self) -> DiagnosticsResult {
        let safe_mode = self.info.is_some_and(|info| info.safe_mode);
        Ok(Diagnostics {
            boot_count: self.info.map_or(0, |info| info.boot_count),
            safe_mode,
            stage: if safe_mode { stage::FW_SAFE_MODE } else { stage::FW_RUNNING },
            uptime_ms: self.uptime_ms,
            dropped_events: 0,
            watchdog_armed: false,
            last_crash: None,
        })
    }

    fn telemetry(&mut self) -> TelemetryResult {
        Ok(SystemTelemetry {
            cpu_mhz: 90,
            dcdc_active: true,
            dvdd_mv: 1_400,
            analog_rails_on: true,
            audio_pll_enabled: true,
            wfi_percent_tenths: 994,
            wfi_wakeups: self.uptime_ms / 20,
            jack_adc: 202,
            uptime_ms: self.uptime_ms,
        })
    }

    fn presence(&mut self) -> PresenceStatus {
        PresenceStatus { tier: 1, remaining_ms: 120_000, passthrough: false }
    }

    fn measure_impedance(&mut self) -> ImpedanceResult {
        Ok(ImpedanceReport {
            left_ohms: Some(32),
            right_ohms: Some(32),
            left_ac_ohms: Some(36),
            right_ac_ohms: Some(36),
            left_status: 0x0d,
            right_status: 0x0d,
            hp_status: 0x02,
            load_stat: 0x00,
        })
    }

    fn audio_meters(&mut self) -> AudioMeters {
        let phase = ((self.uptime_ms / 10) % 16) as i16;
        AudioMeters {
            active: true,
            peak_left_centi_db: -1_800 + phase * 60,
            peak_right_centi_db: -2_000 + phase * 55,
            rms_left_centi_db: -2_400 + phase * 45,
            rms_right_centi_db: -2_550 + phase * 40,
            clipped_samples: 0,
            limiter_hits: 0,
        }
    }

    fn audio_spectrum(&mut self) -> AudioSpectrum {
        let t = (self.uptime_ms / 40) as usize;
        let mut bands = [0u8; SPECTRUM_BANDS];
        for (i, slot) in bands.iter_mut().enumerate() {
            let slope = 132i32 - (i as i32 * 3);
            let wave = ((((t * 3 + i * 7) % 16) as i32) - 8).abs() * 3;
            *slot = (slope - wave).clamp(24, 152) as u8;
        }
        AudioSpectrum {
            active: true,
            bands_db_x2: bands,
            true_peak_left_centi_db: -1_150,
            true_peak_right_centi_db: -1_220,
            lufs_momentary_centi: -1_520,
            lufs_short_term_centi: -1_580,
            auto_level_gain_centi_db: -20,
        }
    }

    fn play_test_tone(&mut self, _request: TestToneRequest) -> Ack {
        Ok(())
    }

    fn subscribe(&mut self, events: EventMask) -> EventsResult {
        self.events = events & (EventMask::KEYS | EventMask::LOG);
        self.pending_log = self.events.contains(EventMask::LOG);
        Ok(self.events)
    }

    fn update_begin(&mut self, manifest: UpdateManifest) -> Ack {
        self.storage.update_begin(manifest)
    }

    fn update_chunk(&mut self, block: UpdateBlock) -> CountResult {
        self.storage.update_chunk(block)
    }

    fn update_finish(&mut self) -> SlotResult {
        self.storage.update_finish(verify)
    }

    fn dev_unlock(&mut self, _request: DevUnlockRequest) -> Ack {
        self.unlocked = true;
        Ok(())
    }

    fn dev_peek(&mut self, request: PeekRequest) -> WordsResult {
        if !self.unlocked {
            return Err(CaseError::NotUnlocked);
        }
        let mut words = heapless::Vec::new();
        for i in 0..u32::from(request.words) {
            let _ = words.push(request.address.wrapping_add(4 * i) ^ 0x5A5A_5A5A);
        }
        Ok(WordBlock { address: request.address, words })
    }
}

struct Running {
    controls: Controls,
    firmware: Firmware,
}

pub struct Sim {
    mode: Mode,
    brom: SimBrom,
    running: Option<Box<Running>>,
    retained: [[u8; kumi_diag::AREA_SIZE]; 2],
    inbox: VecDeque<Vec<u8>>,
}

impl Default for Sim {
    fn default() -> Self {
        Self::new()
    }
}

impl Sim {
    pub fn new() -> Self {
        Self::with_flash(&sample::stock())
    }

    pub fn with_flash(flash: &[u8]) -> Self {
        Self {
            mode: Mode::Stock,
            brom: SimBrom::new(flash),
            running: None,
            retained: [[0; kumi_diag::AREA_SIZE]; 2],
            inbox: VecDeque::new(),
        }
    }

    pub fn mode(&self) -> Mode {
        self.mode
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.inbox.clear();
        self.halt();
        if mode == Mode::Kumi {
            self.start(None);
        }
        self.mode = mode;
    }

    pub fn flash(&self) -> Option<&[u8]> {
        self.running.is_none().then_some(self.brom.flash.as_slice())
    }

    fn halt(&mut self) {
        if let Some(running) = self.running.take() {
            self.brom.flash = running.firmware.storage.into_flash().0;
        }
    }

    fn start(&mut self, info: Option<BootInfo>) {
        let mut storage = Storage::open(RamFlash(std::mem::take(&mut self.brom.flash), None), info);
        let settings = storage.load_settings();
        let firmware = Firmware {
            storage,
            info,
            events: EventMask::NONE,
            pending_log: false,
            uptime_ms: 0,
            unlocked: false,
            active_eq: settings.active_eq,
        };
        self.running = Some(Box::new(Running { controls: Controls::new(settings), firmware }));
        self.mode = Mode::Kumi;
    }

    fn boot_stage0(&mut self) {
        let key = demo_key().verifying_key();
        let flash = RamFlash(std::mem::take(&mut self.brom.flash), Some(Writer::Stage0));
        let [current, last_crash] = self.retained;
        let mut diag = DiagAreas { current, last_crash };
        let (outcome, _, flash) = Stage0 { key: Some(&key), dry_run: false }.run(flash, &mut NoMemory, &mut diag);
        self.retained = [diag.current, diag.last_crash];
        self.brom.flash = flash.0;
        match outcome {
            Outcome::Jump { info, .. } => self.start(Some(info)),
            Outcome::WouldJump { .. } | Outcome::BootRom(_) => self.mode = Mode::Service,
        }
    }

    fn boot_from_flash(&mut self) {
        let public = demo_key().verifying_key().to_bytes();
        let head = &self.brom.flash[..STAGE0.size as usize];
        match spv::brom_verdict(&self.brom.flash) {
            Verdict::Service => self.mode = Mode::Service,
            Verdict::Boots { .. } if head.windows(32).any(|w| w == public) => self.boot_stage0(),
            Verdict::Boots { .. } => self.mode = Mode::Stock,
        }
    }

    fn serve(&mut self, data: &[u8]) {
        let Some(running) = self.running.as_mut() else {
            return;
        };
        let Running { controls, firmware } = running.as_mut();
        firmware.uptime_ms = firmware.uptime_ms.wrapping_add(10);
        let mut out = [0u8; frame::MAX_FRAME];
        let reply = serve_frame(controls, firmware, data, &mut out).map(|used| out[..used].to_vec());
        firmware.active_eq = controls.active_eq();
        if controls.dirty() && firmware.storage.writable() && firmware.storage.save(controls.settings()).is_ok() {
            controls.clear_dirty();
        }
        if std::mem::take(&mut firmware.pending_log) {
            let line = LogLine {
                uptime_ms: firmware.uptime_ms,
                level: LogLevel::Info,
                module: String::try_from("runtime").unwrap_or_default(),
                text: String::try_from("Event stream started").unwrap_or_default(),
            };
            let mut topic = [0u8; frame::MAX_FRAME];
            if let Some(used) = frame::encode(LogTopic::TOPIC_KEY, frame::TOPIC_SEQUENCE, &line, &mut topic) {
                self.inbox.push_back(topic[..used].to_vec());
            }
        }
        self.inbox.extend(reply);
        match firmware.storage.take_reboot() {
            Some(RebootTarget::BootRom) => {
                self.halt();
                self.mode = Mode::Service;
            }
            Some(_) => {
                self.halt();
                self.boot_stage0();
            }
            None => {}
        }
    }

    pub fn send(&mut self, data: &[u8]) -> bool {
        match self.mode {
            Mode::Stock => false,
            Mode::Service => {
                let reboots = self.brom.reboots;
                match self.brom.transfer(data) {
                    Ok(reply) => {
                        self.inbox.push_back(reply);
                        if self.brom.reboots != reboots {
                            self.boot_from_flash();
                        }
                        true
                    }
                    Err(_) => false,
                }
            }
            Mode::Kumi => {
                self.serve(data);
                true
            }
        }
    }

    pub fn receive(&mut self) -> Option<Vec<u8>> {
        self.inbox.pop_front()
    }
}

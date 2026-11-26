// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
#![recursion_limit = "256"]
use kumi_install::GateEvidence;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use heapless::{String, Vec};
use kumi_client::events::Event;
use kumi_client::wire::{FrameLink, WireState};
use kumi_icd::*;
use kumi_jni::api;
use kumi_jni::sim::{self, Mode, Sim};
use postcard_rpc::Topic;
use serde_json::{Map, Value, json};

fn v<T: serde::Serialize>(value: T) -> Value {
    serde_json::to_value(value).unwrap()
}

fn hex(key: postcard_rpc::Key) -> std::string::String {
    api::hex(&key.to_bytes())
}

#[derive(Clone)]
struct Port(Rc<RefCell<Sim>>, Rc<RefCell<Option<usize>>>);

impl FrameLink for Port {
    type Error = std::string::String;

    fn send(&mut self, frame: &[u8]) -> Result<(), std::string::String> {
        if self.0.borrow_mut().send(frame) { Ok(()) } else { Err("no device".into()) }
    }

    fn receive(&mut self) -> Result<std::vec::Vec<u8>, std::string::String> {
        self.0.borrow_mut().receive().ok_or_else(|| "timeout".into())
    }

    fn try_receive(&mut self) -> Result<Option<std::vec::Vec<u8>>, std::string::String> {
        Ok(self.0.borrow_mut().receive())
    }
}

impl kumi_buxx::Link for Port {
    fn transfer(&mut self, packet: &[u8]) -> Result<std::vec::Vec<u8>, kumi_buxx::Error> {
        if let Some(left) = self.1.borrow_mut().as_mut() {
            if *left == 0 {
                return Err(kumi_buxx::Error::Link("unplugged".into()));
            }
            *left -= 1;
        }
        FrameLink::send(self, packet).map_err(kumi_buxx::Error::Link)?;
        FrameLink::receive(self).map_err(kumi_buxx::Error::Link)
    }
}

fn quiet() -> impl FnMut(u64, u64, &str) {
    |_, _, _| {}
}

fn temp(name: &str) -> std::string::String {
    let dir = std::env::temp_dir().join(format!("kumi-contract-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir.join("backup.bin").to_string_lossy().into_owned()
}

fn bridge_replies() -> Value {
    let key = sim::demo_public_hex();
    let package = sim::demo_package([0, 1, 0], 1);
    let port = Port(Rc::new(RefCell::new(Sim::new())), Rc::new(RefCell::new(None)));
    port.0.borrow_mut().set_mode(Mode::Service);
    let identify = api::identify(port.clone());
    let path = temp("replies");
    let backup = api::backup(port.clone(), &path, &mut quiet());
    let sha = backup["ok"]["sha256"].as_str().unwrap().to_string();

    *port.1.borrow_mut() = Some(40);
    let install_failed =
        api::install(port.clone(), &path, &sha, &package, &key, &GateEvidence::for_simulation(), 0, &mut quiet());
    *port.1.borrow_mut() = None;
    let resume = install_failed["err"]["resume"].as_u64().unwrap() as usize;
    let installed =
        api::install(port.clone(), &path, &sha, &package, &key, &GateEvidence::for_simulation(), resume, &mut quiet());
    let rebooted = api::reboot_from_service(port.clone());

    let mut wire = WireState::default();
    let call_ok = api::call(port.clone(), &mut wire, "system/confirm", "null");
    let call_with_topics = api::call(port.clone(), &mut wire, "events/subscribe", r#"{"keys":true,"log":true}"#);
    let call_failed = api::call(port.clone(), &mut wire, "no/such", "null");
    let poll_topics = api::poll_topics(port.clone(), &mut wire, 4);
    let update = api::update(port.clone(), &mut wire, &sim::demo_package([0, 2, 0], 2), &key, &mut quiet());
    let update_refused = api::update(port.clone(), &mut wire, &sim::demo_package([0, 3, 0], 3), &key, &mut quiet());

    let profile = r#"{"preamp_centi_db":-600,"bands":[{"kind":"Peak","frequency_hz":1000,"q_milli":2000,"gain_centi_db":600}]}"#;
    json!({
        "info": api::info(),
        "install_gates_missing": api::install_gates(
            &kumi_install::GateEvidence::new()
                .with(kumi_install::Gate::G3Emulator, kumi_install::gates::G3_EMULATOR_EVIDENCE)
                .with(kumi_install::Gate::G4ResetRetention, kumi_install::gates::G4_HARDWARE_EVIDENCE),
        ),
        "install_gates_recorded": api::install_gates(&kumi_install::GateEvidence::recorded()),
        "install_gates_complete": api::install_gates(&kumi_install::GateEvidence::for_simulation()),
        "open_package": api::open_package(&package, &key),
        "open_package_failed": api::open_package(&package[..100], &key),
        "identify": identify,
        "backup": backup,
        "install_failed": install_failed,
        "install": installed,
        "reboot_from_service": rebooted,
        "call_ok": call_ok,
        "call_with_topics": call_with_topics,
        "call_failed": call_failed,
        "poll_topics": poll_topics,
        "update": update,
        "update_refused": update_refused,
        "eq_response": api::eq_response(profile, 48_000, 5, 20.0, 20_000.0),
        "eq_auto_preamp": api::eq_auto_preamp(profile, 48_000),
        "eq_invalid_profile": api::eq_auto_preamp(r#"{"preamp_centi_db":100,"bands":[]}"#, 48_000),
    })
}

fn keys() -> Value {
    let mut endpoints = Map::new();
    for (path, request, response) in ENDPOINT_LIST.endpoints {
        endpoints.insert(path.to_string(), json!({ "request": hex(*request), "response": hex(*response) }));
    }
    let mut topics = Map::new();
    for (path, key) in TOPICS_OUT_LIST.topics {
        topics.insert(path.to_string(), json!(hex(*key)));
    }
    json!({ "endpoints": endpoints, "topics": topics, "error": hex(postcard_rpc::standard_icd::ERROR_KEY) })
}

fn limits() -> Value {
    json!({
        "protocol": PROTOCOL_VERSION,
        "eq_slots": EQ_SLOTS,
        "eq_max_bands": EQ_MAX_BANDS,
        "eq_min_hz": EqBand::MIN_HZ,
        "eq_max_hz": EqBand::MAX_HZ,
        "eq_min_q_milli": EqBand::MIN_Q_MILLI,
        "eq_max_q_milli": EqBand::MAX_Q_MILLI,
        "eq_max_gain_centi_db": EqBand::MAX_GAIN_CENTI_DB,
        "eq_min_preamp_centi_db": EqProfile::MIN_PREAMP_CENTI_DB,
        "output_min_volume_limit_centi_db": OutputSettings::MIN_VOLUME_LIMIT_CENTI_DB,
        "output_max_balance_centi_db": OutputSettings::MAX_BALANCE_CENTI_DB,
        "mic_max_gain_centi_db": MicSettings::MAX_GAIN_CENTI_DB,
        "layers": LAYERS,
        "keymap_chunk": KEYMAP_CHUNK,
        "macro_slots": MACRO_SLOTS,
        "macro_max_steps": MACRO_MAX_STEPS,
        "combo_slots": COMBO_SLOTS,
        "key_min_usage": KeyAction::MIN_KEY_USAGE,
        "key_max_usage": KeyAction::MAX_KEY_USAGE,
        "consumer_max_usage": KeyAction::MAX_CONSUMER_USAGE,
        "key_min_tapping_term_ms": KeyConfig::MIN_TAPPING_TERM_MS,
        "key_max_tapping_term_ms": KeyConfig::MAX_TAPPING_TERM_MS,
        "key_min_one_shot_ms": KeyConfig::MIN_ONE_SHOT_MS,
        "key_max_one_shot_ms": KeyConfig::MAX_ONE_SHOT_MS,
        "key_min_combo_term_ms": KeyConfig::MIN_COMBO_TERM_MS,
        "key_max_combo_term_ms": KeyConfig::MAX_COMBO_TERM_MS,
        "meter_floor_centi_db": METER_FLOOR_CENTI_DB,
        "test_tone_min_hz": TestToneRequest::MIN_HZ,
        "test_tone_max_hz": TestToneRequest::MAX_HZ,
        "test_tone_min_ms": TestToneRequest::MIN_MS,
        "test_tone_max_ms": TestToneRequest::MAX_MS,
        "max_unlock_seconds": MAX_UNLOCK_SECONDS,
        "word_block": WORD_BLOCK,
        "byte_block": BYTE_BLOCK,
        "i2c_max_address": I2cRequest::MAX_ADDRESS,
        "update_block": UPDATE_BLOCK,
        "crash_message_len": CRASH_MESSAGE_LEN,
        "log_line_len": LOG_LINE,
        "max_payload": MAX_PAYLOAD,
        "audio_min_level_centi_dbfs": AudioStart::MIN_LEVEL_CENTI_DBFS,
        "audio_min_tone_hz": AudioStart::MIN_TONE_HZ,
        "audio_max_tone_hz": AudioStart::MAX_TONE_HZ,
        "eq_response_max_points": api::MAX_RESPONSE_POINTS,
    })
}

fn samples() -> &'static Value {
    static SAMPLES: OnceLock<Value> = OnceLock::new();
    SAMPLES.get_or_init(build_samples)
}

fn build_samples() -> Value {
    let mut bands = Vec::new();
    bands.push(EqBand { kind: FilterKind::LowShelf, frequency_hz: 90, q_milli: 707, gain_centi_db: 450 }).unwrap();
    bands.push(EqBand { kind: FilterKind::HighPass, frequency_hz: 20, q_milli: 500, gain_centi_db: 0 }).unwrap();
    let profile = EqProfile { preamp_centi_db: -450, bands };
    let actions = [
        KeyAction::Transparent,
        KeyAction::Disabled,
        KeyAction::Key(4),
        KeyAction::Modifiers(0x22),
        KeyAction::KeyWithModifiers { usage: 5, modifiers: 1 },
        KeyAction::Consumer(0xE9),
        KeyAction::LayerHold(1),
        KeyAction::LayerToggle(3),
        KeyAction::TapHoldMod { tap: 0x29, hold_mods: 1 },
        KeyAction::TapHoldLayer { tap: 0x2C, hold_layer: 1 },
        KeyAction::OneShotMod(2),
        KeyAction::Macro(0),
        KeyAction::CaseControl(CaseHotkey::CycleEq),
        KeyAction::CapsWord,
        KeyAction::Mouse(MouseAction::LeftClick),
    ];
    let mut macro_steps = Vec::new();
    macro_steps.push(MacroStep { usage: 4, modifiers: 2 }).unwrap();
    macro_steps.push(MacroStep { usage: 5, modifiers: 0 }).unwrap();
    let macro_program = MacroProgram { steps: macro_steps };
    let mut entries = Vec::new();
    for (i, action) in actions.iter().enumerate() {
        entries.push(KeymapEntry { layer: 0, source: 4 + i as u8, action: *action }).unwrap();
    }
    let device_info = DeviceInfo {
        protocol: PROTOCOL_VERSION,
        firmware: Version { major: 0, minor: 1, patch: 2 },
        build: String::try_from("ci-17").unwrap(),
        chip_id: 0x4901_4480,
        flash_jedec: [0xC8, 0x65, 0x16],
        serial: Some(*b"SN000001"),
        running_from_ram: false,
        capabilities: Capabilities::AUDIO_OUT
            | Capabilities::PARAMETRIC_EQ
            | Capabilities::KEYMAP
            | Capabilities::FIRMWARE_UPDATE,
    };
    let crash = CrashReport {
        boot_count: 6,
        reason: CrashReason::Trap,
        stage: stage::FW_RUNNING,
        mcause: 2,
        mepc: 0x0000_1F40,
        mtval: 0xFFFF_FFFF,
        ra: 0x0000_1E00,
        sp: 0x0012_7F00,
        uptime_ms: 5_120,
        message: String::try_from("runtime fault").unwrap(),
    };
    let key_event = KeyEvent { source: 4, pressed: true, layer: 0, action: KeyAction::Key(4) };
    let log_line = LogLine {
        uptime_ms: 1234,
        level: LogLevel::Warn,
        module: String::try_from("usb").unwrap(),
        text: String::try_from("reset").unwrap(),
    };
    let tone = AudioStart { rate_hz: 48_000, channel: AudioChannel::Left, level_centi_dbfs: -2_000, tone_hz: 1_000 };
    let calibration = AudioCalibration {
        system_clock: 0x0000_0103,
        core_pll: 0x0001_2C04,
        decoded_cpu_hz: Some(144_000_000),
        frame_clock_hz: Some(1_000),
        timers_quiesced: true,
        fault: None,
        trims: [17, 22],
        windows: [3, 4],
        counts: Vec::from_slice(&[[48_000, 44_100], [48_010, 44_090]]).unwrap(),
        cycles: 14_400_000,
        pll: [0x11, 0x22, 0x33],
    };
    let running = AudioBench {
        state: AudioBenchState::Running,
        cpu_hz: Some(144_000_000),
        calibration: Some(calibration.clone()),
        stream: Some(AudioStream {
            start: tone,
            amplitude: 3_276,
            ring_address: 0x0011_0000,
            ring_bytes: 4_608,
            periods: 48,
            started_us: Some(1_250),
            dacs: 1,
            cursor: AudioCursorMap {
                remaining: 0x2003_0010,
                status: 0x2003_0014,
                enable: 0x2003_0018,
                i2s_status: 0x2004_0000,
                monitor: 0x0013_6000,
            },
        }),
        failure: None,
        halt: None,
    };
    let failed = AudioBench {
        state: AudioBenchState::Failed,
        cpu_hz: Some(144_000_000),
        calibration: Some(calibration),
        stream: None,
        failure: Some(AudioFailure {
            phase: AudioPhase::Dac,
            fault: AudioFault::Dac(AudioDacFault::Bus { address: 0x31, register: 0x0001_0001, error: AudioBusFault::Nack }),
        }),
        halt: Some(AudioHalt { cycles: 1_200, enable: 0, remaining: [0, 0] }),
    };
    json!({
        "poke": v(PokeRequest { address: 0x2011_0120, value: 0x0000_00A5, mask: 0x0000_00FF }),
        "count_result": v(CountResult::Ok(0x0000_01A5)),
        "i2c_request": v(I2cRequest { bus: 0, address: 0x33, write: Vec::from_slice(&[0x01]).unwrap(), read: 2 }),
        "i2c_reply": v(I2cResult::Ok(I2cReply { acked: true, data: Vec::from_slice(&[0x03, 0x41]).unwrap() })),
        "i2c_nack": v(I2cResult::Ok(I2cReply { acked: false, data: Vec::new() })),
        "audio_bench_requests": [
            v(AudioBenchRequest::Calibrate), v(AudioBenchRequest::Start(tone)), v(AudioBenchRequest::Stop), v(AudioBenchRequest::Status),
        ],
        "audio_bench_running": v(AudioBenchResult::Ok(running)),
        "audio_bench_failed": v(AudioBenchResult::Ok(failed)),
        "audio_bench_states": [
            v(AudioBenchState::Idle), v(AudioBenchState::Starting), v(AudioBenchState::Running), v(AudioBenchState::Failed),
        ],
        "audio_channels": [v(AudioChannel::Left), v(AudioChannel::Right), v(AudioChannel::Both)],
        "audio_phases": [
            v(AudioPhase::Calibrate), v(AudioPhase::Setup), v(AudioPhase::Clock), v(AudioPhase::ResetLow), v(AudioPhase::ResetHigh),
            v(AudioPhase::Dac), v(AudioPhase::Dma), v(AudioPhase::Unmute), v(AudioPhase::Stop),
        ],
        "device_info": v(&device_info),
        "device_info_unset_serial": v(DeviceInfo { serial: None, running_from_ram: true, capabilities: Capabilities::KEYMAP | Capabilities::LAYERS, ..device_info.clone() }),
        "audio_status": v(AudioStatus {
            jack: Jack::Headset,
            usb_audio_present: true,
            stream: Some(StreamFormat { kind: StreamKind::Dop, rate_hz: 176_400, bits: 24, channels: 2 }),
            host_volume_centi_db: [-600, -650],
            host_muted: false,
            eq_active: Some(1),
            counters: AudioCounters { underruns: 1, overruns: 0, feedback_corrections: 12, dac_faults: 0 },
        }),
        "audio_status_idle": v(AudioStatus {
            jack: Jack::Empty,
            usb_audio_present: false,
            stream: None,
            host_volume_centi_db: [0, 0],
            host_muted: true,
            eq_active: None,
            counters: AudioCounters::default(),
        }),
        "output": v(OutputSettings { filter: DacFilter::SlowPhaseCompensated, volume_limit_centi_db: -300, balance_centi_db: 50, dsd_direct: true }),
        "dsp": v(DspSettings {
            crossfeed: CrossfeedMode::Natural,
            limiter: true,
            mono: false,
            invert_phase: false,
            loudness: true,
            swap_channels: false,
        }),
        "crossfeed_modes": [v(CrossfeedMode::Off), v(CrossfeedMode::Subtle), v(CrossfeedMode::Natural), v(CrossfeedMode::Strong)],
        "dac_extras": v(DacExtras {
            high_pass_filter: true,
            de_emphasis: false,
            line_out: false,
            class_h: ClassHMode::LowPowerLv,
            auto_uac_on_jack: true,
            stay_awake_while_active: true,
        }),
        "class_h_modes": [v(ClassHMode::AdaptiveHv), v(ClassHMode::LowPowerLv), v(ClassHMode::FixedHv)],
        "impedance_report": v(ImpedanceResult::Ok(ImpedanceReport {
            left_ohms: Some(49),
            right_ohms: Some(49),
            left_ac_ohms: Some(53),
            right_ac_ohms: Some(53),
            left_status: 0x0d,
            right_status: 0x0d,
            hp_status: 0x02,
            load_stat: 0x00,
        })),
        "audio_meters": v(AudioMeters {
            active: true,
            peak_left_centi_db: -620,
            peak_right_centi_db: -640,
            rms_left_centi_db: -1_250,
            rms_right_centi_db: -1_280,
            clipped_samples: 0,
            limiter_hits: 3,
        }),
        "test_tone_request": v(TestToneRequest {
            channel: AudioChannel::Both,
            frequency_hz: 1_000,
            duration_ms: 1_500,
            invert_right: true,
        }),
        "system_telemetry": v(TelemetryResult::Ok(SystemTelemetry {
            cpu_mhz: 90,
            dcdc_active: true,
            dvdd_mv: 1_400,
            analog_rails_on: false,
            audio_pll_enabled: false,
            wfi_percent_tenths: 998,
            wfi_wakeups: 420,
            jack_adc: 202,
            uptime_ms: 8_400,
        })),
        "key_config": v(KeyConfig { tapping_term_ms: 200, one_shot_timeout_ms: 3_000, combo_term_ms: 50 }),
        "macro_slot_update": v(MacroSlotUpdate { slot: 0, program: macro_program.clone() }),
        "macro_result_ok": v(MacroResult::Ok(Some(macro_program))),
        "macro_result_empty": v(MacroResult::Ok(None)),
        "combo_slot_update": v(ComboSlotUpdate {
            slot: 0,
            combo: Some(KeyCombo { key_a: 0x0d, key_b: 0x0e, action: KeyAction::Key(0x29) }),
        }),
        "combo_result_ok": v(ComboResult::Ok(Some(KeyCombo { key_a: 0x0d, key_b: 0x0e, action: KeyAction::Key(0x29) }))),
        "combo_result_empty": v(ComboResult::Ok(None)),
        "mouse_actions": [
            v(MouseAction::MoveUp),
            v(MouseAction::MoveDown),
            v(MouseAction::MoveLeft),
            v(MouseAction::MoveRight),
            v(MouseAction::LeftClick),
            v(MouseAction::RightClick),
            v(MouseAction::MiddleClick),
            v(MouseAction::WheelUp),
            v(MouseAction::WheelDown),
        ],
        "case_hotkeys": [
            v(CaseHotkey::CycleEq),
            v(CaseHotkey::CycleFilter),
            v(CaseHotkey::CycleCrossfeed),
            v(CaseHotkey::ToggleLimiter),
            v(CaseHotkey::ToggleMono),
            v(CaseHotkey::ToggleMute),
            v(CaseHotkey::ToggleLineOut),
            v(CaseHotkey::ToggleLoudness),
            v(CaseHotkey::ToggleCapsWord),
        ],
        "eq_profile": v(&profile),
        "eq_update": v(EqSlotUpdate { slot: 2, profile: profile.clone() }),
        "eq_result_ok": v(EqResult::Ok(Some(profile))),
        "eq_result_empty": v(EqResult::Ok(None)),
        "eq_result_err": v(EqResult::Err(CaseError::InvalidArgument)),
        "ack_ok": v(Ack::Ok(())),
        "ack_err": v(Ack::Err(CaseError::NotUnlocked)),
        "ack_err_unknown_code": v(Ack::Err(CaseError::from_code(42))),
        "eq_selection_none": v(EqSelection::None),
        "eq_selection_some": v(Some(3u8)),
        "mic": v(MicSettings { gain_centi_db: 1_200 }),
        "keymap_page": v(KeymapPage { layer: 1, start: 0 }),
        "keymap_chunk": v(KeymapChunk { entries, more: true }),
        "key_event": v(key_event),
        "keyboard_status": v(KeyboardStatus {
            link_up: true,
            frames: 10,
            bad_frames: 1,
            controller_commands: 2,
            active_layer: 0,
            caps_word: true,
            host_leds: 0x02,
        }),
        "slots": v(SlotTable {
            booted: Some(Slot::A),
            slots: [
                SlotInfo { slot: Slot::A, state: SlotState::Confirmed, version: Some(Version { major: 0, minor: 1, patch: 2 }), size: 40_000 },
                SlotInfo { slot: Slot::B, state: SlotState::Empty, version: None, size: 0 },
            ],
        }),
        "diagnostics": v(DiagnosticsResult::Ok(Diagnostics {
            boot_count: 7,
            safe_mode: true,
            stage: stage::FW_SAFE_MODE,
            uptime_ms: 830,
            dropped_events: 3,
            watchdog_armed: false,
            last_crash: Some(crash),
        })),
        "diagnostics_clean": v(DiagnosticsResult::Ok(Diagnostics {
            boot_count: 1,
            safe_mode: false,
            stage: stage::FW_RUNNING,
            uptime_ms: 0,
            dropped_events: 0,
            watchdog_armed: false,
            last_crash: None,
        })),
        "event_mask": v(EventMask::KEYS | EventMask::LOG),
        "events_result": v(EventsResult::Ok(EventMask::KEYS)),
        "topic_messages": v([
            Event::Key(key_event),
            Event::Log(log_line.clone()),
            Event::Uart(ByteBlock { bytes: Vec::from_slice(&[0x4B, 0x01]).unwrap() }),
        ]),
        "reboot_targets": [v(RebootTarget::Normal), v(RebootTarget::OtherSlot), v(RebootTarget::BootRom)],
        "settings_scopes": [v(SettingsScope::All), v(SettingsScope::Audio), v(SettingsScope::Keymap)],
        "dac_filters": [
            v(DacFilter::FastLowLatency), v(DacFilter::FastPhaseCompensated), v(DacFilter::SlowLowLatency),
            v(DacFilter::SlowPhaseCompensated), v(DacFilter::NonOversampling),
        ],
        "case_errors": CaseError::KNOWN.iter().map(v).collect::<std::vec::Vec<_>>(),
        "jacks": [v(Jack::Empty), v(Jack::Headphones), v(Jack::Headset), v(Jack::Unknown)],
        "stream_kinds": [v(StreamKind::Pcm), v(StreamKind::Dop), v(StreamKind::NativeDsd)],
        "filter_kinds": [v(FilterKind::Peak), v(FilterKind::LowShelf), v(FilterKind::HighShelf), v(FilterKind::LowPass), v(FilterKind::HighPass)],
        "slot_names": [v(Slot::A), v(Slot::B)],
        "slot_states": [v(SlotState::Empty), v(SlotState::Trial), v(SlotState::Confirmed), v(SlotState::Invalid)],
        "log_levels": [v(LogLevel::Trace), v(LogLevel::Debug), v(LogLevel::Info), v(LogLevel::Warn), v(LogLevel::Error)],
        "crash_reasons": [v(CrashReason::Panic), v(CrashReason::Trap)],
        "capability_names": Capabilities::NAMES.iter().map(|(name, _)| *name).collect::<std::vec::Vec<_>>(),
        "event_names": EventMask::NAMES.iter().map(|(name, _)| *name).collect::<std::vec::Vec<_>>(),
        "stages": stage::NAMES.iter().map(|(code, name)| json!({ "code": code, "name": name })).collect::<std::vec::Vec<_>>(),
        "log_line": v(log_line),
        "peek": v(PeekRequest { address: 0x0010_2000, words: 4 }),
        "unlock": v(DevUnlockRequest { seconds: 300 }),
        "limits": limits(),
        "endpoints": api::endpoint_paths(),
        "keys": keys(),
        "topic_paths": [AudioStatusTopic::PATH, KeyEventTopic::PATH, LogTopic::PATH, UartMirrorTopic::PATH],
        "bridge": api::info(),
        "bridge_replies": bridge_replies(),
    })
}

#[test]
fn the_json_contract_matches_the_committed_fixture() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/contract.json");
    let current = serde_json::to_string_pretty(samples()).unwrap() + "\n";
    if std::env::var_os("KUMI_BLESS").is_some() {
        std::fs::write(path, &current).unwrap();
    }
    let committed = std::fs::read_to_string(path).unwrap_or_default();
    assert!(committed == current, "the JSON the bridge emits changed; rerun with KUMI_BLESS=1 and update the app models");
}

#[test]
fn the_fixture_json_parses_back_into_the_protocol_types() {
    let fixture = samples();
    let parse = |name: &str| fixture[name].clone();
    assert_eq!(serde_json::from_value::<DeviceInfo>(parse("device_info")).unwrap().serial, Some(*b"SN000001"));
    assert_eq!(serde_json::from_value::<DeviceInfo>(parse("device_info_unset_serial")).unwrap().serial, None);
    assert_eq!(serde_json::from_value::<EqResult>(parse("eq_result_empty")).unwrap(), Ok(None));
    assert_eq!(serde_json::from_value::<Ack>(parse("ack_err_unknown_code")).unwrap(), Err(CaseError::Unknown(42)));
    assert!(serde_json::from_value::<DiagnosticsResult>(parse("diagnostics")).unwrap().unwrap().last_crash.is_some());
    assert_eq!(serde_json::from_value::<EventMask>(parse("event_mask")).unwrap(), EventMask::KEYS | EventMask::LOG);
    assert_eq!(serde_json::from_value::<std::vec::Vec<Event>>(parse("topic_messages")).unwrap().len(), 3);
    let replies = &fixture["bridge_replies"];
    for name in
        ["identify", "backup", "install", "call_ok", "call_with_topics", "poll_topics", "update", "eq_response", "eq_auto_preamp"]
    {
        assert!(replies[name].get("ok").is_some(), "{name}: {}", replies[name]);
    }
    for name in ["open_package_failed", "install_failed", "call_failed", "update_refused", "eq_invalid_profile"] {
        assert!(replies[name]["err"]["kind"].is_string(), "{name}: {}", replies[name]);
    }
    assert!(replies["install_failed"]["err"]["boots"].is_array());
    assert_eq!(replies["call_with_topics"]["topics"][0]["path"], "log");
    assert_eq!(serde_json::from_value::<SlotInfo>(replies["update"]["ok"].clone()).unwrap().state, SlotState::Trial);
    assert!(!serde_json::from_value::<I2cResult>(parse("i2c_nack")).unwrap().unwrap().acked);
    let bench = serde_json::from_value::<AudioBenchResult>(parse("audio_bench_failed")).unwrap().unwrap();
    assert_eq!(bench.failure.unwrap().phase, AudioPhase::Dac);
    let requests = serde_json::from_value::<std::vec::Vec<AudioBenchRequest>>(parse("audio_bench_requests")).unwrap();
    assert!(requests.iter().all(AudioBenchRequest::is_valid));
}

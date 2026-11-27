// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::HashSet;
use std::fmt::Debug;

use heapless::{String, Vec};
use kumi_icd::*;
use postcard_rpc::{Endpoint, Key2};
use serde::Serialize;
use serde::de::DeserializeOwned;

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + Debug>(value: &T) -> usize {
    let bytes = postcard::to_stdvec(value).unwrap();
    let back: T = postcard::from_bytes(&bytes).unwrap();
    assert_eq!(&back, value);
    bytes.len()
}

fn string<const N: usize>(fill: char) -> String<N> {
    let mut text = String::new();
    while text.push(fill).is_ok() {}
    text
}

fn full_bench() -> AudioBench {
    let mut counts = Vec::new();
    while counts.push([u32::MAX; 2]).is_ok() {}
    let start = AudioStart { rate_hz: u32::MAX, channel: AudioChannel::Both, level_centi_dbfs: i16::MIN, tone_hz: u32::MAX };
    AudioBench {
        state: AudioBenchState::Failed,
        cpu_hz: Some(u32::MAX),
        calibration: Some(AudioCalibration {
            system_clock: u32::MAX,
            core_pll: u32::MAX,
            decoded_cpu_hz: Some(u32::MAX),
            frame_clock_hz: Some(u32::MAX),
            timers_quiesced: true,
            fault: Some(AudioClockFault::InvalidReference { trim: u8::MAX, reference: u32::MAX }),
            trims: [u8::MAX; 2],
            windows: [u16::MAX; 2],
            counts,
            cycles: u32::MAX,
            pll: [u32::MAX; 3],
        }),
        stream: Some(AudioStream {
            start,
            amplitude: i32::MIN,
            ring_address: u32::MAX,
            ring_bytes: u32::MAX,
            periods: u32::MAX,
            started_us: Some(u32::MAX),
            dacs: u8::MAX,
            cursor: AudioCursorMap {
                remaining: u32::MAX,
                status: u32::MAX,
                enable: u32::MAX,
                i2s_status: u32::MAX,
                monitor: u32::MAX,
            },
        }),
        failure: Some(AudioFailure {
            phase: AudioPhase::Stop,
            fault: AudioFault::Dac(AudioDacFault::Readback {
                address: u8::MAX,
                register: u32::MAX,
                expected: u8::MAX,
                actual: u8::MAX,
            }),
        }),
        halt: Some(AudioHalt { cycles: u32::MAX, enable: u32::MAX, remaining: [u32::MAX; 2] }),
    }
}

fn full_eq() -> EqProfile {
    let mut bands = Vec::new();
    while bands.push(EqBand { kind: FilterKind::HighShelf, frequency_hz: 40_000, q_milli: 20_000, gain_centi_db: -1_800 }).is_ok()
    {
    }
    EqProfile { preamp_centi_db: -2_400, bands }
}

#[test]
fn endpoint_and_topic_keys_are_unique_even_when_shortened() {
    let mut paths = HashSet::new();
    let mut short = HashSet::new();
    for (path, request, response) in ENDPOINT_LIST.endpoints {
        assert!(paths.insert(*path), "duplicate path {path}");
        assert!(short.insert(Key2::from_key8(*request).to_bytes()), "short key collision at {path}");
        assert!(request.to_bytes() != response.to_bytes() || path.starts_with("postcard-rpc"), "{path}");
    }
    for (path, key) in TOPICS_OUT_LIST.topics.iter().chain(TOPICS_IN_LIST.topics) {
        assert!(paths.insert(*path), "duplicate path {path}");
        assert!(short.insert(Key2::from_key8(*key).to_bytes()), "short key collision at {path}");
    }
}

#[test]
fn every_user_endpoint_is_listed() {
    let paths: HashSet<&str> = ENDPOINT_LIST.endpoints.iter().map(|(path, _, _)| *path).collect();
    for path in [
        "device/info",
        "system/diagnostics",
        "events/subscribe",
        "audio/status",
        "audio/output/get",
        "audio/output/set",
        "audio/dsp/get",
        "audio/dsp/set",
        "audio/eq/get",
        "audio/eq/set",
        "audio/eq/select",
        "audio/mic/get",
        "audio/mic/set",
        "keys/map/get",
        "keys/map/set",
        "keys/map/clear",
        "keys/macro/get",
        "keys/macro/set",
        "keys/combo/get",
        "keys/combo/set",
        "keys/config/get",
        "keys/config/set",
        "keys/status",
        "settings/save",
        "settings/reset",
        "system/slots",
        "system/reboot",
        "system/confirm",
        "update/begin",
        "update/chunk",
        "update/finish",
        "dev/unlock",
        "dev/peek",
        "dev/poke",
        "dev/i2c",
        "dev/uart/send",
        "dev/fault",
        "dev/flash/test",
        "dev/audio",
    ] {
        assert!(paths.contains(path), "missing {path}");
    }
}

#[test]
fn messages_round_trip() {
    round_trip(&DeviceInfo {
        protocol: PROTOCOL_VERSION,
        firmware: Version { major: 0, minor: 1, patch: 0 },
        build: String::try_from("kumi-dev").unwrap(),
        chip_id: 0x4901_4480,
        flash_jedec: [0xC8, 0x65, 0x16],
        serial: Some([1, 2, 3, 4, 5, 6, 7, 8]),
        running_from_ram: true,
        capabilities: Capabilities::AUDIO_OUT | Capabilities::KEYMAP | Capabilities::DEVELOPER,
    });
    round_trip(&AudioStatus {
        jack: Jack::Headset,
        usb_audio_present: true,
        stream: Some(StreamFormat { kind: StreamKind::Dop, rate_hz: 176_400, bits: 24, channels: 2 }),
        host_volume_centi_db: [-1_250, -1_300],
        host_muted: false,
        eq_active: Some(2),
        counters: AudioCounters { underruns: 3, ..AudioCounters::default() },
    });
    round_trip(&EqSlotUpdate { slot: 3, profile: full_eq() });
    round_trip(&DspSettings {
        crossfeed: CrossfeedMode::Natural,
        limiter: true,
        mono: false,
        invert_phase: true,
        loudness: true,
        swap_channels: false,
    });
    round_trip(&ComboSlotUpdate { slot: 0, combo: Some(KeyCombo { key_a: 0x0d, key_b: 0x0e, action: KeyAction::CapsWord }) });
    round_trip::<EqResult>(&Err(CaseError::InvalidArgument));
    round_trip::<EqResult>(&Ok(None));
    round_trip::<EqResult>(&Ok(Some(full_eq())));
    round_trip::<EventsResult>(&Ok(EventMask::KEYS | EventMask::LOG));
    round_trip::<Ack>(&Ok(()));
    round_trip(&KeymapEntry { layer: 1, source: 0x39, action: KeyAction::KeyWithModifiers { usage: 0x29, modifiers: 0x01 } });
    round_trip(&KeyEvent { source: 0x04, pressed: true, layer: 0, action: KeyAction::LayerHold(1) });
    round_trip(&SlotTable {
        booted: Some(Slot::A),
        slots: [
            SlotInfo {
                slot: Slot::A,
                state: SlotState::Confirmed,
                version: Some(Version { major: 1, minor: 2, patch: 3 }),
                size: 98_304,
            },
            SlotInfo { slot: Slot::B, state: SlotState::Empty, version: None, size: 0 },
        ],
    });
    round_trip(&UpdateManifest {
        slot: Slot::B,
        version: Version { major: 1, minor: 0, patch: 0 },
        size: 120_000,
        sha256: [7; 32],
        signature: [[1; 32], [2; 32]],
    });
    round_trip(&AudioBenchRequest::Start(AudioStart {
        rate_hz: 44_100,
        channel: AudioChannel::Left,
        level_centi_dbfs: -2_000,
        tone_hz: 1_000,
    }));
    round_trip::<AudioBenchResult>(&Ok(full_bench()));
    round_trip::<AudioBenchResult>(&Err(CaseError::NotUnlocked));
    round_trip(&AudioFailure {
        phase: AudioPhase::Setup,
        fault: AudioFault::Setup { address: 0x2007_0000, expected: 1, actual: 0 },
    });
    round_trip(&AudioFault::Dac(AudioDacFault::NegativeControl(AudioBusFault::Timeout)));
    round_trip(&AudioFault::Clock(AudioClockFault::DisjointTrimWindows));
    round_trip(&LogLine {
        uptime_ms: 12_345,
        level: LogLevel::Warn,
        module: String::try_from("usb").unwrap(),
        text: String::try_from("reset").unwrap(),
    });
}

#[test]
fn largest_messages_fit_one_frame() {
    let mut words = Vec::new();
    while words.push(u32::MAX).is_ok() {}
    let mut bytes = Vec::new();
    while bytes.push(0xFF).is_ok() {}
    let mut update = Vec::new();
    while update.push(0xAA).is_ok() {}
    let mut chunk = Vec::new();
    while chunk
        .push(KeymapEntry { layer: 3, source: 0xFF, action: KeyAction::KeyWithModifiers { usage: 0xFF, modifiers: 0xFF } })
        .is_ok()
    {}
    let sizes = [
        round_trip(&EqSlotUpdate { slot: 3, profile: full_eq() }),
        round_trip(&WordBlock { address: u32::MAX, words }),
        round_trip(&I2cRequest { bus: 1, address: 0x7F, write: bytes.clone(), read: 64 }),
        round_trip(&I2cReply { acked: true, data: bytes }),
        round_trip(&UpdateBlock { offset: u32::MAX, data: update }),
        round_trip(&KeymapChunk { entries: chunk, more: true }),
        round_trip::<AudioBenchResult>(&Ok(full_bench())),
        round_trip(&LogLine { uptime_ms: u32::MAX, level: LogLevel::Error, module: string('m'), text: string('x') }),
        round_trip(&DeviceInfo {
            protocol: u16::MAX,
            firmware: Version { major: 255, minor: 255, patch: 255 },
            build: string('b'),
            chip_id: u32::MAX,
            flash_jedec: [0xFF; 3],
            serial: Some([0xFF; 8]),
            running_from_ram: true,
            capabilities: Capabilities::from_bits(u32::MAX),
        }),
        round_trip(&Diagnostics {
            boot_count: u32::MAX,
            safe_mode: true,
            stage: u16::MAX,
            uptime_ms: u32::MAX,
            dropped_events: u32::MAX,
            watchdog_armed: true,
            last_crash: Some(CrashReport {
                boot_count: u32::MAX,
                reason: CrashReason::Trap,
                stage: u16::MAX,
                mcause: u32::MAX,
                mepc: u32::MAX,
                mtval: u32::MAX,
                ra: u32::MAX,
                sp: u32::MAX,
                uptime_ms: u32::MAX,
                message: string('x'),
            }),
        }),
    ];
    for size in sizes {
        assert!(size <= MAX_PAYLOAD, "message of {size} bytes exceeds the payload limit");
        assert!(size + frame::MAX_HEADER <= frame::MAX_FRAME);
    }
}

#[test]
fn wire_format_is_stable() {
    let band = EqBand { kind: FilterKind::Peak, frequency_hz: 1_000, q_milli: 707, gain_centi_db: -300 };
    assert_eq!(postcard::to_stdvec(&band).unwrap(), [0x00, 0xE8, 0x07, 0xC3, 0x05, 0xD7, 0x04]);
    assert_eq!(postcard::to_stdvec(&RebootTarget::BootRom).unwrap(), [0x02]);
    assert_eq!(postcard::to_stdvec(&Ack::Err(CaseError::Busy)).unwrap(), [0x01, 0x02]);
    assert_eq!(postcard::to_stdvec(&Ack::Err(CaseError::WrongState)).unwrap(), [0x01, 0x07]);
    assert_eq!(postcard::to_stdvec(&Ack::Err(CaseError::Unknown(300))).unwrap(), [0x01, 0xAC, 0x02]);
    assert_eq!(postcard::to_stdvec(&(Capabilities::KEYMAP | Capabilities::LAYERS)).unwrap(), [0x30]);
    assert_eq!(postcard::to_stdvec(&(Capabilities::FIRMWARE_UPDATE | Capabilities::KEYMAP)).unwrap(), [0x90, 0x02]);
    assert_eq!(postcard::to_stdvec(&(EventMask::KEYS | EventMask::UART)).unwrap(), [0x09]);
    assert_eq!(postcard::to_stdvec(&Some([1u8; 8])).unwrap(), [0x01, 1, 1, 1, 1, 1, 1, 1, 1]);
    assert_eq!(postcard::to_stdvec(&SlotState::Invalid).unwrap(), [0x03]);
    let tone = AudioStart { rate_hz: 48_000, channel: AudioChannel::Right, level_centi_dbfs: -2_000, tone_hz: 1_000 };
    assert_eq!(
        postcard::to_stdvec(&AudioBenchRequest::Start(tone)).unwrap(),
        [0x01, 0x80, 0xF7, 0x02, 0x01, 0x9F, 0x1F, 0xE8, 0x07]
    );
    assert_eq!(postcard::to_stdvec(&AudioBenchRequest::Status).unwrap(), [0x03]);
}

#[test]
fn growing_errors_or_flags_does_not_change_endpoint_keys() {
    use postcard_schema::Schema;
    use postcard_schema::schema::DataModelType;
    for named in [CaseError::SCHEMA, Capabilities::SCHEMA, EventMask::SCHEMA] {
        match named.ty {
            DataModelType::NewtypeStruct(inner) => assert!(matches!(inner.ty, DataModelType::U16 | DataModelType::U32)),
            other => panic!("{} is not an integer on the wire: {other:?}", named.name),
        }
    }
}

#[test]
fn unknown_error_codes_and_flags_survive_a_round_trip() {
    for code in 0..400u16 {
        let error = CaseError::from_code(code);
        assert_eq!(error.code(), code);
        let back: CaseError = postcard::from_bytes(&postcard::to_stdvec(&error).unwrap()).unwrap();
        assert_eq!(back, error);
        let json: CaseError = serde_json::from_value(serde_json::to_value(error).unwrap()).unwrap();
        assert_eq!(json, error);
    }
    assert_eq!(CaseError::from_code(2), CaseError::Busy);
    assert_eq!(CaseError::from_code(8), CaseError::PresenceRequired);
    assert_eq!(CaseError::from_code(9), CaseError::Unknown(9));
    assert_eq!(serde_json::to_value(CaseError::NotUnlocked).unwrap(), serde_json::json!("NotUnlocked"));
    assert_eq!(serde_json::to_value(CaseError::Unknown(42)).unwrap(), serde_json::json!(42));
    assert_eq!(CaseError::KNOWN.iter().map(|e| e.name().unwrap()).collect::<std::vec::Vec<_>>(), CaseError::NAMES);

    let future = Capabilities::from_bits(Capabilities::KEYMAP.bits() | 1 << 20);
    let back: Capabilities = postcard::from_bytes(&postcard::to_stdvec(&future).unwrap()).unwrap();
    assert_eq!(back, future);
    let json = serde_json::to_value(future).unwrap();
    assert_eq!(json["keymap"], true);
    assert_eq!(json["dsd"], false);
    assert_eq!(json["other_bits"], 1 << 20);
    assert_eq!(serde_json::from_value::<Capabilities>(json).unwrap(), future);
    assert_eq!(serde_json::to_value(Capabilities::DSD).unwrap().as_object().unwrap().len(), Capabilities::NAMES.len());
    assert_eq!(serde_json::from_value::<EventMask>(serde_json::json!({"keys": true, "future": true})).unwrap(), EventMask::KEYS);
    assert_eq!(serde_json::from_value::<EventMask>(serde_json::json!(3)).unwrap(), EventMask::KEYS | EventMask::LOG);
    assert!(serde_json::from_value::<EventMask>(serde_json::json!({"keys": 1})).is_err());
    assert_eq!(format!("{:?}", EventMask::KEYS | EventMask::from_bits(1 << 9)), "EventMask(keys | 0x200)");
}

#[test]
fn frames_carry_a_header_and_split_into_packets() {
    let mut out = [0u8; 64];
    let used = frame::encode(GetMic::REQ_KEY, postcard_rpc::header::VarSeq::Seq2(0x1234), &(), &mut out).unwrap();
    assert_eq!(used, frame::header_len(&postcard_rpc::header::VarSeq::Seq2(0)));
    let (header, body) = frame::decode(&out[..used]).unwrap();
    assert_eq!(header.key, postcard_rpc::header::VarKey::Key8(GetMic::REQ_KEY));
    assert!(body.is_empty());
    assert!(frame::encode(GetMic::REQ_KEY, frame::TOPIC_SEQUENCE, &[u32::MAX; 16], &mut out).is_none());

    let sizes = |len: usize, max: usize| frame::packets(&vec![7u8; len], max).map(<[u8]>::len).collect::<std::vec::Vec<_>>();
    assert_eq!(sizes(0, frame::FULL_SPEED_PACKET), [0]);
    assert_eq!(sizes(10, frame::FULL_SPEED_PACKET), [10]);
    assert_eq!(sizes(64, frame::FULL_SPEED_PACKET), [64, 0]);
    assert_eq!(sizes(130, frame::FULL_SPEED_PACKET), [64, 64, 2]);
    assert_eq!(sizes(130, frame::HIGH_SPEED_BULK_PACKET), [130]);
    assert_eq!(sizes(512, frame::HIGH_SPEED_BULK_PACKET), [512, 0]);
    assert_eq!(sizes(frame::MAX_FRAME, frame::HIGH_SPEED_BULK_PACKET), [512, frame::MAX_FRAME - 512]);

    for max_packet in [frame::FULL_SPEED_PACKET, frame::HIGH_SPEED_BULK_PACKET] {
        let mut reassembler = frame::Reassembler::<1024>::new(max_packet);
        let message: std::vec::Vec<u8> = (0..1024u32).map(|i| i as u8).collect();
        let mut done = std::vec::Vec::new();
        for packet in frame::packets(&message, max_packet) {
            if let Some(frame) = reassembler.push(packet) {
                done.push(frame.to_vec());
            }
        }
        assert_eq!(done, std::slice::from_ref(&message));
        for packet in frame::packets(&[0u8; 1536], max_packet) {
            assert!(reassembler.push(packet).is_none(), "an oversized frame is dropped");
        }
        assert!(reassembler.push(&[]).is_none(), "an empty transfer is not a frame");
        assert_eq!(reassembler.push(&[1, 2, 3]), Some(&[1u8, 2, 3][..]));
        assert!(reassembler.push(&vec![0u8; max_packet + 1]).is_none(), "a packet larger than the endpoint is rejected");
    }
}

#[test]
fn every_stage_has_a_distinct_name() {
    for (code, name) in stage::NAMES {
        assert_eq!(stage::name(code), Some(name));
        assert_eq!(stage::NAMES.iter().filter(|(other, _)| *other == code).count(), 1);
    }
    assert_eq!(stage::name(0x7777), None);
}

#[test]
fn validation_rules() {
    let good = EqBand { kind: FilterKind::Peak, frequency_hz: 31, q_milli: 1_000, gain_centi_db: 600 };
    assert!(good.is_valid());
    assert!(!EqBand { frequency_hz: 5, ..good }.is_valid());
    assert!(!EqBand { q_milli: 50, ..good }.is_valid());
    assert!(!EqBand { gain_centi_db: 1_900, ..good }.is_valid());
    assert!(!EqBand { gain_centi_db: -1_900, ..good }.is_valid());
    assert!(full_eq().is_valid());
    assert!(!EqProfile { preamp_centi_db: 100, bands: Vec::new() }.is_valid());
    assert!(PeekRequest { address: 0x0010_0000, words: 32 }.is_valid());
    assert!(!PeekRequest { address: 0x0010_0002, words: 1 }.is_valid());
    assert!(!PeekRequest { address: 0x0010_0000, words: 0 }.is_valid());
    assert!(!PeekRequest { address: u32::MAX - 3, words: 2 }.is_valid());
    assert!(DevUnlockRequest { seconds: 600 }.is_valid());
    assert!(!DevUnlockRequest { seconds: 601 }.is_valid());
    assert!(PokeRequest { address: 0x0010_0004, value: 1, mask: 1 }.is_valid());
    assert!(!PokeRequest { address: 0x0010_0002, value: 1, mask: 1 }.is_valid());
    let i2c = I2cRequest { bus: 0, address: 0x31, write: Vec::new(), read: 64 };
    assert!(i2c.is_valid());
    assert!(!I2cRequest { read: 65, ..i2c.clone() }.is_valid());
    assert!(!I2cRequest { address: 0x80, ..i2c }.is_valid());

    let output = OutputSettings {
        filter: DacFilter::FastLowLatency,
        volume_limit_centi_db: -6_000,
        balance_centi_db: -600,
        dsd_direct: false,
    };
    assert!(output.is_valid());
    assert!(!OutputSettings { volume_limit_centi_db: 1, ..output }.is_valid());
    assert!(!OutputSettings { volume_limit_centi_db: -6_001, ..output }.is_valid());
    assert!(!OutputSettings { balance_centi_db: 601, ..output }.is_valid());
    assert!(!OutputSettings { balance_centi_db: i16::MIN, ..output }.is_valid());
    assert!(MicSettings { gain_centi_db: 3_000 }.is_valid());
    assert!(!MicSettings { gain_centi_db: -1 }.is_valid());
    assert!(!MicSettings { gain_centi_db: 3_001 }.is_valid());
    assert!(!EqSlotUpdate { slot: EQ_SLOTS, profile: full_eq() }.is_valid());
    assert!(eq_selection_is_valid(None) && eq_selection_is_valid(Some(EQ_SLOTS - 1)) && !eq_selection_is_valid(Some(EQ_SLOTS)));

    for action in [
        KeyAction::Transparent,
        KeyAction::Disabled,
        KeyAction::Key(0x04),
        KeyAction::Key(0xDF),
        KeyAction::Modifiers(0x22),
        KeyAction::KeyWithModifiers { usage: 0x29, modifiers: 1 },
        KeyAction::Consumer(0x3FF),
        KeyAction::LayerHold(LAYERS - 1),
        KeyAction::LayerToggle(1),
    ] {
        assert!(action.is_valid(), "{action:?}");
    }
    for action in [
        KeyAction::Key(0x01),
        KeyAction::Key(0xE0),
        KeyAction::KeyWithModifiers { usage: 0xE7, modifiers: 1 },
        KeyAction::Consumer(0),
        KeyAction::Consumer(0x400),
        KeyAction::LayerHold(0),
        KeyAction::LayerToggle(LAYERS),
    ] {
        assert!(!action.is_valid(), "{action:?}");
    }
    assert!(!KeymapEntry { layer: LAYERS, source: 4, action: KeyAction::Key(5) }.is_valid());
    assert!(!KeymapPage { layer: LAYERS, start: 0 }.is_valid());

    let tone = AudioStart { rate_hz: 48_000, channel: AudioChannel::Left, level_centi_dbfs: -2_000, tone_hz: 1_000 };
    assert!(AudioBenchRequest::Start(tone).is_valid());
    assert!(AudioStart { level_centi_dbfs: 0, tone_hz: 20_000, rate_hz: 44_100, ..tone }.is_valid());
    assert!(AudioStart { level_centi_dbfs: AudioStart::MIN_LEVEL_CENTI_DBFS, ..tone }.is_valid());
    assert!(AudioStart { tone_hz: 0, rate_hz: 1, ..tone }.is_valid());
    assert!(!AudioStart { level_centi_dbfs: 1, ..tone }.is_valid());
    assert!(!AudioStart { level_centi_dbfs: AudioStart::MIN_LEVEL_CENTI_DBFS - 1, ..tone }.is_valid());
    assert!(!AudioStart { tone_hz: 19, ..tone }.is_valid());
    assert!(!AudioStart { tone_hz: 20_001, rate_hz: 384_000, ..tone }.is_valid());
    assert!(!AudioStart { tone_hz: 16_000, rate_hz: 32_000, ..tone }.is_valid());
    assert!(!AudioBenchRequest::Start(AudioStart { tone_hz: u32::MAX, ..tone }).is_valid());
    assert!(
        [AudioBenchRequest::Calibrate, AudioBenchRequest::Stop, AudioBenchRequest::Status]
            .iter()
            .all(AudioBenchRequest::is_valid)
    );
    assert_eq!(
        [AudioChannel::Left, AudioChannel::Right, AudioChannel::Both].map(AudioChannel::slots),
        [[true, false], [false, true], [true, true]]
    );
}

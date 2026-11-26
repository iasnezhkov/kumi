// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

use kumi_icd::*;
use postcard_rpc::header::{VarHeader, VarKey, VarSeq};
use postcard_rpc::standard_icd::{ERROR_KEY, WireError};
use postcard_rpc::{Endpoint, Key, Key1, Topic};
use serde::Serialize;
use serde_json::{Value, json};

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn frame<T: Serialize>(key: Key, seq_no: VarSeq, payload: &T) -> Vec<u8> {
    let mut out = [0u8; kumi_icd::frame::MAX_FRAME];
    let used = kumi_icd::frame::encode(key, seq_no, payload, &mut out).unwrap();
    out[..used].to_vec()
}

fn pair<E: Endpoint>(sequence: u16, request: &E::Request, responses: &[(&str, E::Response)]) -> Value
where
    E::Request: Serialize,
    E::Response: Serialize,
{
    let sequence = VarSeq::Seq2(sequence);
    let mut header = [0u8; kumi_icd::frame::MAX_HEADER];
    let used = kumi_icd::frame::write_header(E::RESP_KEY, sequence, &mut header).unwrap();
    let mut value = json!({"request": hex(&frame(E::REQ_KEY, sequence, request)), "response_header": hex(&header[..used])});
    for (name, response) in responses {
        value[*name] = json!(hex(&frame(E::RESP_KEY, sequence, response)));
    }
    value
}

fn topic_header<T: Topic>() -> String {
    let mut out = [0u8; kumi_icd::frame::MAX_HEADER];
    let used = kumi_icd::frame::write_header(T::TOPIC_KEY, kumi_icd::frame::TOPIC_SEQUENCE, &mut out).unwrap();
    hex(&out[..used])
}

const BENCH_TONE: AudioStart =
    AudioStart { rate_hz: 48_000, channel: AudioChannel::Left, level_centi_dbfs: -2_000, tone_hz: 1_000 };
const BENCH_BAD_RATE: AudioStart = AudioStart { rate_hz: 50_000, ..BENCH_TONE };
const SLOT_REFUSED_POKES: [(&str, u32, u32, u32); 21] = [
    ("boot_gate", 0x2000_0010, 0, 0x2_0000),
    ("spi_clock_enable", 0x2000_2000, 0, 0x80),
    ("usb_clock_enable", 0x2000_2000, 0, 0x10_0000),
    ("whole_clock_gate", 0x2000_2000, 0, u32::MAX),
    ("spi_reset", 0x2000_3000, 0, 0x80),
    ("system_clock", 0x2000_2010, 0, 0),
    ("spi_clock", 0x2000_2020, 0, 0),
    ("usb_clock", 0x2000_2090, 0, 0),
    ("reset_request", 0x2000_3024, 2, 2),
    ("usb_phy", 0x2000_6000, 0, 0),
    ("dma_destination", 0x2002_0208, 0, u32::MAX),
    ("dma_enable", 0x2002_1000, 0, u32::MAX),
    ("core_pll", 0x2100_1000, 0, 0),
    ("boot_input", 0x2100_4010, 0, 0),
    ("boot_flag", 0x2100_801C, 0x8000_0000, 0x8000_0000),
    ("flash_controller", 0x2009_0000, 0, 0),
    ("serial_flash", 0x200A_0010, 0, 0),
    ("usb_controller", 0x200D_0000, 0, 0),
    ("keyboard_uart", 0x2004_0020, 0, 0),
    ("boot_info", 0x0013_7E00, 0, 0),
    ("stage0_verify", 0x0012_8008, 0, 0),
];
const SLOT_PERMITTED_I2C: [(&str, u8, u8, &[u8], u8); 3] =
    [("probe_bus1", 1, 0x6b, &[], 0), ("pointer_read", 0, 0x6b, &[0], 1), ("dac_write", 0, 0x33, &[2, 0, 0, 1], 0)];
const SLOT_REFUSED_I2C: [(&str, u8, u8, &[u8], u8); 4] = [
    ("write", 0, 0x6b, &[0, 1], 0),
    ("send_byte", 1, 0x6b, &[3], 0),
    ("long_prefix", 1, 0x6b, &[0, 0], 1),
    ("dac_write_on_bus1", 1, 0x33, &[2, 0], 0),
];

const EMULATOR_SERIAL: Option<[u8; 8]> = kumi_chip::efuse::derive_serial([0x400d_0002, 0, 0, 0, 0, 0, 0, 0]);

fn slot_info(build: &str, running_from_ram: bool, lab: bool) -> DeviceInfo {
    let audio = running_from_ram || lab;
    DeviceInfo {
        protocol: PROTOCOL_VERSION,
        firmware: Version { major: 0, minor: 1, patch: 0 },
        build: build.try_into().unwrap(),
        chip_id: 0x4901_4480,
        flash_jedec: [0xc8, 0x65, 0x16],
        serial: EMULATOR_SERIAL,
        running_from_ram,
        capabilities: (Capabilities::KEYMAP | Capabilities::LAYERS | Capabilities::DEVELOPER)
            .with(Capabilities::AUDIO_OUT | Capabilities::PARAMETRIC_EQ | Capabilities::DSD, audio)
            .with(Capabilities::LAB, lab),
    }
}

fn poke(sequence: u16, address: u32, value: u32, mask: u32) -> Value {
    pair::<DevPoke>(
        sequence,
        &PokeRequest { address, value, mask },
        &[
            ("ok", Ok(value)),
            ("invalid_argument", Err(CaseError::InvalidArgument)),
            ("not_unlocked", Err(CaseError::NotUnlocked)),
            ("wrong_state", Err(CaseError::WrongState)),
        ],
    )
}

fn i2c_table(first: u16, table: &[(&str, u8, u8, &[u8], u8)]) -> Value {
    let mut out = serde_json::Map::new();
    for (index, &(name, bus, address, write, read)) in table.iter().enumerate() {
        let request = I2cRequest { bus, address, write: heapless::Vec::from_slice(write).unwrap(), read };
        out.insert(
            name.into(),
            pair::<DevI2c>(
                first + index as u16,
                &request,
                &[
                    ("hardware_fault", Err(CaseError::HardwareFault)),
                    ("invalid_argument", Err(CaseError::InvalidArgument)),
                    ("not_unlocked", Err(CaseError::NotUnlocked)),
                    ("wrong_state", Err(CaseError::WrongState)),
                ],
            ),
        );
    }
    out.into()
}

fn slot() -> Value {
    let mut refused_pokes = serde_json::Map::new();
    for (index, (name, address, value, mask)) in SLOT_REFUSED_POKES.into_iter().enumerate() {
        refused_pokes.insert(name.into(), poke(0x2810 + index as u16, address, value, mask));
    }
    json!({
        "device_info": pair::<GetDeviceInfo>(0x2800, &(), &[
            ("release", slot_info("slot", false, false)),
            ("lab", slot_info("slot-lab", false, true)),
            ("ram_lab", slot_info("ram-lab", true, true)),
        ]),
        "poke_gpio": poke(0x2801, 0x2011_0120, 0, u32::MAX),
        "poke_i2c_select": poke(0x2802, 0x2000_2028, 1, u32::MAX),
        "poke_i2c_clock": poke(0x2803, 0x2000_2000, 0x1000, 0x1000),
        "refused_pokes": refused_pokes,
        "permitted_i2c": i2c_table(0x2830, &SLOT_PERMITTED_I2C),
        "refused_i2c": i2c_table(0x2840, &SLOT_REFUSED_I2C),
    })
}

fn developer() -> Value {
    let faults = [
        ("illegal_instruction", FaultKind::IllegalInstruction),
        ("breakpoint", FaultKind::Breakpoint),
        ("panic", FaultKind::Panic),
    ];
    let mut fault = serde_json::Map::new();
    for (index, (name, kind)) in faults.into_iter().enumerate() {
        fault.insert(
            name.into(),
            pair::<DevFault>(0x2200 + index as u16, &kind, &[("ok", Ok(())), ("not_unlocked", Err(CaseError::NotUnlocked))]),
        );
    }
    let report = FlashTestReport::default();
    json!({
        "unlock": pair::<DevUnlock>(0x2100, &DevUnlockRequest { seconds: MAX_UNLOCK_SECONDS }, &[("ok", Ok(()))]),
        "unlock_one_second": pair::<DevUnlock>(0x2101, &DevUnlockRequest { seconds: 1 }, &[("ok", Ok(()))]),
        "subscribe_keys_uart": pair::<SubscribeEvents>(0x2102, &(EventMask::KEYS | EventMask::UART), &[
            ("unlocked", Ok(EventMask::KEYS | EventMask::UART)),
            ("locked", Ok(EventMask::KEYS)),
        ]),
        "fault": fault,
        "flash_test": pair::<DevFlashTest>(0x2300, &(), &[
            ("not_unlocked", Err(CaseError::NotUnlocked)),
            ("wrong_state", Err(CaseError::WrongState)),
            ("zero_report", Ok(report)),
        ]),
        "peek_chip_id": pair::<DevPeek>(0x2600, &PeekRequest { address: 0x2000_0000, words: 1 }, &[
            ("ok", Ok(WordBlock { address: 0x2000_0000, words: heapless::Vec::from_slice(&[0x4901_4480]).unwrap() })),
            ("not_unlocked", Err(CaseError::NotUnlocked)),
        ]),
        "peek_flash_head": pair::<DevPeek>(0x2602, &PeekRequest { address: 0x4800_0000, words: 1 }, &[
            ("erased", Ok(WordBlock { address: 0x4800_0000, words: heapless::Vec::from_slice(&[0xFFFF_FFFF]).unwrap() })),
            ("not_unlocked", Err(CaseError::NotUnlocked)),
        ]),
        "poke_gpio": pair::<DevPoke>(0x2601, &PokeRequest { address: 0x2011_0120, value: 0, mask: u32::MAX }, &[
            ("ok", Ok(0)),
            ("not_unlocked", Err(CaseError::NotUnlocked)),
        ]),
        "audio_status": pair::<DevAudio>(0x2700, &AudioBenchRequest::Status, &[
            ("not_unlocked", Err(CaseError::NotUnlocked)),
            ("wrong_state", Err(CaseError::WrongState)),
        ]),
        "audio_calibrate": pair::<DevAudio>(0x2701, &AudioBenchRequest::Calibrate, &[
            ("not_unlocked", Err(CaseError::NotUnlocked)),
            ("wrong_state", Err(CaseError::WrongState)),
        ]),
        "audio_stop": pair::<DevAudio>(0x2702, &AudioBenchRequest::Stop, &[
            ("not_unlocked", Err(CaseError::NotUnlocked)),
            ("wrong_state", Err(CaseError::WrongState)),
        ]),
        "audio_start_uncalibrated": pair::<DevAudio>(0x2703, &AudioBenchRequest::Start(BENCH_TONE), &[
            ("not_unlocked", Err(CaseError::NotUnlocked)),
            ("wrong_state", Err(CaseError::WrongState)),
        ]),
        "audio_bad_rate": pair::<DevAudio>(0x2704, &AudioBenchRequest::Start(BENCH_BAD_RATE), &[
            ("not_unlocked", Err(CaseError::NotUnlocked)),
            ("invalid_argument", Err(CaseError::InvalidArgument)),
            ("wrong_state", Err(CaseError::WrongState)),
        ]),
        "uart_send": pair::<DevUartSend>(
            0x2750,
            &ByteBlock { bytes: heapless::Vec::from_slice(b"record:1\n").unwrap() },
            &[
                ("ok", Ok(())),
                ("not_unlocked", Err(CaseError::NotUnlocked)),
                ("unsupported", Err(CaseError::Unsupported)),
            ],
        ),
        "uart_send_empty": pair::<DevUartSend>(
            0x2751,
            &ByteBlock { bytes: heapless::Vec::new() },
            &[("invalid_argument", Err(CaseError::InvalidArgument))],
        ),
        "slot": slot(),
        "diagnostics": pair::<GetDiagnostics>(0x2400, &(), &[]),
        "ping": pair::<PingEndpoint>(0x2500, &0x4b55_4d49, &[("ok", 0x4b55_4d49)]),
        "topics": {
            "uart_mirror": topic_header::<UartMirrorTopic>(),
            "key_events": topic_header::<KeyEventTopic>(),
            "log": topic_header::<LogTopic>(),
        },
    })
}

fn add<E: Endpoint>(vectors: &mut Vec<Value>, name: &str, request: E::Request, response: E::Response)
where
    E::Request: Serialize,
    E::Response: Serialize,
{
    let sequence = VarSeq::Seq2(0x1234 + vectors.len() as u16);
    vectors.push(json!({
        "name": name, "request": hex(&frame(E::REQ_KEY, sequence, &request)),
        "response": hex(&frame(E::RESP_KEY, sequence, &response))
    }));
}

fn main() {
    let booted_image_size = std::env::var("KUMI_BOOTED_IMAGE_SIZE").ok().map(|size| size.parse::<u32>().unwrap());
    let mut vectors = Vec::new();
    let audio = booted_image_size.is_none();
    add::<GetDeviceInfo>(
        &mut vectors,
        "device_info",
        (),
        DeviceInfo {
            protocol: PROTOCOL_VERSION,
            firmware: Version { major: 0, minor: 1, patch: 0 },
            build: (if audio { "ram" } else { "slot" }).try_into().unwrap(),
            chip_id: 0x4901_4480,
            flash_jedec: [0xc8, 0x65, 0x16],
            serial: EMULATOR_SERIAL,
            running_from_ram: audio,
            capabilities: (Capabilities::KEYMAP | Capabilities::LAYERS | Capabilities::DEVELOPER)
                .with(Capabilities::AUDIO_OUT | Capabilities::PARAMETRIC_EQ | Capabilities::DSD, audio)
                .with(Capabilities::PERSISTENT_SETTINGS | Capabilities::FIRMWARE_UPDATE, booted_image_size.is_some()),
        },
    );
    add::<PingEndpoint>(&mut vectors, "ping", 0x4b55_4d49, 0x4b55_4d49);
    add::<SubscribeEvents>(&mut vectors, "subscribe_nothing", EventMask::NONE, Ok(EventMask::NONE));
    add::<SubscribeEvents>(&mut vectors, "subscribe_unavailable_topics", EventMask::AUDIO | EventMask::UART, Ok(EventMask::NONE));
    add::<GetAudioStatus>(
        &mut vectors,
        "audio_status",
        (),
        AudioStatus {
            jack: Jack::Empty,
            usb_audio_present: audio,
            stream: None,
            host_volume_centi_db: if audio { [-2_000; 2] } else { [0; 2] },
            host_muted: false,
            eq_active: None,
            counters: AudioCounters::default(),
        },
    );
    add::<GetKeyboardStatus>(&mut vectors, "keyboard_status", (), KeyboardStatus::default());
    let defaults = kumi_settings::StoredSettings::default();
    add::<GetOutputSettings>(&mut vectors, "output_default", (), defaults.output);
    let output = OutputSettings { volume_limit_centi_db: -600, ..defaults.output };
    add::<SetOutputSettings>(&mut vectors, "output_set", output, Ok(()));
    add::<GetOutputSettings>(&mut vectors, "output_after", (), output);
    let mic = MicSettings { gain_centi_db: 300 };
    add::<SetMic>(&mut vectors, "mic_set", mic, Ok(()));
    add::<GetMic>(&mut vectors, "mic_after", (), Ok(mic));
    let empty_eq = EqProfile { preamp_centi_db: 0, bands: heapless::Vec::new() };
    add::<SetEq>(&mut vectors, "eq_set", EqSlotUpdate { slot: 0, profile: empty_eq.clone() }, Ok(()));
    add::<SelectEq>(&mut vectors, "eq_select", None, Ok(()));
    add::<GetEq>(&mut vectors, "eq_after", 0, Ok(Some(empty_eq)));
    add::<GetEq>(&mut vectors, "eq_bad_slot", EQ_SLOTS, Err(CaseError::InvalidArgument));
    add::<ResetSettings>(&mut vectors, "reset_audio", SettingsScope::Audio, Ok(()));
    let entry = KeymapEntry { layer: 0, source: 4, action: KeyAction::Key(5) };
    add::<SetKey>(&mut vectors, "remap_a_to_b", entry, Ok(()));
    let entries = [entry].into_iter().collect();
    add::<GetKeymap>(
        &mut vectors,
        "keymap_roundtrip",
        KeymapPage { layer: 0, start: 0 },
        Ok(KeymapChunk { entries, more: false }),
    );
    for source in 5..=16 {
        add::<SetKey>(
            &mut vectors,
            &format!("set_key_{source}"),
            KeymapEntry {
                layer: 0,
                source,
                action: if source == 16 { KeyAction::Disabled } else { KeyAction::Key(source + 30) },
            },
            Ok(()),
        );
    }
    let mut entries = heapless::Vec::new();
    entries.push(entry).unwrap();
    for source in 5..=16 {
        entries
            .push(KeymapEntry {
                layer: 0,
                source,
                action: if source == 16 { KeyAction::Disabled } else { KeyAction::Key(source + 30) },
            })
            .unwrap();
    }
    let sequence = VarSeq::Seq1(91);
    let request = frame(GetKeymap::REQ_KEY, sequence, &KeymapPage { layer: 0, start: 0 });
    let response = frame(GetKeymap::RESP_KEY, sequence, &Ok::<_, CaseError>(KeymapChunk { entries, more: false }));
    assert_eq!(response.len(), 64);
    vectors.push(json!({"name": "keymap_many", "request": hex(&request), "response": hex(&response)}));
    add::<ClearKeymap>(&mut vectors, "clear_keymap", (), Ok(()));
    add::<GetKeymap>(
        &mut vectors,
        "keymap_empty",
        KeymapPage { layer: 0, start: 0 },
        Ok(KeymapChunk { entries: heapless::Vec::new(), more: false }),
    );
    add::<SetKey>(
        &mut vectors,
        "bad_layer",
        KeymapEntry { layer: LAYERS, source: 4, action: KeyAction::Key(5) },
        Err(CaseError::InvalidArgument),
    );
    add::<SetKey>(
        &mut vectors,
        "bad_base_layer_hold",
        KeymapEntry { layer: 0, source: 4, action: KeyAction::LayerHold(0) },
        Err(CaseError::InvalidArgument),
    );
    add::<SetKey>(
        &mut vectors,
        "bad_consumer_usage",
        KeymapEntry { layer: 0, source: 4, action: KeyAction::Consumer(0x400) },
        Err(CaseError::InvalidArgument),
    );
    add::<ResetSettings>(&mut vectors, "reset_keymap", SettingsScope::Keymap, Ok(()));
    add::<SetKey>(&mut vectors, "layer_hold", KeymapEntry { layer: 0, source: 57, action: KeyAction::LayerHold(1) }, Ok(()));
    add::<SetKey>(&mut vectors, "layer_one_a_to_c", KeymapEntry { layer: 1, source: 4, action: KeyAction::Key(6) }, Ok(()));
    add::<SaveSettings>(
        &mut vectors,
        "save_settings",
        (),
        if booted_image_size.is_some() { Ok(()) } else { Err(CaseError::Unsupported) },
    );
    add::<Reboot>(
        &mut vectors,
        "reboot_without_fallback",
        RebootTarget::OtherSlot,
        Err(if booted_image_size.is_some() { CaseError::WrongState } else { CaseError::Unsupported }),
    );
    add::<ConfirmBoot>(
        &mut vectors,
        "confirm_boot",
        (),
        if booted_image_size.is_some() { Ok(()) } else { Err(CaseError::Unsupported) },
    );
    add::<GetSlots>(
        &mut vectors,
        "slots_read_from_flash",
        (),
        SlotTable {
            booted: booted_image_size.map(|_| Slot::A),
            slots: [
                SlotInfo {
                    slot: Slot::A,
                    state: if booted_image_size.is_some() { SlotState::Confirmed } else { SlotState::Empty },
                    version: booted_image_size.map(|_| Version { major: 0, minor: 1, patch: 0 }),
                    size: booted_image_size.unwrap_or(0),
                },
                SlotInfo { slot: Slot::B, state: SlotState::Empty, version: None, size: 0 },
            ],
        },
    );
    add::<UpdateBegin>(
        &mut vectors,
        "update_begin_unsupported",
        UpdateManifest {
            slot: Slot::A,
            version: Version { major: 0, minor: 1, patch: 0 },
            size: 64,
            sha256: [0; 32],
            signature: [[0; 32]; 2],
        },
        Err(if booted_image_size.is_some() { CaseError::WrongState } else { CaseError::Unsupported }),
    );
    let data = (0..49).collect();
    let sequence = VarSeq::Seq4(0xabcd_1234);
    let request = frame(UpdateChunk::REQ_KEY, sequence, &UpdateBlock { offset: 0, data });
    assert_eq!(request.len(), 64);
    vectors.push(json!({"name": "full_packet_request", "request": hex(&request),
        "response": hex(&frame(UpdateChunk::RESP_KEY, sequence, &Err::<u32, _>(if booted_image_size.is_some() { CaseError::WrongState } else { CaseError::Unsupported })))}));
    add::<UpdateFinish>(
        &mut vectors,
        "update_finish_without_begin",
        (),
        Err(if booted_image_size.is_some() { CaseError::WrongState } else { CaseError::Unsupported }),
    );
    add::<DevFault>(&mut vectors, "fault_locked", FaultKind::Panic, Err(CaseError::NotUnlocked));
    add::<DevFlashTest>(
        &mut vectors,
        "flash_test_locked",
        (),
        Err(if booted_image_size.is_some() { CaseError::WrongState } else { CaseError::NotUnlocked }),
    );
    add::<SubscribeEvents>(&mut vectors, "subscribe_uart_locked", EventMask::UART, Ok(EventMask::NONE));
    let refused = |error| Err(if booted_image_size.is_some() { CaseError::WrongState } else { error });
    add::<DevAudio>(&mut vectors, "audio_locked", AudioBenchRequest::Status, refused(CaseError::NotUnlocked));
    add::<DevUnlock>(&mut vectors, "developer_unlock", DevUnlockRequest { seconds: 10 }, Ok(()));
    add::<DevUnlock>(&mut vectors, "bad_unlock_duration", DevUnlockRequest { seconds: 0 }, Err(CaseError::InvalidArgument));
    add::<DevPeek>(
        &mut vectors,
        "peek_usb_refused",
        PeekRequest { address: 0x200D_0000, words: 1 },
        Err(CaseError::InvalidArgument),
    );
    add::<DevPoke>(
        &mut vectors,
        "poke_outside_peripherals",
        PokeRequest { address: 0, value: 0, mask: 0 },
        Err(if booted_image_size.is_some() { CaseError::WrongState } else { CaseError::InvalidArgument }),
    );
    add::<DevPoke>(
        &mut vectors,
        "poke_unaligned",
        PokeRequest { address: 2, value: 0, mask: 0 },
        Err(CaseError::InvalidArgument),
    );
    add::<DevI2c>(
        &mut vectors,
        "i2c_bad_bus",
        I2cRequest { bus: 2, address: 0x31, write: heapless::Vec::new(), read: 1 },
        Err(if booted_image_size.is_some() { CaseError::WrongState } else { CaseError::InvalidArgument }),
    );
    add::<DevI2c>(
        &mut vectors,
        "i2c_read_too_long",
        I2cRequest { bus: 0, address: 0x31, write: heapless::Vec::new(), read: 65 },
        Err(CaseError::InvalidArgument),
    );
    add::<DevAudio>(
        &mut vectors,
        "audio_bad_rate",
        AudioBenchRequest::Start(BENCH_BAD_RATE),
        refused(CaseError::InvalidArgument),
    );
    add::<DevAudio>(
        &mut vectors,
        "audio_tone_without_whole_periods",
        AudioBenchRequest::Start(AudioStart { tone_hz: 997, ..BENCH_TONE }),
        refused(CaseError::InvalidArgument),
    );
    add::<DevAudio>(&mut vectors, "audio_start_uncalibrated", AudioBenchRequest::Start(BENCH_TONE), Err(CaseError::WrongState));
    add::<DevAudio>(
        &mut vectors,
        "audio_level_above_full_scale",
        AudioBenchRequest::Start(AudioStart { level_centi_dbfs: 100, ..BENCH_TONE }),
        Err(CaseError::InvalidArgument),
    );
    add::<DevAudio>(
        &mut vectors,
        "audio_tone_above_nyquist",
        AudioBenchRequest::Start(AudioStart { tone_hz: 20_000, rate_hz: 32_000, ..BENCH_TONE }),
        Err(CaseError::InvalidArgument),
    );
    add::<DevUartSend>(&mut vectors, "uart_unsupported", ByteBlock { bytes: heapless::Vec::new() }, Err(CaseError::Unsupported));
    let sequence = VarSeq::Seq4(0x7654_3210);
    vectors.push(json!({"name": "unknown_key", "request": hex(&frame(Key::for_path::<()>("missing"), sequence, &())),
        "response": hex(&frame(ERROR_KEY, sequence, &WireError::UnknownKey))}));
    vectors.push(json!({"name": "truncated_keymap_entry", "request": hex(&frame(SetKey::REQ_KEY, sequence, &())),
        "response": hex(&frame(ERROR_KEY, sequence, &WireError::DeserFailed))}));
    let mut short = [0u8; 8];
    let used = VarHeader { key: VarKey::Key1(Key1::from_key8(GetDeviceInfo::REQ_KEY)), seq_no: sequence }
        .write_to_slice(&mut short)
        .unwrap()
        .0
        .len();
    vectors.push(json!({"name": "one_byte_key", "request": hex(&short[..used]),
        "response": hex(&frame(ERROR_KEY, sequence, &WireError::KeyTooSmall))}));
    println!("{}", serde_json::to_string_pretty(&json!({"vectors": vectors, "developer": developer()})).unwrap());
}

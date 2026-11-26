// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::{String, Vec};
use kumi_control::Controls;
use kumi_icd::*;
use kumi_service::*;
use kumi_settings::StoredSettings;
use postcard_rpc::Endpoint;
use serde::Serialize;
use serde::de::DeserializeOwned;

struct TestProvider {
    audio_supported: bool,
    saves: u32,
    saved: Option<StoredSettings>,
    reboots: Vec<RebootTarget, 4>,
    events: EventMask,
}

impl TestProvider {
    fn new() -> Self {
        Self { audio_supported: true, saves: 0, saved: None, reboots: Vec::new(), events: EventMask::NONE }
    }
}

fn diagnostics() -> Diagnostics {
    Diagnostics {
        boot_count: 7,
        safe_mode: true,
        stage: stage::FW_SAFE_MODE,
        uptime_ms: 1_234,
        dropped_events: 2,
        watchdog_armed: false,
        last_crash: Some(CrashReport {
            boot_count: 6,
            reason: CrashReason::Trap,
            stage: stage::FW_RUNNING,
            mcause: 2,
            mepc: 0x0010_0040,
            mtval: 0xFFFF_FFFF,
            ra: 0x0010_0100,
            sp: 0x0013_0000,
            uptime_ms: 99,
            message: String::try_from("runtime fault").unwrap(),
        }),
    }
}

fn device_info() -> DeviceInfo {
    DeviceInfo {
        protocol: PROTOCOL_VERSION,
        firmware: Version { major: 0, minor: 2, patch: 0 },
        build: String::try_from("test").unwrap(),
        chip_id: 0x4901_4480,
        flash_jedec: [0xC8, 0x65, 0x16],
        serial: Some([1, 2, 3, 4, 5, 6, 7, 8]),
        running_from_ram: false,
        capabilities: Capabilities::AUDIO_OUT | Capabilities::PARAMETRIC_EQ | Capabilities::KEYMAP,
    }
}

impl Provider for TestProvider {
    fn audio_supported(&self) -> bool {
        self.audio_supported
    }

    fn device_info(&mut self) -> DeviceInfo {
        device_info()
    }
    fn diagnostics(&mut self) -> DiagnosticsResult {
        Ok(diagnostics())
    }
    fn subscribe(&mut self, events: EventMask) -> EventsResult {
        self.events = events & (EventMask::KEYS | EventMask::LOG);
        Ok(self.events)
    }
    fn audio_status(&mut self) -> AudioStatus {
        AudioStatus {
            jack: Jack::Headphones,
            usb_audio_present: true,
            stream: None,
            host_volume_centi_db: [0, 0],
            host_muted: false,
            eq_active: None,
            counters: AudioCounters::default(),
        }
    }
    fn keyboard_status(&mut self) -> KeyboardStatus {
        KeyboardStatus {
            link_up: true,
            frames: 10,
            bad_frames: 0,
            controller_commands: 2,
            active_layer: 0,
            caps_word: false,
            host_leds: 0,
        }
    }
    fn slots(&mut self) -> SlotTable {
        SlotTable {
            booted: Some(Slot::A),
            slots: [
                SlotInfo {
                    slot: Slot::A,
                    state: SlotState::Confirmed,
                    version: Some(Version { major: 0, minor: 2, patch: 0 }),
                    size: 0x9000,
                },
                SlotInfo { slot: Slot::B, state: SlotState::Empty, version: None, size: 0 },
            ],
        }
    }
    fn save(&mut self, settings: &StoredSettings) -> Ack {
        self.saves += 1;
        self.saved = Some(settings.clone());
        Ok(())
    }
    fn reboot(&mut self, target: RebootTarget) -> Ack {
        self.reboots.push(target).unwrap();
        Ok(())
    }
    fn confirm_boot(&mut self) -> Ack {
        Ok(())
    }
}

fn call<E: Endpoint>(c: &mut Controls, p: &mut TestProvider, req: &E::Request) -> Result<E::Response, DispatchError>
where
    E::Request: Serialize,
    E::Response: DeserializeOwned,
{
    let mut payload = [0u8; 1024];
    let n = postcard::to_slice(req, &mut payload).unwrap().len();
    let mut out = [0u8; 1024];
    let used = dispatch(c, p, E::REQ_KEY, &payload[..n], &mut out)?;
    Ok(postcard::from_bytes::<E::Response>(&out[..used]).unwrap())
}

fn profile() -> EqProfile {
    let mut bands = Vec::new();
    bands.push(EqBand { kind: FilterKind::Peak, frequency_hz: 100, q_milli: 1_000, gain_centi_db: 300 }).unwrap();
    EqProfile { preamp_centi_db: -100, bands }
}

#[test]
fn control_endpoints_round_trip_through_the_dispatcher() {
    let mut c = Controls::new(StoredSettings::default());
    let mut p = TestProvider::new();

    let out = OutputSettings {
        filter: DacFilter::NonOversampling,
        volume_limit_centi_db: -400,
        balance_centi_db: 0,
        dsd_direct: false,
    };
    assert_eq!(call::<SetOutputSettings>(&mut c, &mut p, &out), Ok(Ok(())));
    assert_eq!(call::<GetOutputSettings>(&mut c, &mut p, &()), Ok(out));

    assert_eq!(call::<SetEq>(&mut c, &mut p, &EqSlotUpdate { slot: 0, profile: profile() }), Ok(Ok(())));
    assert_eq!(call::<GetEq>(&mut c, &mut p, &0u8), Ok(Ok(Some(profile()))));
    assert_eq!(call::<GetEq>(&mut c, &mut p, &1u8), Ok(Ok(None)));
    assert_eq!(call::<GetEq>(&mut c, &mut p, &EQ_SLOTS), Ok(Err(CaseError::InvalidArgument)));
    assert_eq!(call::<SelectEq>(&mut c, &mut p, &Some(0u8)), Ok(Ok(())));
    assert_eq!(call::<SelectEq>(&mut c, &mut p, &Some(3u8)), Ok(Err(CaseError::WrongState)));
    assert_eq!(call::<SetMic>(&mut c, &mut p, &MicSettings { gain_centi_db: -5 }), Ok(Err(CaseError::InvalidArgument)));
    assert_eq!(
        call::<SetOutputSettings>(&mut c, &mut p, &OutputSettings { balance_centi_db: i16::MAX, ..out }),
        Ok(Err(CaseError::InvalidArgument))
    );

    let entry = KeymapEntry { layer: 0, source: 4, action: KeyAction::Consumer(0xE9) };
    assert_eq!(call::<SetKey>(&mut c, &mut p, &entry), Ok(Ok(())));
    let page = call::<GetKeymap>(&mut c, &mut p, &KeymapPage { layer: 0, start: 0 }).unwrap().unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0], entry);
}

#[test]
fn unsupported_audio_does_not_change_settings_or_report_success() {
    let mut controls = Controls::new(StoredSettings::default());
    let original = controls.settings().clone();
    let mut provider = TestProvider::new();
    provider.audio_supported = false;
    let output = OutputSettings { volume_limit_centi_db: -600, ..original.output };
    assert_eq!(call::<SetOutputSettings>(&mut controls, &mut provider, &output), Ok(Err(CaseError::Unsupported)));
    assert_eq!(
        call::<SetMic>(&mut controls, &mut provider, &MicSettings { gain_centi_db: 300 }),
        Ok(Err(CaseError::Unsupported))
    );
    assert_eq!(
        call::<SetEq>(&mut controls, &mut provider, &EqSlotUpdate { slot: 0, profile: profile() }),
        Ok(Err(CaseError::Unsupported))
    );
    assert_eq!(call::<SelectEq>(&mut controls, &mut provider, &None), Ok(Err(CaseError::Unsupported)));
    assert_eq!(call::<ResetSettings>(&mut controls, &mut provider, &SettingsScope::Audio), Ok(Err(CaseError::Unsupported)));
    assert_eq!(controls.settings(), &original);
    assert!(!controls.dirty());
}

#[test]
fn provider_endpoints_are_routed() {
    let mut c = Controls::new(StoredSettings::default());
    let mut p = TestProvider::new();
    assert_eq!(call::<GetDeviceInfo>(&mut c, &mut p, &()), Ok(device_info()));
    assert_eq!(call::<GetKeyboardStatus>(&mut c, &mut p, &()).unwrap().active_layer, 0);
    assert_eq!(call::<GetSlots>(&mut c, &mut p, &()).unwrap().booted, Some(Slot::A));
    let entry = KeymapEntry { layer: 1, source: 9, action: KeyAction::Key(0x10) };
    assert_eq!(call::<SetKey>(&mut c, &mut p, &entry), Ok(Ok(())));
    assert_eq!(call::<SaveSettings>(&mut c, &mut p, &()), Ok(Ok(())));
    assert_eq!(call::<Reboot>(&mut c, &mut p, &RebootTarget::OtherSlot), Ok(Ok(())));
    assert_eq!(p.saves, 1);
    assert_eq!(p.saved.as_ref(), Some(c.settings()));
    assert_eq!(call::<PingEndpoint>(&mut c, &mut p, &0xDEAD_BEEF), Ok(0xDEAD_BEEF));
    assert_eq!(p.reboots.as_slice(), [RebootTarget::OtherSlot]);
}

#[test]
fn locked_developer_endpoints_and_unknown_keys_are_refused() {
    let mut c = Controls::new(StoredSettings::default());
    let mut p = TestProvider::new();
    assert_eq!(call::<DevUnlock>(&mut c, &mut p, &DevUnlockRequest { seconds: 10 }), Ok(Err(CaseError::Unsupported)));
    assert_eq!(call::<DevPeek>(&mut c, &mut p, &PeekRequest { address: 0, words: 1 }), Ok(Err(CaseError::Unsupported)));
    assert_eq!(call::<UpdateFinish>(&mut c, &mut p, &()), Ok(Err(CaseError::Unsupported)));

    let mut out = [0u8; 64];
    assert_eq!(
        dispatch(&mut c, &mut p, postcard_rpc::Key::for_path::<()>("no/such/endpoint"), &[], &mut out),
        Err(DispatchError::UnknownKey)
    );
    assert_eq!(dispatch(&mut c, &mut p, SetMic::REQ_KEY, &[0xFF, 0xFF, 0xFF, 0xFF], &mut out), Err(DispatchError::Decode));
}

#[test]
fn developer_requests_outside_their_limits_are_refused_before_the_provider() {
    let mut c = Controls::new(StoredSettings::default());
    let mut p = TestProvider::new();
    assert_eq!(call::<DevUnlock>(&mut c, &mut p, &DevUnlockRequest { seconds: 65_535 }), Ok(Err(CaseError::InvalidArgument)));
    assert_eq!(call::<DevPeek>(&mut c, &mut p, &PeekRequest { address: 0, words: 255 }), Ok(Err(CaseError::InvalidArgument)));
    assert_eq!(call::<DevPeek>(&mut c, &mut p, &PeekRequest { address: 2, words: 1 }), Ok(Err(CaseError::InvalidArgument)));
    assert_eq!(call::<DevPeek>(&mut c, &mut p, &PeekRequest { address: 0, words: 1 }), Ok(Err(CaseError::Unsupported)));
    assert_eq!(
        call::<DevPoke>(&mut c, &mut p, &PokeRequest { address: 0x0010_0002, value: 0, mask: 0 }),
        Ok(Err(CaseError::InvalidArgument))
    );
    assert_eq!(
        call::<DevI2c>(&mut c, &mut p, &I2cRequest { bus: 0, address: 0x31, write: Vec::new(), read: 200 }),
        Ok(Err(CaseError::InvalidArgument))
    );
    let tone = AudioStart { rate_hz: 48_000, channel: AudioChannel::Both, level_centi_dbfs: -2_000, tone_hz: 1_000 };
    assert_eq!(call::<DevAudio>(&mut c, &mut p, &AudioBenchRequest::Start(tone)), Ok(Err(CaseError::Unsupported)));
    assert_eq!(call::<DevAudio>(&mut c, &mut p, &AudioBenchRequest::Status), Ok(Err(CaseError::Unsupported)));
    for bad in [AudioStart { level_centi_dbfs: 100, ..tone }, AudioStart { tone_hz: 24_000, ..tone }] {
        assert_eq!(call::<DevAudio>(&mut c, &mut p, &AudioBenchRequest::Start(bad)), Ok(Err(CaseError::InvalidArgument)));
    }
}

#[test]
fn diagnostics_and_event_subscription_reach_the_provider() {
    let mut c = Controls::new(StoredSettings::default());
    let mut p = TestProvider::new();
    assert_eq!(call::<GetDiagnostics>(&mut c, &mut p, &()), Ok(Ok(diagnostics())));
    let wanted = EventMask::KEYS | EventMask::AUDIO | EventMask::UART;
    assert_eq!(call::<SubscribeEvents>(&mut c, &mut p, &wanted), Ok(Ok(EventMask::KEYS)));
    assert_eq!(p.events, EventMask::KEYS);
    assert_eq!(call::<SubscribeEvents>(&mut c, &mut p, &EventMask::NONE), Ok(Ok(EventMask::NONE)));

    struct Minimal;
    impl Provider for Minimal {
        fn device_info(&mut self) -> DeviceInfo {
            device_info()
        }
        fn audio_status(&mut self) -> AudioStatus {
            TestProvider::new().audio_status()
        }
        fn keyboard_status(&mut self) -> KeyboardStatus {
            KeyboardStatus::default()
        }
        fn slots(&mut self) -> SlotTable {
            TestProvider::new().slots()
        }
        fn save(&mut self, _: &StoredSettings) -> Ack {
            Ok(())
        }
        fn reboot(&mut self, _: RebootTarget) -> Ack {
            Ok(())
        }
        fn confirm_boot(&mut self) -> Ack {
            Ok(())
        }
    }
    let mut out = [0u8; 64];
    let mut payload = [0u8; 8];
    let request = postcard::to_slice(&EventMask::KEYS, &mut payload).unwrap();
    let used = dispatch(&mut c, &mut Minimal, SubscribeEvents::REQ_KEY, request, &mut out).unwrap();
    assert_eq!(postcard::from_bytes::<EventsResult>(&out[..used]).unwrap(), Err(CaseError::Unsupported));
    let used = dispatch(&mut c, &mut Minimal, GetDiagnostics::REQ_KEY, &[], &mut out).unwrap();
    assert_eq!(postcard::from_bytes::<DiagnosticsResult>(&out[..used]).unwrap(), Err(CaseError::Unsupported));
}

#[test]
fn frames_carry_the_response_key_and_echo_the_sequence_number() {
    use postcard_rpc::header::{VarHeader, VarKey, VarSeq};
    use postcard_rpc::standard_icd::{ERROR_KEY, WireError};

    let mut c = Controls::new(StoredSettings::default());
    let mut p = TestProvider::new();
    let mut out = [0u8; 512];
    let encode = |header: VarHeader, body: &[u8]| {
        let mut buf = [0u8; 64];
        let used = header.write_to_slice(&mut buf).unwrap().0.len();
        let mut frame = buf[..used].to_vec();
        frame.extend_from_slice(body);
        frame
    };

    let frame = encode(VarHeader { key: VarKey::Key8(GetMic::REQ_KEY), seq_no: VarSeq::Seq4(0xA1B2_C3D4) }, &[]);
    let used = serve_frame(&mut c, &mut p, &frame, &mut out).unwrap();
    let (header, body) = VarHeader::take_from_slice(&out[..used]).unwrap();
    assert_eq!(header.key, VarKey::Key8(GetMic::RESP_KEY));
    assert!(matches!(header.seq_no, VarSeq::Seq4(0xA1B2_C3D4)));
    assert!(postcard::from_bytes::<MicResult>(body).unwrap().is_ok());

    let unknown = VarHeader { key: VarKey::Key8(postcard_rpc::Key::for_path::<()>("no/such")), seq_no: VarSeq::Seq1(7) };
    let used = serve_frame(&mut c, &mut p, &encode(unknown, &[]), &mut out).unwrap();
    let (header, body) = VarHeader::take_from_slice(&out[..used]).unwrap();
    assert_eq!(header.key, VarKey::Key8(ERROR_KEY));
    assert!(matches!(header.seq_no, VarSeq::Seq1(7)));
    assert_eq!(postcard::from_bytes::<WireError>(body).unwrap(), WireError::UnknownKey);

    let bad = encode(VarHeader { key: VarKey::Key8(SetMic::REQ_KEY), seq_no: VarSeq::Seq2(9) }, &[0xFF, 0xFF, 0xFF, 0xFF]);
    let used = serve_frame(&mut c, &mut p, &bad, &mut out).unwrap();
    let (_, body) = VarHeader::take_from_slice(&out[..used]).unwrap();
    assert_eq!(postcard::from_bytes::<WireError>(body).unwrap(), WireError::DeserFailed);

    assert_eq!(serve_frame(&mut c, &mut p, &[], &mut out), None);
    assert_eq!(serve_frame(&mut c, &mut p, &[0x0F, 1, 2], &mut out), None);
}

#[test]
fn shortened_keys_are_matched_exactly_and_one_byte_keys_are_refused() {
    use postcard_rpc::header::{VarHeader, VarKey, VarSeq};
    use postcard_rpc::standard_icd::{ERROR_KEY, WireError};
    use postcard_rpc::{Key1, Key2, Key4};

    let mut c = Controls::new(StoredSettings::default());
    let mut p = TestProvider::new();
    let mut out = [0u8; 512];
    let mut serve = |key: VarKey| {
        let mut buf = [0u8; 16];
        let used = VarHeader { key, seq_no: VarSeq::Seq1(3) }.write_to_slice(&mut buf).unwrap().0.len();
        let size = serve_frame(&mut c, &mut p, &buf[..used], &mut out).unwrap();
        let (header, body) = VarHeader::take_from_slice(&out[..size]).unwrap();
        (header.key, body.to_vec())
    };
    let (key, body) = serve(VarKey::Key2(Key2::from_key8(GetMic::REQ_KEY)));
    assert_eq!(key, VarKey::Key8(GetMic::RESP_KEY));
    assert!(postcard::from_bytes::<MicResult>(&body).unwrap().is_ok());
    let (key, _) = serve(VarKey::Key4(Key4::from_key8(GetDeviceInfo::REQ_KEY)));
    assert_eq!(key, VarKey::Key8(GetDeviceInfo::RESP_KEY));
    let (key, body) = serve(VarKey::Key1(Key1::from_key8(GetMic::REQ_KEY)));
    assert_eq!(key, VarKey::Key8(ERROR_KEY));
    assert_eq!(postcard::from_bytes::<WireError>(&body).unwrap(), WireError::KeyTooSmall);
}

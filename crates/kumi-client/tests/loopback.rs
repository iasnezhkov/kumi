// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use heapless::{String, Vec};
use kumi_client::{Client, ClientError, Transport};
use kumi_control::Controls;
use kumi_icd::*;
use kumi_service::{Provider, dispatch};
use kumi_settings::StoredSettings;

#[derive(Default)]
struct Firmware {
    saves: u32,
    reboots: u32,
}

impl Provider for Firmware {
    fn device_info(&mut self) -> DeviceInfo {
        DeviceInfo {
            protocol: PROTOCOL_VERSION,
            firmware: Version { major: 0, minor: 2, patch: 0 },
            build: String::try_from("loop").unwrap(),
            chip_id: 0x4901_4480,
            flash_jedec: [0xC8, 0x65, 0x16],
            serial: None,
            running_from_ram: false,
            capabilities: Capabilities::AUDIO_OUT | Capabilities::PARAMETRIC_EQ | Capabilities::KEYMAP,
        }
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
            frames: 0,
            bad_frames: 0,
            controller_commands: 0,
            active_layer: 0,
            caps_word: false,
            host_leds: 0,
        }
    }
    fn slots(&mut self) -> SlotTable {
        SlotTable {
            booted: Some(Slot::A),
            slots: [
                SlotInfo { slot: Slot::A, state: SlotState::Confirmed, version: None, size: 0 },
                SlotInfo { slot: Slot::B, state: SlotState::Empty, version: None, size: 0 },
            ],
        }
    }
    fn save(&mut self, _settings: &StoredSettings) -> Ack {
        self.saves += 1;
        Ok(())
    }
    fn reboot(&mut self, _target: RebootTarget) -> Ack {
        self.reboots += 1;
        Ok(())
    }
    fn confirm_boot(&mut self) -> Ack {
        Ok(())
    }
}

struct Loopback {
    controls: Controls,
    provider: Firmware,
}

impl Transport for Loopback {
    type Error = ();

    fn request(&mut self, key: postcard_rpc::Key, _response: postcard_rpc::Key, payload: &[u8]) -> Result<std::vec::Vec<u8>, ()> {
        let mut out = [0u8; 1024];
        let used = dispatch(&mut self.controls, &mut self.provider, key, payload, &mut out).map_err(|_| ())?;
        Ok(out[..used].to_vec())
    }
}

fn client() -> Client<Loopback> {
    Client::new(Loopback { controls: Controls::new(StoredSettings::default()), provider: Firmware::default() })
}

fn profile() -> EqProfile {
    let mut bands = Vec::new();
    bands.push(EqBand { kind: FilterKind::LowShelf, frequency_hz: 80, q_milli: 707, gain_centi_db: 400 }).unwrap();
    EqProfile { preamp_centi_db: -200, bands }
}

#[test]
fn the_client_drives_the_case_through_the_service() {
    let mut c = client();
    assert_eq!(c.device_info().unwrap().chip_id, 0x4901_4480);

    c.set_output(OutputSettings {
        filter: DacFilter::SlowLowLatency,
        volume_limit_centi_db: -250,
        balance_centi_db: 0,
        dsd_direct: false,
    })
    .unwrap();
    assert_eq!(c.output().unwrap().volume_limit_centi_db, -250);

    c.set_eq(EqSlotUpdate { slot: 2, profile: profile() }).unwrap();
    assert_eq!(c.eq(2).unwrap(), Some(profile()));
    assert_eq!(c.eq(1).unwrap(), None);
    c.select_eq(Some(2)).unwrap();

    c.set_key(KeymapEntry { layer: 0, source: 4, action: KeyAction::Consumer(0xCD) }).unwrap();
    assert_eq!(c.keymap(KeymapPage { layer: 0, start: 0 }).unwrap().entries.len(), 1);

    assert_eq!(c.save_settings(), Ok(()));
    assert_eq!(c.reboot(RebootTarget::OtherSlot), Ok(()));
    assert_eq!(c.ping(77), Ok(77));
    let transport = c.into_transport();
    assert_eq!((transport.provider.saves, transport.provider.reboots), (1, 1));
}

#[test]
fn device_side_errors_reach_the_caller_as_values() {
    let mut c = client();
    assert_eq!(c.eq(EQ_SLOTS), Err(ClientError::Case(CaseError::InvalidArgument)));
    assert_eq!(c.select_eq(Some(1)), Err(ClientError::Case(CaseError::WrongState)));
    assert_eq!(c.diagnostics(), Err(ClientError::Case(CaseError::Unsupported)));
    assert_eq!(c.subscribe(EventMask::KEYS), Err(ClientError::Case(CaseError::Unsupported)));
    assert!(c.mic().is_ok());
    assert_eq!(c.call::<GetEq>(&EQ_SLOTS).unwrap(), Err(CaseError::InvalidArgument));
}

#[test]
fn the_whole_keymap_of_a_layer_arrives_across_pages() {
    let mut c = client();
    for source in (10..90u8).rev() {
        c.set_key(KeymapEntry { layer: 2, source, action: KeyAction::Key(source) }).unwrap();
    }
    c.set_key(KeymapEntry { layer: 1, source: 3, action: KeyAction::Disabled }).unwrap();
    let all = c.keymap_all(2).unwrap();
    assert_eq!(all.iter().map(|entry| entry.source).collect::<std::vec::Vec<_>>(), (10..90).collect::<std::vec::Vec<_>>());
    assert_eq!(c.keymap_all(1).unwrap().len(), 1);
    assert!(c.keymap_all(0).unwrap().is_empty());
    assert_eq!(c.keymap_all(LAYERS), Err(ClientError::Case(CaseError::InvalidArgument)));
}

#[test]
fn a_transport_failure_is_distinct_from_a_device_error() {
    struct Dead;
    impl Transport for Dead {
        type Error = &'static str;
        fn request(
            &mut self,
            _key: postcard_rpc::Key,
            _response: postcard_rpc::Key,
            _payload: &[u8],
        ) -> Result<std::vec::Vec<u8>, &'static str> {
            Err("cable unplugged")
        }
    }
    let mut c = Client::new(Dead);
    assert_eq!(c.device_info(), Err(ClientError::Transport("cable unplugged")));
}

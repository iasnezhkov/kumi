// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use std::collections::VecDeque;

use ed25519_dalek::SigningKey;
use heapless::String;
use kumi_client::events::Event;
use kumi_client::update::UpdateError;
use kumi_client::wire::{FrameError, FrameLink, FrameTransport};
use kumi_client::{Client, ClientError};
use kumi_control::Controls;
use kumi_icd::*;
use kumi_service::{Provider, serve_frame};
use kumi_settings::StoredSettings;
use postcard_rpc::header::{VarHeader, VarKey, VarSeq};
use postcard_rpc::standard_icd::{ERROR_KEY, WireError};
use postcard_rpc::{Endpoint, Key, Topic};

#[derive(Default)]
struct Case {
    received: Vec<u8>,
    manifest: Option<UpdateManifest>,
    booted: Option<Slot>,
}

impl Provider for Case {
    fn device_info(&mut self) -> DeviceInfo {
        DeviceInfo {
            protocol: PROTOCOL_VERSION,
            firmware: Version { major: 0, minor: 1, patch: 0 },
            build: String::try_from("wire").unwrap(),
            chip_id: 0x4901_4480,
            flash_jedec: [0xC8, 0x65, 0x16],
            serial: Some([1; 8]),
            running_from_ram: false,
            capabilities: Capabilities::FIRMWARE_UPDATE,
        }
    }
    fn audio_status(&mut self) -> AudioStatus {
        AudioStatus {
            jack: Jack::Empty,
            usb_audio_present: false,
            stream: None,
            host_volume_centi_db: [0, 0],
            host_muted: false,
            eq_active: None,
            counters: AudioCounters::default(),
        }
    }
    fn keyboard_status(&mut self) -> KeyboardStatus {
        KeyboardStatus::default()
    }
    fn slots(&mut self) -> SlotTable {
        SlotTable {
            booted: self.booted,
            slots: [
                SlotInfo {
                    slot: Slot::A,
                    state: SlotState::Confirmed,
                    version: Some(Version { major: 0, minor: 1, patch: 0 }),
                    size: 0,
                },
                SlotInfo { slot: Slot::B, state: SlotState::Empty, version: None, size: 0 },
            ],
        }
    }
    fn save(&mut self, _settings: &StoredSettings) -> Ack {
        Ok(())
    }
    fn reboot(&mut self, _target: RebootTarget) -> Ack {
        Ok(())
    }
    fn confirm_boot(&mut self) -> Ack {
        Ok(())
    }
    fn update_begin(&mut self, manifest: UpdateManifest) -> Ack {
        if manifest.slot != Slot::B {
            return Err(CaseError::WrongState);
        }
        self.manifest = Some(manifest);
        self.received.clear();
        Ok(())
    }
    fn update_chunk(&mut self, block: UpdateBlock) -> CountResult {
        if block.offset as usize != self.received.len() {
            return Err(CaseError::InvalidArgument);
        }
        self.received.extend_from_slice(&block.data);
        Ok(self.received.len() as u32)
    }
    fn update_finish(&mut self) -> SlotResult {
        let manifest = self.manifest.ok_or(CaseError::WrongState)?;
        if self.received.len() as u32 != manifest.size || kumi_install::sha256(&self.received) != manifest.sha256 {
            return Err(CaseError::VerifyFailed);
        }
        Ok(SlotInfo { slot: manifest.slot, state: SlotState::Trial, version: Some(manifest.version), size: manifest.size })
    }
}

struct Usb {
    controls: Controls,
    case: Case,
    inbox: VecDeque<Vec<u8>>,
    noise_before_each_reply: usize,
    stale_before_each_reply: bool,
    unplugged: bool,
}

fn encoded<T: serde::Serialize>(key: Key, seq: VarSeq, value: &T) -> Vec<u8> {
    let mut buf = [0u8; 256];
    let used = frame::encode(key, seq, value, &mut buf).unwrap();
    buf[..used].to_vec()
}

fn log_line(uptime_ms: u32) -> LogLine {
    LogLine { uptime_ms, level: LogLevel::Info, module: String::try_from("t").unwrap(), text: String::new() }
}

fn topic_frame(seq: u8) -> Vec<u8> {
    encoded(LogTopic::TOPIC_KEY, VarSeq::Seq1(seq), &log_line(5))
}

impl FrameLink for Usb {
    type Error = &'static str;

    fn send(&mut self, frame: &[u8]) -> Result<(), &'static str> {
        if self.unplugged {
            return Err("unplugged");
        }
        for i in 0..self.noise_before_each_reply {
            self.inbox.push_back(topic_frame(i as u8));
        }
        if self.stale_before_each_reply
            && let Some((header, _)) = frame::decode(frame)
            && let VarSeq::Seq4(seq) = header.seq_no
        {
            let old = VarSeq::Seq4(seq.wrapping_sub(1));
            self.inbox.push_back(encoded(GetMic::RESP_KEY, old, &Ok::<_, CaseError>(MicSettings { gain_centi_db: 1 })));
            self.inbox.push_back(encoded(ERROR_KEY, old, &WireError::UnknownKey));
            self.inbox.push_back(vec![0xFF]);
        }
        let mut out = [0u8; 1024];
        if let Some(used) = serve_frame(&mut self.controls, &mut self.case, frame, &mut out) {
            self.inbox.push_back(out[..used].to_vec());
        }
        Ok(())
    }

    fn receive(&mut self) -> Result<Vec<u8>, &'static str> {
        self.inbox.pop_front().ok_or("timeout")
    }

    fn try_receive(&mut self) -> Result<Option<Vec<u8>>, &'static str> {
        if self.unplugged {
            return Err("unplugged");
        }
        Ok(self.inbox.pop_front())
    }
}

fn client(noise: usize) -> Client<FrameTransport<Usb>> {
    Client::new(FrameTransport::new(Usb {
        controls: Controls::new(StoredSettings::default()),
        case: Case { booted: Some(Slot::A), ..Case::default() },
        inbox: VecDeque::new(),
        noise_before_each_reply: noise,
        stale_before_each_reply: false,
        unplugged: false,
    }))
}

#[test]
fn calls_cross_the_frame_layer_and_topics_are_kept_aside() {
    let mut c = client(3);
    assert_eq!(c.device_info().unwrap().build.as_str(), "wire");
    c.set_mic(MicSettings { gain_centi_db: 600 }).unwrap();
    assert_eq!(c.mic().unwrap().gain_centi_db, 600);
    let events = c.transport_mut().take_events();
    assert_eq!(events, vec![Event::Log(log_line(5)); 9]);
    assert_eq!(c.transport_mut().state().discarded_frames, 0);
}

#[test]
fn late_replies_and_errors_of_earlier_requests_are_discarded_not_kept_as_events() {
    let mut c = client(1);
    c.transport_mut().link_mut().stale_before_each_reply = true;
    assert_eq!(c.mic().unwrap().gain_centi_db, 0);
    assert_eq!(c.device_info().unwrap().build.as_str(), "wire");
    assert_eq!(c.transport_mut().take_events().len(), 2);
    assert_eq!(c.transport_mut().state().discarded_frames, 6);
}

#[test]
fn events_arrive_without_a_request_and_in_order() {
    let mut c = client(0);
    let key = KeyEvent { source: 4, pressed: true, layer: 0, action: KeyAction::Key(4) };
    let inbox = &mut c.transport_mut().link_mut().inbox;
    inbox.push_back(encoded(KeyEventTopic::TOPIC_KEY, frame::TOPIC_SEQUENCE, &key));
    inbox.push_back(encoded(GetMic::RESP_KEY, VarSeq::Seq4(99), &Ok::<_, CaseError>(MicSettings { gain_centi_db: 1 })));
    inbox.push_back(encoded(LogTopic::TOPIC_KEY, frame::TOPIC_SEQUENCE, &log_line(9)));
    inbox.push_back(encoded(KeyEventTopic::TOPIC_KEY, frame::TOPIC_SEQUENCE, &[0xFFu8; 3]));
    assert_eq!(c.events(2).unwrap(), [Event::Key(key)]);
    assert_eq!(c.events(8).unwrap(), [Event::Log(log_line(9))]);
    assert_eq!(c.events(8).unwrap(), []);
    let state = c.transport_mut().state();
    assert_eq!((state.discarded_frames, state.dropped_events), (1, 1));

    c.transport_mut().link_mut().inbox.push_back(topic_frame(0));
    c.mic().unwrap();
    c.transport_mut().link_mut().unplugged = true;
    assert_eq!(c.events(1), Err(ClientError::Transport(FrameError::Link("unplugged"))));
    c.transport_mut().link_mut().unplugged = false;
    assert_eq!(c.events(1).unwrap(), [Event::Log(log_line(5))], "events received before a link error are kept");
}

#[test]
fn a_wire_error_for_our_request_is_reported_as_such() {
    struct Gone;
    impl Endpoint for Gone {
        type Request = ();
        type Response = ();
        const PATH: &'static str = "gone";
        const REQ_KEY: Key = Key::for_path::<()>("gone");
        const RESP_KEY: Key = Key::for_path::<()>("gone/reply");
    }
    let mut c = client(2);
    assert_eq!(c.call::<Gone>(&()), Err(ClientError::Transport(FrameError::Wire(WireError::UnknownKey))));
    assert_eq!(c.ping(5), Ok(5));
}

#[test]
fn event_paths_are_the_topic_paths() {
    let events = [
        (Event::Audio(Case::default().audio_status()), AudioStatusTopic::PATH),
        (Event::Key(KeyEvent { source: 1, pressed: false, layer: 2, action: KeyAction::Disabled }), KeyEventTopic::PATH),
        (Event::Log(log_line(1)), LogTopic::PATH),
        (Event::Uart(ByteBlock { bytes: heapless::Vec::from_slice(&[1, 2]).unwrap() }), UartMirrorTopic::PATH),
    ];
    for (event, path) in events {
        assert_eq!(event.path(), path);
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["path"], path);
        assert_eq!(serde_json::from_value::<Event>(json).unwrap(), event);
    }
}

#[test]
fn an_unknown_endpoint_and_a_dead_link_are_reported_distinctly() {
    let mut c = client(0);
    c.transport_mut().link_mut().unplugged = true;
    assert_eq!(c.device_info(), Err(ClientError::Transport(FrameError::Link("unplugged"))));

    let mut c = client(0);
    let mut buf = [0u8; 32];
    let header = VarHeader { key: VarKey::Key8(Key::for_path::<()>("gone")), seq_no: VarSeq::Seq4(1) };
    let used = header.write_to_slice(&mut buf).unwrap().0.len();
    let mut out = [0u8; 64];
    let link = c.transport_mut().link_mut();
    let n = serve_frame(&mut link.controls, &mut link.case, &buf[..used], &mut out).unwrap();
    let (_, body) = VarHeader::take_from_slice(&out[..n]).unwrap();
    assert_eq!(postcard::from_bytes::<WireError>(body).unwrap(), WireError::UnknownKey);
}

#[test]
fn an_update_streams_the_image_into_the_inactive_slot() {
    let signer = SigningKey::from_bytes(&[8; 32]);
    let image = kumi_install::sample::image(&signer, [0, 2, 0], 2);
    let mut c = client(1);
    let mut last = 0;
    let slot = c.install_update(&image, |done, total| {
        assert!(done > last && done <= total);
        last = done;
    });
    let slot = slot.unwrap();
    assert_eq!(slot.slot, Slot::B);
    assert_eq!(slot.version, Some(Version { major: 0, minor: 2, patch: 0 }));
    assert_eq!(last as usize, image.len());
    assert_eq!(c.transport_mut().link_mut().case.received, image);

    let mut truncated = image.clone();
    truncated.pop();
    assert_eq!(c.install_update(&truncated, |_, _| {}), Err(UpdateError::Image));

    let mut c = client(0);
    c.transport_mut().link_mut().case.booted = None;
    assert_eq!(c.install_update(&image, |_, _| {}), Err(UpdateError::NoFreeSlot));
}

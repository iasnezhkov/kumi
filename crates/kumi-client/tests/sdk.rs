// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
use kumi_client::kumi_icd::*;
use kumi_client::virtual_case::{CHIP_ID, CHIP_ID_ADDRESS, VirtualCase, VirtualError};
use kumi_client::wire::{FrameError, FrameTransport};
use kumi_client::{Client, ClientError, Event, PROTOCOL_VERSION};
use postcard_rpc::standard_icd::WireError;

fn case() -> Client<FrameTransport<VirtualCase>> {
    Client::over(VirtualCase::new())
}

#[test]
fn the_virtual_case_describes_itself_like_the_ram_loaded_runtime() {
    let mut c = case();
    let info = c.device_info().unwrap();
    assert_eq!(info.protocol, PROTOCOL_VERSION);
    assert_eq!(info.chip_id, CHIP_ID);
    assert!(info.running_from_ram);
    assert!(info.capabilities.contains(Capabilities::KEYMAP | Capabilities::LAYERS | Capabilities::DEVELOPER));
    assert!(!info.capabilities.contains(Capabilities::PERSISTENT_SETTINGS));
    assert!(!info.capabilities.contains(Capabilities::AUDIO_OUT));
    assert_eq!(c.slots().unwrap().booted, None);
    assert_eq!(c.ping(41), Ok(41));
    let diagnostics = c.diagnostics().unwrap();
    assert_eq!(diagnostics.stage, stage::FW_RUNNING);
    assert_eq!(diagnostics.last_crash, None);
    assert!(c.save_settings().unwrap_err().is_unsupported());
    let output = c.output().unwrap();
    assert!(c.set_output(output).unwrap_err().is_unsupported());
    assert!(c.audio_bench_status().unwrap_err().is_unsupported());
    assert!(c.reboot(RebootTarget::OtherSlot).unwrap_err().is_unsupported());
}

#[test]
fn a_remapped_key_reaches_the_key_tester_with_its_new_action() {
    let mut c = case();
    c.set_key(KeymapEntry { layer: 0, source: 0x04, action: KeyAction::Key(0x05) }).unwrap();
    c.set_key(KeymapEntry { layer: 1, source: 0x39, action: KeyAction::Disabled }).unwrap();
    assert_eq!(c.keymap_all(0).unwrap(), [KeymapEntry { layer: 0, source: 0x04, action: KeyAction::Key(0x05) }]);
    assert_eq!(c.keymap_layers().unwrap().len(), 2);

    assert_eq!(c.subscribe(EventMask::KEYS | EventMask::LOG | EventMask::AUDIO), Ok(EventMask::KEYS | EventMask::LOG));
    let started = c.take_events();
    assert!(matches!(started.as_slice(), [Event::Log(line)] if line.module.as_str() == "runtime"), "{started:?}");

    c.link_mut().tap(0x04);
    let events = c.events(8).unwrap();
    let actions: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Key(key) => Some((key.source, key.pressed, key.action)),
            _ => None,
        })
        .collect();
    assert_eq!(actions, [(0x04, true, KeyAction::Key(0x05)), (0x04, false, KeyAction::Key(0x05))]);
    assert_eq!(c.keyboard_status().unwrap().frames, 2);

    c.unmap_key(0, 0x04).unwrap();
    c.link_mut().tap(0x04);
    assert!(c.events(8).unwrap().iter().any(|event| matches!(event, Event::Key(key) if key.action == KeyAction::Key(0x04))));
    c.clear_keymap().unwrap();
    assert!(c.keymap_layers().unwrap().is_empty());
    assert_eq!(c.keymap_all(LAYERS), Err(ClientError::Case(CaseError::InvalidArgument)));
}

#[test]
fn developer_requests_need_an_unlock_that_runs_out() {
    let mut c = case();
    assert!(c.dev_peek(CHIP_ID_ADDRESS, 1).unwrap_err().is_not_unlocked());
    assert_eq!(c.subscribe(EventMask::UART), Ok(EventMask::NONE));
    c.dev_unlock(2).unwrap();
    assert_eq!(c.subscribe(EventMask::UART), Ok(EventMask::UART));
    assert_eq!(c.dev_peek(CHIP_ID_ADDRESS, 2).unwrap().words.as_slice(), [CHIP_ID, 0]);
    assert_eq!(c.dev_peek(CHIP_ID_ADDRESS + 2, 1), Err(ClientError::Case(CaseError::InvalidArgument)));
    assert_eq!(c.dev_poke(0x2011_0120, 0xA5, 0xFF), Ok(0xA5));
    assert_eq!(c.dev_poke(0x2011_0120, 0x0100, 0xFF00), Ok(0x01A5));

    c.link_mut().add_i2c_device(0, 0x33);
    let probe = c.dev_i2c(0, 0x31, &[], 0).unwrap();
    assert!(!probe.acked);
    assert!(c.dev_i2c(0, 0x33, &[0x10, 0xAB, 0xCD], 0).unwrap().acked);
    assert_eq!(c.dev_i2c(0, 0x33, &[0x10], 2).unwrap().data.as_slice(), [0xAB, 0xCD]);
    assert_eq!(c.dev_i2c(0, 0x80, &[], 0), Err(ClientError::Case(CaseError::InvalidArgument)));
    assert_eq!(c.dev_i2c(2, 0x33, &[], 0), Err(ClientError::Case(CaseError::InvalidArgument)));
    assert_eq!(c.dev_i2c(0, 0x33, &[0; BYTE_BLOCK + 1], 0), Err(ClientError::Encode));

    c.link_mut().advance(2_000);
    assert!(c.dev_peek(CHIP_ID_ADDRESS, 1).unwrap_err().is_not_unlocked());
}

#[test]
fn a_fault_restarts_the_case_and_leaves_a_crash_record() {
    let mut c = case();
    c.set_key(KeymapEntry { layer: 0, source: 0x04, action: KeyAction::Key(0x05) }).unwrap();
    assert!(c.dev_fault(FaultKind::Breakpoint).unwrap_err().is_not_unlocked());
    c.dev_unlock(60).unwrap();
    c.dev_fault(FaultKind::IllegalInstruction).unwrap();
    let diagnostics = c.diagnostics().unwrap();
    assert_eq!(diagnostics.boot_count, 2);
    let crash = diagnostics.last_crash.unwrap();
    assert_eq!((crash.reason, crash.mcause, crash.boot_count), (CrashReason::Trap, 2, 1));
    assert!(c.keymap_layers().unwrap().is_empty(), "a RAM-loaded runtime loses its settings when it restarts");
    assert!(c.dev_peek(CHIP_ID_ADDRESS, 1).unwrap_err().is_not_unlocked());
    c.reboot(RebootTarget::BootRom).unwrap();
    assert_eq!(c.link_mut().reboots(), [RebootTarget::BootRom]);
    assert_eq!(c.link_mut().boot_count(), 3);
}

#[test]
fn errors_print_and_classify() {
    let unsupported: ClientError<FrameError<std::io::Error>> = ClientError::Case(CaseError::Unsupported);
    assert_eq!(unsupported.to_string(), "the case answered Unsupported");
    assert!(unsupported.is_unsupported() && !unsupported.is_unknown_endpoint() && !unsupported.is_link());
    let unknown: ClientError<FrameError<std::io::Error>> = ClientError::Transport(FrameError::Wire(WireError::UnknownKey));
    assert!(unknown.is_unknown_endpoint());
    assert_eq!(unknown.to_string(), "transport: the case refused the frame: UnknownKey");
    let lost: ClientError<FrameError<std::io::Error>> =
        ClientError::Transport(FrameError::Link(std::io::Error::other("cable unplugged")));
    assert!(lost.is_link());
    assert_eq!(std::error::Error::source(&lost).map(|e| e.to_string()), Some("link: cable unplugged".into()));
    let boxed: Box<dyn std::error::Error> = Box::new(lost);
    assert_eq!(boxed.to_string(), "transport: link: cable unplugged");
    assert_eq!(ClientError::<FrameError<std::io::Error>>::Decode.case_error(), None);
}

#[test]
fn a_boxed_link_works_as_a_transport() {
    struct Io(VirtualCase);
    impl kumi_client::wire::FrameLink for Io {
        type Error = std::io::Error;
        fn send(&mut self, frame: &[u8]) -> std::io::Result<()> {
            Ok(self.0.send(frame)?)
        }
        fn receive(&mut self) -> std::io::Result<Vec<u8>> {
            Ok(self.0.receive()?)
        }
    }
    let link: Box<dyn kumi_client::wire::FrameLink<Error = std::io::Error>> = Box::new(Io(VirtualCase::new()));
    let mut c = Client::over(link);
    assert_eq!(c.device_info().unwrap().build.as_str(), "virtual");
    let mut empty = VirtualCase::new();
    assert_eq!(kumi_client::wire::FrameLink::receive(&mut empty), Err(VirtualError::NoFrame));
}

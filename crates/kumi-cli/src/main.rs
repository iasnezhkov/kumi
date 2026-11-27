// SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
mod audio;
mod brom;
mod emulator;

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use emulator::ChildLink;
use kumi_client::bridge::BridgeLink;
use kumi_client::events::Event;
use kumi_client::usb::{UsbLink, device_mode, parse_selector, speed_name};
use kumi_client::wire::{FrameError, FrameLink, FrameTransport};
use kumi_client::{Client, ClientError};
use kumi_icd::{
    COMBO_SLOTS, CaseError, CaseHotkey, ComboSlotUpdate, CrashReason, DevFault, DevFlashTest, DevI2c, DevPeek, DevPoke,
    DevUnlock, DevUnlockRequest, EventMask, FaultKind, I2cRequest, KeyAction, KeyCombo, KeyConfig, KeymapEntry, MACRO_MAX_STEPS,
    MACRO_SLOTS, MAX_UNLOCK_SECONDS, MacroProgram, MacroSlotUpdate, MacroStep, MouseAction, PeekRequest, PokeRequest,
    RebootTarget, WORD_BLOCK,
};
use serde_json::{Value, json};

const USAGE: &str =
    "usage: kumi (--usb [--usb-device BUS:ADDR] | --emulator CONFIG [--server PATH] | --adb SERIAL --helper PATH --usb-path PATH [--deadline SECONDS]) COMMAND
commands:
  info | ping | status | diagnostics | telemetry | meters | slots | keys [LAYER] | save | confirm
  list                with --usb: list attached Kumi (1209:0001), boot ROM (0666:0888) and stock (0661:0881) USB devices
  keys set LAYER SOURCE ACTION
                      remap one key: ACTION is key:USAGE, mods:MASK, combo:USAGE+MASK, consumer:CODE,
                      hold:LAYER, toggle:LAYER, taphold-mod:TAP+MASK, taphold-layer:TAP+LAYER, oneshot:MASK,
                      macro:SLOT, capsword, mouse:up|down|left|right|lclick|rclick|mclick|wheel-up|wheel-down,
                      case:eq|filter|crossfeed|limiter|mono|mute|lineout|loudness|capsword, off or pass
  keys clear          remove every remapping
  keys config [set [--tapping-term MS] [--one-shot-timeout MS] [--combo-term MS]]
                      read or update Tap-Hold (80..1000 ms), One-Shot (250..10000 ms) and Combo (15..200 ms) timing
  keys macro [SLOT] | keys macro set SLOT [USAGE[+MODS] ...] | keys macro clear SLOT
                      inspect or program hardware macros (slots 0..7, up to 16 steps each)
  keys combo [SLOT] | keys combo set SLOT KEY_A KEY_B ACTION | keys combo clear SLOT
                      inspect or program simultaneous two-key chords (slots 0..7)
  output              show DAC filter, volume limit, balance and DSD direct settings
  output set [--filter fast-ll|fast-pc|slow-ll|slow-pc|nos] [--limit DB] [--balance DB] [--dsd-direct on|off]
  dac-extras          show CS43198 HPF, de-emphasis, line-out, Class-H power mode and auto-UAC on jack
  dac-extras set [--hpf on|off] [--deemp on|off] [--line-out on|off] [--class-h adaptive-hv|low-power|fixed-hv] [--auto-uac on|off]
  dsp                 show audiophile DSP settings (crossfeed, soft limiter, mono, phase invert, loudness, channel swap)
  dsp set [--crossfeed off|subtle|natural|strong] [--limiter on|off] [--mono on|off] [--invert-phase on|off] [--loudness on|off] [--swap-channels on|off]
  dsp-extras          show Auto-level (LUFS), subsonic filter and mono-bass settings
  dsp-extras set [--auto-level on|off] [--target-lufs DB] [--subsonic HZ] [--mono-bass HZ]
  spectrum            show live 24-band spectrum, true-peak and LUFS measurements
  audio test-tone [left|right|both|phase|stop] [HZ] [MS]
                      play a safe -20 dBFS channel/phase check tone (48 kHz, auto-stops after MS)
  eq [SLOT]           show active EQ selection and saved EQ profile(s) (slots 0..3)
  eq set SLOT PREAMP_DB [KIND:FREQ_HZ:Q:GAIN_DB ...]
                      store up to 10 parametric bands (KIND: peak|lowshelf|highshelf|lowpass|highpass)
  eq select SLOT|off  select active EQ slot 0..3 or bypass (off)
  reboot [normal|other|brom]
  update IMAGE
  events [SECONDS]    with SECONDS: subscribe to keys and log and print events as JSON lines;
                      without (emulator only): inject two key reports and list the event stream
  uart-mirror SECONDS subscribe to the keyboard UART mirror and print the bytes as JSON lines
  dev unlock SECONDS  developer mode for 1..600 seconds
  dev fault illegal|breakpoint|panic
                      make the firmware fault on purpose; the case resets and records the crash
  dev flash-test      the firmware's own flash write test in its scratch window
  dev peek ADDRESS [WORDS]
                      read 1..32 words from RAM, the PLIC, peripherals or the flash window (not USB or UART)
  dev poke ADDRESS VALUE [MASK]
                      write a peripheral register (RAM-loaded or lab runtime), print the value read back
  dev i2c BUS ADDRESS [WRITE-HEX|-] [READ]
                      one transaction on I2C controller 0 or 1: probe, write, or write up to 4 bytes then read
  dev uart-send HEX [LISTEN_MS]
                      lab runtime only: transmit 1..64 raw bytes on the keyboard UART TX pin, optionally listening for RX
  audio dac-probe     M4 step 2 from a RAM-loaded runtime: unlock, DAC power-on, I2C0 setup and its checks, the
                      DAC reset pulse, both CS43198 identities and the 0x32 NACK, volumes or a bus scan, then quiet
  audio calibrate     M4 step 1 in a RAM-loaded runtime: clock registers, CPU clock decoded and measured against USB
                      frames, timer quiesce, the audio PLL trim sweeps with every raw count (blocks the runtime ~0.1 s)
  audio silence RATE  M4 step 3: start I2S and DMA at RATE from a ring of zeros, DAC muted, output disabled
  audio tone RATE [left|right|both] [TONE_HZ] [LEVEL_DB]
                      M4 step 4: the same with a sine (default both, 1000 Hz, -20 dBFS), output enabled, DAC at -40 dB
  audio cursor SECONDS
                      sample the DMA cursor every 10 ms, print each sample as a JSON line, then frames per second and wraps
  audio status | audio stop
                      the bench state; stop halts the DMA, mutes the DAC and leaves the board quiet
  audio impedance     measure CS43198 headphone DC load impedance (HPLOAD) on left and right channels, then quiet
  audio telemetry     read live CPU clock, DC-DC/LDO/DVDD, analog rail, GPIO5/17, LRADC and EFUSE state
  brom identify | brom backup PATH | brom install PACKAGE BACKUP SHA256 KEY-HEX [FROM]
  brom restore BACKUP SHA256 [stock|exact] [FROM] | brom update-stage0 STAGE0 KEY-HEX
  brom run BIN LOAD ENTRY [REPORT LEN] | brom run-elf ELF | brom prefer a|b | brom reboot
                      works over --usb directly or --adb with --usb-path naming the 0666:0888 service device
  exercise IMAGE      (emulator) settings, events, a signed update, trial boot, confirmation and rollback
  exercise-recovery   (emulator) runtime faults until safe mode and then the boot ROM, with RAM kept or lost
--usb opens Kumi 1209:0001 directly over libusb-1.0 (--usb-device BUS:ADDR when more than one is attached);
--adb runs `adb -s SERIAL shell -T timeout -s KILL SECONDS PATH kumi-bridge USB-PATH` (KUMI_ADB names another adb);
the emulator server path comes from --server or the \"server\" key of CONFIG
exit codes: 0 done, 1 error, 2 usage, 3 the firmware does not support or allow the request";

const STREAM_POLL_MS: u64 = 100;
const DEFAULT_DEADLINE: u32 = 900;

enum Link {
    Emulator(ChildLink),
    Adb(BridgeLink),
    Usb(UsbLink),
}

impl FrameLink for Link {
    type Error = io::Error;

    fn send(&mut self, frame: &[u8]) -> io::Result<()> {
        match self {
            Link::Emulator(link) => link.send(frame),
            Link::Adb(link) => link.send(frame),
            Link::Usb(link) => link.send(frame),
        }
    }

    fn receive(&mut self) -> io::Result<Vec<u8>> {
        match self {
            Link::Emulator(link) => link.receive(),
            Link::Adb(link) => link.receive(),
            Link::Usb(link) => link.receive(),
        }
    }

    fn try_receive(&mut self) -> io::Result<Option<Vec<u8>>> {
        match self {
            Link::Emulator(link) => link.try_receive(),
            Link::Adb(link) => link.try_receive(),
            Link::Usb(link) => link.try_receive(),
        }
    }
}

type Connection = Client<FrameTransport<Link>>;
type CallError = ClientError<FrameError<io::Error>>;

#[derive(Debug)]
struct Failure {
    code: i32,
    message: String,
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure { code: 1, message }
    }
}

impl From<&str> for Failure {
    fn from(message: &str) -> Self {
        Failure { code: 1, message: message.into() }
    }
}

fn refused(message: String) -> Failure {
    Failure { code: 3, message }
}

fn case_failure(path: &str, error: CallError) -> Failure {
    match error {
        ClientError::Case(CaseError::Unsupported) => {
            refused(format!("{path}: the firmware answers Unsupported (not implemented in this build)"))
        }
        ClientError::Case(CaseError::NotUnlocked) => {
            refused(format!("{path}: developer mode is locked; run `kumi dev unlock SECONDS` first"))
        }
        ClientError::Transport(FrameError::Wire(wire)) if format!("{wire:?}") == "UnknownKey" => {
            refused(format!("{path}: this firmware does not serve the endpoint"))
        }
        other => Failure::from(format!("{path}: {other:?}")),
    }
}

fn checked<T, E: core::fmt::Debug>(result: Result<T, E>) -> Result<T, String> {
    result.map_err(|error| format!("{error:?}"))
}

fn emulator(client: &mut Connection) -> Result<&mut ChildLink, String> {
    match client.transport_mut().link_mut() {
        Link::Emulator(link) => Ok(link),
        Link::Adb(_) | Link::Usb(_) => Err("this command needs --emulator".into()),
    }
}

fn emulator_command(client: &mut Connection, request: Value) -> Result<Value, String> {
    checked(emulator(client)?.command(request))
}

fn flash_writes(client: &mut Connection) -> Result<u64, String> {
    let report = emulator_command(client, json!({"op":"report"}))?;
    report["flash_writes"].as_u64().ok_or_else(|| "the emulator report has no flash write count".into())
}

fn writes_after_settling(client: &mut Connection, entry: KeymapEntry) -> Result<u64, String> {
    let before = flash_writes(client)?;
    checked(client.set_key(entry))?;
    let start = checked(client.diagnostics())?.uptime_ms;
    for _ in 0..8 {
        emulator_command(client, json!({"op":"idle", "ms": 500}))?;
        if checked(client.diagnostics())?.uptime_ms.wrapping_sub(start) > 3_000 {
            return Ok(flash_writes(client)? - before);
        }
    }
    Err("emulated time did not advance past the autosave delay".into())
}

fn exercise(client: &mut Connection, image: &Path) -> Result<Value, String> {
    let before = checked(client.device_info())?;
    checked(client.confirm_boot())?;
    let entry = KeymapEntry { layer: 0, source: 4, action: KeyAction::Key(5) };
    checked(client.set_key(entry))?;
    checked(client.save_settings())?;
    checked(client.reboot(RebootTarget::Normal))?;
    let saved = checked(client.keymap_all(0))?;
    if !saved.contains(&entry) {
        return Err("settings were lost across reboot".into());
    }
    let autosaved = writes_after_settling(client, KeymapEntry { layer: 0, source: 5, action: KeyAction::Key(6) })?;
    if autosaved == 0 {
        return Err("a change was not saved on its own".into());
    }
    let events = exercise_events(client)?;
    let image = checked(std::fs::read(image))?;
    let installed = checked(client.install_update(&image, |_, _| {}))?;
    checked(client.reboot(RebootTarget::Normal))?;
    let updated = checked(client.device_info())?;
    let trial = checked(client.slots())?;
    if trial.booted != Some(installed.slot) || Some(updated.firmware) != installed.version {
        return Err("updated slot was not selected".into());
    }
    checked(client.confirm_boot())?;
    checked(client.reboot(RebootTarget::OtherSlot))?;
    let restored = checked(client.device_info())?;
    if restored.firmware != before.firmware {
        return Err("rollback did not select the previous image".into());
    }
    let report = emulator_command(client, json!({"op":"report"}))?;
    Ok(
        json!({"before":before,"saved":saved,"events":events,"installed":installed,"trial":trial,"updated":updated,"restored":restored,"report":report}),
    )
}

fn exercise_events(client: &mut Connection) -> Result<Value, String> {
    let wanted = EventMask::KEYS | EventMask::LOG;
    let granted = checked(client.subscribe(wanted))?;
    if granted != wanted {
        return Err(format!("the runtime granted {granted:?} instead of {wanted:?}"));
    }
    emulator_command(client, json!({"op":"uart", "hex":"4b01640b04000004000000000008004b01640b0400000000000000000400"}))?;
    let diagnostics = checked(client.diagnostics())?;
    let mut events = Vec::new();
    let mut logs = Vec::new();
    for event in checked(client.events(0))? {
        match event {
            Event::Key(key) => events.push(key),
            Event::Log(line) => logs.push(line),
            other => return Err(format!("unexpected event {other:?}")),
        }
    }
    let stopped = checked(client.subscribe(EventMask::NONE))?;
    if !stopped.is_empty() || events.len() != 2 || !events[0].pressed || events[1].pressed || logs.is_empty() {
        return Err(format!("event stream incomplete: {events:?}, {logs:?}"));
    }
    Ok(json!({"events":events,"logs":logs,"diagnostics":diagnostics}))
}

fn exercise_recovery(client: &mut Connection) -> Result<Value, String> {
    checked(client.confirm_boot())?;
    let model = emulator_command(client, json!({"op":"report"}))?;
    let retained = model["retention"] == "kept";
    let mut reports = Vec::new();
    for (kind, mcause, safe_mode) in
        [("illegal", 2, false), ("breakpoint", 3, false), ("illegal", 2, true), ("breakpoint", 3, true)]
    {
        emulator_command(client, json!({"op":"fault", "kind":kind}))?;
        let diagnostics = checked(client.diagnostics())?;
        let crash_matches = match &diagnostics.last_crash {
            Some(crash) => retained && crash.reason == CrashReason::Trap && crash.mcause == mcause,
            None => !retained,
        };
        if !crash_matches || diagnostics.safe_mode != safe_mode {
            return Err(format!("invalid recovery state with RAM {}: {diagnostics:?}", model["retention"]));
        }
        if safe_mode && reports.iter().all(|report: &kumi_icd::Diagnostics| !report.safe_mode) {
            let written = writes_after_settling(client, KeymapEntry { layer: 0, source: 7, action: KeyAction::Key(8) })?;
            if written != 0 {
                return Err(format!("safe mode saved settings on its own ({written} flash writes)"));
            }
        }
        reports.push(diagnostics);
    }
    let final_state = emulator_command(client, json!({"op":"fault", "kind":"illegal"}))?;
    if final_state["mode"] != "bootrom" {
        return Err("five starts without reaching the health point did not enter boot ROM".into());
    }
    let report = emulator_command(client, json!({"op":"report"}))?;
    Ok(json!({"diagnostics": reports, "report": report}))
}

fn stream(client: &mut Connection, seconds: u64, mut each: impl FnMut(&Event) -> Option<Value>) -> Result<u64, Failure> {
    let started = Instant::now();
    let emulated = matches!(client.transport_mut().link_mut(), Link::Emulator(_));
    let rounds = seconds * 1000 / STREAM_POLL_MS;
    let mut printed = 0;
    let mut last_ping = Instant::now();
    for round in 0..=rounds {
        let events = if emulated {
            emulator_command(client, json!({"op":"idle", "ms": STREAM_POLL_MS}))?;
            checked(client.ping(round as u32))?;
            client.transport_mut().take_events()
        } else {
            if last_ping.elapsed() >= Duration::from_secs(1) {
                checked(client.ping(round as u32))?;
                last_ping = Instant::now();
            }
            let mut events = client.transport_mut().take_events();
            let deadline = Instant::now() + Duration::from_millis(STREAM_POLL_MS);
            while Instant::now() < deadline {
                events.extend(checked(client.events(8))?);
            }
            events
        };
        for event in &events {
            if let Some(mut line) = each(event) {
                line["t_ms"] = json!(started.elapsed().as_millis() as u64);
                println!("{line}");
                printed += 1;
            }
        }
    }
    Ok(printed)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn number(argument: Option<&str>, what: &str) -> Result<u32, Failure> {
    let parsed = argument.and_then(|text| match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse::<u32>().ok(),
    });
    parsed.ok_or_else(|| Failure { code: 2, message: format!("give {what} as a decimal or 0x-prefixed number") })
}

fn hex_bytes(argument: Option<&str>) -> Result<Vec<u8>, Failure> {
    let text = argument.unwrap_or("-");
    if text == "-" {
        return Ok(Vec::new());
    }
    let invalid = || Failure { code: 2, message: "give the bytes to write as hex digits, for example 0102, or -".into() };
    if !text.len().is_multiple_of(2) {
        return Err(invalid());
    }
    (0..text.len()).step_by(2).map(|at| u8::from_str_radix(&text[at..at + 2], 16).map_err(|_| invalid())).collect()
}

fn small(argument: Option<&str>, what: &str) -> Result<u8, Failure> {
    u8::try_from(number(argument, what)?).map_err(|_| Failure { code: 2, message: format!("{what} must fit in 0..255") })
}

fn key_action(argument: Option<&str>) -> Result<KeyAction, Failure> {
    let usage = || Failure {
        code: 2,
        message: "give the action as key:USAGE, mods:MASK, combo:USAGE+MASK, consumer:CODE, hold:LAYER, toggle:LAYER, \
                  taphold-mod:TAP+MASK, taphold-layer:TAP+LAYER, oneshot:MASK, macro:SLOT, capsword, \
                  mouse:up|down|left|right|lclick|rclick|mclick|wheel-up|wheel-down, \
                  case:eq|filter|crossfeed|limiter|mono|mute|lineout|loudness|capsword, off or pass"
            .into(),
    };
    let text = argument.ok_or_else(usage)?;
    let (kind, value) = text.split_once(':').unwrap_or((text, ""));
    let byte = |value: &str| small(Some(value), "the action value").map_err(|_| usage());
    Ok(match kind {
        "off" if value.is_empty() => KeyAction::Disabled,
        "pass" if value.is_empty() => KeyAction::Transparent,
        "capsword" if value.is_empty() => KeyAction::CapsWord,
        "key" => KeyAction::Key(byte(value)?),
        "mods" => KeyAction::Modifiers(byte(value)?),
        "combo" => {
            let (key, mods) = value.split_once('+').ok_or_else(usage)?;
            KeyAction::KeyWithModifiers { usage: byte(key)?, modifiers: byte(mods)? }
        }
        "consumer" => {
            let code = number(Some(value), "the consumer code").map_err(|_| usage())?;
            KeyAction::Consumer(u16::try_from(code).map_err(|_| usage())?)
        }
        "hold" => KeyAction::LayerHold(byte(value)?),
        "toggle" => KeyAction::LayerToggle(byte(value)?),
        "taphold-mod" => {
            let (tap, mods) = value.split_once('+').ok_or_else(usage)?;
            KeyAction::TapHoldMod { tap: byte(tap)?, hold_mods: byte(mods)? }
        }
        "taphold-layer" => {
            let (tap, layer) = value.split_once('+').ok_or_else(usage)?;
            KeyAction::TapHoldLayer { tap: byte(tap)?, hold_layer: byte(layer)? }
        }
        "oneshot" => KeyAction::OneShotMod(byte(value)?),
        "macro" => KeyAction::Macro(byte(value)?),
        "mouse" => KeyAction::Mouse(match value {
            "up" => MouseAction::MoveUp,
            "down" => MouseAction::MoveDown,
            "left" => MouseAction::MoveLeft,
            "right" => MouseAction::MoveRight,
            "lclick" => MouseAction::LeftClick,
            "rclick" => MouseAction::RightClick,
            "mclick" => MouseAction::MiddleClick,
            "wheel-up" => MouseAction::WheelUp,
            "wheel-down" => MouseAction::WheelDown,
            _ => return Err(usage()),
        }),
        "case" => KeyAction::CaseControl(match value {
            "eq" => CaseHotkey::CycleEq,
            "filter" => CaseHotkey::CycleFilter,
            "crossfeed" => CaseHotkey::CycleCrossfeed,
            "limiter" => CaseHotkey::ToggleLimiter,
            "mono" => CaseHotkey::ToggleMono,
            "mute" => CaseHotkey::ToggleMute,
            "lineout" => CaseHotkey::ToggleLineOut,
            "loudness" => CaseHotkey::ToggleLoudness,
            "capsword" => CaseHotkey::ToggleCapsWord,
            _ => return Err(usage()),
        }),
        _ => return Err(usage()),
    })
}

fn parse_macro_step(text: &str) -> Result<MacroStep, Failure> {
    let usage = || Failure { code: 2, message: "macro step must be USAGE or USAGE+MODS".into() };
    let (u, m) = match text.split_once('+') {
        Some((u, m)) => (u, Some(m)),
        None => (text, None),
    };
    let usage_code = small(Some(u), "macro key usage").map_err(|_| usage())?;
    let modifiers = match m {
        Some(raw) => small(Some(raw), "macro modifiers").map_err(|_| usage())?,
        None => 0,
    };
    let step = MacroStep { usage: usage_code, modifiers };
    if step.is_valid() { Ok(step) } else { Err(usage()) }
}

fn keys_config(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    match args.first().map(String::as_str) {
        None => Ok(json!(client.key_config().map_err(|e| case_failure("keys/config/get", e))?)),
        Some("set") => {
            let shape = "keys config set [--tapping-term MS] [--one-shot-timeout MS] [--combo-term MS]";
            if args.len() < 3 || !(args.len() - 1).is_multiple_of(2) {
                return Err(Failure { code: 2, message: shape.into() });
            }
            let mut next = client.key_config().map_err(|e| case_failure("keys/config/get", e))?;
            let mut i = 1;
            while i < args.len() {
                let value = number(args.get(i + 1).map(String::as_str), &args[i])?;
                let ms = u16::try_from(value).map_err(|_| Failure { code: 2, message: shape.into() })?;
                match args[i].as_str() {
                    "--tapping-term" => next.tapping_term_ms = ms,
                    "--one-shot-timeout" => next.one_shot_timeout_ms = ms,
                    "--combo-term" => next.combo_term_ms = ms,
                    _ => return Err(Failure { code: 2, message: shape.into() }),
                }
                i += 2;
            }
            if !next.is_valid() {
                return Err(Failure {
                    code: 2,
                    message: format!(
                        "tapping-term must be {}..{} ms, one-shot-timeout {}..{} ms and combo-term {}..{} ms",
                        KeyConfig::MIN_TAPPING_TERM_MS,
                        KeyConfig::MAX_TAPPING_TERM_MS,
                        KeyConfig::MIN_ONE_SHOT_MS,
                        KeyConfig::MAX_ONE_SHOT_MS,
                        KeyConfig::MIN_COMBO_TERM_MS,
                        KeyConfig::MAX_COMBO_TERM_MS
                    ),
                });
            }
            client.set_key_config(next).map_err(|e| case_failure("keys/config/set", e))?;
            Ok(json!(next))
        }
        _ => Err(Failure {
            code: 2,
            message: "keys config [set [--tapping-term MS] [--one-shot-timeout MS] [--combo-term MS]]".into(),
        }),
    }
}

fn keys_combo(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    let parse_slot = |raw: Option<&str>| -> Result<u8, Failure> {
        let slot = small(raw, "combo slot")?;
        if slot < COMBO_SLOTS { Ok(slot) } else { Err(Failure { code: 2, message: "combo slot must be 0..7".into() }) }
    };
    match args.first().map(String::as_str) {
        None => {
            let mut slots = Vec::new();
            for slot in 0..COMBO_SLOTS {
                let combo = client.combo_slot(slot).map_err(|e| case_failure("keys/combo/get", e))?;
                slots.push(json!({ "slot": slot, "combo": combo }));
            }
            Ok(json!({ "combos": slots }))
        }
        Some("set") => {
            if args.len() != 5 {
                return Err(Failure { code: 2, message: "keys combo set SLOT KEY_A KEY_B ACTION".into() });
            }
            let slot = parse_slot(args.get(1).map(String::as_str))?;
            let key_a = small(args.get(2).map(String::as_str), "key A")?;
            let key_b = small(args.get(3).map(String::as_str), "key B")?;
            let action = key_action(args.get(4).map(String::as_str))?;
            let update = ComboSlotUpdate { slot, combo: Some(KeyCombo { key_a, key_b, action }) };
            if !update.is_valid() {
                return Err(Failure {
                    code: 2,
                    message: "combo requires two distinct non-zero keys and a non-transparent action".into(),
                });
            }
            client.set_combo(update).map_err(|e| case_failure("keys/combo/set", e))?;
            Ok(json!(update))
        }
        Some("clear") => {
            let slot = parse_slot(args.get(1).map(String::as_str))?;
            let update = ComboSlotUpdate { slot, combo: None };
            client.set_combo(update).map_err(|e| case_failure("keys/combo/set", e))?;
            Ok(json!({ "slot": slot, "cleared": true }))
        }
        Some(slot_text) if args.len() == 1 => {
            let slot = parse_slot(Some(slot_text))?;
            let combo = client.combo_slot(slot).map_err(|e| case_failure("keys/combo/get", e))?;
            Ok(json!({ "slot": slot, "combo": combo }))
        }
        _ => Err(Failure {
            code: 2,
            message: "keys combo [SLOT] | keys combo set SLOT KEY_A KEY_B ACTION | keys combo clear SLOT".into(),
        }),
    }
}

fn keys_macro(client: &mut Connection, args: &[String]) -> Result<Value, Failure> {
    let parse_slot = |raw: Option<&str>| -> Result<u8, Failure> {
        let slot = small(raw, "macro slot")?;
        if slot < MACRO_SLOTS { Ok(slot) } else { Err(Failure { code: 2, message: "macro slot must be 0..7".into() }) }
    };
    match args.first().map(String::as_str) {
        None => {
            let mut slots = Vec::new();
            for slot in 0..MACRO_SLOTS {
                let program = client.macro_slot(slot).map_err(|e| case_failure("keys/macro/get", e))?;
                slots.push(json!({ "slot": slot, "program": program }));
            }
            Ok(json!({ "macros": slots }))
        }
        Some("set") => {
            let slot = parse_slot(args.get(1).map(String::as_str))?;
            if args.len() - 2 > MACRO_MAX_STEPS {
                return Err(Failure { code: 2, message: format!("at most {MACRO_MAX_STEPS} macro steps are allowed") });
            }
            let mut steps = heapless::Vec::new();
            for raw in &args[2..] {
                steps.push(parse_macro_step(raw)?).map_err(|_| Failure { code: 2, message: "too many steps".into() })?;
            }
            let update = MacroSlotUpdate { slot, program: MacroProgram { steps } };
            client.set_macro(update.clone()).map_err(|e| case_failure("keys/macro/set", e))?;
            Ok(json!(update))
        }
        Some("clear") => {
            let slot = parse_slot(args.get(1).map(String::as_str))?;
            let update = MacroSlotUpdate { slot, program: MacroProgram::default() };
            client.set_macro(update).map_err(|e| case_failure("keys/macro/set", e))?;
            Ok(json!({ "slot": slot, "cleared": true }))
        }
        Some(slot_text) if args.len() == 1 => {
            let slot = parse_slot(Some(slot_text))?;
            let program = client.macro_slot(slot).map_err(|e| case_failure("keys/macro/get", e))?;
            Ok(json!({ "slot": slot, "program": program }))
        }
        _ => Err(Failure {
            code: 2,
            message: "keys macro [SLOT] | keys macro set SLOT [USAGE[+MODS] ...] | keys macro clear SLOT".into(),
        }),
    }
}

fn hex_word(value: u32) -> String {
    format!("0x{value:08x}")
}

fn seconds(argument: Option<&str>, limit: u64) -> Result<u64, Failure> {
    match argument.map(str::parse::<u64>) {
        Some(Ok(value)) if (1..=limit).contains(&value) => Ok(value),
        _ => Err(Failure { code: 2, message: format!("give a duration of 1..{limit} seconds") }),
    }
}

fn events_stream(client: &mut Connection, seconds: u64) -> Result<Value, Failure> {
    let granted = client.subscribe(EventMask::KEYS | EventMask::LOG).map_err(|e| case_failure("events/subscribe", e))?;
    let mut keys = 0u64;
    let mut logs = 0u64;
    let printed = stream(client, seconds, |event| match event {
        Event::Key(_) | Event::Log(_) => {
            if matches!(event, Event::Key(_)) {
                keys += 1;
            } else {
                logs += 1;
            }
            Some(json!(event))
        }
        _ => None,
    })?;
    client.subscribe(EventMask::NONE).map_err(|e| case_failure("events/subscribe", e))?;
    Ok(json!({"seconds": seconds, "granted": granted, "key_events": keys, "log_lines": logs, "printed": printed}))
}

fn uart_mirror(client: &mut Connection, seconds: u64) -> Result<Value, Failure> {
    let granted = client.subscribe(EventMask::UART).map_err(|e| case_failure("events/subscribe", e))?;
    if !granted.contains(EventMask::UART) {
        let _ = client.subscribe(EventMask::NONE);
        return Err(refused(format!(
            "dev/uart/mirror: the firmware granted {granted:?} without the UART mirror; unlock developer mode with \
             `kumi dev unlock SECONDS` first, or this firmware has no mirror"
        )));
    }
    let mut bytes = 0u64;
    let printed = stream(client, seconds, |event| match event {
        Event::Uart(block) => {
            bytes += block.bytes.len() as u64;
            Some(json!({"uart": hex(&block.bytes)}))
        }
        _ => None,
    })?;
    client.subscribe(EventMask::NONE).map_err(|e| case_failure("events/subscribe", e))?;
    Ok(json!({"seconds": seconds, "granted": granted, "blocks": printed, "bytes": bytes}))
}

fn developer(client: &mut Connection, argument: impl Fn(usize) -> Option<String>) -> Result<Value, Failure> {
    match argument(1).as_deref() {
        Some("unlock") => {
            let seconds = seconds(argument(2).as_deref(), u64::from(MAX_UNLOCK_SECONDS))? as u16;
            client.call_case::<DevUnlock, _>(&DevUnlockRequest { seconds }).map_err(|e| case_failure("dev/unlock", e))?;
            Ok(json!({"unlocked_seconds": seconds}))
        }
        Some("fault") => {
            let (name, kind) = match argument(2).as_deref() {
                Some("illegal") => ("illegal", FaultKind::IllegalInstruction),
                Some("breakpoint") => ("breakpoint", FaultKind::Breakpoint),
                Some("panic") => ("panic", FaultKind::Panic),
                _ => return Err(Failure { code: 2, message: "dev fault illegal|breakpoint|panic".into() }),
            };
            let next = "the case resets; reconnect and run `kumi diagnostics` for the crash record";
            match client.call_case::<DevFault, _>(&kind) {
                Ok(()) => Ok(json!({"fault": name, "acknowledged": true, "next": next})),
                Err(ClientError::Transport(FrameError::Link(error))) => {
                    Ok(json!({"fault": name, "acknowledged": false, "link_closed": error.to_string(), "next": next}))
                }
                Err(error) => Err(case_failure("dev/fault", error)),
            }
        }
        Some("flash-test") => {
            let report = client.call_case::<DevFlashTest, _>(&()).map_err(|e| case_failure("dev/flash/test", e))?;
            Ok(json!(report))
        }
        Some("peek") => {
            let address = number(argument(2).as_deref(), "the address")?;
            let words = match argument(3) {
                Some(text) => number(Some(&text), "the word count")?,
                None => 1,
            };
            if !(1..=WORD_BLOCK as u32).contains(&words) {
                return Err(Failure { code: 2, message: format!("give 1..{WORD_BLOCK} words") });
            }
            let request = PeekRequest { address, words: words as u8 };
            let block = client.call_case::<DevPeek, _>(&request).map_err(|e| case_failure("dev/peek", e))?;
            Ok(json!({"address": hex_word(block.address), "words": block.words.iter().map(|w| hex_word(*w)).collect::<Vec<_>>()}))
        }
        Some("poke") => {
            let address = number(argument(2).as_deref(), "the address")?;
            let value = number(argument(3).as_deref(), "the value")?;
            let mask = match argument(4) {
                Some(text) => number(Some(&text), "the mask")?,
                None => u32::MAX,
            };
            let request = PokeRequest { address, value, mask };
            let back = client.call_case::<DevPoke, _>(&request).map_err(|e| case_failure("dev/poke", e))?;
            Ok(
                json!({"address": hex_word(address), "value": hex_word(value), "mask": hex_word(mask), "read_back": hex_word(back)}),
            )
        }
        Some("i2c") => {
            let bus = number(argument(2).as_deref(), "the bus")?;
            let address = number(argument(3).as_deref(), "the 7-bit address")?;
            let write = hex_bytes(argument(4).as_deref())?;
            let read = match argument(5) {
                Some(text) => number(Some(&text), "the read length")?,
                None => 0,
            };
            let usage = || Failure { code: 2, message: "dev i2c BUS(0|1) ADDRESS(0..0x7f) [WRITE-HEX|-] [READ 0..64]".into() };
            let bus = u8::try_from(bus).map_err(|_| usage())?;
            let address = u8::try_from(address).map_err(|_| usage())?;
            let read = u8::try_from(read).map_err(|_| usage())?;
            let write = heapless::Vec::from_slice(&write).map_err(|_| usage())?;
            let request = I2cRequest { bus, address, write, read };
            let reply = client.call_case::<DevI2c, _>(&request).map_err(|e| case_failure("dev/i2c", e))?;
            let data: String = reply.data.iter().map(|b| format!("{b:02x}")).collect();
            Ok(json!({"bus": bus, "address": format!("0x{address:02x}"), "acked": reply.acked, "data": data}))
        }
        Some("uart-send") => {
            let raw = hex_bytes(argument(2).as_deref())?;
            if raw.is_empty() || raw.len() > kumi_icd::BYTE_BLOCK {
                return Err(Failure { code: 2, message: "dev uart-send HEX [LISTEN_MS] (1..64 bytes)".into() });
            }
            let listen_ms = match argument(3) {
                Some(text) => {
                    let ms = number(Some(&text), "the listen window in milliseconds")?;
                    if !(1..=10_000).contains(&ms) {
                        return Err(Failure { code: 2, message: "give LISTEN_MS in 1..10000".into() });
                    }
                    Some(u64::from(ms))
                }
                None => None,
            };
            if let Some(ms) = listen_ms {
                let granted = client.subscribe(EventMask::UART).map_err(|e| case_failure("events/subscribe", e))?;
                if !granted.contains(EventMask::UART) {
                    let _ = client.subscribe(EventMask::NONE);
                    return Err(refused("dev/uart/mirror: unlock developer mode first".into()));
                }
                let _ = checked(client.events(32));
                client.dev_uart_send(&raw).map_err(|e| case_failure("dev/uart/send", e))?;
                let mut chunks = Vec::new();
                let mut rx_bytes = 0usize;
                let deadline = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < deadline {
                    for event in checked(client.events(8))? {
                        if let Event::Uart(block) = event {
                            rx_bytes += block.bytes.len();
                            chunks.push(hex(&block.bytes));
                        }
                    }
                }
                let _ = client.subscribe(EventMask::NONE);
                Ok(json!({"sent": raw.len(), "rx_bytes": rx_bytes, "rx": chunks}))
            } else {
                client.dev_uart_send(&raw).map_err(|e| case_failure("dev/uart/send", e))?;
                Ok(json!({"sent": raw.len()}))
            }
        }
        _ => Err(Failure { code: 2, message: USAGE.into() }),
    }
}

enum Transport {
    Emulator { config: PathBuf, server: Option<PathBuf> },
    Adb { serial: String, helper: String, usb_path: String, deadline: u32 },
    Usb { selector: Option<(u8, u8)> },
}

struct Options {
    transport: Transport,
    command: Vec<String>,
}

fn options(args: &[String]) -> Result<Options, String> {
    let mut config = None;
    let mut server = None;
    let mut serial = None;
    let mut helper = None;
    let mut usb_path = None;
    let mut usb = false;
    let mut usb_device = None;
    let mut deadline = DEFAULT_DEADLINE;
    let mut rest = args.iter().skip(1);
    let mut command = Vec::new();
    while let Some(arg) = rest.next() {
        match arg.as_str() {
            "--usb" if command.is_empty() => usb = true,
            "--usb-device" if command.is_empty() => {
                let raw = rest.next().ok_or_else(|| USAGE.to_string())?;
                usb_device = Some(parse_selector(raw).map_err(|e| e.to_string())?);
                usb = true;
            }
            "--emulator" if command.is_empty() => config = rest.next().map(PathBuf::from),
            "--server" if command.is_empty() => server = rest.next().map(PathBuf::from),
            "--adb" if command.is_empty() => serial = rest.next().cloned(),
            "--helper" if command.is_empty() => helper = rest.next().cloned(),
            "--usb-path" if command.is_empty() => usb_path = rest.next().cloned(),
            "--deadline" if command.is_empty() => {
                deadline = rest.next().and_then(|value| value.parse().ok()).ok_or_else(|| USAGE.to_string())?
            }
            "-h" | "--help" => return Err(USAGE.into()),
            _ => command.push(arg.clone()),
        }
    }
    let transport = match (usb, config, serial, helper, usb_path) {
        (true, None, None, None, None) if server.is_none() => Transport::Usb { selector: usb_device },
        (false, Some(config), None, None, None) => Transport::Emulator { config, server },
        (false, None, Some(serial), Some(helper), Some(usb_path)) if server.is_none() => {
            Transport::Adb { serial, helper, usb_path, deadline }
        }
        _ => return Err(USAGE.into()),
    };
    if command.is_empty() {
        return Err(USAGE.into());
    }
    Ok(Options { transport, command })
}

fn connect(transport: &Transport) -> Result<Link, String> {
    match transport {
        Transport::Emulator { config, server } => Ok(Link::Emulator(checked(ChildLink::spawn(config, server.as_deref()))?)),
        Transport::Adb { serial, helper, usb_path, deadline } => {
            Ok(Link::Adb(BridgeLink::adb(serial, helper, usb_path, *deadline).map_err(|error| format!("--adb: {error}"))?))
        }
        Transport::Usb { selector } => {
            Ok(Link::Usb(UsbLink::open_kumi_at(*selector).map_err(|error| format!("--usb: {error}"))?))
        }
    }
}

fn run(args: &[String]) -> Result<Value, Failure> {
    let options = options(args).map_err(|message| Failure { code: 2, message })?;
    if options.command[0] == "list" {
        return match &options.transport {
            Transport::Usb { selector } => {
                let devices = UsbLink::list_case_devices().map_err(|error| format!("--usb: {error}"))?;
                let items: Vec<Value> = devices
                    .into_iter()
                    .filter(|d| selector.is_none_or(|(bus, addr)| d.bus == bus && d.address == addr))
                    .map(|d| {
                        json!({
                            "bus": d.bus,
                            "address": d.address,
                            "selector": format!("{}:{}", d.bus, d.address),
                            "vendor": format!("0x{:04x}", d.vendor),
                            "product": format!("0x{:04x}", d.product),
                            "mode": device_mode(d.vendor, d.product).unwrap_or("unknown"),
                            "speed": speed_name(d.speed),
                        })
                    })
                    .collect();
                Ok(json!({ "devices": items }))
            }
            Transport::Emulator { .. } | Transport::Adb { .. } => Err(Failure { code: 2, message: "list requires --usb".into() }),
        };
    }
    if options.command[0] == "brom" {
        let argument = |index: usize| options.command.get(index).cloned();
        return match &options.transport {
            Transport::Adb { serial, helper, usb_path, deadline } => brom::run(serial, helper, usb_path, *deadline, argument),
            Transport::Usb { selector } => brom::run_usb(*selector, argument),
            Transport::Emulator { .. } => Err(Failure { code: 2, message: "brom commands need --usb or --adb".into() }),
        };
    }
    let mut client = Client::new(FrameTransport::new(connect(&options.transport)?));
    let argument = |index: usize| options.command.get(index).cloned();
    match options.command[0].as_str() {
        "info" => Ok(json!(checked(client.device_info())?)),
        "ping" => Ok(json!(checked(client.ping(0x4b55_4d49))?)),
        "status" => Ok(json!({
            "audio": checked(client.audio_status())?,
            "keyboard": checked(client.keyboard_status())?,
        })),
        "diagnostics" => Ok(json!(client.diagnostics().map_err(|e| case_failure("system/diagnostics", e))?)),
        "telemetry" => Ok(json!(client.telemetry().map_err(|e| case_failure("system/telemetry", e))?)),
        "meters" => Ok(json!(client.audio_meters().map_err(|e| case_failure("audio/meters", e))?)),
        "events" => match argument(1) {
            Some(value) => events_stream(&mut client, seconds(Some(&value), 3600)?),
            None => Ok(exercise_events(&mut client)?),
        },
        "uart-mirror" => uart_mirror(&mut client, seconds(argument(1).as_deref(), 3600)?),
        "dev" => developer(&mut client, argument),
        "output" => audio::output(&mut client, &options.command[1..]),
        "dac-extras" => audio::dac_extras(&mut client, &options.command[1..]),
        "dsp" => audio::dsp(&mut client, &options.command[1..]),
        "dsp-extras" => audio::dsp_extras(&mut client, &options.command[1..]),
        "spectrum" => Ok(json!(client.audio_spectrum().map_err(|e| case_failure("audio/spectrum", e))?)),
        "eq" => audio::eq(&mut client, &options.command[1..]),
        "audio" => match argument(1).as_deref() {
            Some("dac-probe") => audio::dac_probe(&mut client),
            Some("calibrate") => audio::calibrate(&mut client),
            Some(kind @ ("tone" | "silence")) => {
                let request = audio::parse_start(&options.command[2..], kind == "tone")?;
                audio::start(&mut client, request)
            }
            Some("test-tone") => audio::test_tone(&mut client, &options.command[2..]),
            Some("cursor") => audio::cursor(&mut client, seconds(argument(2).as_deref(), 600)?),
            Some("status") => audio::status(&mut client),
            Some("stop") => audio::stop(&mut client),
            Some("impedance") => audio::impedance(&mut client),
            Some("telemetry") => audio::telemetry(&mut client),
            _ => Err(Failure { code: 2, message: USAGE.into() }),
        },
        "exercise-recovery" => Ok(exercise_recovery(&mut client)?),
        "slots" => Ok(json!(checked(client.slots())?)),
        "keys" => match argument(1).as_deref() {
            Some("set") => {
                let layer = small(argument(2).as_deref(), "the layer")?;
                let source = small(argument(3).as_deref(), "the source usage")?;
                let action = key_action(argument(4).as_deref())?;
                let entry = KeymapEntry { layer, source, action };
                client.set_key(entry).map_err(|e| case_failure("keys/map/set", e))?;
                Ok(json!({"set": entry}))
            }
            Some("clear") => {
                client.clear_keymap().map_err(|e| case_failure("keys/map/clear", e))?;
                Ok(json!({"cleared": true}))
            }
            Some("config") => keys_config(&mut client, &options.command[2..]),
            Some("macro") => keys_macro(&mut client, &options.command[2..]),
            Some("combo") => keys_combo(&mut client, &options.command[2..]),
            other => {
                let layer = other.map_or(Ok(0), |value| value.parse()).map_err(|_| "invalid layer".to_string())?;
                Ok(json!(client.keymap_all(layer).map_err(|e| case_failure("keys/map/get", e))?))
            }
        },
        "save" => Ok(json!(checked(client.save_settings())?)),
        "confirm" => Ok(json!(checked(client.confirm_boot())?)),
        "reboot" => {
            let target = match argument(1).as_deref().unwrap_or("normal") {
                "normal" => RebootTarget::Normal,
                "other" => RebootTarget::OtherSlot,
                "brom" => RebootTarget::BootRom,
                _ => return Err("invalid reboot target".into()),
            };
            Ok(json!(checked(client.reboot(target))?))
        }
        "update" => {
            let image = checked(std::fs::read(argument(1).ok_or("missing image path")?))?;
            Ok(json!(checked(client.install_update(&image, |done, total| eprintln!("{done}/{total}")))?))
        }
        "exercise" => Ok(exercise(&mut client, Path::new(&argument(1).ok_or("missing image path")?))?),
        _ => Err(Failure { code: 2, message: USAGE.into() }),
    }
}

fn main() {
    match run(&std::env::args().collect::<Vec<_>>()) {
        Ok(value) => println!("{}", serde_json::to_string_pretty(&value).unwrap()),
        Err(failure) => {
            eprintln!("{}", failure.message);
            std::process::exit(failure.code);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_actions_parse_every_kind_and_refuse_the_rest() {
        assert_eq!(key_action(Some("key:0x05")).unwrap(), KeyAction::Key(5));
        assert_eq!(key_action(Some("mods:2")).unwrap(), KeyAction::Modifiers(2));
        assert_eq!(key_action(Some("combo:0x1d+0x01")).unwrap(), KeyAction::KeyWithModifiers { usage: 0x1d, modifiers: 1 });
        assert_eq!(key_action(Some("consumer:0xe9")).unwrap(), KeyAction::Consumer(0xe9));
        assert_eq!(key_action(Some("hold:1")).unwrap(), KeyAction::LayerHold(1));
        assert_eq!(key_action(Some("toggle:2")).unwrap(), KeyAction::LayerToggle(2));
        assert_eq!(key_action(Some("taphold-mod:0x29+1")).unwrap(), KeyAction::TapHoldMod { tap: 0x29, hold_mods: 1 });
        assert_eq!(key_action(Some("taphold-layer:0x2c+1")).unwrap(), KeyAction::TapHoldLayer { tap: 0x2c, hold_layer: 1 });
        assert_eq!(key_action(Some("oneshot:2")).unwrap(), KeyAction::OneShotMod(2));
        assert_eq!(key_action(Some("macro:3")).unwrap(), KeyAction::Macro(3));
        assert_eq!(key_action(Some("case:eq")).unwrap(), KeyAction::CaseControl(CaseHotkey::CycleEq));
        assert_eq!(key_action(Some("case:lineout")).unwrap(), KeyAction::CaseControl(CaseHotkey::ToggleLineOut));
        assert_eq!(key_action(Some("case:loudness")).unwrap(), KeyAction::CaseControl(CaseHotkey::ToggleLoudness));
        assert_eq!(key_action(Some("case:capsword")).unwrap(), KeyAction::CaseControl(CaseHotkey::ToggleCapsWord));
        assert_eq!(key_action(Some("capsword")).unwrap(), KeyAction::CapsWord);
        assert_eq!(key_action(Some("mouse:up")).unwrap(), KeyAction::Mouse(MouseAction::MoveUp));
        assert_eq!(key_action(Some("mouse:lclick")).unwrap(), KeyAction::Mouse(MouseAction::LeftClick));
        assert_eq!(key_action(Some("mouse:wheel-down")).unwrap(), KeyAction::Mouse(MouseAction::WheelDown));
        assert_eq!(key_action(Some("off")).unwrap(), KeyAction::Disabled);
        assert_eq!(key_action(Some("pass")).unwrap(), KeyAction::Transparent);
        for bad in [None, Some("key"), Some("key:256"), Some("combo:4"), Some("consumer:0x10000"), Some("off:1"), Some("jump:1")]
        {
            assert_eq!(key_action(bad).unwrap_err().code, 2, "{bad:?}");
        }
    }

    fn args(text: &str) -> Vec<String> {
        text.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn transports_are_chosen_from_the_options() {
        let emulator = options(&args("kumi --emulator e.json --server s.py info")).unwrap();
        assert!(matches!(emulator.transport, Transport::Emulator { ref server, .. } if server.is_some()));
        let adb = options(&args(
            "kumi --adb SER --helper /data/local/tmp/h --usb-path /dev/bus/usb/001/007 --deadline 60 dev unlock 30",
        ))
        .unwrap();
        match adb.transport {
            Transport::Adb { serial, helper, usb_path, deadline } => {
                assert_eq!(
                    (serial.as_str(), helper.as_str(), usb_path.as_str(), deadline),
                    ("SER", "/data/local/tmp/h", "/dev/bus/usb/001/007", 60)
                );
            }
            Transport::Emulator { .. } | Transport::Usb { .. } => panic!("expected adb"),
        }
        assert_eq!(adb.command, ["dev", "unlock", "30"]);
        let usb = options(&args("kumi --usb info")).unwrap();
        assert!(matches!(usb.transport, Transport::Usb { selector: None }));
        let usb_sel = options(&args("kumi --usb-device 1:12 diagnostics")).unwrap();
        assert!(matches!(usb_sel.transport, Transport::Usb { selector: Some((1, 12)) }));
        for bad in [
            "kumi info",
            "kumi --usb",
            "kumi --usb --emulator e.json info",
            "kumi --usb-device bad info",
            "kumi --adb SER --helper h info",
            "kumi --emulator e.json --adb SER --helper h --usb-path p info",
            "kumi --adb SER --helper h --usb-path p --server s info",
            "kumi --emulator e.json",
        ] {
            assert!(options(&args(bad)).is_err(), "{bad}");
        }
    }

    #[test]
    fn durations_are_bounded() {
        assert_eq!(seconds(Some("600"), 600).unwrap(), 600);
        for bad in [None, Some("0"), Some("601"), Some("x")] {
            assert_eq!(seconds(bad, 600).unwrap_err().code, 2);
        }
    }

    #[test]
    fn unsupported_and_locked_answers_exit_with_code_3() {
        assert_eq!(case_failure("dev/fault", ClientError::Case(CaseError::Unsupported)).code, 3);
        let locked = case_failure("dev/peek", ClientError::Case(CaseError::NotUnlocked));
        assert_eq!(locked.code, 3);
        assert!(locked.message.contains("dev unlock"));
        assert_eq!(case_failure("x", ClientError::Case(CaseError::Busy)).code, 1);
        assert_eq!(case_failure("x", ClientError::Decode).code, 1);
    }
}

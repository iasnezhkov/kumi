<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Protocol

Anyone can control a case running Kumi: our app, another app, a script or a web page.

## Transport

A vendor-class interface (`FF/4B/01`) with bulk endpoints carries `postcard-rpc`. It is separate
from the HID keyboard interface, so a client can claim it without disturbing typing.

| Platform | How a client connects | No driver or privilege needed because |
|---|---|---|
| Android | USB Host API; an intent filter for the Kumi VID/PID | the app asks the user once and can open itself when the case is attached |
| Windows 10/11 | WinUSB | MS OS 2.0 descriptors make Windows bind WinUSB itself |
| macOS | IOKit through `nusb` | no kernel extension is involved |
| Linux | usbfs through `nusb` | the shipped udev rule tags the device `uaccess` |
| ChromeOS, Chrome, Edge | WebUSB | the browser asks the user; a landing-page descriptor points to the configurator |
| iPhone, iPad | no USB access for apps | standard keyboard and audio still work; settings live in the case |

Today a case is reached through the phone: the Android app over the USB Host API, or a computer
over adb and the phone helper ([SDK](SDK.md)). The computer and browser rows are the plan: the
firmware does not yet send MS OS 2.0 or WebUSB descriptors, and no udev rule is shipped.

Why not a serial port or vendor HID: CDC-ACM needs a separate library on Android and a port choice
on computers; vendor HID is limited to 64-byte reports and still goes through the USB Host API on
Android. CDC-ACM remains an optional console in developer mode.

## Self-description and compatibility

- An endpoint key is a hash of its path and schema; an incompatible client gets a refusal instead
  of misread data. Requests normally carry the 8-byte key; 2- and 4-byte keys are accepted because
  they are unique across the endpoint list (a test checks this), and 1-byte keys are refused with
  `WireError::KeyTooSmall`.
- `postcard-rpc/ping` echoes a `u32`. `postcard-rpc/schemas/get` is listed by `postcard-rpc` but
  not served (the reply is `WireError::UnknownKey`); clients discover the API from `kumi-icd`, from
  this document and from `device/info`.
- `device/info` reports the protocol version and `Capabilities`; clients show only what the
  firmware can do.
- Types that grow without changing any key: `CaseError` travels as a `u16` code
  (`Unsupported` 0, `InvalidArgument` 1, `Busy` 2, `NotUnlocked` 3, `HardwareFault` 4,
  `StorageFull` 5, `VerifyFailed` 6, `WrongState` 7); a code a client does not know arrives as
  `CaseError::Unknown(code)`. `Capabilities` and `EventMask` travel as `u32` bit sets; unknown bits
  are kept. Adding an error code or a flag therefore changes no endpoint key.
- Other enums (`Jack`, `StreamKind`, `FilterKind`, `SlotState`, `KeyAction` and the rest) are
  closed in version 1: a new variant is a new endpoint version. Tap-hold and macro actions will
  arrive as `keys/map2/*` with a larger action type; `keys/map/*` keeps working for the version 1
  actions.
- Paths and types of version 1 freeze at release; later versions add endpoints and never change
  existing ones. `wire_format_is_stable` in `kumi-icd` pins encodings, and the `keys` section of
  `crates/kumi-jni/tests/fixtures/contract.json` pins every request, response and topic key, so a
  schema change cannot pass unnoticed.
- The wire format is `postcard`: varints, zigzag for signed integers, length-prefixed sequences;
  see the [postcard specification](https://postcard.jamesmunns.com/wire-format).

## Endpoints

| Path | Request | Response |
|---|---|---|
| `postcard-rpc/ping` | `u32` | the same `u32` |
| `device/info` | — | firmware version, build, chip id, flash JEDEC id, serial or none, running from RAM, capabilities |
| `system/diagnostics` | — | boot count, safe mode, start-up stage, uptime, dropped events, watchdog state, last crash |
| `events/subscribe` | event mask | the granted mask; a bus reset clears it |
| `audio/status` | — | jack, USB audio present, stream format, host volume and mute, active EQ slot, counters |
| `audio/output/get`, `audio/output/set` | output settings | DAC filter, volume limit, balance, DSD direct |
| `audio/eq/get`, `audio/eq/set`, `audio/eq/select` | slot / slot and profile / slot or none | a profile or none for an empty slot; parametric bands (kind, frequency, Q, gain) and preamp |
| `audio/mic/get`, `audio/mic/set` | microphone gain | |
| `keys/map/get`, `keys/map/set`, `keys/map/clear` | page / entry / — | layer, source usage, action |
| `keys/status` | — | link state, frame counters, active layer |
| `settings/save`, `settings/reset` | — / scope | |
| `system/slots` | — | the A/B table |
| `system/reboot` | normal, other slot, boot ROM | |
| `system/confirm` | — | confirms the running trial image |
| `update/begin`, `update/chunk`, `update/finish` | manifest / block / — | signed update into the inactive slot; the slot becomes a trial |
| `dev/unlock`, `dev/peek`, `dev/poke`, `dev/i2c`, `dev/uart/send` | | developer mode, time-limited |
| `dev/audio` | calibrate, start (rate, channel, level, tone or silence), stop, status | the M4 bring-up bench: state, calibration report, stream with the addresses to peek, last failure, last halt; RAM-loaded runtime only |

Requests outside these limits get `InvalidArgument` before anything changes: EQ slot below 4,
band 10 Hz to 40 kHz, Q 0.1 to 20, gain up to ±18 dB, preamp −24 to 0 dB; volume limit −60 to
0 dB, balance up to ±6 dB, microphone gain 0 to 30 dB; layer below 4, key usage `0x04`–`0xDF`,
consumer usage `0x001`–`0x3FF`, layer targets 1 to 3; peek and poke word-aligned, I2C address up to
`0x7F` and at most 64 bytes read; a `dev/audio` start at −96 to 0 dBFS with no tone or a tone of
20 Hz to 20 kHz below half the rate. Audio settings and `settings/reset` with the audio scope answer
`Unsupported` on firmware without audio. Selecting an empty EQ slot answers `WrongState`.
`contract.json` lists the limits under `limits`.

Slot states: `Empty`, `Trial` (a verified update or the first boot after an install; it has a
limited number of boots to be confirmed), `Confirmed`, `Invalid`. Confirming a trial keeps the
other confirmed slot as the rollback image; an unconfirmed trial falls back to it after its boots
run out.

`system/diagnostics` reports start-up stages as frozen `u16` codes (`kumi_icd::stage`): 0x0000
unknown, 0x0001 reset, 0x0010–0x0015 stage0 entry, journal, verified, loaded, jump, boot ROM,
0x0100–0x0107 firmware entry, clocks, timer, interrupts, UART, USB, audio, storage, 0x01F0
running, 0x0200 safe mode. New stages get new codes; codes are never reused.

Topics from the device: `audio/status/changed`, `keys/events` (a key tester that sees what the
firmware sees), `log`, `dev/uart/mirror`. `events/subscribe` takes an `EventMask` (`keys`, `log`,
`audio`, `uart`) and answers with the subset the firmware can deliver; the development runtime
grants `keys` and `log`, and `uart` (the raw keyboard UART bytes) only while developer mode is
unlocked. Events queued before a reply are sent before it, so any request works as
a barrier: after a reply, every event the case produced before handling the request has arrived.

`device/info.serial` is `None` when the case has no identifier; the development runtime reports
`None` and omits the USB serial string.

## Client layers

`kumi-icd` (types and the frame helper) → `kumi-client` (Rust: connection, calls, events,
updates) → bindings: UniFFI for Kotlin and Swift, WASM for the web, a Python package for scripts.
Other languages can implement the format from the specification above. `kumi-cli` is the `kumi`
command line tool; today it drives the host emulator (`kumi --emulator CONFIG [--server PATH]
COMMAND`, where `CONFIG` names the emulator server unless `--server` does).

Frames on the vendor endpoints are postcard-rpc frames: a `VarHeader` (8-byte key, sequence
number) followed by the postcard body, sent in 64-byte packets and ended by a short packet (a
zero-length one when the frame fills its last packet). A reply carries the endpoint's response key
and the request's sequence number; an unknown key or an undecodable body is answered with
postcard-rpc's standard error key and `WireError`. Topic messages carry their topic key and
sequence number 0. `kumi_icd::frame` encodes and decodes headers and splits and reassembles packets.

`kumi-client` returns device errors as `ClientError::Case`, so a typed call yields the value or
one error; [SDK](SDK.md) describes the client, its transports and the versioning rule for apps. `FrameTransport` keeps topic messages that arrive during a call as typed `Event`s,
discards late replies of earlier requests, and `poll_events` reads events without a request.

The Android app links `kumi-jni`, a thin JNI layer over `kumi-client`, `kumi-install`,
`kumi-buxx` and `kumi-dsp`. The app only moves bytes over the USB Host API; framing, package
checks, install plans, updates and EQ response curves run in the shared Rust code. Calls that use
the link are serialized inside the library, so `pollTopics` cannot take the reply of a concurrent
`call`. Values cross the boundary as JSON in serde's default representation of the `kumi-icd`
types (error codes as names, capability and event sets as objects of booleans);
`crates/kumi-jni/tests/fixtures/contract.json` pins that representation, the bridge's own replies,
the enum names and the limits, and the app's tests parse the same file. The built-in simulated
case runs the firmware's own storage, journal and boot policy (`kumi-runtime-core`,
`kumi-boot-core`) over a RAM flash.

## Firmware packages

A `.kumi` package carries everything an installer writes: `KUMIPKG1`, version, generation, the
lengths, a build name, stage0, the slot image, and an ed25519 signature over all of it. A client
verifies the signature with the public key it trusts, then the installer's own checks (stage0
boots and carries the key, image signature valid, versions match) before anything reaches the case.
Apps ship packages in their resources and may fetch newer ones from release pages; both paths go
through the same check.

On Android a USB interface has one owner at a time. To let other apps use case features without
fighting over it, the Kumi app will act as a broker with a public bound-service and intent API (EQ,
profiles, key events) behind a `kumi.permission.CONTROL` permission.

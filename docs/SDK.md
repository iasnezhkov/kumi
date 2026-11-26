<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# SDK

Three pure-Rust crates plus an optional JNI bridge make up the Kumi SDK. All of them are the exact
code the `kumi` CLI and the Android app run — never a second implementation:

| Crate | What it gives an app |
|---|---|
| `kumi-icd` | `no_std` protocol schema (`postcard-rpc` endpoints, topics, types, limits, and frame helpers) shared between firmware and hosts |
| `kumi-client` | typed client for all 45 endpoints and topics of the [protocol](PROTOCOL.md), native USB (`--usb` via `libusb` / IOKit), adb bridge, virtual case for tests, and signed A/B updates; re-exports `kumi-icd` as `kumi_client::kumi_icd` |
| `kumi-flasher` | installing Kumi, returning to stock, backing up the full 4 MiB flash, and rebooting out of the boot ROM service mode (`0666:0888`) over any `kumi_buxx::Link` — zero JNI dependency |
| `kumi-jni` | Android `cdylib` (`libkumi_jni.so`) exposing a JSON bridge (`kumi_jni::api`), EQ response math (`kumi-dsp`), and a simulated case (`kumi_jni::sim`) over the same crates; re-exports `kumi-flasher` as `kumi_jni::installer` |

The crates are not on crates.io (`publish = false`); depend on them by path or git:

```toml
[dependencies]
kumi-client = { path = "../kumi/crates/kumi-client" }
kumi-flasher = { path = "../kumi/crates/kumi-flasher" }
```

## What works on a real case (`1.0.0`)

Verified live on a flash-booted SPV5048 case (everyday `"slot"` with
90 MHz Quad-SPI XIP `stage0` and A/B rollback slots)
over both direct desktop USB (`kumi --usb`) and Android USB Host:

| Feature | Client call | State |
|---|---|---|
| identity, serial (eFuse) and capabilities | `device_info` | works (`build: "slot"` everyday, `"slot-lab"` lab) |
| diagnostics, crash record and hardware telemetry | `diagnostics`, `telemetry` | works (64-bit WFI %, 4-tier DVFS MHz, 1.4 V DC-DC mV, jack ADC) |
| DAC output, volume limit, balance, DSD direct | `output`, `set_output` | works (click-free 0.5 dB ramp) |
| DAC extras (Class-H + 1 Vrms LV, HPF, de-emphasis, auto-UAC, Line-Out) | `dac_extras`, `set_dac_extras` | works (persists across sleep and updates) |
| real-time DSP (crossfeed, ISO 226 dynamic loudness, limiter, mono, L↔R swap, phase invert) | `dsp`, `set_dsp` | works (RV32F audio pipeline) |
| 10-band parametric EQ (4 flash slots) | `eq`, `set_eq`, `select_eq` | works (44.1–384 kHz PCM) |
| live stereo Peak / RMS meters & test tones | `audio_meters`, `play_test_tone` | works (L / R / Phase 1 kHz) |
| headphone DC + 1 kHz AC impedance check (CS43198 HPLOAD) | `measure_impedance` | works (DC 49 Ω + 1 kHz AC measured live, automatic `AUDIO_PLL` & I2S gating) |
| 4-layer keymap, Tap-Hold, One-Shot, Caps Word, 8 key combos, HID mouse keys, 9 case hotkeys & 8 macros | `keymap`, `set_key`, `combo_slot`, `set_combo`, `macro_slot`, `set_macro`, `key_config`, `set_key_config` | works (persists in flash `FORMAT=4`, runs on every host) |
| key tester and event stream | `subscribe`, `events`, `take_events` | works |
| explicit save & factory defaults reset | `save_settings`, `reset_settings` | works (double-buffered power-fail-safe journal, lossless V1–V3 migration) |
| signed A/B firmware updates & slot switching | `install_update`, `slots`, `confirm_boot`, `reboot(Normal \| OtherSlot \| BootRom)` | works (30 s auto-confirm & rollback) |
| stock backup, install and return to stock | `kumi_flasher::{backup, install, restore_stock, reboot}` | works (`GateEvidence::recorded()`, verified twice) |
| firmware build modes & developer mode | `dev_unlock`, `dev_peek`, `dev_poke`, `dev_i2c`, `audio_bench_*` | one everyday firmware (`"slot"`) includes timed read diagnostics (`dev_unlock` + `dev_peek` + telemetry + crash logs); optional `--features lab` (`"slot-lab"`) or RAM-loaded `brom run-elf` (`"ram"`) unlocks raw MMIO `poke` / I2C / audio bench |

Always branch on `device_info().capabilities`, not on a hardcoded table: each firmware advertises
what it can serve.

## Transports

`Client<T: Transport>` sends a request and returns the decoded response. For any link that carries
whole `postcard-rpc` frames, `Client::over(link)` wraps it in `FrameTransport`, numbering requests,
matching replies, keeping topic events that arrive mid-call, and discarding stale replies.

| Transport | Type / Interface | Notes |
|---|---|---|
| direct desktop USB | `usb::UsbLink::open()` (`kumi --usb`) | claims vendor interface `FF/4B/01` (bulk IN `0x82` / OUT `0x03`) directly on macOS and Linux (on Windows, use browser WebUSB or the adb bridge). Connect via the case's **phone-facing male USB-C plug** with a female USB-C adapter or USB hub (the case's bottom USB-C port is charging-only) |
| browser WebUSB | `navigator.usb` (Chrome / Edge / Brave on Desktop & Android) | Kumi advertises MS OS 2.0 (`WINUSB`) and a WebUSB BOS descriptor (`https://nonbiri.dev/mindone/case`) on interface `FF/4B/01`. Any web app can speak binary `postcard-rpc` frames directly to bulk endpoints `0x03` / `0x82` with short-packet / ZLP frame termination |
| adb bridge to a phone | `bridge::BridgeLink::adb(...)` | runs `tools/android/spv40-usb.c` over `adb shell`, framing packets with 2-byte little-endian lengths |
| virtual case | `virtual_case::VirtualCase` | in-process simulator running the real `kumi-service`, `kumi-control`, and `kumi-keymap` crates with no hardware |
| host emulator | `kumi --emulator CONFIG` (`tools/emulator_server.py`) | executes the real boot ROM, `stage0`, and RV32 runtime in Unicorn |
| Android native | `kumi-jni` (`libkumi_jni.so`) | built with `--no-default-features` (no desktop `libusb`/`adb`); JSON contract pinned in `crates/kumi-jni/tests/fixtures/contract.json` |

A custom transport implements `wire::FrameLink` (`send`, `receive`, optional `try_receive`):

```rust
use kumi_client::{Client, kumi_icd::{KeyAction, KeymapEntry}};
use kumi_client::usb::UsbLink;

let link = UsbLink::open()?;
let mut case = Client::over(link);
let info = case.device_info()?;
case.set_key(KeymapEntry { layer: 0, source: 0x04, action: KeyAction::Key(0x05) })?;
```

## Errors

Every call returns `Result<T, ClientError<E>>`, where `E` is the transport error
(`wire::FrameError<L::Error>` over a frame link):

| Variant | Meaning |
|---|---|
| `Case(CaseError)` | the case understood the request and refused it: `Unsupported`, `InvalidArgument`, `Busy`, `NotUnlocked`, `HardwareFault`, `StorageFull`, `VerifyFailed`, `WrongState`, or `Unknown(code)` from a newer firmware |
| `Transport(FrameError::Link(e))`, `Transport(FrameError::NoReply)` | the link failed or timed out |
| `Transport(FrameError::Wire(WireError::UnknownKey))` | older firmware does not implement this endpoint (`is_unknown_endpoint()`) |
| `Encode` / `Decode` | payload exceeds protocol limits or response did not match the schema |

`ClientError`, `FrameError`, `UpdateError`, and `kumi_flasher::InstallError` are `#[non_exhaustive]`,
implement `Display` and `std::error::Error`, and provide helpers `case_error()`, `is_unsupported()`,
`is_not_unlocked()`, `is_unknown_endpoint()`, and `is_link()`.

## Events

`subscribe(mask)` returns the subset granted (`KEYS`, `LOG`, and `UART` while developer mode is
unlocked). Bus reset or restart clears subscriptions. Queued topic frames flush before RPC replies,
so `take_events()` drains events produced before a call and `events(max_frames)` polls passively.

## Protocol versions and WebUSB framing

- `kumi_client::PROTOCOL_VERSION` is the newest protocol the SDK knows; `device_info().protocol` is
  what the firmware serves. Existing endpoint paths and schemas stay frozen; new features add new
  endpoints.
- Each `postcard-rpc` frame on bulk endpoints `0x03` (OUT) / `0x82` (IN) starts with an 8-byte
  FNV1a-64 endpoint key (hashed over `(path, schema)`), a 4-byte little-endian `seq_no` (`!= 0` for
  RPC calls, `0` for topics), and the `postcard`-serialized body. Frames whose length is an exact
  multiple of `wMaxPacketSize` (512 B at High Speed, 64 B at Full Speed) terminate with a zero-length
  packet (ZLP).
- All JSON shapes and enum variants for host bridges are pinned in
  `crates/kumi-jni/tests/fixtures/contract.json`.

## Installing Kumi and returning to stock

`kumi-flasher` speaks the immutable boot ROM service protocol (`0666:0888`, `kumi-buxx`) over any
`kumi_buxx::Link` (512-byte BUXX packets). `backup`, `install`, and `restore_stock` require chip id
`0x49014480` and a 4 MiB GigaDevice (`0xC8`) flash.

| Function | Does |
|---|---|
| `identify(link)` | chip id, JEDEC id, block protection bits, capacity |
| `open_package(data, public_key)` | verifies a `.kumi` bundle's Ed25519 signature (`verify_strict`) and layout |
| `backup(link, path, progress)` | reads the entire 4 MiB flash twice, compares both passes, writes atomically with `fsync`, and verifies readback |
| `install(link, &backup, package, public_key, &gates, from, progress)` | verifies backup SHA-256, stock header, and live sector 0, then executes a power-fail-resumable write plan |
| `restore_stock(link, &backup, RestoreMode::Exact \| StockOnly, from, progress)` | writes the verified stock backup back bit-for-bit (or stock image only) and verifies every write |
| `reboot(link)` | leaves boot ROM service mode |

An interrupted flash plan returns `InstallError::Interrupted` with `resume_from()` and boot safety
status; every other error guarantees `wrote_nothing() == true`.

## Examples

```sh
cargo run -p kumi-client --example info
cargo run -p kumi-client --example remap -- 0x04 0x05
cargo run -p kumi-client --example events -- 10
cargo run -p kumi-client --example developer
cargo run -p kumi-jni --example install
```

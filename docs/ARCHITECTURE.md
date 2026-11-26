<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Architecture

## Layers

```
 community apps ─┐   Kumi app (Android) ─┐   kumi CLI ─┐   web configurator (WebUSB) ─┐
                 └────────────── kumi-client (Rust; Kotlin/Swift/JS/Python bindings) ─┘
                                        │ USB vendor bulk interface, postcard-rpc
 ┌───────────────────────────── kumi-fw firmware (Embassy) ─────────────────────────────┐
 │ usb: UAC2 + HID + vendor │ audio: DMA/I2S, EQ, DAC │ keys: layers, remap │ rpc, log    │
 │ storage: settings (KV)   │ power                   │ kbd-link: UART to the keyboard MCU │
 ├────────── kumi-dsp, kumi-keymap, kumi-kbdlink, usb-uac2, kumi-storage (chip-independent) ┤
 ├────────── snap-board (what is wired to what on this board) ───────────────────────────┤
 ├────────── spv5048-hal + spv5048-pac (clocks, interrupts, UART, I2C, SPI flash, I2S/DMA, │
 │           a MUSB driver for embassy-usb)                                               │
 └────────── kumi-boot (stage0 in SPV@ format, A/B slots, ed25519, rollback) ─────────────┘
```

Each layer is usable alone: the chip layer serves any device built on the SPV5048, the
chip-independent crates serve any firmware, and the product layer is a complete reference.

## Crates

| Crate | Purpose | State |
|---|---|---|
| `kumi-icd` | protocol: endpoints, topics, types, validation | done |
| `kumi-dsp` | parametric EQ (peak, shelves, low/high pass), auto preamp, saturation | done |
| `kumi-keymap` | layers and remapping at HID report level, consumer keys | done |
| `kumi-hid` | HID report descriptor and the boot/report-mode wire encoding of the keyboard and consumer reports | done |
| `kumi-kbdlink` | UART protocol of the keyboard MCU, stock-identical replies | done |
| `kumi-layout` | flash and RAM maps, the single flash write policy (`region_for_write`) including the scratch window, Kumi image header, CRC32 | done |
| `kumi-chip` | boot ROM flash routines behind one adapter (sessions, page splitting, read-back, sector and 64 KiB block erase, write policy), the flash identity check, the scratch-window flash test and its no-op patterns, the reset sequence, chip registers; used by stage0 and the runtime | done |
| `kumi-boot-core` | boot journal, slot policy, SHA-256 and ed25519 verification, the whole stage0 decision, the boot ROM loop breaker and the hashed load | done |
| `kumi-diag` | boot breadcrumbs and crash records in RAM, the crash log in flash | done |
| `kumi-runtime-core` | the runtime's storage: journal updates, health point, settings, crash log, updates of the inactive slot | done |
| `kumi-settings` | power-cut-safe A/B settings store over the settings region, reusing the protocol types | done |
| `kumi-control` | the validated control model behind the settings endpoints (output, EQ slots, mic, keymap, reset); live apply with no commit step and an `AutoSave` throttle that persists on its own | done |
| `kumi-service` | request dispatcher: routes a decoded endpoint key to `kumi-control` or a firmware `Provider`, with postcard | done |
| `kumi-install` | boot ROM model, ELF reader, stage0 and image packer with signing (`kumi-pack`), install plans from the live head that require gate evidence, restore plans, executor | done |
| `kumi-buxx` | boot ROM service protocol with guarded flash writes, boot ROM model | done |
| `kumi-audio` | M4 audio: audio PLL calibration and family switch, PCM profiles, CS43198 programs as data with a one-transaction-per-poll runner, I2C0 controller, I2S/DMA driver, PCM flow and feedback, UAC2 descriptors and class requests; register drivers over a `Registers` trait, no atomics ([Audio](AUDIO.md)) | host-tested; hardware bring-up pending (M4) |
| `spv5048-pac`, `spv5048-hal` | registers from an SVD, drivers, MUSB driver for `embassy-usb` | planned (M1–M2) |
| `usb-uac2` | USB Audio Class 2.0 for `embassy-usb`, which ships only UAC1 | planned (M4) |
| `kumi-stage0` (`firmware/stage0`) | stage0 binary: stock entry path, flash identity check, journal-only writes plus its one disarm, dry-run, verify and G2b entries | emulator execution matches the captured G2 RAM dry-run report; the G2b session runs in the emulator; flash boot acceptance pending (G2b on hardware, G4) |
| `kumi-fw` (`firmware/runtime`) | development application: Embassy MUSB High Speed (a Full Speed build switch), HID, vendor RPC, UART relay, remapping and layers, developer mode (`dev/fault`, `dev/flash/test`, peek, poke, I2C, UART mirror), the M4 audio bench | runs on the case from RAM (05.11); flash boot pending |
| `kumi-client` | the SDK ([SDK](SDK.md)): typed calls and errors over a frame transport, events without a request, A/B update upload, the adb bridge link, an in-process virtual case served by the firmware's dispatcher | done |
| `kumi-jni` | the Android app's bridge: boot ROM install, backup and restore plans (typed in `installer`, JSON in `api`), client calls, EQ response and auto preamp from `kumi-dsp`, a simulator running the real stage0 and runtime storage | done |
| `kumi-cli` (`kumi`) | command line client for the host emulator (`tools/emulator_server.py`) and, over adb and the phone helper, the real case | done for both; a computer USB transport is planned (M2) |
| UniFFI bindings | Kotlin/Swift/Python bindings on `kumi-client` | planned |

## Settings

Changes take effect the moment they arrive. A control endpoint updates the one in-RAM copy of the
settings that the audio engine and keyboard read live, so a new EQ, volume or key is heard or typed
at once; there is no commit step and nothing for a client to show as a save button. The stock
firmware keeps settings in RAM too, but loses them when the case sleeps.

Persistence is separate and automatic. Each change marks the model dirty; the runtime's main loop
turns that into two signals, a key map rebuild and a pending save in `kumi-control::AutoSave`. The
throttle is not a timer reset, so a burst of changes is written once, 2 seconds after the first of them
(counted in USB frames), and a USB suspend, a bus reset or a reboot request flushes immediately so
nothing in flight is lost. The write lands in the power-cut-safe A/B store in `kumi-settings`. The
`SaveSettings` endpoint stays as an explicit "flush now" for a client that wants it, but the case never
needs it to keep a setting. A runtime loaded into RAM by a host has no storage and keeps settings in
RAM only.

## Stack

The current development runtime uses `riscv32imc`, a cooperative polling loop, and a High Speed
MUSB implementation under `firmware/runtime`. The loop polls Embassy USB, RPC and HID independently
and drains UART without waiting for the host to read HID. Bounded queues preserve key transitions;
overflow releases keys and increments the error counter. Read-only boot metadata is sampled before
USB connects, so scanning the journal cannot block a live request. Started by stage0 from a signed
slot, it receives the `BootInfo` address in `a0`, copies the record into its own memory and accepts it
only from that entry; it saves settings, reaches a health point (USB configured for 30 seconds by
frame count) where it confirms a trial or clears the journal's count of unhealthy starts and saves
the last crash to the crash log, reboots and installs signed updates into the other slot. It writes
only the journal, the settings store, the crash log and the inactive slot, through the same boot ROM
flash adapter and write policy as stage0, and reads every write back. Loaded into RAM by a host, `a0`
does not carry that address, so it has no slot and answers those requests `Unsupported`; the only flash
it writes then is the scratch window, during an unlocked `dev/flash/test` (`docs/DIAGNOSTICS.md`).
Audio setters are `Unsupported` in both cases. It runs with interrupts disabled and has no watchdog: the case has
no machine timer at `0x12000000` (M1 probe), and no other interrupt source has been measured, so a
hang lasts until the case is reset. Crash reports carry the uptime counted in USB frames. The stack
below describes the intended complete firmware; the polling runtime does not provide an interrupt
executor, audio playback or low-power sleep.

- Rust `no_std`, target `riscv32imc-unknown-none-elf`. DSP target builds add `+f` explicitly;
  the base target does not introduce atomic instructions.
- Embassy: the `arch-riscv32` executor and a time driver on a timer that still has to be found:
  the M1 probe saw no machine timer at `0x12000000`.
- `embassy-usb` with our own MUSB driver; `postcard-rpc` on the vendor interface.
- `sequential-storage` for settings, `ed25519-dalek` for image signatures.
- Audio does not run in async tasks: DMA and its interrupt move samples through lock-free rings,
  and Embassy runs everything else.
- Code and data execute from ILM/DLM. Flash is storage only, so erasing or programming it never
  stalls execution.

## DSP

The EQ uses trapezoidal-integrated state-variable sections (Simper/Zavalishin), designed in f64 and
run in f32. The first version used direct-form biquads, and its own test showed them unstable in
f32 for a 20 Hz band at 768 kHz: `1 − cos ω` falls below f32 precision. The state-variable form
keeps band gain within 1 % at 20, 31 and 62 Hz at 768 kHz and tolerates parameter changes while
running. With every band flat and no preamp the processing is skipped, so the path stays bit-perfect.

## USB device

High Speed, VID `0x1209` from pid.codes, a test PID until one is assigned.

| Interface | Class | Purpose |
|---|---|---|
| Audio control and streaming OUT, plus IN for a headset | UAC2, asynchronous with feedback | audio; present only while headphones are plugged in, as on the stock firmware, so that Android does not route all audio into the case |
| Keyboard and consumer keys | HID with boot protocol | typing anywhere, including boot loaders and iOS |
| Control | vendor `FF/4B/01`, bulk | `postcard-rpc`, logs, firmware update; MS OS 2.0 and WebUSB descriptors |

The keyboard interface carries two reports, keyboard as report ID 1 and consumer control as report
ID 2 (`kumi-hid`). In report protocol both reports are prefixed with their ID; when the host selects
boot protocol, the keyboard sends the plain 8-byte boot report with no ID, so boot loaders and iOS
work unchanged. A host-side walk of the report descriptor checks that the declared report sizes match
the bytes the firmware actually sends.

## Flash map (4 MiB)

| Offset | Size | Region | Written by |
|---|---|---|---|
| `0x000000` | 64 KiB | stage0, the image the boot ROM starts | installer; stage0 only zeroes its own magic to disarm itself |
| `0x010000` | 8 KiB | boot journal | stage0 and firmware through the journal |
| `0x040000` | 1 MiB | slot A | firmware update |
| `0x140000` | 1 MiB | slot B | firmware update |
| `0x240000` | 64 KiB | crash log | firmware |
| `0x300000` | 256 KiB | copy of the stock image, for returning to stock | installer only |
| `0x3E0000` | 128 KiB | settings | firmware |

`kumi-layout::region_for_write` enforces the last column in code, through the boot ROM flash adapter
in `kumi-chip` that stage0 and the runtime share: stage0 may write the journal and, to disarm itself,
the four bytes of its own `SPV@` magic at offset 4; the runtime may write the journal, the inactive
slot, the crash log and the settings. The installer works from service mode through the boot ROM
protocol and its own plan checks.

A third writer, `Writer::Scratch`, reaches exactly `0x3F0000..0x400000` (`kumi-layout::SCRATCH`), the
64 KiB block that gates G6 and G7 used, and nothing else. It lies inside the settings region, so only
code that cannot be running from an installed Kumi uses it: a runtime loaded into RAM by a host, for
`dev/flash/test`, and stage0's G2b entry, called from service mode. Neither writes unless the area it
uses reads erased, and both leave it erased.

## RAM map

| Range | Use |
|---|---|
| `0x0000_0000..0x0001_0000` | ILM window for image code (size to be confirmed by the probe) |
| `0x0010_0000..0x0010_2000` | boot ROM data, never loaded into |
| `0x0010_2000..0x0012_8000` | DLM window for image data |
| `0x0012_8000..0x0013_7D00` | stage0 code, data and stack. It stays resident after the jump: the runtime calls stage0's verify entry at `0x128008` (marker `KSV1` at `0x12800C`) to check an update with the key built into stage0, so the firmware must not use this range, and it is never a load destination. Service mode calls the dry run at `0x128004` and the G2b session at `0x128010` |
| `0x0013_7D00` | last crash, copied by stage0 (256 bytes) |
| `0x0013_7E00` | `BootInfo` from stage0 to the firmware (256 bytes) |
| `0x0013_7F00` | crash record of the current boot (256 bytes) |
| `0x0013_8000..0x0014_0000` | boot ROM stack in service mode, never touched |

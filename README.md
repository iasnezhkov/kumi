<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

<div align="center">

# Kumi

### Custom firmware, open control protocol, and companion Android app for the iKKO Snap keyboard &amp; DAC case

**SpaceTouch SPV5048 · Andes D25F RV32IMCF · USB High Speed · CS43198 DAC · Rust `no_std` · Android 15–17**

</div>

---

> Source-available firmware and companion app for the iKKO Snap case of the iKKO MindOne.
> Read [Status](#status) and the [flashing procedure](docs/FLASHING.md) before touching hardware.
> Host emulation is separate from hardware acceptance.

Kumi replaces the stock firmware of the iKKO Snap case with an independent Rust `no_std` firmware,
a verified A/B bootloader (`stage0`), an open USB control protocol (`postcard-rpc`), a cross-platform
CLI (`kumi`), and a companion Android app (`Kumi`) with an embedded physical-keyboard autocomplete
bar (`Kumi Strip`).

Chip support, board support, chip-independent libraries, and the product layer stay separated so each
layer can be tested on the host and reused on other SpaceTouch SPV5048 devices.

## Principles

1. **Keep recovery available.** Development runs from RAM through the immutable boot ROM's service
   mode (`0666:0888`). Flash writes follow power-fail-resumable plans gated by hardware checks
   ([Flashing](docs/FLASHING.md)), keep a verified backup of the stock firmware, and support full
   return to stock at any time.
2. **One definition of the protocol.** Types and endpoints live once in `kumi-icd`; firmware, CLI,
   Android JNI bridge, and WebUSB clients share that schema and verify compatibility by endpoint key.
3. **Standard USB classes first.** Keyboard/consumer/mouse is standard HID and audio is UAC2, so
   typing and audio work on Android, Linux, macOS, Windows, and iOS without any app. Only control
   uses a vendor bulk interface (`FF/4B/01`) with MS OS 2.0 (WinUSB) descriptors and opt-in WebUSB
   BOS (`--features webusb`).
4. **Measured, not assumed.** Hardware statements carry evidence and confidence labels; logic has
   host and emulator tests, and hardware acceptance is tracked separately.
5. **No vendor code or firmware images.** This repository contains our own code, hardware notes,
   and checksums — never vendor binaries, flash dumps, or signing keys.

## Status (`1.1.0`)

Prebuilt signed firmware bundles (`kumi-<ver>.kumi`), signed Android APKs (`kumi-app-<ver>.apk`),
and checksums are published on [GitHub Releases](https://github.com/iasnezhkov/kumi/releases).

| Part | State |
|---|---|
| Protocol `kumi-icd` (55 endpoints + ping, 4 topics, frame helper) | done, host-tested; version 1 wire protocol with additive endpoints |
| Bootloader `firmware/stage0`: 360 MHz DC-DC + 90 MHz Quad-SPI XIP flash read (~30 ms verify+load), single-pass journal scan, Ed25519 A/B slots, monotonic anti-rollback, `FLAG_LAB` release isolation, rollback, crash preservation | hardware-verified (~1.3 s cold start) |
| Runtime `firmware/runtime`: Embassy USB High Speed (UAC2 async 44.1–384 kHz PCM + DoP64/128 + Native DSD64/128/256, 24-band post-DSP RTA FFT, EBU R128 LUFS / True-Peak + Auto-Level AGC, two-layer PEQ with AutoEQ base + 5-band tone + Subsonic/Mono-Bass, ISO 226 dynamic loudness + crossfeed + limiter, CS43198 soft-ramp 400 kHz I2C + Class-H/LV + true fixed 2 Vrms Line-Out + HPLOAD DC/AC impedance + test tones, HID keyboard + hardware HID Consumer media gestures + QMK 4-layer keymaps/tap-hold/one-shot/combos/macros/mouse keys, stay-awake toggle, OTA auto-mute, hardened release slots (`dev/*` `Unsupported`, `Shift + Backspace` BootROM guard), vendor RPC, settings V4, A/B updates, 4-tier 30–360 MHz DVFS + PLIC WFI idle, watchdog, SipHash-2-4 eFuse serial) | hardware- and emulator-verified |
| Core crates (`kumi-dsp`, `kumi-keymap`, `kumi-kbdlink`, `kumi-audio`, `kumi-layout`, `kumi-chip`, `kumi-boot-core`, `kumi-diag`, `kumi-settings`, `kumi-control`, `kumi-service`, `kumi-install`, `kumi-buxx`, `kumi-flasher`) | done, host-tested (`no_std` where applicable) |
| SDK `kumi-client` ([SDK](docs/SDK.md)) & CLI `kumi-cli` (`kumi`) | direct desktop USB (`libusb`), Android `adb` bridge, host emulator, and in-process virtual case |
| Signed firmware bundle `.kumi` (`kumi-pack bundle`) | done, Ed25519 strict verification |
| Companion Android app `app/` (`Kumi`, Android 15–17, no root required) | USB Host control, guided Stock → Kumi installer &amp; return-to-stock with transient AudioFocus pause during OTA, smooth spline 24-band RTA + LUFS/dBTP + vertical/horizontal VU meters, two-layer 10-band EQ (AutoEQ + 5-band Tone) &amp; 16 presets, true 2 Vrms Line-Out with auto volume save/restore, QMK layers/36-key presets, 13 UI languages, Stable / Developer (RC) update channels, **Kumi Strip** bottom autocomplete IME (20 languages including Korean 2-beolsik and Simplified/Traditional Chinese Pinyin with offline/downloadable dictionaries, AW862xx typing haptics, emoji/RAM-only clipboard drawer), **System Shortcuts** (Home screen instant overlay type-to-search, optional Backspace-as-Back outside text fields, speed keys), signature-protected launcher bridge, simulated demo case |

When loaded into RAM from the boot ROM without `stage0`, flash-backed features (persistent settings,
boot confirmation, A/B updates) report their absence through `device/info` capabilities.

## Layout

| Path | What |
|---|---|
| `crates/` | host-testable Rust libraries (most `no_std`), shared by firmware, CLI, and JNI bridge |
| `firmware/` | RV32 target binaries (`kumi-stage0`, `kumi-fw`, `kumi-probe`), in a separate Cargo workspace |
| `app/` | companion Android app (**Kumi** &amp; **Kumi Strip** IME, Kotlin + Jetpack Compose + Material 3); see [app/README.md](app/README.md) |
| `tools/` | host emulator (`brom_emu.py`), test runner, tree hygiene linter, and Android USB helper (`tools/android/spv40-usb.c`) |
| `docs/` | architecture, hardware, audio, flashing, diagnostics, protocol, SDK, and roadmap |
| `check.sh` | single entry point for `fast`, `full`, and `app` checks |

## Build and test

### Firmware and Rust crates

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo target-build
(cd firmware && cargo build --release)
python3 tools/tree-lint.py
```

`KUMI_STOCK_DUMP=<path to a 4 MiB flash dump>` enables tests that install Kumi over a real stock
image and restore it bit for bit. Dumps are never committed.

### Companion Android app

```sh
cd app
./gradlew testDebugUnitTest assembleDebug lint
```

Native `libkumi_jni.so` is compiled automatically from `crates/kumi-jni` using `rustup`
(`aarch64-linux-android`) and Android NDK `28.2.13676358`. Pass `-Pkumi.skipNative=true` to run JVM
unit tests and lint without the Android NDK. See [app/README.md](app/README.md) for all Gradle
properties (`kumi.releaseKey`, `kumi.packagesDir`, `kumi.releasesRepo`).

## Host emulation

The Unicorn-based emulator executes your own 64 KiB boot ROM and 4 MiB stock flash dumps, or a
rebuilt RV32 ELF, modeling NOR/SPI, MUSB device + host, PLIC, UART, and selected registers. Keep
dumps and generated outputs outside the source tree.

```sh
python3 -m venv .venv
.venv/bin/python -m pip install -r tools/requirements-emu.txt
(cd firmware && cargo build --release -p kumi-fw)
cargo run --release -p kumi-client --example emulator-vectors > /tmp/kumi-rpc.json
.venv/bin/python tools/brom_emu.py --mode stock --brom "$BROM" --flash "$STOCK" --output /tmp/stock-report.json
.venv/bin/python tools/brom_emu.py --mode runtime --brom "$BROM" --flash "$STOCK" \
  --elf firmware/target/riscv32imc-unknown-none-elf/release/kumi-fw \
  --rpc-vectors /tmp/kumi-rpc.json --output /tmp/runtime-report.json
KUMI_RUNTIME_ELF=firmware/target/riscv32imc-unknown-none-elf/release/kumi-fw \
  KUMI_RPC_VECTORS=/tmp/kumi-rpc.json .venv/bin/python tools/run_tests.py tools
```

`kumi` also runs full update and recovery scenarios against `tools/emulator_server.py`:

```sh
cargo build --release -p kumi-cli
target/release/kumi --emulator emulator.json exercise update.bin
target/release/kumi --emulator emulator.json exercise-recovery
```

Without private dumps, runtime tests run against a synthetic ROM ABI fixture (`FixtureRomRuntimeTests`);
`KUMI_BROM_DUMP`, `KUMI_STOCK_DUMP`, `KUMI_STAGE0_BIN`, `KUMI_G2_REPORT`, `KUMI_RUNTIME_IMAGE`, and
`KUMI_NATIVE_RPC_VECTORS` enable the complete real-ROM suite (`KUMI_REQUIRE_EMULATOR=1` turns skips
into failures). Snapshots are cached under `KUMI_EMULATOR_CACHE` (used by `tools/run_tests.py` and
`check.sh`) and keyed by every emulator input and source file in `CODE_FILES` in `tools/brom_emu.py`.

## Building installable images

Signing keys never enter the repository. Create a development keypair (prints the hex public key):

```sh
cargo run --release -p kumi-install --bin kumi-pack -- keygen ~/kumi-dev.key
```

`stage0` embeds the Ed25519 public key at build time; built without one, it accepts no image and
always enters the boot ROM. `kumi-pack` packs `stage0`, signs images, and builds `.kumi` bundles:

```sh
(cd firmware && KUMI_PUBLIC_KEY_HEX=<public key> cargo build --release)
cargo run --release -p kumi-install --bin kumi-pack -- stage0 \
  firmware/target/riscv32imc-unknown-none-elf/release/kumi-stage0 stage0.bin <public key>
cargo run --release -p kumi-install --bin kumi-pack -- image \
  firmware/target/riscv32imc-unknown-none-elf/release/kumi-fw image.bin ~/kumi-dev.key 1.0.0 1000000
cargo run --release -p kumi-install --bin kumi-pack -- bundle \
  stage0.bin image.bin ~/kumi-dev.key 1.0.0 kumi-1.0.0.kumi
cargo run --release -p kumi-install --bin kumi-pack -- inspect image.bin <public key>
```

## Documentation

| Document | Contents |
|---|---|
| [Architecture](docs/ARCHITECTURE.md) | layers, crates, USB interfaces, runtime model, flash map |
| [Hardware](docs/HARDWARE.md) | SPV5048, CS43198, keyboard MCU, evidence &amp; confidence labels |
| [Audio](docs/AUDIO.md) | UAC2, I2S/DMA, CS43198 sequences, RV32F EQ/DSP, DoP &amp; Native DSD |
| [Flashing](docs/FLASHING.md) | boot ROM rules, hardware gates, install, A/B update, rollback, return to stock |
| [Diagnostics](docs/DIAGNOSTICS.md) | crash records, safe mode, stage0 reports, M1 probe |
| [Protocol](docs/PROTOCOL.md) | open `postcard-rpc` control interface for apps, CLI, and WebUSB |
| [SDK](docs/SDK.md) | Rust client and flasher crates, transports, errors, events, examples |
| [Roadmap](docs/ROADMAP.md) | milestones, hardware feature status, comparison with stock firmware |
| [Android app](app/README.md) | Kumi Android app architecture, Kumi Strip IME, dictionaries, Gradle build |
| [Contributing](CONTRIBUTING.md) | issue reports, patch rules, pre-submit checks |

## Companion repositories

- **Kernel (Linux 6.18 GKI / MT6789):** [iasnezhkov/android_kernel_ikko_mindone](https://github.com/iasnezhkov/android_kernel_ikko_mindone)
- **Device tree (LineageOS 24):** [iasnezhkov/android_device_ikko_mindone](https://github.com/iasnezhkov/android_device_ikko_mindone)
- **WebUSB configurator:** <https://nonbiri.dev/mindone/case>

## License

Licensed under the **PolyForm Noncommercial License 1.0.0** (`PolyForm-Noncommercial-1.0.0`), see
[LICENSE](LICENSE) and [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

Personal, hobbyist, educational, and non-commercial research use is permitted. Use in commercial
products or commercial offerings is not permitted under this license.

For commercial licensing, OEM integration, or collaboration, contact Ilia Snezhkov at
`iasnezhkov@nonbiri.dev` or [nonbiri.dev/about](https://nonbiri.dev/about).

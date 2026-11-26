<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Roadmap

Each milestone runs entirely from RAM first; the flash is written only after the gates in
[Flashing](FLASHING.md) are passed.

| Milestone | Result | Acceptance |
|---|---|---|
| M0 | workspace, protocol, DSP, keymap, keyboard link, image format, A/B journal, diagnostics, installer, boot ROM protocol | host tests and RV32 builds (done) |
| M1 | start-up in RAM: CPU clock, ILM/DLM sizes, FPU, timer, interrupts, keyboard UART frames | the probe report; frames captured while typing |
| M2 | USB High Speed on `embassy-usb`: HID keyboard, vendor interface, `postcard-rpc`; client and CLI | the phone types through the case; the CLI and an unprivileged app read `device/info` |
| M3 | layers and remapping in the case, the keyboard MCU commands | the key tester shows remapped keys through `keys/events` |
| M4 | DACs over I2C, I2S and DMA, UAC2 at 44.1–192 kHz, volume in the DAC, audio presence following the jack | playback with zero underruns |
| M5 | EQ, 384 and 768 kHz, DoP and native DSD, headset microphone | measured frequency response matches the design |
| M6 | flash driver, settings storage (the store is written and host-tested in `kumi-settings`), stage0 protection | a power cut during a settings write loses nothing |
| M7 | stage0 binary, signed A/B updates, rollback, install and return to stock from the app and CLI | a broken slot falls back; both broken reach the boot ROM |
| M8 | research: charger and battery, keyboard MCU protocol (LEDs, sleep), hardware EQ block | a report with measurements |

## Feature state (Kumi 1.0.1)

**hardware**: works on the real case booted from flash (`stage0` 360 MHz + 90 MHz Quad-SPI XIP + signed A/B slots); **code**: written
and host-, model- or emulator-tested, not yet accepted on the case; **missing**: not written. The last
column says whether the open part can be finished without the case.

| Feature | Milestone | State | Open part | Without the case |
|---|---|---|---|---|
| start-up from flash (`stage0` 360 MHz + DC-DC + 90 MHz Quad-SPI XIP ~30 ms load + single-pass journal scan, ~1.3 s cold boot), 4-tier workload DVFS (30/90/180/360 MHz), PLIC `wfi` ~99.5% idle sleep, L1 I-cache, RT40 `.storage`, `CMU_CLKEN0` I2S clock gating, EFUSE USB Serial, `system/telemetry` | M1, M7 | hardware | — | — |
| hardware watchdog (`0x20003020`, `watchdog_armed: true`) | M1 | hardware | — | — |
| USB High Speed, combined HID keyboard + consumer + 3-button mouse/wheel, vendor RPC, MS OS 2.0 + WebUSB URL descriptors, Remote Wakeup | M2 | hardware | — | — |
| CLI over direct host USB (`kumi --usb`) and over the phone (`kumi --adb`) | M2 | hardware | — | — |
| SDK `kumi-client` + `kumi-flasher` (`no-JNI`) with transports and a virtual case ([SDK](SDK.md)) | M2 | hardware (through the CLI) | bindings for Swift, Python, WASM (Kotlin JNI complete) | yes |
| Android app (`app/`, Material 3, EN+RU, GitHub Releases updates, Stock-to-Kumi wizard, **Kumi Strip** bottom autocomplete IME with EN/RU/UK/JA offline dictionaries, Speed Keys, Android 15–17 without root) | M2 | hardware | stock Android 15 end-to-end install pass | no |
| web configurator (WebUSB at `nonbiri.dev/mindone/case`) | M2 | code | live Chrome desktop/Android WebUSB check | code yes, acceptance no |
| remapping in the case, key tester (`keys/events`), hold/toggle layers | M3 | hardware | live multi-key layer acceptance on phone | no |
| QMK tap-hold (mod/layer), one-shot mods, Caps Word, 8 key combos, HID mouse keys, 8×16 hardware macros, 9 CaseHotkeys, host LED tracking, timing config | M3 | hardware | live typing & mouse-key acceptance on phone | no |
| keyboard MCU commands (LEDs, sleep) | M3, M8 | hardware (partial) | stock relay + keep-awake (`GPIO5`) live; extended MCU commands undecoded | no |
| everyday vs lab builds: timed `dev/unlock` + `dev/peek` in everyday `"slot"`; raw `poke`/I2C/bench in `"slot-lab"` or RAM `"ram"` | M4 | hardware | — | — |
| UAC2 playback 44.1–384 kHz (S16/S24/S32), click-free 400 kHz I2C + CS43198 Soft-Ramp volume, Class-H + true 1 Vrms LV (`HP_OUT_CTL_1=0x11`) / HPF / De-emphasis / Line-Out / Auto-UAC (`LRADC0`) | M4, M5 | hardware | analog listening sweep on phone | no |
| RV32F 10-band EQ + ISO 226 volume-adaptive Dynamic Loudness + Bauer/Meier Crossfeed + C1 Soft-Clip Limiter + Mono + L↔R Swap + Phase Invert + live VU `audio/meters` + `HPLOAD` DC & 1 kHz AC `audio/impedance` + `audio/test-tone` | M5 | hardware | analog frequency-response sweep | no |
| DoP64/128 (`176.4`/`352.8 kHz`), `dsd_direct`, Native DSD64/128/256 (`Alt 4 FORMAT_RAW_DATA`) | M5 | hardware | live Linux ALSA `DSD_U32_BE` playback check on Alt 4 (descriptor + emulator verified) | no |
| headset microphone (`audio_in`) | M5 | missing | hardware limitation: mic line not routed to SPV5048 ADC on Snap PCB | no |
| boot ROM flash adapter, settings store (`FORMAT=4` with V1–V3 migration), live apply, autosave deferred during playback | M6 | hardware | — | — |
| `stage0` in flash, signed A/B updates, trial confirmation, rollback, crash records, safe mode | M7 | hardware | CLI/app return-to-stock acceptance on phone | no |
| install, backup, restore and `stage0` update from `kumi --usb brom ...` | M7 | hardware | — | — |
| broker service for other Android apps (`kumi.permission.CONTROL`) | — | missing | service, permission, intent API | yes |
| charger and battery telemetry | M8 | missing | hardware limitation: no charger IC connected to SPV5048 I2C0/I2C1 (`battery: false`) | no |

## How Kumi improves on the stock firmware

| Stock | Kumi |
|---|---|
| settings live in RAM and are lost when the case sleeps | changes apply live with no save button, persist on their own, and are restored at every start |
| no remapping in the case | layers and remapping in the case, working on every host |
| EQ coefficient tables for fixed rates | parametric EQ designed for the actual rate, auto preamp, bit-perfect when flat |
| no firmware updates | signed A/B updates with automatic rollback |
| saving settings erases 8 KiB including neighbouring data; a written config block is never applied | journaled, power-fail-safe storage |
| a private protocol that needs system privileges on Android | an open, self-describing protocol usable without privileges |
| no diagnostics | crash records, safe mode, logs |

Audio dropouts seen on the phone were traced to the phone's USB audio path, not to the case, so
they are not listed as a stock firmware defect.

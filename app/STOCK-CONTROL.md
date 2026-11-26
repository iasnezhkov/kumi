<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Stock Firmware Control Surface

Summary of what the app can safely inspect or modify when the iKKO Snap case still runs its factory
firmware (`0661:0881`, HID report `0x4B`). Reading stock HID directly requires platform privileges
(`/dev/hidrawN` in the `system` build); installing Kumi over USB Host works on standard Android 15+
without root.

**Batched memory reads:** single-word reads (`GET 0x00`, 1 word) echo back without answering on
stock firmware. `MemoryPlanner` always groups reads into multi-word batches (verified up to an
8 KiB state snapshot on hardware).

## 1. Read-only inspection (hardware-verified)

| Data | Command | Status |
|---|---|---|
| Firmware version (`V0.1`) | `GET 0x0C` | verified |
| Microphone gain (`C+8`) | `GET 0x02` | verified |
| Output trim (`C+10`) | `GET 0x03` | verified |
| EQ banks 0–8 (8 bands: frequency, Q, gain, filter type, preamp, coefficients) | `GET 0x09` | verified (bank 0 captured) |
| Live RAM audio state (volume, EQ profile, DAC filter, DSD mode, sample rate, UAC L/R volume, trim, mute, USB mode, settings marker) | `GET 0x00` (batched) | verified |
| USB speed/attributes, Type-C roles, phone battery, input device | Android sysfs / `BatteryManager` / `InputManager` | verified |

## 2. RAM writes (experimental, lost on sleep/reconnect)

| Action | Command | Status |
|---|---|---|
| Write EQ band + apply | `SET 0x09` + `0x0A` | gated behind experimental toggle; RAM-only |
| Set microphone gain | `SET 0x02` | gated behind experimental toggle; 1.5 dB step, RAM-only |
| Change DAC digital filter byte (`0x112881`) | `SET 0x00` | gated behind experimental toggle; applies on next sample-rate switch |

## 3. Service mode &amp; forbidden stock flash writes

- Entering boot ROM service mode (`0666:0888`) via `4B 01 FF` is performed exclusively by
  `transport/ServiceMode` at the start of a Kumi install or return-to-stock flow; all chip/flash
  validation, double-read backups, and flash write plans live in `kumi-jni` (`kumi-flasher`).
- Stock flash-save commands (`SET 0x01`, `0x04`, `0x05`) erase 8 KiB sectors including neighboring
  data and are permanently blocked (`FORBIDDEN_SET_COMMANDS`) even when experimental writes are
  enabled.

## 4. Stock limitations addressed by Kumi firmware

- Settings written to stock RAM are lost every time the case enters its 3-minute auto-sleep.
- In-case key remapping, layers, combos, macros, tap-hold, and mouse keys are absent in stock.
- Parametric EQ across all sample rates, dynamic loudness, crossfeed, limiter, Class-H / 1 Vrms LV
  control, and `HPLOAD` headphone impedance checks require Kumi firmware.
- Charger and battery telemetry are physically unconnected to SPV5048 I2C buses on the Snap PCB
  (`battery: false` on both stock and Kumi).

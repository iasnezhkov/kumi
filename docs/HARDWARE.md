<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Hardware

The case holds four kinds of chips. Kumi runs on one of them, the SPV5048, and has to live with the
others exactly as they are. Confidence labels: **measured** (read from a device or a dump),
**code** (read from the stock firmware disassembly), **hypothesis** (not yet checked).

## The controller and its board

| Block | What is known | Confidence |
|---|---|---|
| Controller | SpaceTouch SPV5048, chip id `0x49014480` | measured |
| CPU | SpaceTouch SPV5048 (Andes D25F), RV32IMCF (single-precision FPU verified; no A, no D), 360 MHz Core PLL with 4-tier workload DVFS (30 MHz USB suspend / 90 MHz idle & 44.1–48 kHz bit-perfect PCM / 180 MHz DSP ≤ 96 kHz & 88.2–192 kHz PCM & DoP / 360 MHz high-rate DSP & 352.8–384 kHz & Native DSD), external 1.4 V DC-DC core rail, PLIC WFI idle sleep (~99.5% idle) + L1 I-cache | measured |
| Memory | ILM at `0x0` (64 KiB, hot code + `run_loop`), DLM `0x100000..0x140000` (256 KiB, data and stack), independent 256 KiB RT40 SRAM at `0x40000000` (`.storage`), flash window `0x48000000` | measured |
| Boot | the boot ROM looks for the header at flash offset 0 (`SPV@` at `+0x04`) and checks an additive sum, otherwise it enters service mode `0666:0888` | measured |
| Flash | GigaDevice, 4 MiB, JEDEC `C8 65 16`, block protection off, about 192 KiB used by the stock image; Kumi uses dual 1 MiB A/B slots (`0x040000`, `0x140000`), power-fail-safe boot journal, 256 KiB preserved stock copy (`0x300000`), and 128 KiB double-buffered settings journal (`0x3E0000`, `FORMAT=4` with V1–V3 migration) | measured |
| USB | MUSB device controller at `0x200D0000`; High Speed 480 Mbit/s, combined HID keyboard + consumer (Report IDs 1–2, 91-byte default descriptor; 145-byte descriptor with Report ID 3 relative mouse/wheel when keymap uses mouse actions) + vendor `postcard-rpc` (`FF/4B/01`) + UAC2 + MS OS 2.0 (WinUSB) / WebUSB landing URL (`https://nonbiri.dev/mindone/case`) | measured |
| Audio | Cirrus Logic CS43198 DAC answering on I2C0 at `0x33` (`43 19 80`; `0x31` absent), hardware I2S0 PCM/DoP/Native-DSD serializer + DMA ch2 (clock-gated off in `CMU_CLKEN0` when idle), asynchronous UAC2 44.1–384 kHz (S16/S24/S32), RV32F 10-band parametric EQ + ISO 226 volume-adaptive dynamic loudness + crossfeed + limiter + mono + L↔R swap + phase invert + stereo peak/RMS meters, DoP64/128 (with DSD silence substitution) and Native DSD64/128/256, CS43198 Class-H (with true 1 Vrms LV stage `HP_OUT_CTL_1=0x11`) / HPF / de-emphasis / Line-Out / HPLOAD DC + 1 kHz AC headphone impedance measurement, LRADC0 jack sense | measured |
| Keyboard | a separate MCU scans the matrix and sends ready HID reports over UART | measured |
| Power | a separate charger chip, 500 mAh battery, a "power bank / keyboard and DAC" switch, a charge indicator | vendor description; protocol unknown |

## Working with the other chips

The rule: **towards other chips, Kumi behaves like the stock firmware until a measurement shows it
may do otherwise.** An unknown gets a safe default and a log entry, never a guess.

| Neighbour | What Kumi does |
|---|---|
| Keyboard MCU (UART) | `kumi-kbdlink` speaks the stock protocol. It accepts relayed HID reports (command `64`) and commands (`01`, `03`–`06` media, `07` volume, `0A` mute, `0F`/`10` EQ) and answers queries `11` (version), `12` (sample rate) and `13` (stream status) byte for byte as the stock firmware does. Unknown frames never stop the firmware; they are counted and mirrored to `dev/uart/mirror`. |
| Sleep and power | The controller can lose power at any millisecond. Nothing in RAM is irreplaceable, flash is written only through power-fail-safe journals, start-up is fast, and settings are applied at every boot. While audio plays, the firmware answers the MCU's status query truthfully, in case the MCU uses it to stay awake. |
| USB host (phone or computer) | High and Full Speed, suspend and resume, re-enumeration; audio interfaces only while headphones are plugged in; an honest `bMaxPower`. |
| CS43198 DACs | the stock power-up order (reset line, then registers), start muted and ramp the volume, a hearing-safety volume limit. |
| Charger chip | read only, and only after it has been identified on I2C; `battery: false` in the capabilities until then; writing its registers is forbidden in code. |

### Keyboard MCU protocol

Frames are `4B`, group (`01` set, `80` get), command, length, body, two trailer bytes.
The stock reply sender sums the body bytes into 16 bits and stores the sum little-endian in the
trailer, then transmits length + 4 bytes. The only frame captured from the MCU (an empty keyboard
report) carries the same sum big-endian, and the stock receiver never checks it. Kumi therefore
accepts either byte order, counts mismatches, and replies exactly like the stock firmware.

## Open questions

| Question | What is known | How it gets settled |
|---|---|---|
| Talking to the case from a computer | Verified live on macOS via the case's phone-side USB-C plug with a female USB-C adapter: Kumi enumerates at High Speed (`1209:0001`) with keyboard, vendor RPC (`kumi --usb`), and UAC2 CoreAudio. The case's own bottom USB-C port is charging-only: plugged directly into a Mac (24.11) it neither enumerates the case nor powers SPV5048. | Closed. |
| Phone visible to a computer through the case | The phone's single USB 2.0 controller (`musb-hdrc`) is host or device, never both, and the case's bottom USB-C port is charging-only (05.11 and 24.11: no D+/D− pass-through to the host PC). | Closed unless a hidden PD/mux switch is found in the phone-in-case forced-device test. |
| Power into the case's own port | Measured 05.11 with stock firmware: plugging a cable into the case's USB-C port detaches the Type-C link to the phone, re-attaches it within 0.2 s with the phone as the power sink (PD 5 V 2 A), swaps the data role back to the phone as host, and the case enumerates twice within 1.3 s. Android removes the USB headset each time and broadcasts `ACTION_AUDIO_BECOMING_NOISY`, so music apps pause. | Retest with Kumi in flash to confirm whether the second enumeration is gone. |
| Keyboard MCU identity | Sends ready reports and commands; its firmware is not in the SPV5048 flash; the chip is unidentified. | Capture both UART directions while typing, using Fn, sleeping and switching modes. |
| Sleep | After three minutes without key presses the whole case disappears from USB (the Type-C link detaches), and a key press wakes it. The stock SPV5048 code has no 180-second timer. Measured 05.11: it also happens in boot ROM service mode; with the stock firmware playing UAC2 audio at 192 kHz the case stayed attached for seven minutes without a key press. On direct Mac USB host with Kumi in flash (24.11), `board::keep_awake()` (`GPIO5`) and USB activity keep the case awake continuously. | Measure idle sleep timing with the phone attached and Kumi booted from flash. |
| Charger and battery | 500 mAh, a mode switch, a charge LED; protocol unknown. Live scans of I2C0 (`0x08..0x77`) find only the CS43198 at `0x33`, and scans of I2C1 on `GPIO38/39` (mux 5 and 6, 112 addresses) find no responding charger device; no fuel-gauge/ADC path is exposed to SPV5048. | Remains `battery: false` unless a keyboard-MCU UART query or LRADC/GPIO charging signal is discovered. |

Each open question is closed the same way: an experiment through developer mode (UART mirror,
I2C, memory reads), a recorded measurement, a decision in code, and a test built from the captured
frames.

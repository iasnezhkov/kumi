<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Audio

`crates/kumi-audio` holds everything the runtime needs for milestones M4 and M5 (see [Roadmap](ROADMAP.md)):
the audio PLL and its calibration, the PCM and DSD profiles, the CS43198 programs, the I2C0 controller, the I2S
and DMA driver, the PCM/DoP/Native-DSD flow with feedback, and UAC2 descriptors and class requests. It touches no
hardware itself. Register drivers talk to a `Registers` trait (`read`, `write`, `update`, `delay_us`),
DAC programs talk to an `I2cBus` trait, and the PCM ring talks to a `FrameStore` trait, so host tests
record every access and the runtime supplies volatile MMIO. There are no atomics and no interrupts:
every wait is either a bounded poll or a stepper that the main loop polls with the current time.

As of `Kumi 1.0.0`, the full M4/M5 audio path runs from flash on the real case (4-tier 30/90/180/360 MHz
DVFS with external 1.4 V DC-DC `U1+L1`, 90 MHz Quad-SPI XIP `stage0`, Andes L1 I-cache, RT40 `.storage`, RV32F hardware FPU): asynchronous UAC2
playback across 44.1–384 kHz (S16/S24/S32), hardware RV32F 10-band parametric EQ + ISO 226 volume-adaptive Dynamic
Loudness + Bauer/Meier Crossfeed + C1 Soft-Clip Limiter + Mono + L↔R Channel Swap + Phase Invert + live stereo
peak/RMS/limiter meters up to 384 kHz S32, click-free volume ramping (400 kHz I2C0 Fast Mode + CS43198
`PCM_PATH_1 = 0xB0` Soft Ramp / Zero Crossing + write-only `volume_fast` + MUSB FIFO drain), CS43198 Class-H
(including true 1 Vrms LV low-gain stage `HP_OUT_CTL_1 = 0x11` in `LowPowerLv`) / HPF / De-emphasis / Line-Out /
Auto-UAC + `CMU_CLKEN0` I2S clock gating when idle, unprivileged `HPLOAD` DC + 1 kHz AC headphone impedance
measurement, auto-stopping L/R/Phase test tones, DoP64/DoP128 (`176.4`/`352.8 kHz`) with `dsd_direct`, and Native
DSD64/128/256 (`Alt 4 FORMAT_RAW_DATA`) enumeration.
This page keeps every fact traceable to its source or live measurement.

## Sources

| Key | Source |
|---|---|
| A | earlier clean-room analysis of stock DAC/clock sequences (`riscv32imac` reference notes, never run on the case) |
| T | conditional traces of the stock DAC routines `0x13E4` (PCM) and `0x15F8` (DSD) at 44.1, 48, 192 and 768 kHz |
| R | stock audio and USB descriptor inventory captured from the case |
| P | audio power-up sequence observed for service mode |
| M1 | the M1 hardware probe report (`firmware/probe`) |
| S | stock firmware enumerated by an Android phone (`/proc/asound/card1/stream0`) |
| L | the Linux `cs43130` codec driver, which covers the CS43198 (`sound/soc/codecs/cs43130.[ch]`), and Linux `snd-usb-audio` |
| U | the USB 2.0 and USB Audio 2.0 specifications |
| K | sibling SpaceTouch SPV1x DMA register documentation (`dev_dma.h` / `dma.c`): a DMA block at the same address with the same channel and control layout, four channels instead of at least eight |
| W | the Mentor USB OTG (MUSB) register layout, as used by Linux `drivers/usb/musb` |

Status: **measured** means read from the case or from what the stock firmware showed a host;
**stock** means inferred from the stock image (A, T, R, P) without running it; **assumed** means a
Kumi decision or a hypothesis with no stock evidence. A later measurement moves a row, and its test
pins the new value.

## Fact table

### Clocks

| Fact | Value in the crate | Source | Status |
|---|---|---|---|
| CPU clock in the boot ROM state | 60.001 MHz, `mcycle` against 32 USB frames | M1 | measured |
| System clock register `0x20002010` | `0x00000004`: source 4 (core PLL), divider 1 | M1 | measured |
| Core ISA | RV32IMCF, no A, no D | M1 | measured |
| CPU clock decode (`clock::decode_cpu_hz`) | source codes 0–5, divider table 1…32, core PLL `0x21001000` fields; 60 MHz needs field `[10:4] = 10` with bit 3 clear | A `clock.rs` | stock; `0x21001000` was never read |
| Audio PLL registers | control `0x21001010`, fraction `0x21001014`, trim `0x21000010[19:16]` | A `audio_clock.rs` | stock |
| Audio PLL profiles | 4 stock profiles; integer/fraction 6/150995, 4/100663, 5/676122, 3/800273 | A `pll.rs` | stock |
| PLL reference | 12 MHz (24 MHz divided by the field written as `0x200` in `[12:8]`): with it every profile lands within 10 Hz of 73.728, 49.152, 67.7376 and 45.1584 MHz | derived from the profiles | assumed |
| Lock window | measured/reference count ratio within 0.99583…1.00417 of 3.072, 2.048, 2.8224, 1.8816 | A `pll.rs`, its test reproduces the stock doubles bit for bit | stock |
| Trim sweep | per trim: settle 1 ms, enable, 2 ms, timers 0 and 1 counting 130 µs (`0x20010020/40`, counts at `+0x08`, clock selects `0x20002060/64` = 5 and 11, gate bit 5 of `0x20002000/0x20003000`), disable, 60 µs | A `audio_clock.rs` | stock; the timer block's interrupts are in R |
| Trim choice | middle of a single contiguous run of accepted trims; two runs are an error | A `pll.rs` | stock |
| Trim sweep on the case | 05.11, RAM-loaded Kumi, 48 kHz family: reference 3134–3137 per window; measured by trim 0..15: 10572 10139 9692 9253 8813 8383 7944 7498 7072 6648 6465 6463 6461 6461 5987 5512, against the window 6391..6445; no trim accepted (`NoTrimWindow`). Trims 10–13 plateau at a ratio of about 2.062 instead of 2.048 (+0.67 %) | measured (`kumi audio calibrate`) | measured; the target or the reference assumption is off, see the next row |
| PLL chosen by the stock | after a real power cut and stock playback at 192 kHz: control `0x00040247`, fraction 100663, trim 11, read with `dev peek` before any calibration (twice) | measured 05.11 | measured |
| Trim sweep with paired counter access | starting both timers with one pair of stores and reading both counts with one pair of loads (8c48c48f): 48 kHz trims 10–13 at 6435–6441 inside 6416..6469, trim 11 chosen, as the stock; 44.1 kHz trims 11–14 at 5908–5915 inside 5876..5925, trim 12; 105.8 ms. Stopping the timers before reading reads 0 counts | measured 05.11 | measured |
| DMA cursor during a 48 kHz silent stream | the stream reached `Running` 146 ms after the start; over 2 s the firmware monitor counted about 9.9 million frames and 4831 wraps in 5103 cursor polls, the host sampling of `0x20020218` about 8 800 frames per second, both far from 48 000. Every one of the 18 host samples had `(value & 0xffff) + 1` a multiple of 4 and at most the ring size, spread over the whole ring; `0x20021008` read `0x00040404` (transfer end, half end and slot end of channel 2) at both host reads although the firmware clears bit 2 on every poll. After `audio stop` the DMA enable reads 0 and the remaining count `0xffffffff` | measured 05.11 | measured; the reading is in the next row |
| What the 05.11 cursor numbers mean | the count is in bytes with the − 1 of the length register (the multiple of 4 in every sample), counting down to all ones when the channel ends (the stop reading; `dma_get_transfer_residual` in K adds 1 to bits 15:0). A wrap on 95 % of the polls means the decoded offset went backwards almost every 0.39 ms poll: either the DMA is not paced by the I2S and runs a 16 KiB lap in about 0.41 ms (about 40 MB/s, just slower than the poll, so each poll sees almost a full lap; it also keeps the transfer-end flag set), or the count runs upwards at the nominal rate. The host samples, 117 ms apart against a 42.7 ms lap, cannot tell; `i2s::tests` reproduce both readings. Kumi now learns the decoding at every start (`I2sDma::learn`: 64 reads 50 µs apart, direction and unit from the rate against the nominal 8 bytes per frame within 25 %, the + 1 from the residue) and refuses a counter that moves at another pace (`ProbeFault::Pace`) | K, the measurement above | assumed: a hypothesis, the unpaced DMA the more likely of the two; the probe settles it on the case (monitor words `decoding` and `probe_counts_per_second`) |
| Timer and I2S clock registers in the boot ROM state | `0x20002060`, `0x20002064`, `0x20002078`, `0x20021000` read 0 | M1 | measured (reset values only) |

### PCM profiles

| Fact | Value | Source | Status |
|---|---|---|---|
| Rates | 44.1, 48, 88.2, 96, 176.4, 192, 352.8, 384 kHz | A `pcm_profile.rs`, T | stock |
| DAC serial-port rate code `0x1000B` | 1…8 in that order | T, matches the rate table of L | stock |
| DAC ASP N/M `0x40010..13` | 147/1280, 1/8, 147/640, 1/4, 147/320, 1/2, 147/160, 1/1 (64·fs from 24.576 MHz) | T, identical to the 64-bit frame table of L | stock |
| SPV I2S clock word `0x20002078` | `((divider − 1) << 4) \| 3`, divider 16, 8, 4, 2 from the family master clock | A `pcm_profile.rs` | stock |
| 705.6 and 768 kHz | the stock trace at 768 kHz programs the DAC exactly as at 384 kHz, so the stock firmware must convert the rate on the SPV side (its rate converter at `0x20170000` is only partly decoded); Kumi does not offer these rates | T, R | stock |
| Rates the stock firmware shows a host | 44.1…768 kHz on all four alternate settings | S | measured |
| Packet size | nominal frames per interval plus 0.1 %, rounded up; 768 kHz at High Speed gives the stock 776 bytes | A, R | stock |
| Full Speed rates | 44.1–96 kHz, the largest rate at which a 32-bit stereo packet fits 1023 bytes | U | assumed |

### CS43198 programs

| Fact | Value | Source | Status |
|---|---|---|---|
| Two DACs | I2C addresses `0x33` (primary) and `0x31`, programmed in that order | R, T | stock |
| DACs answering on the case | after `board::POWER_ON`, `i2c::SETUP` and the reset pulse from a RAM-loaded Kumi, without headphones plugged in: `0x33` returns the CS43198 identity `43 19 80`; `0x31` does not acknowledge, like the absent `0x32`; a scan of `0x08..0x77` on controller 0 finds only `0x33`. Hypotheses: the second DAC needs a rail or reset that only the headphone path enables, sits elsewhere, or is not fitted | measured 05.11 (`kumi audio dac-probe`, `dev i2c`) | measured; cause of the missing `0x31` unknown |
| Identity | device ID bytes `0x43`, `0x19`, `0x8?` | A `i2c_backend.rs`, chip id `0x43198` in L | stock; never read from the case (the service-mode scan saw nothing because the audio rails were off) |
| Frame | three register bytes big-endian, a pad byte `0x01`, then the value; a read sends the four bytes, STOP, then reads one byte | R, A `i2c.rs` | stock (L uses a pad byte too) |
| PCM start (`dac::pcm_start`) | prepare 14 writes per DAC and 15 ms; timing 16 writes, filter, path `0x90003=A0`, `0x90004=00`; volume; enable `B0000=1E`, `80000=31`, `F0010=11`, `10006=01`, 15 ms, `F0011=07`, `20000=01` | T; the test replays the 44.1 kHz trace write for write and the other rates by their rate fields | stock |
| Readback after start | 11 registers per DAC | A `dac48.rs` | assumed (archive addition, not stock) |
| Filter | read-modify-write of `0x90000` keeping bits 4:0; fast/slow × low-latency/phase-compensated = `00/40/80/C0`, NOS = `20` | R, bit names in L | stock |
| Filter change | only while both headphone outputs are powered down (`0x20000` bit 4) | A `dac48.rs` | stock |
| Volume | half-dB attenuation, `0xFF` mute, written only to `0x33`: left to `0x90001`, right to `0x90002` | T writes both to `0x33`; the left/right mapping is A's; L names `0x90001` volume B | stock; left/right is open |
| Volume levels | Kumi writes the exact attenuation up to `0xFE` (−127 dB); the stock firmware turns anything below −59.5 dB into mute | R (stock clamp) | assumed (Kumi) |
| Click-free volume ramping (`0.1.25`) | `PCM_PATH_1` (`0x90003`) = `0xB0` (Soft Ramp + Zero Crossing) with write-only `volume_fast`/`dsd_volume_fast` over 400 kHz I2C0 (`0x20070000 = 0x11e00` during stream, restored to `0x17800` on stop), duplicate skip, and MUSB ISO FIFO drain before/after each step; 16 rapid macOS volume steps at 352.8 kHz S32 with EQ + Crossfeed + Limiter gave `overruns=0, underruns=0, dac_faults=0` | live macOS CoreAudio 25.11 (`0.1.25`, Slot B) | measured |
| CS43198 extras & `HPLOAD` (`0.1.27`) | `CLASS_H_CTL` (`0xB0000`) + `HP_OUT_CTL_1` (`0x80000`): `0x1E / 0x31` Adaptive HV, `0x1C / 0x11` Low-Power LV (true 1 Vrms low-gain stage), `0x06 / 0x31` Fixed HV; `PCM_FILT_OPT` (`0x90000`) HPF (`bit 1`, `0x02`) & De-emphasis (`bit 0`, `0x01`); Line-Out fixed `0 dBFS`; unprivileged `audio/impedance` runs `HPLOAD` DC + 1 kHz AC (`HP_MEAS_LOAD = 1000/6 = 166`) and returns CS43198 cleanly to RCO sleep (`0x10006=0x06, 0x20000=0xFE`) with `CMU_CLKEN0` I2S clock gated off (`bit 14`), measured live at `DC L=49 Ω / R=49 Ω`, `AC 1 kHz L=17 Ω / R=19 Ω` (`status=0x0c`) | live hardware 25.11 (`0.1.27`, Slot B) | measured |
| DSD start (`dac::dsd_start_rate`) | prepare as PCM; `70002=E0`, `70003=00`, `70004=13` (DSD64) / `17` (DSD128) / `1B` (DSD256) per DAC; DSD volume `70000/70001` on `0x33` (0 dB unity when `dsd_direct` is enabled); enable without `F0011` | stock ILM `0x0b0c` and live hardware 24.11 (`0.1.21`: DoP64 `70004=13`, DoP128 `70004=17`, `vol=28/28` and `00/00`) | measured |
| Standby and resume | standby: `F0010` bit 0 clear, headphone power down, 10 ms, ASP power down; resume: `10010=99`, `80032=20`, power up ASP then HP, 12 ms, both back to 0 | A `dac48.rs`; resume matches `pop_free_seq2` of L | stock, but not reached in the found stock lifecycle |
| Power-down bits | ASP `0x40`, HP `0x10` in `0x20000` | L, A | stock |
| Negative control | address `0x32` must not acknowledge, twice, between two identity reads | A `i2c_backend.rs` | assumed (test design) |

### I2C0

| Fact | Value | Source | Status |
|---|---|---|---|
| Controller | base `0x20070000`, configuration `0x17800` = 100 kHz from 24 MHz | R, P | stock |
| Commands | start-write `0x1100 \| addr<<1`, byte `0x100 \| b`, stop `0x21FF`, start-read `0x1001 \| addr<<1`, receive `0x100`; pending flags `+0x0C`, status `+0x10`, data `+0x14` | A, R | stock |
| NACK and bus-error bits | an address nobody acknowledges (0x31 and 0x32 on 05.11) leaves pending `0x10` and status `0x18`; a successful read leaves pending 0 and status `0x08`, a successful address probe status `0x28`; pending bit 4 with status bit 4 is therefore a NACK. Pending bit 2 or completion with status bit 1 is kept as NACK from a related SDK, pending bit 4 alone as a bus error | measured on the case through `dev/i2c` and `dev/peek`; the rest A, SDK | measured for address NACK; the other patterns assumed |
| Bus setup (`i2c::SETUP`) | gate bit 4 and bit 12 of `0x20002000`, reset bit 12 of `0x20003000` with 1 ms, selector `0x20002028 = 1`, GPIO6/7 pads `0xC4` | P, A `i2c_backend.rs` | stock |
| Controller reachable | an op-list agent in RAM drove this controller on the case | RAM probe | measured |
| Timeouts | 2 ms per transaction, then 200 µs for the STOP; a failed STOP disables the controller until `recover` | A | assumed |
| First DAC write after the PLL switch | with the 2 ms budget the first write of the PCM start program (`CRYSTAL 0x020052` to `0x33`) timed out on the case; with 20 ms (`STREAM_I2C_BUDGET_US` in the bench) the start completes and the stream runs. The DAC probably stretches SCL while its clocks settle | measured 05.11 | measured; the actual stretch time is unmeasured |

### Board

| Fact | Value | Source | Status |
|---|---|---|---|
| DAC reset | GPIO34 pad `0x20110088`: `0x10` asserted, `0x210` released; 4 ms each way | P (A used 2 ms) | stock; the line's purpose is a hypothesis |
| Output enable | GPIO5 pad `0x20110014`: `0xD0` off, `0x2D0` on | R (`0x1880` writes `0x10/0x210` with `0xC0`) | stock; amplifier enable or mute is a hypothesis |
| Power writes before the DAC (`board::POWER_ON`) | `0x200020F8 = 0x106`, `0x20110300 = 1`, `0x20110310 = 0x400` | A `audio_hw.rs` only | assumed; purpose unknown |
| GPIO20 routing | the stock main loop toggles `0x20110050` between `0x10` and `0x210` | R | stock; not used by Kumi yet |

### I2S and DMA

| Fact | Value | Source | Status |
|---|---|---|---|
| I2S block | base `0x20140000`; control `0x08080D94` (PCM 32-bit slots), `0x08880E94` (hardware DoP serializer stripping `[31:24]` marker and shifting `[23:8]` at `16*fs`), `0x08C80F94` (hardware Native DSD shifting 32 bits/ch at `32*fs` on the same GPIO0–2 pins); `+8` control (`0x00030304` direct DMA to TX FIFO); `+12 = 6` interrupt enable, `+0x10` interrupt status, `+0x14` FIFO status, TX FIFO `+0x20`. DSD clock `0x20002078` from 44.1 kHz PLL (`45.1584 MHz`): `0xF3` (DSD64), `0x73` (DSD128), `0x33` (DSD256). Native DSD `DSD_U32_BE` words are 32-bit byte-swapped (`Flow::push_native_dsd_words`); DSD ring silence uses `0x69696969` | stock ILM `0xabb0` / `0x1e4d0` and live hardware 24.11 (`0.1.21` DoP64/128 verified) | measured |
| Pins | GPIO0–2 pads, function `0x46` | A | stock |
| Clock and reset | bits 14 and 0 of `0x20002000` and `0x20003000` | A | stock |
| DMA channel 2 | `0x20020200`: source, `+4 = 3`, destination, `+12 = 3`, length − 1, control `0x0C55FC06` (circular), remaining `+0x18` | A | stock |
| DMA channel layout | K names the words source, source slot offset (bits 31:22, 64-byte units), destination, destination slot offset, length (bits 19:16 slots − 1, bits 15:0 bytes per slot − 1), control, status (bits 19:16 remaining slots, bits 15:0 remaining bytes − 1) and version `+0x1C`; in its control word, bits 11:10 are the source width (3 = 32 bits), 15 reload, 16 destination is a device, 18 linked-list mode, 22:20 the destination request line. `0x0C55FC06` therefore selects request line 5 and sets bit 18 and the unnamed bits 26, 27, 14 and 2 | K | assumed (sibling chip); request line 5 is the first suspect if the probe reports `Pace` |
| DMA enable and status | enable `0x20021000` with a write mask in bits 31:16, interrupt enable `0x20021004`, status `0x20021008` bit 2 (write 1 to clear); `0x2000801C` bit 2 is the stock firmware's copy of the interrupt-enable word (it copies it into `0x20021004`), not request routing. Control `0x0C55FC06`: source memory, incrementing, 32-bit; destination device, fixed, 32-bit; destination request line 5 (I2S0 TX) in bits 23:20; priority 7; circular bit 15. Stock writes the control word before the length | A; R shows the status register in the DMA interrupt handler; stock `ilm.dis` 0x7F00, 0x7FB8, 0x7FE4 | stock |
| Buffer | 16-byte aligned, at most 64 KiB (16-bit remaining count), whole stereo frames of 8 bytes | A | stock |
| Halt | clear I2S enable, mask the channel, wait until it is off and the count is still for 32 reads, at most 4096 reads | A | assumed |

### USB Audio Class 2

| Fact | Value | Source | Status |
|---|---|---|---|
| Stock speed and endpoints | High Speed; async OUT `0x07`, feedback IN `0x83`, 16.16, data interval 125 µs | S | measured |
| Stock alternate settings | 1 S16, 2 S24 (3 bytes), 3 S32, 4 raw data (`bmFormats` bit 31, Linux shows DSD raw) | S, R | measured |
| Topology | clock source 1, input terminal 3 (USB streaming), output terminal 4 (headphones `0x0302`), feature unit 10 with master mute and per-channel volume, category `0x0A` | R | stock |
| Clock source attributes | stock `0x07` (programmable and SOF-synchronised); Kumi `0x03`, because the stream is asynchronous | R, U | assumed (Kumi) |
| Volume range | −60…0 dB in 0.5 dB steps | R | stock |
| Clock validity | always valid unless calibration failed: Linux checks validity right after SET_CUR and gives up with `ENXIO`, and the stock firmware answers 1 | L `clock.c`, R | assumed (Kumi) |
| Rate change | SET_CUR takes effect for the next stream start; Linux sets the rate with the interface at alternate 0 and selects the alternate afterwards | L `endpoint.c`, R | assumed (Kumi) |
| Feedback | Full Speed 10.14 in 3 bytes every frame, High Speed 16.16 in 4 bytes every 8 microframes; correction up to ±0.1 % from the ring level | U, S for High Speed, A for the correction | assumed for Full Speed |
| Lock delay | 2 decoded samples | R | stock |
| Kumi function | interfaces 2 (control) and 3 (streaming) after the keyboard (0) and the vendor interface (1); OUT `0x07` and feedback IN `0x84`; alternates 1–3 (S16, S24, S32) plus High-Speed alternate 4 (`FORMAT_RAW_DATA` `bmFormats=0x80000000`, 32-bit Native DSD) at 44.1–384 kHz (`44.1–96 kHz` at Full Speed) | Kumi (`uac::config`), live USB enumeration 24.11 | measured |
| Kumi data interval | High Speed `bInterval` 2 for the data endpoint: one packet per 250 µs (2 microframes), up to 97 frames (388/582/776 bytes for S16/S24/S32/raw DSD); feedback stays 16.16 per microframe every 8 microframes. With the 60 MHz Core PLL, Andes L1 I-cache and RT40 `.storage`, the polling main loop runs at 21,876–34,674 polls/s at 352.8–384 kHz (`ur=0, ov=0, lost=0`) | live macOS CoreAudio 24.11 (`0.1.20`/`0.1.21`) | measured |
| MUSB isochronous endpoints | TxCSR and RxCSR bit 14 (ISO) with Mode bit 13 on IN as for bulk, OverRun bit 2 on OUT and UnderRun bit 2 on IN, double packet buffering with bit 4 of RxFIFOsz; the OUT FIFO is 2 × 1024 bytes and the whole FIFO plan ends at byte 3168 of 4096 | W, A, live hardware 24.11 | measured |

Totals: 69 facts, 20 measured, 35 stock, 14 assumed.

## Bring-up order for M4

Each step starts from the case running the Kumi runtime from RAM, with the developer endpoints
unlocked. Each step lists what to capture; a step that disagrees with this page stops the sequence,
and the measurement goes into the table and the tests before the next step.

1. **Clocks.** Read `0x20002010` and `0x21001000` and check that `clock::read_cpu_hz` gives the
   `FrameClock` measurement (60 MHz expected). Run `clock::quiesce_timers`, then
   `Calibration::run`. Capture both trim windows (16-bit masks), the chosen trims and the raw
   reference/measured counts of one sweep; expect one contiguous window per family and reference
   counts near 3120 (24 MHz over 130 µs). Repeat after a cold start to see whether the trims move.
   `kumi ... audio calibrate` runs this step in the runtime through `dev/audio`: both registers,
   `read_cpu_hz` next to the `FrameClock` measurement (fed on USB frame edges since start-up), the
   quiesce result, every reference/measured pair of both sweeps with the bounds each one is checked
   against, the windows, the trims, the PLL registers afterwards and the duration. `Calibration::run`
   is one blocking call, so USB and the keyboard UART are not serviced for those ~105 ms.
2. **I2C probe of the DACs.** Apply `board::POWER_ON`, `i2c::SETUP` and check `SETUP_EXPECT`; pulse
   `board::DAC_RESET_PULSE`. Run `dac::validate_pair`. Capture the six identity bytes, the pending and
   status words of the NACK from `0x32` (`Controller::last_fault`), and, if the DACs do not answer, a
   bus scan of `0x08..0x77` on both controllers. Read back `0x31` volume registers to settle the
   volume-target question.
   `kumi ... audio dac-probe` runs this whole step from the host over `dev/peek`, `dev/poke` and `dev/i2c`
   with the constants of this crate, and leaves the board in `board::QUIET`; the bus scan covers controller
   0 only, because controller 1 has no setup sequence yet.
3. **I2S and DMA with silence.** Run `stream::Start::pcm` at 48 kHz with `Volume::MUTED`, prefill
   the ring with zeros, `I2sDma::start`. Capture the cursor every 10 ms for one second: frames per
   second must equal the rate within the crystal tolerance, `wraps` must step once per ring period.
   Check `0x20140010` and `0x20021008` for error bits. Then `stream::stop` and confirm the channel
   halts within the poll bound.
   `kumi ... audio silence 48000` applies `board::POWER_ON` and `i2c::SETUP` (checked against
   `SETUP_EXPECT`), polls `Start::pcm` from the main loop, one DAC transaction per pass, then starts
   `I2sDma` on a 2048-frame ring of zeros in DLM; the output stays disabled and the DAC muted. The
   main loop reads `I2sDma::cursor` on every pass and publishes `mcycle`, frame, `wraps`, polls and
   errors in a monitor block. `kumi ... audio cursor 1` peeks that block and the DMA remaining count
   every 10 ms, prints each sample, then frames per second by the host clock and by `mcycle`, the wraps
   counted by the firmware and from the register, and both status registers before and after.
   `kumi ... audio stop` runs `stream::stop` and `board::QUIET` and reports whether the channel is off
   and its count still. The stream also stops when developer mode expires or the bus resets.
4. **Tone.** Fill the ring from a generated 1 kHz sine at −20 dBFS, first on the left channel only,
   then the right, then both; unmute with `dac::volume`. Capture the analog output (phone recording
   through a headset adapter or a scope): level, channel order (settles the `0x90001`/`0x90002`
   mapping), clicks at start and stop. Repeat at 44.1 kHz to exercise the family switch.
   `kumi ... audio tone 48000 left` (then `right` and `both`, and `audio tone 44100 left`) does the
   same with whole periods of the sine (`bringup::Tone`), so the circular DMA repeats the ring with no
   refill; after `I2sDma::start` it enables the output and sets both DAC volumes to -40 dB with
   `dac::volume`, so the output is the digital level 40 dB lower (about −60 dBFS by default). `Start::pcm`
   would program both DACs; on 05.11 `0x31` did not acknowledge, with or without headphones, so the bench
   identifies both after the reset pulse and starts on the DACs that answer (`dac::answering`,
   `dac::only`; `AudioStream.dacs`), and refuses with `NoDac` when `0x33` is silent. While the tone plays,
   `kumi ... dev i2c 0 0x33 09000101ff` mutes `0x90001` alone, which tells which channel it controls.
5. **USB UAC2, higher rates, RV32F EQ and DSD (24.11 hardware acceptance).**
   **Status: verified live on the case from flash (`0.1.20` and `0.1.21` lab builds, macOS CoreAudio host).**
   - PCM 44.1–384 kHz (`S16`/`S24`/`S32`, `bInterval = 2`, 250 µs packets) locks within ±2 ppm on `mcycle`
     with `underruns = 0, overruns = 0, lost_packets = 0, dac_faults = 0`. Periodic SPI-NOR settings
     autosave (`SaveTrigger::Tick`) is deferred while `uac::Stream::streaming()` is true so 4 KiB sector
     erases (~50 ms) never stall playback.
   - Hardware RV32F 10-band parametric EQ (`kumi-dsp` `rv32f` feature emitting `fmadd.s`, `fmsub.s`,
     `fcvt.w.s rmm`) runs live with measured per-rate budgets (`max_eq_bands`): 10 bands up to 96 kHz
     (`24,655 polls/s` @ 44.1 kHz, `23,352` @ 48 kHz, `9,942` @ 88.2 kHz, `7,387` @ 96 kHz), 4 bands at
     176.4 kHz (`6,068 polls/s` S16, `1,487` S32 2-shelf+2-peak), 3 bands at 192 kHz (`7,469 polls/s`),
     and automatic bit-perfect bypass above 192 kHz (`21,876–28,678 polls/s` @ 352.8/384 kHz) and in DSD.
   - DoP (`DopDetector` on S24/S32, `0x05`/`0xFA` markers) switches transparently between PCM and hardware
     DSD on live silicon: DoP64 (`176.4 kHz`, `I2S=0x08880e94`, `CLK=0xF3`, `DAC 70004=0x13`, `34,674 polls/s`),
     DoP64 Direct (`vol=00/00`, `34,671 polls/s`), DoP64 S24 (`32,961 polls/s`), and DoP128 (`352.8 kHz`,
     `CLK=0x73`, `DAC 70004=0x17`, `25,581 polls/s`), all with `ur=0, ov=0, lost=0, pauses=0`.
   - Native DSD64/128/256 (`Alt 4 FORMAT_RAW_DATA`, `bmFormats=0x80000000`, `I2S=0x08C80F94`,
     `CLK=0xF3/0x73/0x33`, `DAC 70004=0x13/0x17/0x1B`, 32-bit byte-swap `push_native_dsd_words`) is
     enumerated live on USB (`if=3 alt=4`) and verified in the emulator; live Linux ALSA `DSD_U32_BE`
     streaming remains open until tested against Android/Linux.
   - Non-lab flash builds still gate UAC2 on `board::raw_access()` (`usb_audio_present: false` from flash)
     until headphone jack detection is validated with real headphones so Android does not route audio to an
     unplugged case.

## The equalizer: Q28 or f32

The stock firmware runs eight direct-form biquads in fixed point (A `equalizer.rs`: Q28 coefficients,
64-bit accumulation with an error-feedback residue). Kumi uses `kumi-dsp`: state-variable sections
designed in f64 and run in hardware single-precision f32 (RV32F, measured on silicon).

Enabling `-C target-feature=+f` together with `kumi-dsp`'s `rv32f` feature (explicit inline `fmadd.s`,
`fmsub.s` and `fcvt.w.s rmm` on `riscv32`), moving the 1 KiB stereo scratch buffer out of the stack frame,
interleaving dual-channel FPU dependencies (`t1l/t1r/t2l/t2r`) and specialising the Peak filter path makes
f32 far faster than Q28 on the Andes D25F. Measured live on 60 MHz silicon (`0.1.20`/`0.1.21`), the
runtime enforces exact per-rate band budgets (`max_eq_bands`: 10 bands ≤ 96 kHz, 4 bands @ 176.4 kHz,
3 bands @ 192 kHz, automatic bit-perfect bypass > 192 kHz and during DSD) with zero underruns and zero
double-precision or `stage0`/`probe` FP instructions.

## Integration in the runtime

- `Registers`: volatile 32-bit reads and writes, followed by `fence iorw, iorw` as the archive did,
  and `delay_us` as an `mcycle` busy-wait using `clock::CycleTime` at the measured CPU clock. The
  same value converts `mcycle` into the `now_us` that every stepper takes.
- `I2cBus`: `i2c::Controller` over that `Registers` at `I2C0_BASE`, after `i2c::SETUP`.
- `FrameStore`: the DMA buffer, a static 16-byte aligned array of `[i32; 2]` in DLM written with
  volatile stores; its address and size make the `DmaBuffer`.
- Before USB connects: `quiesce_timers`, `board::POWER_ON`, `Calibration::run`, `i2c::SETUP`, the
  reset pulse, `dac::validate_pair`, then `board::QUIET`. Calibration takes about 105 ms. The runtime
  instead calibrates at the first stream start (`audio::Bench::calibration`, shared with the bench) and
  identifies the DACs at every start with `dac::answering`, because `0x31` never answers on this board
  and calibrating at every boot would cost every boot 105 ms.
- USB: the descriptor block from `uac2::function_descriptor` (split with `uac2::Walk` for an
  `embassy-usb` builder); class requests to `Control::control_in` and `Control::control_out`;
  `set_alternate` from the handler's alternate-setting callback.
- Stream start (alternate 1–3): `stream::Start::pcm_on` polled each loop until done, then prefill the
  `Flow` with host packets, `I2sDma::start`, `I2sDma::learn`, `board::output(true)`. Each loop: read the
  OUT endpoint into `Flow::push_packet`, `I2sDma::cursor` into `Flow::observe`, and whenever the IN
  FIFO is free send `flow::encode_feedback(flow.feedback(nominal))`. An underrun zeroes the ring with
  the DMA still running, and the next packet calls `Flow::resume`; alternate 0 ends in `stream::stop`.
- Volume and mute from the host or the settings: `dac::Volume::from_settings` and write-only
  `dac::volume_fast` / `dac::dsd_volume_fast` over 400 kHz I2C0 (`i2c::CONFIG_400KHZ`) with CS43198
  `PCM_PATH_1 = 0xB0` (Soft Ramp + Zero Crossing) and MUSB FIFO draining before and after each I2C step
  so volume changes never overrun the 500 µs ISO double-buffer (`overruns = 0` at 352.8 kHz S32).
  Counters map to the protocol with `Counters::icd`, and live stereo peak/RMS/limiter levels are reported
  via `audio/meters`.

The runtime enumerates at High Speed and its MUSB driver has isochronous endpoints (`usb.rs`: ISO bit,
OverRun and UnderRun reporting, a double-buffered OUT FIFO, `Endpoint::receive` and `Endpoint::offer`
for the main loop). embassy-usb enables and disables endpoints per alternate in descriptor order, so
an endpoint shared by alternates 1–3 would end disabled for alternates 1 and 2; the UAC2 handler sets
both audio endpoints again in its `set_alternate_setting` callback, which runs last. The archive's
`usb_device.rs` used endpoints 4 and 7 (FIFO offsets 256 and 512, CSR values `0x6048` and `0x5090`);
Kumi uses 7 and 4 with its own FIFO plan. Cold setup code and crates (`kumi_keymap`, `kumi_kbdlink`,
`kumi_keymap::hid`, `kumi_fw::app`, settings/control/service) live in the independent 256 KiB RT40 SRAM
(`.storage` at `0x4000_0000`, `runtime.x`, ~123 KiB free), keeping the hot streaming, EQ, DSP, DoP,
WFI and MUSB FIFO paths in zero-wait-state ILM (`47.1 KiB` used, ~18 KiB free) and the DMA ring and
stack in DLM (~67 KiB free above the 64 KiB stack guard).

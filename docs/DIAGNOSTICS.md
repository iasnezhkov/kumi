<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Diagnostics

First runs of new firmware will fail. Every failure must leave a trace, and the case must return
by itself to a place where the trace can be read.

## Crash record

Three 256-byte areas sit below the boot ROM's service-mode stack (`0x138000..0x140000`), outside
every window an image may load into:

| Address | Area | Written by |
|---|---|---|
| `0x137D00` | last crash: a copy of the current record when it describes a crash | stage0 |
| `0x137E00` | `BootInfo`: slot, safe mode, boot count, image and stage0 versions | stage0 |
| `0x137F00` | current record for this boot | stage0 starts it, the firmware continues it |

The record, written by `kumi-diag`:

| Field | Meaning |
|---|---|
| boot count | ties the record to one boot of the journal |
| crashes before | consecutive crashes before this boot, counted in RAM |
| stage, trail | the current start-up stage and the last 16 stages |
| reason | none (`0`), panic (`1`) or trap (`2`); other values are not produced and a record carrying one is rejected |
| `mcause`, `mepc`, `mtval`, `ra`, `sp` | where it happened |
| uptime, message | when, and a UTF-8 message cut on a character boundary |

Each area carries a magic and a CRC32, so RAM left over from a power cycle or from the stock
firmware is never taken for a record.

stage0 marks its own stages (journal, verified, loaded, jump, disarmed, or boot ROM) in a fresh record
and the firmware attaches to it. Every mark is copied into RAM at once, so a stage0 trap leaves the
stages reached so far. Before starting the fresh record, stage0 copies a record that describes a crash
into the last-crash area, so a crash survives the next boot.

The firmware writes that last crash to the crash log in flash (`0x240000`, `kumi-diag::log`) when it
reaches its health point: 512-byte entries with a sequence number and a CRC, eight to a sector, used
as a ring in which the oldest sector is erased first; a crash already in the log is not written again.
`system/diagnostics` reports the last crash from RAM, or from the newest log entry when RAM holds none,
for example after a power cut.

On a panic or a trap the firmware fills the current record and resets the chip without arming the
boot ROM, so stage0 decides what happens next. The trap handler switches to its own 1 KiB stack at the
top of DLM and points `mtvec` at the reset path first, so a fault while recording still resets. The
runtime arms the SPV5048 hardware watchdog (`0x20003020`, fed on every main-loop iteration) and
reports `watchdog_armed: true`, so a main-loop hang resets cleanly into stage0 without waiting for a
power cut. In safe mode the
firmware ignores stored settings and starts with factory settings: the keyboard link stays up, with the
default key map, because the case is a keyboard first and the stored settings are the likely cause of
a crash loop; if the link itself kept failing, safe mode would fail too and lead to the boot ROM. Safe
mode never saves on its own, so changes made while it runs cannot overwrite the stored settings; an
explicit `settings/save` still writes the current settings, which is how a user replaces settings that
crash the firmware. It serves the last crash over USB (`system/diagnostics`); in the boot ROM the host reads both areas with
`012A`/`012B`.

Whether this RAM survives a warm reset has not been measured (gate G4), so nothing depends on it
alone. The journal counts every start of a confirmed image as an unhealthy start, and the firmware
clears that count once it reaches its health point: USB configured without interruption for
30 seconds, counted in USB frames. A crash, a hang that ends in a power cut, or a power cut before the
health point all count; a crash after the health point does not, because the case was in use. Trial
images use their attempts in the same way. The RAM record adds the details (`previous boot crashed`,
`crashes counted in RAM`) and stands in for the journal when the journal cannot be written.

## Unhealthy starts and safe mode

| Image | What repeated failures before the health point lead to |
|---|---|
| trial (a new update) | three starts; then the slot is invalid and stage0 returns to the confirmed slot. A trial never runs in safe mode. A trial that reaches its health point confirms itself |
| confirmed | after two unhealthy starts, safe mode; after four, the boot ROM; the next start after the boot ROM is in safe mode again |
| trial with nothing to fall back to (first install) | three starts, then the boot ROM, where the installer can write again |

The count resets at the health point, when stage0 falls back to the other slot, on
`system/reboot OtherSlot`, after an update is written and on `system/confirm`. A safe-mode start that
reaches the health point clears the count too, so the next start tries the stored settings again; a
case whose stored settings crash it alternates between two failed starts and one safe-mode start until
a host fixes the settings. A safe-mode start requested by the host is not provided: it would need a
reboot target in the protocol that does not exist.

When the journal cannot be written, stage0 refuses to start a trial image (its attempts could not
be counted) and starts a confirmed one. The crash count kept in RAM then takes the journal's place:
two crashes give safe mode and four the boot ROM.

Every request for the boot ROM is first recorded in the journal; a second request in a row, a request
that cannot be recorded, or a trap or panic in stage0 makes stage0 disarm its own header
(`docs/FLASHING.md`, "Entering the boot ROM").

A flash read error while copying an image into RAM, or bytes that no longer hash to the verified
digest, send the case to the boot ROM instead of jumping into a partly loaded image: stage0 reads the
header again, compares it with the one it verified, and hashes every chunk as it copies it.

## stage0 report

stage0 leaves a 64-byte report at `0x137E40` on every start, so the firmware and the host can see
what it decided and why. The dry run for gate G2 (`docs/FLASHING.md`) makes every decision, writes
nothing to flash and leaves the same report:

| Bytes | Field |
|---|---|
| 0..4 | `KS0R` |
| 4 | outcome: 1 jump, 2 would jump, 10 no bootable slot, 11 crash streak, 12 journal unreadable, 13 journal unwritable, 14 load failed, 15 the flash controller would not pause, 16 no valid public key, 17 the boot ROM routines read different header bytes than the `0x48000000` window |
| 5, 6 | journal found, journal stored |
| 7 | bit 0: a boot ROM request was pending at entry; bit 1: this start recorded one; bit 2: stage0 disarmed its header |
| 8..12 | entry point |
| 12..44 | `BootInfo` |
| 44, 45 | slot A and B check: 0 not checked, 1 valid, 2 invalid |
| 46, 47 | trial attempts left in A and B |
| 48..52 | boot count |
| 52 | unhealthy starts in the journal |
| 53, 54 | previous boot crashed, crashes counted in RAM |
| 56..60 | the flash word at offset 0, read back through the boot ROM during the dry run (0 on a real boot) |
| 60..64 | CRC32 of bytes 0..60 |

A trap or a panic during the dry run or the G2b session returns to the boot ROM with `0xEF` or `0xEE`
in `a0` and leaves a `KS0T` record (`mcause`, `mepc`, `mtval`, `ra`, `sp`) or a `KS0P` record (length
and message) at the same address, sealed with the same CRC. The verify entry the runtime calls (`0x128008`) returns the
same two codes, without a report, so a fault inside it is a failed check rather than a reset. During a
real boot, stage0 instead records the trap or panic in the current crash record, disarms its header
and enters the boot ROM.

### G2b session report

The G2b entry (`0x128010`, called from service mode with `012C` like the dry run) runs the flash session
of a real boot: it reads the identity bytes `0..0x30` through the `0x48000000` window, pauses the flash
controller, enables the SPI clock, initializes the boot ROM routines, compares the identity with what
the boot ROM read routine returns, and at the end de-initializes the routines and releases the
controller. Between the two it writes only through the scratch writer: if the sector at `0x3F0000` reads
erased, it erases that sector, programs one page there and reads it back, and erases the sector again.
The page holds the OR of the flash bytes at the target's offset within the window, its page offset and
offset 0, which for `0x3F0000` are all offset 0: it is a copy of the first page, so a program that lands
there changes nothing. A sector that does
not read erased is left untouched. The session then returns to the boot ROM with the outcome in `a0`
and leaves a 64-byte `KS0W` report at `0x137E40`; it changes no other RAM outside stage0's window:

| Bytes | Field |
|---|---|
| 0..4 | `KS0W` |
| 4 | outcome: 0 passed, 15 the flash controller would not pause, 17 the boot ROM routines read different identity bytes than the window, 18 the scratch sector was not erased, 19 an erase or the program did not read back, 20 the first 4 KiB seen through the window changed |
| 5 | steps reached: bit 0 controller paused, 1 identity matched, 2 sector erased before, 3 first erase verified, 4 page programmed and read back, 5 second erase verified, 6 first 4 KiB unchanged |
| 12..16 | CRC32 of flash `0..0x1000` through the window before the session |
| 16..20 | the same after the session |
| 24..28 | `mcycle` cycles from the controller pause to its release |
| 60..64 | CRC32 of bytes 0..60 |

Other bytes are zero. The boot ROM initializes its own routines again for every flash command, so service
mode keeps working after the session.

## At run time

Logs and counters stream over the vendor interface. Release slot builds disable developer endpoints
(`Unsupported`, `developer: false`). In `"slot-lab"` or RAM-loaded `"ram"` builds, developer
endpoints are available only after `dev/unlock`, for the 1 to 600 seconds it names, counted in USB
frames; a USB bus reset locks them again. Locked, they answer `NotUnlocked`. Only RAM/lab builds
report the `developer` capability; `dev/uart/send` is lab-only.

| Endpoint | What it does |
|---|---|
| `dev/fault` | answers `Ok`, then, once that reply has been written to the host, executes an illegal instruction (`unimp`, `mcause` 2), an `ebreak` (`mcause` 3) or panics with `developer fault`. The crash goes through the normal path below: the current record is filled and the chip resets without arming the boot ROM. If the reply cannot be written, for example after a bus reset, no fault is raised |
| `dev/flash/test` | only in a runtime loaded into RAM by a host (`a0` at entry was not the `BootInfo` address and no `BootInfo` was accepted); started from a slot it answers `WrongState`, because the window lies inside Kumi's settings region. It checks the flash identity `C8 65 16`, then works through the scratch writer, which reaches only `0x3F0000..0x400000`: CRC32 of flash `0..0x1000`; whether the whole window reads erased, and if not it stops without writing; a 256-byte page at `0x3F0000` and 4 bytes at `0x3F012C`, each read back (the second with its whole page); an erase of the 4 KiB sector at `0x3F0000`; a page at `0x3FF000` and an erase of the 64 KiB block, read back; CRC32 of `0..0x1000` again. Programmed bytes are the OR of the flash bytes at the target's window offset, its page offset and offset 0, as in `tools/flash_gates.py`, so a misplaced program changes nothing; a check whose pattern would be all `FF` counts as failed, and `block_erased` needs the page at `0x3FF000` to have been programmed first. Durations are `mcycle` cycles of the boot ROM call alone. The runtime does not poll USB or the UART while it runs (about 0.3 s on hardware from the G7 timings) |
| `dev/audio` | the M4 bring-up bench (`docs/AUDIO.md`), only in a runtime loaded into RAM by a host or in a lab build, and while unlocked. `Calibrate` runs `clock::quiesce_timers` and `Calibration::run` in place, about 105 ms in which the runtime does not poll USB or the UART, and records every timer count it reads. `Start` checks the rate, the tone and a calibration, fills the DMA ring, applies the DAC power and I2C setup and returns `Starting`; the main loop then advances `stream::Start` one step per pass and starts the DMA. `Status` returns the bench; `Stop` halts the DMA, mutes the DAC and leaves the board quiet. Hardware failures answer `HardwareFault` and keep the details for `Status`; a second start while one runs answers `Busy`. While a UAC2 stream from the USB host starts or runs, `Status` reports that stream (its ring, monitor block and failure) and the other requests answer `Busy` |
| `dev/poke`, `dev/i2c` | only while unlocked, and only in a runtime loaded into RAM by a host or in a lab build; otherwise `WrongState`. Poke reaches `0x20000000..0x22000000` except the keyboard UART, the flash controller and flash window block, the serial flash controller and the USB controller (`InvalidArgument`). From a slot, a lab build also refuses the chip id and boot gate block `0x20000000..0x20001000`, the system clock `0x20002010`, the SPI clock `0x20002020`, the USB clock `0x20002090`, the reset request `0x20003024`, the USB PHY block `0x20006000`, the DMA controller `0x20020000..0x20022000` (it could copy into any refused address), the core PLL `0x21001000`, `0x21004010` and the boot flag `0x2100801C`, and in the clock gate `0x20002000` and reset `0x20003000` any mask that includes bit 7 (serial flash) or bits 20 and 27 (USB). I2C from a slot in a lab build: probes and one-byte register reads (one byte written, one read) at any address on controllers 0 and 1; plain writes and longer prefixes only to the DACs `0x33` and `0x31` on controller 0, because the charger chip is unidentified. After a slot boot no firmware code has set up either controller; I2C0 needs `i2c::SETUP` applied through poke first (as `audio dac-probe` does), and I2C1 has no known setup |
| UART mirror | `events/subscribe` grants `uart` only while unlocked. Raw bytes from the keyboard UART are then sent as `dev/uart/mirror` blocks of up to 64 bytes: a block closes when it is full, when a USB frame passes without a new byte, or before any request is handled, so a reply is still a barrier for events. Up to 8 blocks wait for the host; a block that does not fit is dropped and counted in `dropped_events`. Key processing is unchanged. The mirror stops when developer mode expires |

## Lab instrumentation

A lab build keeps three structures in RAM that `dev/peek` reads at their ELF symbol addresses, decoded
by `tools/lab_read.py` (`--section boot|journal|control|all`):

- `KUMI_LAB_BOOT`: the values of the clock, PLL, GPIO, UART and audio registers as the boot ROM and
  stage0 left them, captured once at `kumi_main` entry before the firmware touches them, with the entry,
  USB-ready, running and first-configured cycle counts. It is the cold-boot baseline for the keyboard
  cold-start question and the GPIO map.
- `KUMI_LAB_JOURNAL`: a wrapping log of the last 64 events with cycle-derived timestamps — the boot,
  stage marks, keyboard-link frames both ways, link length and checksum errors, USB reset/suspend/
  resume/configure, VBUS changes, register-watch hits, the measured CPU kHz and the knob — plus a
  written and a dropped counter. The log is sampled inside the main loop, so it records what the case
  saw as it went down without the host polling it awake; it survives a USB-link detach but not a power
  cut, which `diagnostics` (uptime and boot count across a wake) tells apart.
- `KUMI_LAB_CONTROL`: a control block the host writes with `dev/poke` while unlocked. Word 1 is the
  fake-stream knob — a non-zero sample rate makes the firmware answer the keyboard MCU's stream-status
  query as if a stream were running, with no audio, for the sleep experiment. Word 2 is the per-second
  watch budget, and the 16 following pairs are the register address and change mask each watch slot
  samples; three GPIO-input slots are armed by default.

## The M1 probe

`firmware/probe` is a 5 KiB image that the boot ROM loads into ILM at `0x2000` and calls like a
function; it returns to the boot ROM, so the service session stays alive. It installs its own trap
handler for the duration, so reading a missing CSR or register cannot hang the chip, disables
interrupts and restores everything before returning. It never touches the flash. The same image
carries the gate G4 entries at `0x2004..0x2014` (`docs/FLASHING.md`, "First bring-up session").

The 1 KiB report at `0x8000` (layout 3; `tools/probe.py decode` also reads the 512-byte layouts 1
and 2) holds:

- `misa` and the Andes CSRs (`micm_cfg`, `mdcm_cfg`, `milmb`, `mdlmb`, `mmsc_cfg`, …), which give the
  ILM and DLM sizes and bases;
- whether there is an FPU: the `F` bit and a trial `fadd.s`;
- whether the core can fetch instructions from DLM, by running a routine copied to `0x102000`
  (`dlm_exec_ok` or `dlm_fetch_trapped`), which decides whether the DLM-resident stage0 can run;
- whether atomics work: the `A` bit, a trial `amoadd.w` and an `lr.w`/`sc.w` pair on a DLM word,
  with the `mcause` of a trap, which decides the Rust target and how firmware gets atomics;
- the CPU and machine-timer frequencies, measured with `mcycle` and `mtime` against 32 USB frames,
  and `minstret` against `mcycle` in a plain loop and over those 32 frames;
- whether any word of the timer block `0x20010000..0x20010080` changes over 120 000 cycles and how fast
  against `mcycle`, and timer 0 started the way the stock firmware starts it (`0x20010004 |= 0x403`,
  reload `0x20010024`, control `0x20010020 = 3`), sampled three times and restored and read back;
- the PLIC's feature, pending, trigger type, configuration, version, target 0 enable, threshold and
  preemption words and the priorities of sources 1 to 34; the claim register is never read;
- the boot ROM's stack pointer, `mstatus`, `mtvec` and `mie`;
- 30 clock, PLL, flash, USB, timer and boot registers.

```sh
cd firmware
cargo build --release
llvm-objcopy -O binary target/riscv32imc-unknown-none-elf/release/kumi-probe target/kumi-probe.bin
```

`tools/probe.py run` loads it (the ELF or the raw image) through a boot ROM bridge on an Android host
and decodes the report; `tools/probe.py decode` decodes a saved report. The Rust CLI will replace this
script.

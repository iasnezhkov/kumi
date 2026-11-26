<!-- SPDX-FileCopyrightText: 2026 Ilia Snezhkov -->
<!-- SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0 -->

# Flashing, updates and returning to stock

## What the boot ROM does

The boot ROM starts **only the image at flash offset 0**. Its "four attempts" are four SPI pin
mappings, not four flash offsets, so there is no A/B choice at boot ROM level.

| Header field | Rule |
|---|---|
| `+0x04` | must be `SPV@` |
| `+0x0C` bit 0 | enables the checksum |
| `+0x0C` bit 1 | entry alias: set selects `0x48000000`, clear selects `0x4C000000` |
| `+0x28` | number of bytes covered by the checksum |
| `+0x2C` | sum of all those bytes except this field, modulo 2³² |

stage0 carries the stock value `0x4F414D41` in `+0x0C`: the boot ROM enters it at `0x4C000000`, and
stage0, like the stock entry stub at `0x480000A4`, moves to the `0x48000000` alias with an absolute
jump. Entering at `0x48000000` directly has never been exercised by the stock firmware, so Kumi does
not rely on it.

A missing magic or a wrong sum sends the boot ROM into service mode (USB `0666:0888`), from which a
host can read, erase and program the flash and load code into RAM. Service mode is the only recovery
path: there is no SPI programmer for this case, and none will be used. The stock firmware sets bit 31
of `0x2100801C` and then bit 1 of `0x20003024` for its "reboot to service" command. Whether bit 31,
and the RAM that holds the crash records, survive each kind of reset the case goes through has not been
measured (gate G4), so nothing in Kumi relies on it alone: see "Entering the boot ROM" below.

Recomputing the sum of the stock image over `0x202AE` bytes reproduces its header value
`0x00BE33BF`, which confirms the model in `kumi-install::spv`.

## Gates before the first flash write

| # | Condition | How it is checked | State |
|---|---|---|---|
| G1 | code loaded into RAM runs and returns to the boot ROM; the core can fetch instructions from DLM; bit 17 of `0x20000010` is clear | the M1 probe; see below for the DLM test and bit 17 | done |
| G2 | stage0 makes the right decision on the real flash | the stage0 dry run, below | done |
| G2b | stage0's write session works on hardware: the flash controller pauses, the boot ROM routines read the same header bytes as the `0x48000000` window, and the scratch sector is erased, programmed, read back and erased | `stage0_dry.py g2b`: stage0's own session code runs from RAM through its test entry at `0x128010`, writing only `0x3F0000..0x3F0FFF`; step 10 of the bring-up session below | done 05.11: outcome 0, steps `0x7F`, head CRC `0xa8ff5a7f` before and after, 174.5 ms. The first two runs hung because the test read the `0x48000000` window, which hangs the chip in service mode even with the controller unpaused (probe bisect, `tools/g2b_bisect.py`: every boot ROM helper step passes); the test now checks the head through the ROM read routine. At a real boot the window is live, the boot ROM itself jumps through it |
| G3 | an erased header sector or a zeroed `SPV@` magic leads to service mode with USB up after a cold start | in the host emulator, against the real boot ROM dump (`tools/brom_emu.py`, recovery matrix); never on hardware by erasing sector 0, because a failure there would have no way back | done in the emulator |
| G4 | bit 31 of `0x2100801C` and the RAM areas at `0x137D00..0x138000` survive the resets the case actually goes through (the reset bit at `0x20003024`, sleep, a power cut) | `tools/g4.py`: markers in RAM, bit 31 set, reset from RAM, see what comes back; rehearsed against the boot ROM in the emulator ("First bring-up session") | done 05.11: both software resets reach service mode with the RAM kept; a power cut boots stock with the RAM lost |
| G6 | the boot ROM programs pages at the address set by `0122` | program a test page in an unused region (`0x3F0000`) and read it back | done |
| G7 | the boot ROM programs a 4-byte area at an unaligned offset, and answers `0120` for one 4 KiB sector within the reply timeout | in `0x3F0000`: program 4 bytes at `+0x2C` and read back; time a one-sector erase and a 64 KiB erase | done: about 32 ms and 158 ms |

There is no G5: a flash programmer as a last resort does not exist for this case.

`kumi-install::plan_install` takes the gate evidence as an explicit input (`GateEvidence`) and refuses
to plan an installation without a record for G3, G4 and G2b (`PlanError::GateMissing`).
`GateEvidence::recorded()` lists what this tree has recorded: G3 in the emulator, with the boot ROM
dump's SHA-256, G4 on the case, and G2b on hardware.
The simulated case in `kumi-jni` and the tests use `GateEvidence::for_simulation()`.

`tools/flash_gates.py run` performs G6 and G7 through the phone helper's `scratch-bridge` mode, which
passes `0120` and `0121` only inside `0x3F0000..0x3FFFFF`. The helper is `tools/android/spv40-usb.c`; build
it for arm64 with the Android NDK (`aarch64-linux-android35-clang -O2 -o spv40-usb tools/android/spv40-usb.c`)
and copy it to `/data/local/tmp`; the same binary has the `bridge`, `scratch-bridge` and `kumi-bridge` modes. The tool checks that the identity reply is shaped `16 65 c8 00`
(GigaDevice, 4 MiB) and that the status shows no block protection, reads the whole 4 MiB, compares the
stock firmware area with the verified dump and checks that the scratch block is erased. All program tests
run before the first erase. Each programmed pattern is the OR of the current flash bytes at every offset a
faulty boot ROM could hit: the target with the `0x3F0000` base dropped, its page offset and offset 0. For
the 4-byte write at `0x3F012C` that is the OR of the words at `0x000`, `0x02C` and `0x12C`, so a misplaced
write leaves the flash unchanged. The first 64 KiB are compared after every write, and a second 4 MiB read
at the end must equal the first; the dump is not used for this, because the stock firmware writes its own
settings to flash. The default deadline is 1200 s, enough for both full reads.
Keep the host awake during the session: after a USB suspend the boot ROM stalls on resume and the case
needs a replug.

### Executing from DLM

stage0 runs from `0x128000`, in DLM. On an Andes core, instruction fetch from DLM is not guaranteed:
some configurations keep DLM load/store only. The probe settles the question directly: it copies a
two-instruction routine into DLM at `0x102000`, jumps to it under a trap handler that catches an
instruction-access fault and returns to ILM instead of looping, and reports `dlm_exec_ok` if the
routine ran or `dlm_fetch_trapped` if the fetch faulted. If DLM is not executable, stage0 must be
rebuilt to run from ILM with a small trampoline for the final copy into `0x0`; do not install the
DLM-resident stage0 until the probe shows `dlm_executable`.

### Bit 17 of `0x20000010`

Before its loader, the stock entry stub calls `0x48000060`, which does nothing unless bit 17 of
`0x20000010` is set; if it is, the stub programs the NOR controller at `0x20090030..0x2009003C`
from `0x20000024` and header words `+0x08` and `+0x10`. stage0 does not repeat this. The probe
reads `0x20000010` and those four registers; if bit 17 is set, stage0 must repeat the stock setup
before Kumi is installed.

### stage0 write path (G2b)

The dry run below never pauses the flash controller and never writes. Before the first real boot from
flash, a hardware session should exercise exactly what stage0's boot path does: read the header bytes
`0..0x30` through the `0x48000000` window, pause the controller, enable the SPI clock, initialize the
boot ROM routines, read the same bytes back through `0x8225E` and compare them, program and read back
the scratch sector `0x3F0000..0x3F0FFF` (erase, program a page whose bytes are no-ops wherever a
misplaced write could land, read back, erase), release the controller, and check the stock header
is unchanged. stage0 has a test entry for exactly this at `0x128010` that shares the boot's session
code; `tools/stage0_dry.py g2b` loads stage0 into RAM through the boot ROM, calls it and decodes its
64-byte `KS0W` report at `0x137E40`: outcome (0 passed, 15 the controller would not pause, 17 the
identity check failed, 18 the sector was not erased, 19 a write failed, 20 the stock head changed),
the steps that completed, the CRC32 of flash `0..0x1000` before and after, and the session time in
CPU cycles. It has run only in the emulator so far; on the stock case the expected result is outcome
0, steps `0x7F` and both head CRCs `0xa8ff5a7f`.

### stage0 dry run (G2)

The same stage0 binary that will be installed has a second entry for this test. From service mode:

1. write the stage0 image from offset `0x200` (its RAM part) to `0x128000` with `0128`/`0129`;
2. run `0x128004` with `012C`; the boot ROM calls it like a function;
3. read the 64-byte report at `0x137E40` with `012A`/`012B`.

The dry run reads the slots and the journal through the boot ROM's read routine, makes every
decision, verifies signatures, writes nothing to flash and changes no RAM outside its own window and
the report, and returns to the boot ROM with the report kind in `a0`. Its flash adapter has no
write session in this mode, so a write would fail rather than reach the chip. A trap or panic
returns too, with a `KS0T` or `KS0P` record in place of the report (`docs/DIAGNOSTICS.md`).

`tools/stage0_dry.py run --image stage0.bin ...` does these steps through the phone bridge and decodes
the report. On a case with the stock firmware the result is known in advance: the journal region
holds stock code and both slots are erased, so the report must say `no_bootable_slot`, journal not
found, no slot checked, boot count 1: bytes 0 through 55 of the report must equal, byte for byte,
what the same stage0 logic gives on the stock dump on the host
(`the_dry_run_on_the_real_stock_dump_predicts_gate_g2` in `crates/kumi-install/tests/pack.rs`, run
with `KUMI_STOCK_DUMP`). The flash word at bytes 56..59, which the binary fills by reading flash
offset 0 through the boot ROM, must equal the first word of the verified stock dump: `0x0A40006F`
on our case, the jump over the header that precedes `SPV@` at `+0x04`. This proves the boot ROM read
path returned real bytes, not zeros that would make a failed read look like the expected "no bootable
slot". A different decision, a wrong flash head, a trap or panic record, or no return to the boot
ROM fails the gate.

## First bring-up session

One prepared session of about 90 minutes with the case on the MindOne phone. Everything runs from RAM
through the boot ROM's service mode; the only flash writes are G2b and `dev flash-test`, both inside
`0x3F0000..0x3FFFFF`. Every command below is run from the repository root
on the host, and every result goes to a new directory outside the source tree:

```sh
S=<adb serial of the phone>
OUT=<new directory for this session>
PY=.venv/bin/python
PROBE=firmware/target/riscv32imc-unknown-none-elf/release/kumi-probe
FW=firmware/target/riscv32imc-unknown-none-elf/release/kumi-fw
H=/data/local/tmp/spv40-usb-<first 12 hex digits of the helper's SHA-256>
```

### Before the session, on the host

1. Run `check.sh full` with `KUMI_BROM_DUMP`, `KUMI_STOCK_DUMP` and `KUMI_G2_REPORT`; it must pass.
   Build the CLI (`cargo build --release -p kumi-cli`) and the helper
   (`aarch64-linux-android35-clang -O2 -Wall -Wextra -Werror -o $OUT/spv40-usb tools/android/spv40-usb.c`),
   and note `shasum -a 256 $OUT/spv40-usb`.
2. Rehearse G4 on the emulated case: `$PY tools/g4.py rehearse --brom $BROM --flash $STOCK --ram kept
   --image $PROBE --output $OUT/rehearsal-kept.json --power-cycle`, then the same with `--ram lost`.
   Expected: `gate_g4` `passed` and `bit31_kept_ram_lost`, `returned_to_stock` true in both; about a
   minute each.

### Phone and case

3. The phone is healthy and rooted (`adb -s $S root`). Hold a wake lock for the whole session, because the boot ROM
   stalls after a USB suspend: `adb -s $S shell 'echo kumi-case > /sys/power/wake_lock; cat
   /sys/power/wake_lock'` must list `kumi-case`. The tools also set `power/control` to `on` for every
   case device they use.
4. Copy the helper and read its hash back: `adb -s $S push $OUT/spv40-usb $H && adb -s $S shell "chmod
   755 $H && sha256sum $H" | tee $OUT/00-helper.txt`. The hash must equal the host's.
5. With the case attached and running stock: `$PY tools/phone.py --serial $S --helper $H devices >
   $OUT/00-devices.json` lists one `0661:0881`.

### Service mode and the probe

6. `$PY tools/phone.py --serial $S --helper $H enter > $OUT/01-enter.json` sends the stock firmware's
   own service request and waits for `0666:0888`; its `path` is `P` below. If nothing appears within
   20 s, unplug and replug the case and repeat once.
7. `$PY tools/probe.py run --image $PROBE --serial $S --helper $H --usb-path $P --output
   $OUT/02-probe.bin`. Expected: layout 3, status `done`, `cpu_mhz` 60, the CPI of the frame wait,
   `timer_block.ticking` (empty means no counter runs in service mode), `timer0_experiment` with
   `restored_ok` true and whether it `counted`, and the PLIC words; the emulator shows sources 27 and 34
   enabled. The probe returns to the boot ROM, so the case stays at `P`. Saved: the 1 KiB report and
   its decoded `.json`.

### Gate G4

8. `$PY tools/g4.py run --serial $S --helper $H --usb-path $P --image $PROBE --output $OUT/03-g4.json
   --power-cycle`, about five minutes. In order: `no_reset` (markers and bit 31 hold without a reset;
   otherwise the session stops), `software_reset_kumi` (markers at `0x137D00..0x138000` and
   `0xFF00..0x10000`, bit 31 set and bit 1 of `0x20003024` set the way `kumi_chip::reset` does it),
   `software_reset_stock` (the stock firmware's absolute writes), then a prompt: the owner unplugs the
   case for 30 seconds and plugs it back. After each reset the tool waits for the case, reads the markers
   before uploading anything, and reads `0x2100801C`, `0x20003000/20/24`, `0x21008000..0x2100803C` and
   `0x200C1000/04` again. A case that comes back as stock is put into service mode through the stock
   request to read what is left. The session always ends by clearing bit 31, sending `0124` and waiting
   for `0661:0881`. Expected output: the verdict and, when bit 31 brought the case back to service mode,
   an `evidence_line` for `kumi_install::GateEvidence` (`Gate::G4ResetRetention`, recorded as
   `G4_HARDWARE_EVIDENCE` from the session of 05.11). Saved: every record,
   marker comparison and timeline in `$OUT/03-g4.json`.
   - `passed`: bit 31 and the diagnostic RAM survive the software reset;
   - `bit31_kept_ram_lost`: the boot ROM request works and crash records do not survive; the emulator's
     `retention: lost` is the model to use;
   - `failed`: the reset from RAM did not reach service mode; stage0's boot ROM requests then rely on
     its disarm path and installation stays blocked;
   - a watchdog reset cannot be triggered yet, and sleep is a stock firmware behaviour; `--idle-seconds
     200` adds a trial that only shows whether the case leaves USB while it waits in service mode.

### Gate G2b

9. `$PY tools/phone.py --serial $S --helper $H enter > $OUT/04-enter.json` (G4 left the case on
   stock); its `path` is `P` again.
10. `$PY tools/stage0_dry.py g2b --image <stage0.bin packed by check.sh full> --serial $S --helper
    $H --usb-path $P --output $OUT/05-g2b.bin`. stage0's write session runs from RAM through `012C` at
    `0x128010`: it pauses the flash controller, compares the identity, erases, programs and reads back
    one page in `0x3F0000..0x3F0FFF` and erases it again, then checks the head of the flash. The key in
    the image does not matter for this test. Expected on the stock case: outcome 0, steps `0x7F`, both
    CRCs of flash `0..0x1000` `0xa8ff5a7f`, at least about 65 ms; exit code 0 only on a pass. The sector
    must be erased before (G6/G7 left it erased); otherwise outcome 18 and nothing is written. Without
    `--return-to-runtime` the case stays in service mode at `P` for the next step.

### The runtime from RAM

11. `$PY tools/runtime_ram.py --serial $S --helper $H --elf $FW --output $OUT/06-runtime.json` writes the
    runtime's segments with `0128`/`0129`, reads every byte back, zeroes `BootInfo` and jumps with `012C`.
    The boot ROM passes the entry address in `a0`, so the runtime never sees `BootInfo`: storage stays
    read-only and nothing is confirmed. Expected: `1209:0001` within seconds, at USB high speed with
    512-byte vendor endpoints (the helper reads `wMaxPacketSize` and prints it); its `path` is `K` below.
    About 80 KiB, so roughly 600 round trips.

### The CLI on the case

12. `KUMI="target/release/kumi --adb $S --helper $H --usb-path $K"`. `$KUMI info > $OUT/07-info.json`
    (running from RAM, protocol 1, capabilities), `$KUMI diagnostics > $OUT/07-diagnostics.json`,
    `$KUMI keys > $OUT/07-keys.json`, `$KUMI ping`. Exit code 3 means the firmware refused the request
    (`Unsupported`, locked, or an endpoint it does not serve) and is recorded, not a link failure. Each
    `kumi` run starts its own helper on the phone; claiming the interface does not reset the bus, so
    developer mode stays unlocked between runs.
13. `$KUMI events 60 > $OUT/08-events.jsonl` while the owner types for a minute: every key row,
    modifiers and the Fn layer. Expected: the start log line naming the USB speed, `keys/events` lines in
    press and release pairs, the text arriving on the phone, and a closing summary.
14. `$KUMI dev unlock 600` (then `info` shows the `DEVELOPER` capability), and `$KUMI uart-mirror 60 >
    $OUT/09-uart.jsonl` while the owner types again. Expected: `uart` lines of at most 64 bytes carrying
    the keyboard link's `4b 01 64` frames; dropped blocks show up in `diagnostics.dropped_events`. The
    mirror is granted only while developer mode is unlocked, and a USB bus reset locks it again.
15. `$KUMI dev flash-test > $OUT/10-flash-test.json`, only `0x3F0000..0x3FFFFF`, only in a RAM-loaded
    runtime and only while unlocked (refusals in that order: `WrongState`, `NotUnlocked`, then
    `HardwareFault` for a flash that does not identify as `C8 65 16`). USB and the UART stall for about
    0.3 s while it runs. Expected: the window was erased, the page and the unaligned four bytes match,
    sector and block erase, and the stock head CRC unchanged.
16. `$KUMI dev fault illegal > $OUT/11-fault.json`, the last runtime step. The runtime replies first,
    then executes `unimp` (`mcause` 2), records the trap and resets without arming the boot ROM; the
    flash still holds stock, so `0661:0881` returns. Then `phone.py enter` and `$PY tools/diag_read.py
    read --serial $S --helper $H --output $OUT/12-diag.bin`: with RAM kept across these two resets the
    current record shows reason `trap`, `mcause` `0x00000002`; with RAM lost every record is empty, which
    must agree with G4.

### Back to stock

17. `$PY tools/adb_bridge.py reboot --serial $S --helper $H --usb-path <path of the service device>`,
    then `$PY tools/phone.py --serial $S --helper $H wait 0661:0881 > $OUT/13-stock.json`. The owner
    checks that the keyboard types and audio plays. Release the wake lock:
    `adb -s $S shell 'echo kumi-case > /sys/power/wake_unlock'`.

### Abort and recovery

- Unplugging the case is always safe: the flash holds stock, a power-on start clears bit 31 in every
  model we have, and the case comes back as `0661:0881` within a few seconds.
- A case that keeps coming back as `0666:0888`: `$PY tools/g4.py stock --serial $S --helper $H --image
  $PROBE` clears bit 31, sends `0124`, waits for stock and tries twice; `tools/adb_bridge.py reboot`
  sends `0124` alone. If that fails too, unplug.
- A tool that hangs: stop it; the helper runs under `timeout -s KILL` on the phone and releases the
  interface when its stdin closes. One tool at a time, one service session per step, never toggle the
  USB `authorized` attribute.
- After an abort, start again from step 6, skipping the steps already recorded.

## Installing Kumi

Run from boot ROM service mode; `kumi-install::plan_install`. The plan starts from the live first
sector of the case, read when the installation starts. If it is the stock image, the table below
applies. If it is anything else, for example an installed Kumi, the plan first programs zeros over the
header magic and erases sector 0, so every later step leaves a case that enters service mode, never an
old stage0 that could start a half-written slot A. `kumi-jni` reads the live first sector before the
first step and refuses a backup that no longer matches it (`BackupStale`); a resumed installation plans
from the backup's first sector, so the resumed plan is the same plan.

| Step | Action | What boots if power is lost |
|---|---|---|
| 0 | checks: chip id, JEDEC manufacturer `C8` (GigaDevice) and 4 MiB capacity byte `16`, full id read live and recorded, block protection off, a full 4 MiB backup read twice, images signed with our key, stage0 with the checksum flag and the stock entry alias `0x4C000000` | stock |
| 1 | copy the stock 256 KiB to `0x300000`, verify | stock |
| 2 | write the Kumi image to slot A, verify; erase slot B's first sector so a leftover image cannot outrank the new one | stock |
| 3 | program zeros over the header magic, then erase sector 0, which removes the stock header | service mode |
| 4 | erase the rest of the old image and the journal, write the stage0 body, verify the body and the empty journal | service mode |
| 5 | write stage0's first sector page by page in reverse, header page last, with its checksum field left erased | service mode |
| 6 | write the 4-byte checksum, verify stage0 | Kumi |

Every erase is a single 4 KiB sector, so each boot ROM command finishes well inside the reply
timeout and every erase is read back. The header checksum is a plain byte sum, so a partly
programmed header page could otherwise add up by chance; while the checksum field still reads
`0xFFFFFFFF` no sum over 4 MiB can match it, and a partly written checksum never equals the true sum.
Clearing the magic before the erase makes the old header invalid in one direction only, whatever the
chip does inside an interrupted erase.

On its first start stage0 creates the journal and marks slot A as a trial with three attempts. The
firmware confirms itself once it reaches its health point (below); a host can confirm earlier with
`system/confirm`.
A NOR flash model cuts the power after every byte of an erase or a program at about a hundred
points of the plan. After each cut the boot ROM would start the stock image, enter service mode or
start a completely written stage0, never a partial image with a valid sum. The installation then
resumes from the start of the interrupted unit.

Installers take the stock image only from a head the boot ROM model starts and that is not a Kumi
stage0 (`PlanError::BackupIsKumi`), so a backup made after installing can never be written back as
"stock".

### From the command line

The `kumi` command line runs the same installer as the app (`kumi_jni::installer`) through the phone
helper. Build both for the phone (`aarch64-linux-android35-clang -O2 -Wall -Wextra -Werror -o
spv40-usb tools/android/spv40-usb.c`, and `cargo build --profile android --target aarch64-linux-android
-p kumi-cli` with the NDK linker), push them to `/data/local/tmp` and read their hashes back. Run them
on the phone with `--adb local`: through adb over Wi-Fi each 256-byte read costs a round trip of about
65 ms, so a 4 MiB backup takes 18 minutes instead of seconds.

With the wake lock held and the case in service mode at `P` (steps 3 and 6 above),
`K="/data/local/tmp/kumi --adb local --helper /data/local/tmp/spv40-usb --usb-path P --deadline 3000"`:

| Command | What it does |
|---|---|
| `$K brom backup BACKUP` | reads the whole flash twice, compares, writes `BACKUP` with fsync and reads it back; prints its SHA-256 and where stock is |
| `$K brom install PACKAGE BACKUP SHA256 KEY [FROM]` | checks the package signature against `KEY`, the backup against `SHA256` and the live first sector, then runs the plan above through the helper's `install-bridge` mode; `FROM` resumes at the step an interrupted run printed |
| `$K brom restore BACKUP SHA256 [stock\|exact] [FROM]` | writes stock back; `exact` also rewrites the rest of the flash and verifies all 4 MiB against the backup |
| `$K brom reboot` | leaves service mode |

Progress goes to stderr as one JSON line per step and the outcome to stdout. The helper's
`install-bridge` mode forwards erases (4 KiB sectors only) and programs to the whole flash and logs each
one; `bridge` stays read-only and `0127` is never forwarded. Over the boot ROM emulator with the real ROM
dump (`tools/brom_bridge_emu.py` as the helper) install took 1281 steps and the flash then booted
natively into slot A; `restore exact` took 1557 steps and left the flash equal to the backup.

### Firmware packages and the app

Releases ship one signed `.kumi` package (`kumi-pack bundle`): stage0, the slot image and an ed25519
signature over both. The Android app verifies a package with the public key built into it before
anything reaches the case, whether the package came inside the app or from a release page, and then
runs the steps above through `kumi-jni`: service mode, the double-read backup saved on the phone
with fsync and read back, the plan with per-step progress, the boot ROM reboot, and a wait for the
Kumi interface. After a failure the app shows what the case would boot and resumes from the start of
the interrupted unit over the same session. The app's demo mode runs the same code against a
simulated case.

### A lab image

A case kept for hardware research can run a lab build from its slot: `cargo build --release -p kumi-fw
--features lab` in `firmware`. It reports the `lab` capability, the build `slot-lab` and the USB product
`Kumi lab`, and serves `dev/poke` and `dev/i2c` from flash while unlocked, within the limits in
`docs/DIAGNOSTICS.md`; the flash test and the audio bench stay RAM-only. Sign it with the key the
installed stage0 carries, with a version and generation above the installed image, and give a bundle
the build name `lab-<date>`. On a running Kumi, `$KUMI update <image>` writes it to the other slot and `$KUMI
reboot normal` starts it as a trial; from service mode, `brom install` takes the bundle. A release image
installed the same way, or `$KUMI reboot other` while the release slot is still valid, returns the case
to a release build.

## Updates and rollback between Kumi versions

The firmware does this itself, without the boot ROM:

1. mark the inactive slot empty, before erasing it;
2. write and verify the new image, check its signature;
3. mark the slot as a trial with three attempts and prefer it;
4. the new firmware confirms itself at its health point: USB configured without interruption for
   30 seconds, counted in USB frames because the case has no timer; a host may call `system/confirm`
   earlier;
5. without a confirmation, stage0 invalidates the slot after three attempts and boots the other one.

A manual rollback is `system/reboot OtherSlot`; it clears the count of unhealthy starts, so the slot
it switches to does not inherit the other slot's count. If the image in a slot does not carry the
generation the journal recorded for it, for example because the slot was rewritten from service mode,
stage0 treats it as a new trial instead of keeping a `Confirmed` state that belonged to another image.

## Entering the boot ROM

stage0 and the runtime enter the boot ROM the way the stock firmware does: bit 31 of `0x2100801C`,
then bit 1 of `0x20003024` (`kumi-chip::reset`, one definition for both binaries, which also releases
the flash controller and clears the `BootInfo` magic first). Whether bit 31 survives that reset is
gate G4, measured on 05.11: both software resets reached service mode with the RAM kept. If it ever
did not, a decision that repeats on every start, such as no bootable slot, a trial whose attempt cannot
be recorded, a missing key or a crash streak, would loop without USB.

stage0 therefore breaks the loop itself. Before it requests the boot ROM it records "boot ROM
requested" in the journal. If its next start finds that record still set and again wants the boot
ROM, service mode did not take effect, or the host left without changing anything: stage0 programs
zeros over its own `SPV@` magic, the same one-way disarm the installer uses, and resets. With the magic
gone the boot ROM enters service mode at every start, whatever bit 31 does (G3). A start that boots an
image clears the record instead. If the record cannot be written, stage0 disarms at once. A trap or a
panic inside stage0 during a real boot records the crash and disarms too. These four bytes at offset 4
are the only flash write stage0 may make outside the journal (`kumi-layout::region_for_write`,
`Writer::Stage0`). After a disarm the case needs the installer to run again.

Two cases cannot be broken this way, because stage0 cannot touch the flash at all: the flash
controller does not go idle (`flash_unavailable`), or the bytes the boot ROM routines read differ from
the bytes the `0x48000000` window shows (`flash_mismatch`, checked before anything is written). Both
rely on bit 31 reaching service mode (G4).

## Returning to stock

The source is a backup on the computer or phone, or the copy kept at `0x300000`. Both are checked
against the boot ROM rules before anything is written. `kumi-install::plan_restore_stock`:

| Step | Action | What boots if power is lost |
|---|---|---|
| 1 | Kumi reboots into the boot ROM (`system/reboot BootRom`), or the case is already there | Kumi |
| 2 | program zeros over the header magic, erase sector 0 | service mode |
| 3 | write and verify the stock body | service mode |
| 4 | write the stock first sector, header page last, checksum field erased | service mode |
| 5 | write the checksum, verify the stock image | stock |
| 6 | exact mode: rewrite the rest of the flash from the full backup and verify all 4 MiB | stock |

Before writing, the installer checks that the stock image in the backup equals the one this case
holds, at `0x300000` or at the start of the flash, and exact mode is refused unless the backup
starts with the stock image.

Exact mode returns the flash bit for bit. In the model, a real stock dump goes through installation
and exact restoration and comes back with its original SHA-256.

## Boot ROM protocol details used by the installer

Area commands `0120`, `0122`, `0128` and `012A` share one address, remaining-length and chunk
state. `0120` erases its area; `0122` sets the state without erasing; `0121` programs from the
current state address and replies `0` for a verified match, `1` for a mismatch, `2` when nothing is
left. Kumi programs a page with `0122` for that page followed by `0121`, which is how the header page
can be written last. Status-register writes (`0127`) are never used.

## When something goes wrong

| Symptom | Action |
|---|---|
| Kumi runs, stock is wanted | "return to stock" from the app or the CLI, as above |
| a new Kumi version does not start | stage0 returns to the previous slot after three attempts |
| Kumi keeps failing before its health point (crash, hang until the power is cut, or a power cut) | after two such starts it starts in safe mode (factory settings, keyboard and USB), after four it enters the boot ROM, and the next start is in safe mode again; the host reads the crash record and reinstalls |
| the journal cannot be written | a trial image is not started and stage0 disarms itself; a confirmed image starts, and the crash count kept in RAM still leads to safe mode and the boot ROM |
| stage0 asked for the boot ROM twice in a row, or failed inside itself | stage0 has disarmed its header: the case shows up as `0666:0888` at every start; install again |
| the case shows up as `0666:0888` | install or return to stock from a computer or the phone |
| stage0 hangs before USB although its sum is valid | only a reset with bit 31 armed can reach the boot ROM, which is G4; there is no programmer to fall back on, which is why the hardware write path (G2b) and G4 come before any installation |

## Hazard

Repeated service-mode entries and USB re-enumeration storms on a wedged case have hung an Android
host hard. Use one clean service session per task, never toggle the USB `authorized` attribute,
and recover a wedged case with a boot ROM reboot (`0124`) or by unplugging it.

# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import hashlib
import json
from pathlib import Path
import sys
import time

from brom_emu import elf_segments
import odt
import phone as phones
from phone import KUMI, SERVICE, Timeline, matching, wait_for
import ramload
from spv5048 import BOOT_INFO_ADDRESS, BOOT_INFO_SIZE, CHIP_ID

RUNTIME_WINDOWS = ((0x2000, 0x10000), (0x102000, 0x128000), (0x40000000, 0x40040000))
RUNTIME_ENTRY = 0x2000


class RunError(RuntimeError):
    pass


def plan(elf):
    entry, segments = elf_segments(elf)
    if entry != RUNTIME_ENTRY:
        raise RunError(f"runtime entry 0x{entry:x} is not 0x{RUNTIME_ENTRY:x}")
    if entry == BOOT_INFO_ADDRESS:
        raise RunError("the entry would be passed as the BootInfo address")
    loads = []
    for address, contents, memory in segments:
        if not any(start <= address and address + memory <= end for start, end in RUNTIME_WINDOWS):
            raise RunError(f"segment 0x{address:x}+0x{memory:x} is outside the runtime RAM windows")
        if contents:
            ramload.validate_ram_range(address, len(contents))
            loads.append((address, contents, memory))
    return entry, loads


def describe(entry, loads, elf):
    return {"elf_sha256": hashlib.sha256(elf).hexdigest(), "entry": f"0x{entry:08x}",
            "a0_at_entry": "the boot ROM's 012C passes the entry address in a0 (executed boot ROM in the emulator; "
                           "ra 0x82E96 matches the M1 probe on the case), never the BootInfo address 0x137E00, so the "
                           "runtime starts without BootInfo: read-only storage and no boot confirmation",
            "boot_info_cleared": f"0x{BOOT_INFO_ADDRESS:08x}+{BOOT_INFO_SIZE}",
            "segments": [{"address": f"0x{address:08x}", "file_bytes": len(contents), "memory_bytes": memory,
                          "sha256": hashlib.sha256(contents).hexdigest()} for address, contents, memory in loads],
            "bss_zeroed_by": "the runtime's _start"}


def load(exchange, entry, loads, verify=True):
    info = exchange(odt.HANDSHAKE, b"", odt.HANDSHAKE_LENGTH)
    if odt.chip_id(info) != CHIP_ID:
        raise RunError(f"chip 0x{odt.chip_id(info):08x} is not the SPV5048")
    for address, contents, _ in loads:
        if verify:
            ramload.load_and_verify(exchange, address, contents)
        else:
            ramload.upload(exchange, address, contents)
    ramload.upload(exchange, BOOT_INFO_ADDRESS, bytes(BOOT_INFO_SIZE))
    if ramload.readback(exchange, BOOT_INFO_ADDRESS, BOOT_INFO_SIZE) != bytes(BOOT_INFO_SIZE):
        raise RunError("BootInfo did not read back as zeros")
    ramload.run(exchange, entry)


def start(phone, elf, seconds=120, wait_seconds=20.0, verify=True, out=None):
    out = out or (lambda text: print(text, file=sys.stderr, flush=True))
    entry, loads = plan(elf)
    report = {"tool": "tools/runtime_ram.py", "phone": phone.describe(), **describe(entry, loads, elf)}
    services = matching(phone.devices(), {SERVICE})
    if len(services) != 1:
        raise RunError("expected exactly one 0666:0888 boot ROM device")
    device = services[0]
    timeline = Timeline(phone.now)
    started = time.monotonic()
    report["usb_power_control"] = phone.hold_awake(device)
    bridge = phone.bridge(device, seconds)
    try:
        report["boot_rom_identity"] = bridge.identity
        out(f"loading {sum(len(c) for _, c, _ in loads)} bytes into case RAM")
        load(ramload.exchange_over(bridge), entry, loads, verify)
    finally:
        bridge.close()
    report["load_seconds"] = round(time.monotonic() - started, 1)
    timeline.add("jumped", entry=f"0x{entry:08x}")
    back = wait_for(phone, {KUMI}, wait_seconds, gone=device, timeline=timeline)
    report["timeline"] = timeline.events
    report["kumi"] = back.describe() if back else None
    if back is not None:
        report["kumi_power_control"] = phone.hold_awake(back)
    report["ok"] = back is not None
    if back is None:
        report["instruction"] = ("1209:0001 did not appear; unplug the case and plug it back in (RAM only, the flash is "
                                 "untouched)")
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description="Load the Kumi runtime ELF into case RAM through the boot ROM and start it")
    parser.add_argument("--serial", required=True)
    parser.add_argument("--helper", required=True)
    parser.add_argument("--elf", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--deadline-seconds", type=int, default=240)
    parser.add_argument("--wait-seconds", type=float, default=20.0)
    args = parser.parse_args(argv)
    if Path(args.output).exists():
        parser.error(f"{args.output} exists")
    phone = phones.AdbPhone(args.serial, args.helper)
    if not phone.wake_lock():
        parser.error("hold a wake lock first: echo kumi-case > /sys/power/wake_lock")
    report = start(phone, Path(args.elf).read_bytes(), args.deadline_seconds, args.wait_seconds)
    with Path(args.output).open("x") as stream:
        json.dump(report, stream, indent=2)
        stream.write("\n")
    print(json.dumps(report, indent=2))
    return 0 if report["ok"] else 1


if __name__ == "__main__":
    raise SystemExit(main())

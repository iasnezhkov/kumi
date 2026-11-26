# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
from pathlib import Path
import struct

import probe
import ramload

BISECT = 0x8800
ENTRY = probe.LOAD_ADDRESS + 28
WORDS = 16
REQUEST_MAGIC = struct.unpack("<I", b"KS0Q")[0]
RECORD_MAGIC = struct.unpack("<I", b"KS0B")[0]
ROM_INTERRUPTS = 1
XIP_UNPAUSED = 2
XIP_AS_FOUND = 4
STEPS = ["pause", "spi_enable", "init", "identity", "read", "erase", "program", "erase_again"]
CPU_HZ = 60_000_000


def request(limit, flags):
    return struct.pack("<4I", REQUEST_MAGIC, limit, flags, 0)


def decode(raw):
    words = struct.unpack(f"<{WORDS}I", raw[:4 * WORDS])
    if words[0] != RECORD_MAGIC:
        return {"record": None, "error": f"0x{words[4]:x}"}
    limit, flags, step, error = words[1], words[2], words[3], words[4]
    cycles = words[7:7 + step]
    return {
        "record": "bisect",
        "limit": limit,
        "rom_interrupts": bool(flags & ROM_INTERRUPTS),
        "steps_done": STEPS[:step],
        "reached_limit": step == limit,
        "pause_timeout": bool(error & 1),
        "trap_flag": f"0x{error >> 8:x}",
        "jedec": f"0x{words[5]:08x}" if step >= 4 else None,
        "scratch_head": f"0x{words[6]:08x}" if step >= 5 else None,
        "ms_at_60mhz": [round(c * 1000 / CPU_HZ, 3) for c in cycles],
        "xip_word": f"0x{words[15]:08x}" if flags & (XIP_UNPAUSED | XIP_AS_FOUND) else None,
        "xip_mode": "unpaused" if flags & XIP_UNPAUSED else "as_found" if flags & XIP_AS_FOUND else None,
    }


def exchange_bisect(exchange, image, limit, flags):
    info = exchange(ramload.HANDSHAKE, b"", ramload.HANDSHAKE_LENGTH)
    if ramload.chip_id(info) != ramload.CHIP_ID:
        raise ramload.ProtocolError(f"chip 0x{ramload.chip_id(info):08x}")
    digest = ramload.load_and_verify(exchange, probe.LOAD_ADDRESS, image)
    ramload.load_and_verify(exchange, BISECT, request(limit, flags))
    ramload.run(exchange, ENTRY)
    return digest, ramload.readback(exchange, BISECT, 4 * WORDS)


def run(args):
    from adb_bridge import Bridge
    output = ramload.require_new_report(args.output)
    image = ramload.ram_image(Path(args.image).read_bytes(), probe.LOAD_ADDRESS)
    bridge = Bridge(args.serial, args.helper, args.usb_path, args.deadline_seconds)
    try:
        flags = (ROM_INTERRUPTS if args.rom_interrupts else 0) | (XIP_UNPAUSED if args.xip == "unpaused" else 0) | (
            XIP_AS_FOUND if args.xip == "as-found" else 0)
        digest, raw = exchange_bisect(ramload.exchange_over(bridge), image, args.limit, flags)
    finally:
        bridge.close()
    report = decode(raw)
    report["image_sha256"] = digest
    ramload.save_report(output, raw, report)
    print(json.dumps(report, indent=2))
    return 0 if report.get("reached_limit") else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description="G2b bisect: run the boot ROM flash helpers from the probe in RAM, "
                                                 "stopping after LIMIT steps; writes only the sector at 0x3F0000")
    sub = parser.add_subparsers(dest="action", required=True)
    node = sub.add_parser("run")
    node.add_argument("--image", required=True, help="kumi-probe ELF")
    node.add_argument("--limit", type=int, required=True, choices=range(0, len(STEPS) + 1))
    node.add_argument("--rom-interrupts", action="store_true",
                      help="call the helpers with the boot ROM's trap vector and interrupt enable")
    node.add_argument("--xip", choices=("unpaused", "as-found"),
                      help="first read one word of the flash window, with the controller unpaused or as the ROM left it")
    ramload.add_bridge_arguments(node)
    args = parser.parse_args(argv)
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())

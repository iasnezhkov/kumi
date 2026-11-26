# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import hashlib
import json
from pathlib import Path
import struct
import zlib

import odt
import phone as phones
from phone import SERVICE, matching
import ramload
import spv5048
from spv5048 import CHIP_ID
import stage0_dry

AREA = spv5048.DIAG_AREA
RECORD = 256
REASONS = {0: "none", 1: "panic", 2: "trap"}
MESSAGE_AT, MESSAGE_LEN, TRAIL_AT, TRAIL_LEN, TRAIL_COUNT_AT, CRC_AT = 40, 96, 136, 16, 168, 252


def record(raw):
    if raw[:4] != b"KDIA" or raw[4] != 1 or zlib.crc32(raw[:CRC_AT]) != struct.unpack_from("<I", raw, CRC_AT)[0]:
        return None
    mcause, mepc, mtval, ra, sp, uptime = struct.unpack_from("<6I", raw, 12)
    count = raw[TRAIL_COUNT_AT]
    trail = [struct.unpack_from("<H", raw, TRAIL_AT + 2 * (i % TRAIL_LEN))[0]
             for i in range(max(0, count - TRAIL_LEN), count)]
    return {"reason": REASONS.get(raw[5], f"invalid_{raw[5]}"), "stage": f"0x{struct.unpack_from('<H', raw, 6)[0]:04x}",
            "boot_count": struct.unpack_from("<I", raw, 8)[0], "crashes_before": raw[37],
            "registers": {name: f"0x{value:08x}" for name, value in
                          zip(("mcause", "mepc", "mtval", "ra", "sp"), (mcause, mepc, mtval, ra, sp))},
            "uptime_ms": uptime, "message": raw[MESSAGE_AT:MESSAGE_AT + min(raw[36], MESSAGE_LEN)].decode("utf-8", "replace"),
            "trail": [f"0x{stage:04x}" for stage in trail]}


def decode(raw):
    if len(raw) != AREA[1] - AREA[0]:
        raise ValueError("expected the 768 bytes at 0x137D00..0x138000")
    at = {name: address - AREA[0] for name, address in (("last_crash", spv5048.LAST_CRASH_ADDRESS),
                                                         ("boot_info", spv5048.BOOT_INFO_ADDRESS),
                                                         ("current", spv5048.DIAG_RECORD_ADDRESS))}
    report_at = spv5048.STAGE0_REPORT_ADDRESS - AREA[0]
    try:
        stage0 = stage0_dry.decode(raw[report_at:report_at + spv5048.STAGE0_REPORT_SIZE])
    except ValueError:
        stage0 = None
    return {"last_crash": record(raw[at["last_crash"]:at["last_crash"] + RECORD]),
            "boot_info": spv5048.boot_info(raw[at["boot_info"]:at["boot_info"] + spv5048.BOOT_INFO_SIZE]),
            "stage0_report": stage0,
            "current": record(raw[at["current"]:at["current"] + RECORD]),
            "zero_bytes": raw.count(0), "sha256": hashlib.sha256(raw).hexdigest()}


def read_area(phone, seconds=60):
    services = matching(phone.devices(), {SERVICE})
    if len(services) != 1:
        raise SystemExit("expected exactly one 0666:0888 boot ROM device")
    phone.hold_awake(services[0])
    bridge = phone.bridge(services[0], seconds)
    try:
        exchange = ramload.exchange_over(bridge)
        if odt.chip_id(exchange(odt.HANDSHAKE, b"", odt.HANDSHAKE_LENGTH)) != CHIP_ID:
            raise SystemExit("not the SPV5048")
        return ramload.readback(exchange, AREA[0], AREA[1] - AREA[0]), bridge.identity
    finally:
        bridge.close()


def main(argv=None):
    parser = argparse.ArgumentParser(description="Read and decode the Kumi diagnostic RAM 0x137D00..0x138000 from "
                                                 "boot ROM service mode (crash records, BootInfo, stage0 report)")
    sub = parser.add_subparsers(dest="action", required=True)
    node = sub.add_parser("read")
    node.add_argument("--serial", required=True)
    node.add_argument("--helper", required=True)
    node.add_argument("--output", required=True)
    node = sub.add_parser("decode")
    node.add_argument("raw")
    args = parser.parse_args(argv)
    if args.action == "decode":
        print(json.dumps(decode(Path(args.raw).read_bytes()), indent=2))
        return 0
    output = ramload.require_new_report(args.output)
    raw, identity = read_area(phones.AdbPhone(args.serial, args.helper))
    report = decode(raw)
    report["identity"] = identity
    ramload.save_report(output, raw, report)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
from pathlib import Path
import struct
import zlib

import ramload
import spv5048

RAM_IMAGE_OFFSET = spv5048.STAGE0_RAM_IMAGE_OFFSET
LOAD_ADDRESS = spv5048.STAGE0_LOAD_ADDRESS
DRY_ENTRY = spv5048.STAGE0_DRY_ENTRY
G2B_ENTRY = spv5048.STAGE0_RAM[0] + 16
SCRATCH = (0x3F0000, 0x10000)
CPU_HZ = 60_000_000
RAM_IMAGE_LIMIT = 0xFD00 - 0x2000
REPORT_ADDRESS = spv5048.STAGE0_REPORT_ADDRESS
REPORT_BYTES = spv5048.STAGE0_REPORT_SIZE
SPV_MAGIC = 0x40565053
KINDS = {
    1: "jump",
    2: "would_jump",
    10: "no_bootable_slot",
    11: "crash_streak",
    12: "journal_unreadable",
    13: "journal_unwritable",
    14: "load_failed",
    15: "flash_unavailable",
    16: "no_key",
    17: "flash_mismatch",
}
FLAGS = {1: "boot_rom_pending_at_entry", 2: "boot_rom_marker_stored", 4: "disarmed"}
CHECKS = {0: "not_checked", 1: "valid", 2: "invalid"}
G2B_OUTCOMES = {
    0: "passed",
    15: "flash_unavailable",
    17: "flash_mismatch",
    18: "window_not_erased",
    19: "write_failed",
    20: "head_changed",
}
G2B_STEPS = {
    1: "controller_paused",
    2: "identity_matched",
    4: "window_erased_before",
    8: "first_erase_verified",
    16: "page_programmed",
    32: "second_erase_verified",
    64: "head_unchanged",
}
G2B_ALL_STEPS = sum(G2B_STEPS)


def sealed(raw):
    return zlib.crc32(raw[:60]) == struct.unpack_from("<I", raw, 60)[0]


def boot_info(raw):
    info = spv5048.boot_info(raw)
    if info is None:
        return None
    return {
        "slot": "AB"[info["slot"]],
        "safe_mode": info["safe_mode"],
        "boot_count": info["boot_count"],
        "generation": info["generation"],
        "image_version": ".".join(str(b) for b in info["image_version"]),
        "stage0_version": ".".join(str(b) for b in info["stage0_version"]),
    }


def decode(raw):
    if len(raw) < REPORT_BYTES:
        raise ValueError("report must be 64 bytes")
    raw = bytes(raw[:REPORT_BYTES])
    if not sealed(raw):
        raise ValueError("report CRC does not match; stage0 did not finish or the area holds something else")
    magic = raw[0:4]
    if magic == b"KS0T":
        names = ["mcause", "mepc", "mtval", "ra", "sp"]
        return {"record": "trap", **{n: f"0x{v:08x}" for n, v in zip(names, struct.unpack_from("<5I", raw, 4))}}
    if magic == b"KS0P":
        return {"record": "panic", "message": raw[5:5 + raw[4]].decode("utf-8", "replace")}
    if magic == b"KS0W":
        return decode_g2b(raw)
    if magic != b"KS0R":
        raise ValueError(f"unknown record {magic!r}")
    kind = raw[4]
    result = {
        "record": "report",
        "outcome": KINDS.get(kind, f"unknown_{kind}"),
        "journal_found": raw[5] == 1,
        "journal_stored": raw[6] == 1,
        "slot_checks": {"A": CHECKS.get(raw[44]), "B": CHECKS.get(raw[45])},
        "trial_attempts_left": {"A": raw[46], "B": raw[47]},
        "boot_count": struct.unpack_from("<I", raw, 48)[0],
        "flags": [name for bit, name in FLAGS.items() if raw[7] & bit],
        "unhealthy_starts": raw[52],
        "previous_boot_crashed": raw[53] == 1,
        "ram_crashes": raw[54],
    }
    result["flash_head"] = f"0x{struct.unpack_from('<I', raw, 56)[0]:08x}"
    if kind in (1, 2):
        result["entry"] = f"0x{struct.unpack_from('<I', raw, 8)[0]:08x}"
        result["boot_info"] = boot_info(raw[12:44])
    return result


def decode_g2b(raw):
    outcome, steps = raw[4], raw[5]
    before, after = struct.unpack_from("<II", raw, 12)
    cycles = struct.unpack_from("<I", raw, 24)[0]
    return {
        "record": "g2b",
        "outcome": G2B_OUTCOMES.get(outcome, f"unknown_{outcome}"),
        "passed": outcome == 0 and steps == G2B_ALL_STEPS and before == after,
        "steps": [name for bit, name in G2B_STEPS.items() if steps & bit],
        "missing_steps": [name for bit, name in G2B_STEPS.items() if not steps & bit],
        "scratch_sector": f"0x{SCRATCH[0]:06x}",
        "head_crc_before": f"0x{before:08x}",
        "head_crc_after": f"0x{after:08x}",
        "session_cycles": cycles,
        "session_ms_at_60mhz": round(cycles * 1000 / CPU_HZ, 3),
    }


def ram_image(stage0):
    if len(stage0) < RAM_IMAGE_OFFSET + 4 or struct.unpack_from("<I", stage0, 4)[0] != SPV_MAGIC:
        raise ValueError("not a packed stage0 image (kumi-pack stage0)")
    ram = stage0[RAM_IMAGE_OFFSET:]
    if len(ram) > RAM_IMAGE_LIMIT:
        raise ValueError("stage0 RAM image leaves less than 8 KiB of stack")
    return ram


def run(args, entry=DRY_ENTRY):
    output = ramload.require_new_report(args.output)
    ram = ram_image(Path(args.image).read_bytes())
    digest, raw = ramload.run_on_device(args, LOAD_ADDRESS, ram, entry, REPORT_ADDRESS, REPORT_BYTES)
    report = decode(raw)
    report["ram_image_sha256"] = digest
    report["called_entry"] = f"0x{entry:08x}"
    ramload.save_report(output, raw, report)
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description="Kumi gates G2 and G2b: run stage0's dry run or its scratch write session from "
                                                 "RAM through the BROM and read its report")
    sub = parser.add_subparsers(dest="action", required=True)
    node = sub.add_parser("run", help="load the stage0 RAM image, call the dry-run entry, read the report")
    node.add_argument("--image", required=True, help="stage0 image from kumi-pack stage0")
    ramload.add_bridge_arguments(node)
    node = sub.add_parser("g2b", help="load the stage0 RAM image, call the G2b entry, which runs the boot's flash session "
                                      "but writes only the sector at 0x3F0000, and read its report")
    node.add_argument("--image", required=True, help="stage0 image from kumi-pack stage0")
    ramload.add_bridge_arguments(node)
    node = sub.add_parser("decode", help="decode a saved 64-byte report")
    node.add_argument("report")
    args = parser.parse_args(argv)
    if args.action == "run":
        run(args)
    elif args.action == "g2b":
        report = run(args, G2B_ENTRY)
        return 0 if report.get("passed") else 1
    else:
        print(json.dumps(decode(Path(args.report).read_bytes()), indent=2))


if __name__ == "__main__":
    raise SystemExit(main())

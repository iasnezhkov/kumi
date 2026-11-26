# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
from pathlib import Path
import shlex
import struct
import subprocess

from brom_emu import elf_symbols

SYMBOL = "KUMI_LINK_LOG"
RECORDS = 64
RECORD = 16
BODY = RECORD - 4
PEEK_WORDS = 32
DIRECTIONS = {ord("R"): "from_mcu", ord("T"): "to_mcu"}


def decode(raw):
    records = []
    for at in range(0, len(raw) - RECORD + 1, RECORD):
        direction, group, command, length = raw[at:at + 4]
        if direction == 0:
            break
        body = raw[at + 4:at + 4 + min(length, BODY)]
        records.append({"dir": DIRECTIONS.get(direction, f"0x{direction:02x}"), "group": f"0x{group:02x}",
                        "command": f"0x{command:02x}", "length": length, "body": body.hex(),
                        "truncated": length > BODY})
    return records


def words_to_bytes(words):
    return b"".join(struct.pack("<I", int(w, 16) if isinstance(w, str) else w) for w in words)


def read_phone(args, address):
    raw = b""
    for offset in range(0, RECORDS * RECORD, 4 * PEEK_WORDS):
        remote = (f"{shlex.quote(args.kumi)} --adb local --helper {shlex.quote(args.helper)} "
                  f"--usb-path {shlex.quote(args.usb_path)} --deadline 30 dev peek 0x{address + offset:08x} {PEEK_WORDS}")
        output = subprocess.run(["adb", "-s", args.serial, "shell", remote], check=True, capture_output=True,
                                text=True, timeout=60).stdout
        raw += words_to_bytes(json.loads(output[output.index("{"):])["words"])
    return raw


def main(argv=None):
    parser = argparse.ArgumentParser(description="Read and decode the runtime's log of the first 64 keyboard-link "
                                                 "frames since boot (MCU frames and Kumi's replies) through dev peek "
                                                 "on the phone; developer mode must be unlocked")
    parser.add_argument("--elf", required=True, help="the kumi-fw ELF that is running, for the log address")
    parser.add_argument("--serial", required=True)
    parser.add_argument("--kumi", required=True, help="the kumi binary on the phone")
    parser.add_argument("--helper", required=True)
    parser.add_argument("--usb-path", required=True)
    args = parser.parse_args(argv)
    address = elf_symbols(Path(args.elf).read_bytes())[SYMBOL]
    print(json.dumps({"address": f"0x{address:08x}", "records": decode(read_phone(args, address))}, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

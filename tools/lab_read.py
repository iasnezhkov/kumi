# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
from pathlib import Path
import shlex
import struct
import subprocess

from brom_emu import elf_symbols

PEEK_WORDS = 32
BOOT_HEADER = 8
BOOT_FIELDS = ("magic", "entry_cycles", "usb_cycles", "running_cycles", "configured_cycles", "boot_count",
               "loaded_by_host", "entry_count")
JOURNAL_HEADER = 4
JOURNAL_FIELDS = ("magic", "written", "dropped", "cpu_khz")
JOURNAL_RECORDS = 64
RECORD_WORDS = 4
CONTROL_HEADER = 4
CONTROL_FIELDS = ("magic", "fake_stream_hz", "watch_budget", "hold_awake_s")
WATCH_SLOTS = 16
KINDS = {1: "boot", 2: "link_rx", 3: "link_tx", 4: "link_error", 5: "usb", 6: "vbus", 7: "watch",
         8: "watch_start", 9: "clock", 10: "mark", 11: "knob", 12: "sleep_snap"}
USB_EVENTS = {1: "reset", 2: "suspend", 3: "resume", 4: "configured", 5: "deconfigured"}
LINK_ERRORS = {1: "bad_length", 2: "checksum"}


def read_words(args, address, count):
    raw = b""
    for offset in range(0, count * 4, 4 * PEEK_WORDS):
        chunk = min(PEEK_WORDS, count - offset // 4)
        if args.usb:
            cmd = [args.kumi, "--usb", "dev", "peek", f"0x{address + offset:08x}", str(chunk)]
            output = subprocess.run(cmd, check=True, capture_output=True, text=True, timeout=30).stdout
        else:
            remote = (f"{shlex.quote(args.kumi)} --adb local --helper {shlex.quote(args.helper)} "
                      f"--usb-path {shlex.quote(args.usb_path)} --deadline 30 dev peek 0x{address + offset:08x} {chunk}")
            output = subprocess.run(["adb", "-s", args.serial, "shell", remote], check=True, capture_output=True,
                                    text=True, timeout=60).stdout
        for word in json.loads(output[output.index("{"):])["words"]:
            raw += struct.pack("<I", int(word, 16) if isinstance(word, str) else word)
    return [struct.unpack_from("<I", raw, at)[0] for at in range(0, count * 4, 4)]


def decode_record(words):
    stamp, packed, first, second = words
    kind = packed & 0xFF
    a, b, length = (packed >> 8) & 0xFF, (packed >> 16) & 0xFF, (packed >> 24) & 0xFF
    body = struct.pack("<II", first, second)[:min(length, 8)].hex()
    out = {"stamp": stamp, "kind": KINDS.get(kind, kind), "a": a, "b": b, "length": length,
           "first": f"0x{first:08x}", "second": f"0x{second:08x}"}
    if kind == 5:
        out["event"] = USB_EVENTS.get(a, a)
        out["power"] = f"0x{b:02x}"
        out["devctl"] = f"0x{length:02x}"
    elif kind == 6:
        out["devctl"] = f"0x{a:02x}"
        out["power"] = f"0x{b:02x}"
    elif kind in (2, 3):
        out["group"] = f"0x{a:02x}"
        out["command"] = f"0x{b:02x}"
        out["body"] = body
    elif kind == 4:
        out["error"] = LINK_ERRORS.get(a, a)
    elif kind == 10:
        out["stage"] = a | (b << 8)
    elif kind in (7, 8, 12):
        out["slot"] = a
        out["address"] = f"0x{first:08x}"
        out["value"] = f"0x{second:08x}"
    return out


def decode_journal(words):
    header = dict(zip(JOURNAL_FIELDS, words[:JOURNAL_HEADER]))
    header["magic"] = struct.pack("<I", words[0]).decode("latin1")
    written = header["written"]
    kept = min(written, JOURNAL_RECORDS)
    records = []
    for i in range(kept):
        slot = (written - kept + i) % JOURNAL_RECORDS
        at = JOURNAL_HEADER + slot * RECORD_WORDS
        records.append(decode_record(words[at:at + RECORD_WORDS]))
    return {"header": header, "records": records}


def decode_boot(words):
    header = dict(zip(BOOT_FIELDS, words[:BOOT_HEADER]))
    header["magic"] = struct.pack("<I", words[0]).decode("latin1")
    count = header["entry_count"]
    registers = {f"0x{words[BOOT_HEADER + 2 * i]:08x}": f"0x{words[BOOT_HEADER + 2 * i + 1]:08x}" for i in range(count)}
    return {"header": header, "registers": registers}


def decode_control(words):
    header = dict(zip(CONTROL_FIELDS, words[:CONTROL_HEADER]))
    header["magic"] = struct.pack("<I", words[0]).decode("latin1")
    watch = [{"address": f"0x{words[CONTROL_HEADER + 2 * i]:08x}", "mask": f"0x{words[CONTROL_HEADER + 2 * i + 1]:08x}"}
             for i in range(WATCH_SLOTS) if words[CONTROL_HEADER + 2 * i]]
    return {"header": header, "watch": watch}


def main(argv=None):
    parser = argparse.ArgumentParser(description="Read and decode a lab build's startup register snapshot, event "
                                                 "journal and control block through dev peek on the phone; the lab "
                                                 "runtime must be running and developer mode unlocked")
    parser.add_argument("--elf", required=True, help="the running kumi-fw lab ELF, for the symbol addresses")
    parser.add_argument("--usb", action="store_true", help="talk directly to the case over host USB instead of adb")
    parser.add_argument("--serial")
    parser.add_argument("--kumi", required=True, help="the kumi binary (local with --usb, or on the phone with --serial)")
    parser.add_argument("--helper")
    parser.add_argument("--usb-path")
    parser.add_argument("--section", choices=("boot", "journal", "control", "all"), default="all")
    args = parser.parse_args(argv)
    if not args.usb and not (args.serial and args.helper and args.usb_path):
        parser.error("pass either --usb or --serial, --helper and --usb-path")
    symbols = elf_symbols(Path(args.elf).read_bytes())
    boot_words = BOOT_HEADER + 2 * 72
    journal_words = JOURNAL_HEADER + RECORD_WORDS * JOURNAL_RECORDS
    control_words = CONTROL_HEADER + 2 * WATCH_SLOTS
    out = {}
    if args.section in ("boot", "all"):
        out["boot"] = decode_boot(read_words(args, symbols["KUMI_LAB_BOOT"], boot_words))
    if args.section in ("journal", "all"):
        out["journal"] = decode_journal(read_words(args, symbols["KUMI_LAB_JOURNAL"], journal_words))
    if args.section in ("control", "all"):
        out["control"] = decode_control(read_words(args, symbols["KUMI_LAB_CONTROL"], control_words))
    print(json.dumps(out, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

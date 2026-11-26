# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import hashlib
import json
from pathlib import Path
import struct
import time

import odt
from spv5048 import CHIP_ID

FLASH = 0x48000000
FLASH_SIZE = 0x400000
WINDOW = 0x3F0000
WINDOW_END = 0x400000
SECTOR = 0x1000
BLOCK = 0x10000
CHUNK = 256
STOCK_BYTES = 0x30000
HANDSHAKE, ERASE, PROGRAM, AREA, READ, REBOOT, JEDEC, STATUS = 0x110, 0x120, 0x121, 0x122, 0x123, 0x124, 0x125, 0x126
JEDEC_MANUFACTURER = 0xC8
JEDEC_CAPACITY = 0x16
STATUS_BLOCK_PROTECT = 0x7C
FOUR_BYTES_OFFSET = WINDOW + 0x12C
TAIL_OFFSET = WINDOW + BLOCK - SECTOR
DEFAULT_DEADLINE_SECONDS = 1200


class GateFailure(RuntimeError):
    pass


def area(offset, length):
    return struct.pack("<4I", FLASH + offset, length, CHUNK, 0)


def in_window(offset, length):
    return WINDOW <= offset and length > 0 and offset + length <= WINDOW_END


def landing_offsets(offset):
    return sorted({offset - WINDOW, offset % CHUNK, 0})


class Gates:
    def __init__(self, bridge, stock, codec=odt):
        if len(stock) != FLASH_SIZE:
            raise GateFailure("the stock dump must be exactly 4 MiB")
        self.bridge = bridge
        self.stock = stock
        self.codec = codec
        self.before = None
        self.steps = []

    def call(self, command, payload, expected, flags=0):
        packet = self.bridge.send(self.codec.encode_frame(command, payload, flags))
        return self.codec.decode_frame(packet, (command & 0xFF) | 0x200, expected)[0]

    def read(self, offset, length):
        self.call(AREA, area(offset, length), 0)
        data = bytearray()
        while len(data) < length:
            data.extend(self.call(READ, b"", min(CHUNK, length - len(data))))
        return bytes(data)

    def program(self, offset, data):
        if not in_window(offset, len(data)):
            raise GateFailure("program outside the scratch window")
        started = time.monotonic()
        self.call(AREA, area(offset, len(data)), 0)
        for start in range(0, len(data), CHUNK):
            status = self.call(PROGRAM, data[start:start + CHUNK], 1)
            if status != b"\0":
                raise GateFailure(f"program status {status.hex()} at 0x{offset + start:06x}")
        return round((time.monotonic() - started) * 1000, 1)

    def erase(self, offset, length):
        if not in_window(offset, length) or offset % SECTOR or length % SECTOR:
            raise GateFailure("erase outside the scratch window")
        started = time.monotonic()
        self.call(ERASE, area(offset, length), 0)
        return round((time.monotonic() - started) * 1000, 1)

    def pattern(self, offset, length):
        value = bytearray(b"\0" * length)
        for at in landing_offsets(offset):
            for i, byte in enumerate(self.before[at:at + length]):
                value[i] |= byte
        if value == b"\xff" * length:
            raise GateFailure(f"no visible no-op pattern for 0x{offset:06x}")
        return bytes(value)

    def low_block_intact(self, label):
        if self.read(0, BLOCK) != self.before[:BLOCK]:
            raise GateFailure(f"flash below 0x{BLOCK:x} changed after {label}")

    def step(self, name, **facts):
        self.steps.append({"step": name, **facts})

    def identity(self):
        info = self.call(HANDSHAKE, b"", 44, flags=1)
        chip = struct.unpack_from("<I", info, 40)[0]
        if chip != CHIP_ID:
            raise GateFailure(f"chip 0x{chip:08x}")
        jedec = self.call(JEDEC, b"", 4)
        if jedec[2] != JEDEC_MANUFACTURER or jedec[0] != JEDEC_CAPACITY or jedec[3]:
            raise GateFailure(f"flash identity {jedec.hex()} is not a 4 MiB GigaDevice reply shaped 16 65 c8 00")
        status = self.call(STATUS, b"", 4)
        if status[0] & (STATUS_BLOCK_PROTECT | 3):
            raise GateFailure(f"flash status {status.hex()} shows block protection, WEL or WIP")
        self.step("identity", chip=f"0x{chip:08x}", jedec=jedec.hex(), status=status.hex())

    def whole_flash(self, name):
        image = self.read(0, FLASH_SIZE)
        self.step(name, sha256=hashlib.sha256(image).hexdigest())
        return image

    def run(self):
        self.identity()
        self.before = self.whole_flash("flash_before")
        if self.before[:STOCK_BYTES] != self.stock[:STOCK_BYTES]:
            raise GateFailure("stock firmware area differs from the verified dump before any write")
        if self.before[WINDOW:WINDOW_END] != b"\xff" * BLOCK:
            raise GateFailure("scratch block is not erased")
        self.step("scratch_block_erased")
        page = self.pattern(WINDOW, CHUNK)
        ms = self.program(WINDOW, page)
        back = self.read(WINDOW, CHUNK)
        self.low_block_intact("G6")
        if back != page:
            raise GateFailure("G6 page read-back differs")
        self.step("G6_page_program", offset=f"0x{WINDOW:06x}", ms=ms, readback="match")
        four = self.pattern(FOUR_BYTES_OFFSET, 4)
        ms = self.program(FOUR_BYTES_OFFSET, four)
        page_start = FOUR_BYTES_OFFSET & ~(CHUNK - 1)
        back = self.read(page_start, CHUNK)
        self.low_block_intact("G7 four bytes")
        at = FOUR_BYTES_OFFSET - page_start
        expected = b"\xff" * at + four + b"\xff" * (CHUNK - at - 4)
        if back != expected:
            raise GateFailure(f"G7 four-byte read-back differs: {back[at - 4:at + 8].hex()}")
        self.step("G7_four_bytes", offset=f"0x{FOUR_BYTES_OFFSET:06x}", ms=ms, readback="match")
        ms = self.erase(WINDOW, SECTOR)
        back = self.read(WINDOW, SECTOR)
        self.low_block_intact("G7 sector erase")
        if back != b"\xff" * SECTOR:
            raise GateFailure("G7 sector erase left programmed bytes")
        self.step("G7_sector_erase", offset=f"0x{WINDOW:06x}", length=SECTOR, ms=ms, readback="erased")
        tail = self.pattern(TAIL_OFFSET, CHUNK)
        self.program(TAIL_OFFSET, tail)
        if self.read(TAIL_OFFSET, CHUNK) != tail:
            raise GateFailure("tail page read-back differs")
        self.low_block_intact("tail page")
        ms = self.erase(WINDOW, BLOCK)
        back = self.read(WINDOW, BLOCK)
        self.low_block_intact("G7 block erase")
        if back != b"\xff" * BLOCK:
            raise GateFailure("G7 64 KiB erase left programmed bytes")
        self.step("G7_block_erase", offset=f"0x{WINDOW:06x}", length=BLOCK, ms=ms, readback="erased")
        if self.whole_flash("flash_after") != self.before:
            raise GateFailure("flash changed during the gates")


def run(args):
    from adb_bridge import Bridge
    output = Path(args.output)
    if output.exists():
        raise SystemExit(f"{output} exists")
    stock = Path(args.stock).read_bytes()
    if hashlib.sha256(stock).hexdigest() != args.stock_sha256:
        raise SystemExit("stock dump SHA256 mismatch")
    if args.deadline_seconds <= 0:
        raise SystemExit("deadline must be positive")
    bridge = Bridge(args.serial, args.helper, args.usb_path, args.deadline_seconds, mode="scratch-bridge")
    gates = Gates(bridge, stock)
    result = {"helper": args.helper, "usb_path": args.usb_path}
    try:
        gates.run()
        result["passed"] = True
        if args.return_to_runtime:
            gates.call(REBOOT, b"", 0)
            result["returned_to_runtime"] = True
    except Exception as error:
        result["passed"] = False
        result["error"] = f"{type(error).__name__}: {error}"
        result["returned_to_runtime"] = False
    finally:
        result["steps"] = gates.steps
        result["helper_log"] = bridge.helper_log().splitlines()
        bridge.close()
    output.write_text(json.dumps(result, indent=2) + "\n")
    print(json.dumps(result, indent=2))
    return 0 if result["passed"] else 1


def main(argv=None):
    parser = argparse.ArgumentParser(description="Kumi gates G6/G7: boot ROM program and erase, only in flash 0x3F0000-0x3FFFFF")
    sub = parser.add_subparsers(dest="action", required=True)
    node = sub.add_parser("run")
    node.add_argument("--serial", required=True)
    node.add_argument("--helper", required=True)
    node.add_argument("--usb-path", required=True)
    node.add_argument("--stock", required=True)
    node.add_argument("--stock-sha256", required=True)
    node.add_argument("--output", required=True)
    node.add_argument("--deadline-seconds", type=int, default=DEFAULT_DEADLINE_SECONDS)
    node.add_argument("--return-to-runtime", action="store_true")
    args = parser.parse_args(argv)
    return run(args)


if __name__ == "__main__":
    raise SystemExit(main())

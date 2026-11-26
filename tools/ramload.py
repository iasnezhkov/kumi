# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import hashlib
import json
from pathlib import Path
import struct

import odt
from odt import (HANDSHAKE, HANDSHAKE_LENGTH, PACKET_SIZE, Command, ProtocolError, chip_id,
                  encode_frame, integer, require_empty)
from spv5048 import CHIP_ID


WRITE_RAM_AREA = 0x0128
WRITE_RAM_DATA = 0x0129
READ_RAM_AREA = 0x012A
READ_RAM_DATA = 0x012B
EXEC = 0x012C

RAM_SLICE = 256
RAM_LIMIT = 0x00200000
RT40 = (0x40000000, 0x40040000)
BROM_DLM = (0x00100000, 0x00102000)
STACK = (0x00138000, 0x00140000)
RESERVED = (BROM_DLM, STACK)
RAM_REFUSAL = "Command is outside the RAM loader allowlist"


def overlaps(address, length, region):
    start, end = region
    return address < end and start < address + length


def in_rt40(address, length):
    return RT40[0] <= address < RT40[1] and 0 < length <= RT40[1] - address


def validate_ram_range(address, length, allow_reserved=False):
    if in_rt40(address, length):
        return
    if not 0 <= address < RAM_LIMIT:
        raise ProtocolError("RAM address is outside the reconstructed CPU window")
    if not 0 < length <= RAM_LIMIT or address + length > RAM_LIMIT:
        raise ProtocolError("RAM length is outside the reconstructed CPU window")
    if not allow_reserved:
        for region in RESERVED:
            if overlaps(address, length, region):
                raise ProtocolError("RAM range overlaps a BROM-owned or stack region")


def ram_area_check(name):
    def check(payload):
        if len(payload) != 16:
            raise ProtocolError(f"{name} area requires four uint32 values")
        address, length, chunk, reserved = struct.unpack("<4I", payload)
        validate_ram_range(address, length)
        if chunk != RAM_SLICE or reserved:
            raise ProtocolError(f"Unexpected {name} area parameters")
    return check


def require_ram_slice(payload):
    if not 1 <= len(payload) <= RAM_SLICE:
        raise ProtocolError("RAM slice must contain 1..256 bytes")


def require_entry(payload):
    if len(payload) != 4:
        raise ProtocolError("Execution address must be one uint32")


RAM_COMMANDS = {
    HANDSHAKE: Command(0x0210, require_empty, flags=1),
    WRITE_RAM_AREA: Command(0x0228, ram_area_check("RAM write"), reply_length=0),
    WRITE_RAM_DATA: Command(0x0229, require_ram_slice, reply_length=1, status=b"\0"),
    READ_RAM_AREA: Command(0x022A, ram_area_check("RAM read"), reply_length=0),
    READ_RAM_DATA: Command(0x022B, require_empty),
    EXEC: Command(0x022C, require_entry, reply_length=0),
}


def encode_request(command, payload=b""):
    return odt.encode_request(command, payload, RAM_COMMANDS, RAM_REFUSAL)


def decode_reply(packet, command, length):
    return odt.decode_reply(packet, command, length, RAM_COMMANDS, RAM_REFUSAL)[0]


def upload(exchange, address, data):
    validate_ram_range(address, len(data))
    exchange(WRITE_RAM_AREA, struct.pack("<4I", address, len(data), RAM_SLICE, 0), 0)
    for offset in range(0, len(data), RAM_SLICE):
        exchange(WRITE_RAM_DATA, data[offset:offset + RAM_SLICE], 1)


def readback(exchange, address, length):
    validate_ram_range(address, length)
    exchange(READ_RAM_AREA, struct.pack("<4I", address, length, RAM_SLICE, 0), 0)
    data = bytearray()
    while len(data) < length:
        data.extend(exchange(READ_RAM_DATA, b"", min(RAM_SLICE, length - len(data))))
    if len(data) != length:
        raise ProtocolError("RAM read length mismatch")
    return bytes(data)


def load_and_verify(exchange, address, data):
    upload(exchange, address, data)
    echoed = readback(exchange, address, len(data))
    if echoed != data:
        raise ProtocolError("RAM readback differs from the uploaded image")
    return hashlib.sha256(data).hexdigest()


def run(exchange, entry):
    if not 0 <= entry <= 0xFFFFFFFF:
        raise ProtocolError("Execution address must be uint32")
    exchange(EXEC, struct.pack("<I", entry), 0)


def load_and_run(exchange, address, data, entry, expected_chip=None):
    info = exchange(HANDSHAKE, b"", HANDSHAKE_LENGTH)
    if len(info) != HANDSHAKE_LENGTH:
        raise ProtocolError("Handshake must contain eleven uint32 values")
    if expected_chip is not None and chip_id(info) != expected_chip:
        raise ProtocolError(f"Chip mismatch: expected 0x{expected_chip:08x}, got 0x{chip_id(info):08x}")
    digest = load_and_verify(exchange, address, data)
    run(exchange, entry)
    return digest, info


def ram_image(data, address):
    if data[:4] != b"\x7fELF":
        return data
    from brom_emu import elf_segments
    _, segments = elf_segments(data)
    if len(segments) != 1 or segments[0][0] != address or segments[0][2] != len(segments[0][1]):
        raise ProtocolError(f"expected one fully initialized RAM segment at 0x{address:x}")
    return segments[0][1]


def run_on_device(args, address, image, entry, report_address, report_bytes):
    from adb_bridge import Bridge
    bridge = Bridge(args.serial, args.helper, args.usb_path, args.deadline_seconds)
    try:
        exchange = exchange_over(bridge)
        digest, _ = load_and_run(exchange, address, image, entry, expected_chip=CHIP_ID)
        raw = readback(exchange, report_address, report_bytes)
        if args.return_to_runtime:
            bridge.reboot()
    finally:
        bridge.close()
    return digest, raw


def require_new_report(output):
    output = Path(output)
    if output.exists() or Path(str(output) + ".json").exists():
        raise SystemExit(f"{output} or its JSON report exists")
    return output


def save_report(output, raw, report):
    output.write_bytes(raw)
    report["raw_sha256"] = hashlib.sha256(raw).hexdigest()
    Path(str(output) + ".json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report, indent=2))


def add_bridge_arguments(node, deadline=60):
    node.add_argument("--serial", required=True)
    node.add_argument("--helper", required=True)
    node.add_argument("--usb-path", required=True)
    node.add_argument("--output", required=True)
    node.add_argument("--deadline-seconds", type=int, default=deadline)
    node.add_argument("--return-to-runtime", action="store_true")


def exchange_over(channel):
    def exchange(command, payload, expected_length):
        return decode_reply(channel.send(encode_request(command, payload)), command, expected_length)
    return exchange


class Simulator:
    def __init__(self, chip=CHIP_ID):
        self.ram = bytearray(RAM_LIMIT)
        self.rt40 = bytearray(RT40[1] - RT40[0])
        self.chip = chip
        self.write = None
        self.read = None
        self.executed = []
        self.trace = []

    def _bank(self, address):
        if address >= RT40[0]:
            return self.rt40, address - RT40[0]
        return self.ram, address

    def send(self, request):
        command, _sequence, _flags, length = struct.unpack_from("<HBBH", request, 4)
        payload = request[odt.HEADER_SIZE:odt.HEADER_SIZE + length]
        self.trace.append(command)
        if command == HANDSHAKE:
            info = bytearray(HANDSHAKE_LENGTH)
            struct.pack_into("<I", info, 40, self.chip)
            return self._reply(0x0210, bytes(info))
        if command == WRITE_RAM_AREA:
            address, total, _chunk, _reserved = struct.unpack("<4I", payload)
            self.write = [address, total]
            return self._reply(0x0228, b"")
        if command == WRITE_RAM_DATA:
            address, remaining = self.write
            count = min(len(payload), remaining)
            bank, offset = self._bank(address)
            bank[offset:offset + count] = payload[:count]
            self.write = [address + count, remaining - count]
            return self._reply(0x0229, b"\0" if remaining else b"\2")
        if command == READ_RAM_AREA:
            address, total, _chunk, _reserved = struct.unpack("<4I", payload)
            self.read = [address, total]
            return self._reply(0x022A, b"")
        if command == READ_RAM_DATA:
            address, remaining = self.read
            count = min(RAM_SLICE, remaining)
            bank, offset = self._bank(address)
            chunk = bytes(bank[offset:offset + count])
            self.read = [address + count, remaining - count]
            return self._reply(0x022B, chunk)
        if command == EXEC:
            (entry,) = struct.unpack("<I", payload)
            self.executed.append(entry)
            return self._reply(0x022C, b"")
        raise ProtocolError(f"Simulator has no command 0x{command:04x}")

    def _reply(self, reply, payload):
        return encode_frame(reply, payload)


class SimulatorChannel(odt.Channel):
    def __init__(self, simulator):
        super().__init__()
        self.simulator = simulator

    def send(self, request):
        return self.simulator.send(request)

    def exchange(self, command, payload, expected_length):
        data = decode_reply(self.send(encode_request(command, payload)), command, expected_length)
        self.responses.append({"command": command, "length": len(data)})
        return data


def describe():
    return {
        "transport": "service mode BUXX, USB 0666:0888, same CRC32 framing as flash read",
        "handshake": HANDSHAKE,
        "commands": {
            "write_ram_area": WRITE_RAM_AREA, "write_ram_data": WRITE_RAM_DATA,
            "read_ram_area": READ_RAM_AREA, "read_ram_data": READ_RAM_DATA, "execute": EXEC,
        },
        "slice_bytes": RAM_SLICE, "packet_bytes": PACKET_SIZE,
        "ram_window": [0, RAM_LIMIT], "reserved_regions": list(RESERVED),
        "touches_flash": False, "reset_restores_stock": True,
        "trampoline_note": "BROM 0x826EA clears mcause and jumps; it sets no SP/GP, does not disable "
                           "interrupts and issues no fence.i, so a RAM image provides its own startup",
        "device_verified": False,
    }


def make_plan(data, address, entry):
    validate_ram_range(address, len(data))
    if not 0 <= entry <= 0xFFFFFFFF:
        raise ProtocolError("Execution address must be uint32")
    return {
        "image_bytes": len(data), "sha256": hashlib.sha256(data).hexdigest(),
        "link_address": address, "entry_address": entry,
        "write_slices": (len(data) + RAM_SLICE - 1) // RAM_SLICE,
        "entry_inside_image": address <= entry < address + len(data),
        **describe(),
    }


def simulate(data, address, entry):
    simulator = Simulator()
    channel = SimulatorChannel(simulator)
    digest, info = load_and_run(channel.exchange, address, data, entry)
    return {"plan": make_plan(data, address, entry), "uploaded_sha256": digest,
            "readback_matches": True, "chip_id": f"0x{chip_id(info):08x}",
            "executed": [f"0x{value:08x}" for value in simulator.executed],
            "frames": channel.responses}


def main(argv=None):
    parser = argparse.ArgumentParser(prog="ramload.py",
                                     description="RAM upload/readback/execute over service-mode BUXX "
                                                 "(offline; the device path lives in the ODT channel)")
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("inspect", help="print the RAM loader command map")
    for name in ("plan", "simulate"):
        node = sub.add_parser(name, help=f"{name} a RAM image")
        node.add_argument("image")
        node.add_argument("--address", type=integer, default=0x2000)
        node.add_argument("--entry", type=integer)
    args = parser.parse_args(argv)
    if args.action == "inspect":
        print(json.dumps(describe(), indent=2))
        return
    data = Path(args.image).read_bytes()
    entry = args.entry if args.entry is not None else args.address
    result = make_plan(data, args.address, entry) if args.action == "plan" else simulate(data, args.address, entry)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()

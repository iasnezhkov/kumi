# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import hashlib
import json
from pathlib import Path
import struct
from typing import Callable, NamedTuple, Optional
import zlib


MAGIC = b"BUXX"
VID = 0x0666
PID = 0x0888
PACKET_SIZE = 512
HEADER_SIZE = 14
SLICE = 256
FLASH_BASE = 0x48000000
MAX_LENGTH = 4 * 1024 * 1024
HANDSHAKE = 0x0110
READ_AREA = 0x0122
READ_DATA = 0x0123
READ_JEDEC = 0x0125
READ_STATUS = 0x0126
HANDSHAKE_LENGTH = 44
READBACK_REFUSAL = "Command is outside the readback allowlist"


class ProtocolError(ValueError):
    pass


class Command(NamedTuple):
    reply: int
    check: Callable
    flags: int = 0
    reply_length: Optional[int] = None
    status: Optional[bytes] = None


def integer(value):
    return int(value, 0)


def validate_range(address, length):
    if not FLASH_BASE <= address < FLASH_BASE + MAX_LENGTH:
        raise ProtocolError("Address is outside the reference SPV40 flash window")
    if not 0 < length <= MAX_LENGTH or address + length > FLASH_BASE + MAX_LENGTH:
        raise ProtocolError("Length is outside the reference SPV40 flash window")


def validate_chip_id(value):
    if not 0 <= value <= 0xFFFFFFFF:
        raise ProtocolError("Expected chip ID must be uint32")
    return value


def validate_descriptor_sha256(value):
    if len(value) != 64 or any(c not in "0123456789abcdef" for c in value):
        raise ProtocolError("Expected descriptor SHA256 must be 64 lowercase hexadecimal digits")
    return value


def validate_timing(deadline_seconds, timeout_ms=None):
    if timeout_ms is None:
        if not 1 <= deadline_seconds <= 300:
            raise ProtocolError("Deadline must be 1..300 seconds")
    elif not 1 <= timeout_ms <= 5000 or not 1 <= deadline_seconds <= 300:
        raise ProtocolError("Timeout must be 1..5000 ms and deadline 1..300 seconds")


def manifest_path(output):
    return Path(str(output) + ".json")


def require_new_output(output):
    if Path(output).exists() or manifest_path(output).exists():
        raise FileExistsError("Output or manifest already exists")


def require_empty(payload):
    if payload:
        raise ProtocolError("This command requires an empty payload")


def area_check(name, invalid):
    def check(payload):
        if len(payload) != 16:
            raise ProtocolError(f"{name} area requires four uint32 values")
        address, length, chunk, reserved = struct.unpack("<4I", payload)
        validate_range(address, length)
        if chunk != SLICE or reserved:
            raise ProtocolError(invalid)
    return check


READ_COMMANDS = {
    HANDSHAKE: Command(0x0210, require_empty, flags=1),
    READ_AREA: Command(0x0222, area_check("Read", "Unexpected read area parameters")),
    READ_DATA: Command(0x0223, require_empty),
    READ_JEDEC: Command(0x0225, require_empty),
    READ_STATUS: Command(0x0226, require_empty),
}


def encode_frame(command, payload=b"", flags=0):
    header = MAGIC + struct.pack("<HBBH", command, 0, flags, len(payload))
    frame = header + struct.pack("<I", zlib.crc32(header))
    if payload:
        frame += payload + struct.pack("<I", zlib.crc32(payload))
    return frame.ljust(PACKET_SIZE, b"\0")


def decode_frame(packet, reply, expected_length):
    if len(packet) < HEADER_SIZE or packet[:4] != MAGIC:
        raise ProtocolError("Missing ODT response header")
    command, sequence, flags, length = struct.unpack_from("<HBBH", packet, 4)
    if struct.unpack_from("<I", packet, 10)[0] != zlib.crc32(packet[:10]):
        raise ProtocolError("ODT header CRC mismatch")
    if command != reply or sequence != 0:
        raise ProtocolError("Unexpected ODT response command or sequence")
    if length != expected_length:
        raise ProtocolError("Unexpected ODT response length")
    end = HEADER_SIZE + length
    if len(packet) < end + (4 if length else 0):
        raise ProtocolError("Truncated ODT response")
    payload = bytes(packet[HEADER_SIZE:end])
    if length and struct.unpack_from("<I", packet, end)[0] != zlib.crc32(payload):
        raise ProtocolError("ODT payload CRC mismatch")
    return payload, flags


def encode_request(command, payload=b"", commands=READ_COMMANDS, refusal=READBACK_REFUSAL):
    spec = commands.get(command)
    if spec is None:
        raise ProtocolError(refusal)
    spec.check(payload)
    return encode_frame(command, payload, spec.flags)


def decode_reply(packet, command, expected_length, commands=READ_COMMANDS, refusal=READBACK_REFUSAL):
    spec = commands.get(command)
    if spec is None or spec.reply_length not in (None, expected_length):
        raise ProtocolError(refusal)
    payload, flags = decode_frame(packet, spec.reply, expected_length)
    if spec.status is not None and payload != spec.status:
        raise ProtocolError("Flash write returned nonzero status")
    return payload, flags


class Channel:
    def __init__(self):
        self.responses = []

    def send(self, request):
        raise NotImplementedError

    def exchange(self, command, payload, expected_length):
        packet = self.send(encode_request(command, payload))
        data, flags = decode_reply(packet, command, expected_length)
        self.responses.append({"command": command, "response_flags": flags, "length": len(data)})
        return data


def read_flash(exchange, address, length):
    validate_range(address, length)
    exchange(READ_AREA, struct.pack("<4I", address, length, SLICE, 0), 0)
    data = bytearray()
    while len(data) < length:
        data.extend(exchange(READ_DATA, b"", min(SLICE, length - len(data))))
    if len(data) != length:
        raise ProtocolError("Flash read length mismatch")
    return bytes(data)


def chip_id(info):
    return struct.unpack_from("<I", info, 40)[0]


def verified_read(exchange, address, length, expected_chip):
    validate_range(address, length)
    info = exchange(HANDSHAKE, b"", HANDSHAKE_LENGTH)
    if len(info) != HANDSHAKE_LENGTH:
        raise ProtocolError("Handshake must contain eleven uint32 values")
    chip = chip_id(info)
    if chip != expected_chip:
        raise ProtocolError(f"Chip mismatch: expected 0x{expected_chip:08x}, got 0x{chip:08x}")
    first = read_flash(exchange, address, length)
    second = read_flash(exchange, address, length)
    if first != second:
        raise ProtocolError("Two independent flash reads differ")
    return first, info


def save_result(output, data, manifest):
    output = Path(output)
    require_new_output(output)
    digest = hashlib.sha256(data).hexdigest()
    with output.open("xb") as stream:
        stream.write(data)
    if hashlib.sha256(output.read_bytes()).hexdigest() != digest:
        raise IOError("Saved readback hash mismatch")
    manifest.update({"bytes": len(data), "sha256": digest, "identical_reads": 2})
    with manifest_path(output).open("x") as stream:
        json.dump(manifest, stream, indent=2)
        stream.write("\n")
    return digest

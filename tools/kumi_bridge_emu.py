# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
from pathlib import Path
import struct
import sys

from brom_emu import ModelError
import emulator_server
from musb import UsbProtocolError

MAX_FRAME = 4096
MAX_IDENTITY = 18 + 4096


def read_message(stream):
    prefix = stream.read(2)
    if not prefix:
        return None
    if len(prefix) != 2:
        raise EOFError("truncated length prefix")
    length = struct.unpack("<H", prefix)[0]
    if not 1 <= length <= MAX_FRAME:
        raise ValueError(f"host frame of {length} bytes")
    body = stream.read(length)
    if len(body) != length:
        raise EOFError("truncated host frame")
    return body


def write_message(stream, data):
    if not 1 <= len(data) <= MAX_IDENTITY:
        raise ValueError("bridge messages hold 1..4114 bytes")
    stream.write(struct.pack("<H", len(data)) + data)
    stream.flush()


def identity(session):
    host = session.require_runtime()
    device = host.control(bytes.fromhex("8006000100001200"))
    head = host.control(bytes.fromhex("8006000200000900"))
    total = int.from_bytes(head[2:4], "little")
    return device + host.control(struct.pack("<BBHHH", 0x80, 6, 0x200, 0, total))


def drain(session):
    frames = []
    while True:
        try:
            frames.append(session.receive())
        except UsbProtocolError as error:
            if "timed out" in str(error):
                return frames
            raise


def serve(config, source, sink, log=sys.stderr):
    session = emulator_server.Session(config)
    if session.mode != "runtime":
        print("Expected configured Kumi 1209:0001 with one vendor interface FF/4B/01 and bulk IN and OUT", file=log)
        return 1
    write_message(sink, identity(session))
    print("Kumi interface on the emulated case", file=log)
    while True:
        frame = read_message(source)
        if frame is None:
            return 0
        try:
            session.advance()
            session.send(frame)
            for reply in drain(session):
                write_message(sink, reply)
        except ModelError as error:
            print(f"USB device gone: {error}", file=log)
            return 1


def main(argv=None):
    parser = argparse.ArgumentParser(description="The phone helper's kumi-bridge framing (u16 length + frame on "
                                                 "stdin/stdout) served by the emulated case, for host tests")
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args(argv)
    return serve(json.loads(args.config.read_text()), sys.stdin.buffer, sys.stdout.buffer)


if __name__ == "__main__":
    raise SystemExit(main())

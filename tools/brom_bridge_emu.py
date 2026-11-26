# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
from pathlib import Path
import struct
import sys
import time

import brom_emu
import spv5048

PACKET = 512
WRITE_COMMANDS = {0x0120, 0x0121}
STATUS_WRITE = 0x0127
MODES = ("bridge", "install-bridge")
REPLY_POLLS = 200_000


def write_message(stream, data):
    stream.write(struct.pack("<H", len(data)) + data)
    stream.flush()


def refuse(request, mode):
    if request[:4] != b"BUXX":
        return "not a BUXX request"
    command = request[4] | request[5] << 8
    if command == STATUS_WRITE:
        return "status-register writes are never forwarded"
    if command in WRITE_COMMANDS and mode != "install-bridge":
        return "writes need install-bridge"
    return None


def exchange(host, request):
    host.transfer_out(2, request, zero_length_packet=False)
    reply = bytearray()
    while len(reply) < len(request):
        packet = host.read_in(1, polls=REPLY_POLLS)
        if packet is None:
            raise brom_emu.UsbProtocolError("boot ROM did not answer a BUXX request")
        reply.extend(packet)
        if len(packet) < host.max_packet(1, receive=False):
            break
    return bytes(reply)


def serve(brom, flash_path, mode, source, sink, log, trace=False):
    flash = Path(flash_path).read_bytes()
    emu = brom_emu.Emulator(brom, flash, spv5048.SERVICE_MODE_BIT, writable=True)
    host, device, config = brom_emu.service_boot(emu)
    write_message(sink, device + config)
    rebooted = False
    try:
        while True:
            request = source.read(PACKET)
            if not request:
                break
            if len(request) != PACKET:
                raise EOFError("truncated boot ROM request")
            reason = refuse(request, mode)
            if reason:
                print(f"refused: {reason}", file=log)
                break
            command = request[4] | request[5] << 8
            started = time.monotonic()
            reply = exchange(host, request)
            if trace:
                print(json.dumps({"command": f"{command:04x}", "ms": round((time.monotonic() - started) * 1000)}), file=log,
                      flush=True)
            write_message(sink, reply)
            if command == 0x0124:
                rebooted = True
                break
    finally:
        Path(flash_path).write_bytes(bytes(emu.flash.image))
        writes = [(w["command"], w["address"]) for w in emu.flash.writes]
        print(json.dumps({"writes": len(writes), "rebooted": rebooted}), file=log)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Emulated phone helper in bridge or install-bridge mode: the boot ROM "
                                                 "dump in service mode over a flash image file that is updated in place")
    parser.add_argument("--brom", required=True)
    parser.add_argument("--flash", required=True, help="4 MiB flash image, rewritten with the result")
    parser.add_argument("--trace", action="store_true", help="log each boot ROM command and its reply time")
    parser.add_argument("mode", choices=MODES)
    parser.add_argument("usb_path")
    args = parser.parse_args(argv)
    serve(Path(args.brom).read_bytes(), args.flash, args.mode, sys.stdin.buffer, sys.stdout.buffer, sys.stderr,
          args.trace)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

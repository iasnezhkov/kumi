# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import select
import shlex
import struct
import subprocess
import tempfile
import time

from odt import (FLASH_BASE, HANDSHAKE, HANDSHAKE_LENGTH, Channel, Command, ProtocolError,
                  chip_id, decode_reply, encode_request, integer, require_empty,
                  require_new_output, save_result, validate_chip_id, validate_descriptor_sha256,
                  validate_range, validate_timing, verified_read)


RETURN_TO_RUNTIME = 0x0124
RETURN_COMMANDS = {RETURN_TO_RUNTIME: Command(0x0224, require_empty)}
RETURN_REFUSAL = "Command is outside the return-to-runtime allowlist"


class Bridge(Channel):
    def __init__(self, serial, helper, usb_path, seconds, mode="bridge"):
        super().__init__()
        if mode not in ("bridge", "scratch-bridge"):
            raise ProtocolError("Unknown helper bridge mode")
        self.deadline = time.monotonic() + seconds
        self.log = tempfile.TemporaryFile()
        command = f"timeout -s KILL {seconds} {shlex.quote(helper)} {mode} {shlex.quote(usb_path)}"
        self.process = subprocess.Popen(["adb", "-s", serial, "shell", "-T", command],
                                        stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=self.log)
        try:
            raw = self.frame()
            if len(raw) < 27 or raw[8:12] != bytes.fromhex("66068808"):
                raise ProtocolError("Unexpected BROM descriptors")
            total = struct.unpack_from("<H", raw, 20)[0]
            if len(raw) != 18 + total:
                raise ProtocolError("Truncated BROM configuration")
            self.identity = {"usb_path": usb_path, "device_descriptor_hex": raw[:18].hex(),
                             "configuration_descriptor_hex": raw[18:].hex(),
                             "descriptor_sha256": hashlib.sha256(raw).hexdigest()}
        except BaseException:
            self.close()
            raise

    def read_exact(self, length):
        data = bytearray()
        while len(data) < length:
            remaining = min(5, self.deadline - time.monotonic())
            if remaining <= 0 or not select.select([self.process.stdout], [], [], remaining)[0]:
                raise TimeoutError("USB bridge response deadline exceeded")
            part = os.read(self.process.stdout.fileno(), length - len(data))
            if not part:
                self.log.seek(0)
                raise ProtocolError("USB bridge closed: " + self.log.read().decode(errors="replace"))
            data.extend(part)
        return bytes(data)

    def frame(self):
        length = struct.unpack("<H", self.read_exact(2))[0]
        if length > 4114:
            raise ProtocolError("USB bridge frame exceeds the descriptor limit")
        return self.read_exact(length)

    def send(self, request):
        if time.monotonic() >= self.deadline:
            raise TimeoutError("USB bridge deadline exceeded")
        if self.process.stdin.write(request) != len(request):
            raise ProtocolError("Short bridge request write")
        self.process.stdin.flush()
        return self.frame()

    def reboot(self):
        packet = self.send(encode_request(RETURN_TO_RUNTIME, b"", RETURN_COMMANDS, RETURN_REFUSAL))
        decode_reply(packet, RETURN_TO_RUNTIME, 0, RETURN_COMMANDS, RETURN_REFUSAL)

    def helper_log(self):
        self.log.seek(0)
        return self.log.read().decode(errors="replace")

    def close(self):
        if self.process.stdin and not self.process.stdin.closed:
            try:
                self.process.stdin.close()
            except BrokenPipeError:
                pass
        try:
            self.process.wait(timeout=3)
        except subprocess.TimeoutExpired:
            self.process.kill()
            self.process.wait(timeout=3)
        self.process.stdout.close()
        self.log.close()


def main(argv=None):
    parser = argparse.ArgumentParser(description="SPV40 reference readback through an Android USB host")
    parser.add_argument("action", choices=("inspect", "dump", "reboot"))
    parser.add_argument("--serial", required=True)
    parser.add_argument("--helper", required=True)
    parser.add_argument("--usb-path", required=True)
    parser.add_argument("--expected-descriptor-sha256")
    parser.add_argument("--expected-chip-id", type=integer)
    parser.add_argument("--length", type=integer)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--deadline-seconds", type=int, default=180)
    args = parser.parse_args(argv)
    if not re.fullmatch(r"/dev/bus/usb/[0-9]{3}/[0-9]{3}", args.usb_path):
        parser.error("USB path must select one bus and device")
    try:
        validate_timing(args.deadline_seconds)
    except ProtocolError as error:
        parser.error(str(error))
    if args.action == "dump":
        if any(value is None for value in (args.expected_descriptor_sha256, args.expected_chip_id, args.length, args.output)):
            parser.error("dump requires descriptor SHA256, chip ID, length and output")
        validate_range(FLASH_BASE, args.length)
        try:
            validate_descriptor_sha256(args.expected_descriptor_sha256)
            validate_chip_id(args.expected_chip_id)
        except ProtocolError as error:
            parser.error(str(error))
        require_new_output(args.output)
    bridge = Bridge(args.serial, args.helper, args.usb_path, args.deadline_seconds)
    try:
        if args.expected_descriptor_sha256 and bridge.identity["descriptor_sha256"] != args.expected_descriptor_sha256:
            raise ProtocolError("USB descriptor fingerprint mismatch")
        if args.action == "inspect":
            info = bridge.exchange(HANDSHAKE, b"", HANDSHAKE_LENGTH)
            print(json.dumps({"identity": bridge.identity, "handshake_hex": info.hex(),
                              "handshake_words": list(struct.unpack("<11I", info)),
                              "chip_id": chip_id(info)}, indent=2))
        elif args.action == "reboot":
            bridge.reboot()
            print(json.dumps({"return_command_acknowledged": True}))
        else:
            data, info = verified_read(bridge.exchange, FLASH_BASE, args.length, args.expected_chip_id)
            manifest = {"protocol": "SPV40 ODT reference through Android usbfs", "identity": bridge.identity,
                        "handshake_hex": info.hex(), "expected_chip_id": args.expected_chip_id,
                        "start_address": FLASH_BASE, "responses": bridge.responses,
                        "flash_programming_performed": False, "reset_performed": False}
            digest = save_result(args.output, data, manifest)
            print(json.dumps({"output": str(args.output), "bytes": len(data), "sha256": digest}))
    finally:
        bridge.close()


if __name__ == "__main__":
    main()

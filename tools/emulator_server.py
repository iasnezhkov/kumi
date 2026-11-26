# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import hashlib
import json
from pathlib import Path
import struct
import sys

from brom_emu import Emulator, ModelError, elf_symbols, firmware_flash, model_description, usb_host
from ram_host import RamHost, RuntimeStopped, boot_runtime
from spv5048 import SERVICE_MODE_BIT

MAX_FRAME = 4096
MODEL_CHUNK = 3000
RETENTION = ("lost", "kept")
MAX_RESETS = 8


class Session:
    def __init__(self, config):
        self.retention = config.get("retention")
        if self.retention not in RETENTION:
            raise ValueError("config 'retention' must be 'lost' or 'kept': whether RAM survives a software reset "
                             "has not been measured (gate G4), so every run states its assumption")
        self.max_resets = int(config.get("max_resets", MAX_RESETS))
        self.native = config.get("native", True)
        self.rom = Path(config["brom"]).read_bytes()
        self.elf = Path(config["elf"]).read_bytes()
        stock = Path(config["flash"]).read_bytes()
        self.symbols = elf_symbols(self.elf)
        self.inputs = {"brom_sha256": hashlib.sha256(self.rom).hexdigest(), "flash_sha256": hashlib.sha256(stock).hexdigest(),
                       "elf_sha256": hashlib.sha256(self.elf).hexdigest()}
        flash = stock
        if self.native:
            stage0, image = Path(config["stage0"]).read_bytes(), Path(config["image"]).read_bytes()
            self.inputs.update({"stage0_sha256": hashlib.sha256(stage0).hexdigest(),
                                "image_sha256": hashlib.sha256(image).hexdigest()})
            flash = firmware_flash(stock, stage0, image)
        self.boots = []
        self.resets = []
        self.restarts = 0
        self.mode = None
        self.host = None
        self.emu = None
        self.start(flash, 0, None, "power_on")

    def start(self, flash, boot_flag, ram, kind):
        if not self.native:
            boot_flag |= SERVICE_MODE_BIT
        for _ in range(self.max_resets + 1):
            self.emu = Emulator(self.rom, flash, boot_flag=boot_flag, writable=self.native, stop_at_image=False, ram=ram)
            self.host = None
            outcome, result = self.boot()
            if outcome == "reset_requested":
                self.resets.append({"after": kind, "during": "boot", "boot_flag": f"0x{self.emu.boot_flag():08x}"})
                flash, boot_flag, ram = self.reset_state()
                kind = "software_reset"
                continue
            return
        raise ModelError(f"more than {self.max_resets} consecutive resets")

    def boot(self):
        if not self.native:
            try:
                self.host = boot_runtime(self.emu, self.elf)
            except RuntimeStopped as error:
                return error.outcome.get("result"), None
            return self.enter_runtime()
        result = self.emu.boot(100_000_000, stops={self.symbols["kumi_poll"]: "runtime_poll"})
        outcome = (result["outcome"] or {}).get("result")
        if outcome == "reset_requested":
            return outcome, result
        if result["faults"]:
            raise ModelError(f"native boot faulted: {result['faults']}")
        if outcome == "service_ready":
            host = usb_host(self.emu)
            host.attach()
            device, _ = host.enumerate(configuration=None)
            self.mode = "bootrom"
            self.boots.append({"kind": "bootrom", "device_descriptor": device.hex()})
            return outcome, result
        if outcome != "runtime_poll":
            raise ModelError(f"native boot stopped at {result['outcome']}")
        self.host = RamHost(self.emu, self.elf, running=True)
        return self.enter_runtime()

    def enter_runtime(self):
        try:
            self.host.enumerate()
            self.host.drain_in(1)
        except RuntimeStopped as error:
            return error.outcome.get("result"), None
        self.mode = "runtime"
        self.boots.append(self.host.identity or {"kind": "ram", "elf_sha256": self.emu.loaded_elf_sha256})
        return "runtime_poll", None

    def reset_state(self):
        ram = self.emu.ram_image() if self.retention == "kept" else None
        return self.emu.flash.image, self.emu.boot_flag(), ram

    def guarded(self, action):
        try:
            return action()
        except RuntimeStopped as error:
            if error.outcome.get("result") != "reset_requested":
                raise
            self.restarts += 1
            self.resets.append({"after": "runtime", "during": "request", "boot_flag": f"0x{self.emu.boot_flag():08x}"})
            self.start(*self.reset_state(), "software_reset")
            return None

    def require_runtime(self):
        if self.host is None:
            raise ModelError("runtime is not running (boot ROM recovery)")
        return self.host

    def advance(self):
        if self.host is not None:
            self.guarded(self.host.pump)

    def send(self, frame):
        host = self.require_runtime()
        if self.guarded(lambda: host.send_frame(frame) or True) is None:
            raise ModelError("device reset during the transfer")

    def receive(self):
        host = self.require_runtime()
        reply = self.guarded(host.receive_frame)
        if reply is None:
            raise ModelError("device reset during the transfer")
        return reply

    def command(self, request):
        if not isinstance(request, dict):
            raise ValueError("emulator command must be a JSON object")
        op = request.get("op")
        if op == "uart":
            host = self.require_runtime()
            data = bytes.fromhex(request["hex"])
            self.guarded(lambda: host.uart(data))
            return {"hid": [p.hex() for p in self.guarded(lambda: self.require_runtime().drain_in(1)) or []]}
        if op == "reset":
            kind = request.get("kind", "software")
            if kind == "power":
                self.start(self.emu.flash.image, 0, None, "power_on")
            elif kind == "software":
                self.restarts += 1
                self.start(*self.reset_state(), "software_reset")
            else:
                raise ValueError("reset kind must be 'software' or 'power'")
            return {"mode": self.mode, "restarts": self.restarts}
        if op == "idle":
            milliseconds = int(request["ms"])
            if not 0 < milliseconds <= 60_000:
                raise ValueError("idle must be 1..60000 ms")
            host = self.require_runtime()
            self.guarded(lambda: host.idle(milliseconds))
            return {"mode": self.mode}
        if op == "fault":
            host = self.require_runtime()
            kind = request["kind"]
            words = {"illegal": "ffffffff", "breakpoint": "73001000"}
            if kind not in words:
                raise ValueError("invalid fault kind")
            pc = self.emu.pc()
            self.emu.write_memory(pc, bytes.fromhex(words[kind]))
            self.guarded(host.pump)
            return {"mode": self.mode, "restarts": self.restarts}
        if op == "report":
            return self.report(full=False)
        if op == "model":
            text = self.model_text()
            offset = int(request.get("offset", 0))
            if not 0 <= offset <= len(text):
                raise ValueError("model offset out of range")
            return {"total": len(text), "offset": offset, "text": text[offset:offset + MODEL_CHUNK]}
        raise ValueError("unknown emulator command")

    def model_text(self):
        return json.dumps(model_description(self.retention), sort_keys=True)

    def report(self, full=True):
        model = model_description(self.retention)
        if not full:
            model = {key: model[key] for key in ("cpu", "virtual_cpu_hz", "virtual_cycles_per_instruction", "reset_retention")}
            model["assumption_count"] = len(model_description(self.retention)["assumptions"])
            model["sha256"] = hashlib.sha256(self.model_text().encode()).hexdigest()
            model["fetch"] = {"op": "model", "offset": 0}
        return {"mode": self.mode, "boots": self.boots, "restarts": self.restarts, "resets": self.resets,
                "faults": self.emu.faults, "flash_writes": len(self.emu.flash.writes), "instructions": self.emu.steps,
                "retention": self.retention, "inputs": self.inputs, "model": model}


def read_exact(stream, length):
    data = bytearray()
    while len(data) < length:
        part = stream.read(length - len(data))
        if not part:
            if not data:
                return None
            raise EOFError("truncated frame")
        data.extend(part)
    return bytes(data)


def handle(session, message):
    if session is None:
        raise ModelError("emulator session failed to start")
    session.advance()
    if message[0] == 1:
        session.send(message[1:])
        return b""
    if message[0] == 2 and len(message) == 1:
        return session.receive()
    if message[0] == 3:
        return json.dumps(session.command(json.loads(message[1:]))).encode()
    raise ValueError("invalid IPC operation")


def serve(config, source, sink):
    session, startup = None, None
    try:
        session = Session(config)
    except Exception as error:
        startup = f"emulator session failed to start: {type(error).__name__}: {error}"
    while True:
        header = read_exact(source, 4)
        if header is None:
            return
        length, = struct.unpack("<I", header)
        if not 1 <= length <= MAX_FRAME:
            raise ValueError("invalid IPC frame length")
        message = read_exact(source, length)
        if message is None:
            raise EOFError("missing IPC frame")
        try:
            if startup:
                raise ModelError(startup)
            response = b"\0" + handle(session, message)
            if len(response) > MAX_FRAME:
                raise ModelError(f"reply of {len(response)} bytes exceeds the {MAX_FRAME}-byte IPC frame")
        except Exception as error:
            response = b"\1" + f"{type(error).__name__}: {error}".encode()[:MAX_FRAME - 1]
        sink.write(struct.pack("<I", len(response)) + response)
        sink.flush()


def main():
    parser = argparse.ArgumentParser(description="Serve the emulated case to the kumi client over stdin and stdout")
    parser.add_argument("--config", type=Path, required=True)
    args = parser.parse_args()
    serve(json.loads(args.config.read_text()), sys.stdin.buffer, sys.stdout.buffer)


if __name__ == "__main__":
    main()

# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import hashlib

from brom_emu import RETURN_ADDRESS, ModelError, elf_segments, elf_symbols, load_through_rom, service_boot
from musb import UsbHost, UsbProtocolError, UsbStall
import spv5048

RAM_LOADERS = ("rom", "direct")


class RuntimeStopped(ModelError):
    def __init__(self, message, outcome):
        super().__init__(message)
        self.outcome = outcome


class RamHost(UsbHost):
    def __init__(self, emulator, elf, running=False, result=None):
        self.emu = emulator
        self.loader = "direct" if not running and result is None else None
        self.identity = emulator.running_identity(elf) if running else None
        entry, self.segments = emulator.load_elf(elf, load=not running and result is None)
        try:
            self.poll_address = emulator.elf_symbols["kumi_poll"]
            self.fault_address = emulator.elf_symbols["kumi_fault"]
        except KeyError as error:
            raise ValueError("runtime ELF must contain kumi_poll and kumi_fault") from error
        self.stops = {self.poll_address: "runtime_poll"}
        super().__init__(emulator.usb, self.pump, emulator.system.usb_clock_ready, polls=128)
        if result is not None:
            self.check(result)
        elif running:
            self.check(emulator.report())
        else:
            emulator.write_reg(1, RETURN_ADDRESS)
            self.check(emulator.run(20_000_000, entry, self.stops))

    def check(self, result):
        outcome = result["outcome"] or {}
        if result["faults"] or outcome.get("result") != "runtime_poll":
            record = bytes(self.emu.uc.mem_read(spv5048.LAST_CRASH_ADDRESS, 16)).hex()
            raise RuntimeStopped(f"runtime failed: {outcome}, faults={result['faults']}, last_crash={record}", outcome)
        return result

    def pump(self, count=1):
        result = None
        for _ in range(count):
            result = self.emu.resume(50_000_000, self.stops)
            outcome = (result["outcome"] or {}).get("result")
            if outcome == "wfi" and not result["faults"] and self.emu.system.timer3_irq_enabled():
                self.emu.system.raise_timer3()
                result = self.emu.resume(50_000_000, self.stops)
            result = self.check(result)
        return result

    def idle(self, milliseconds):
        self.emu.advance_ms(milliseconds)
        self.pump()

    def connect(self):
        if not self.device.attached:
            self.device.host_attach(True)
        self.pump()

    def enumerate(self):
        self.connect()
        device, configuration = super().enumerate()
        configured = self.control(bytes.fromhex("8008000000000100"))
        if device[8:12] != bytes.fromhex("09120100") or configured != b"\x01":
            raise UsbProtocolError("Kumi USB enumeration failed")
        return {"device_descriptor": device.hex(), "configuration": configuration.hex(), "configured": configured.hex()}

    def bulk_request(self, frame):
        self.send_frame(frame)
        return self.receive_frame()

    def send_frame(self, frame):
        self.transfer_out(3, frame)

    def receive_frame(self):
        return self.transfer_in(2, limit=64)

    def uart(self, data):
        self.emu.uart.push_rx(data)
        for _ in range(max(4, len(data) + 4)):
            self.pump()
            if not self.emu.uart.rx:
                self.pump(2)
                return
        raise UsbProtocolError("UART receive timed out")


def load_runtime(emulator, elf, loader):
    if loader not in RAM_LOADERS:
        raise ValueError(f"loader must be one of {RAM_LOADERS}")
    host, _, _ = service_boot(emulator)
    entry, segments = elf_segments(elf)
    loads = [(address, contents + b"\x00" * (memory - len(contents)))
             for address, contents, memory in segments if emulator.in_ram(address, memory)]
    for index, (address, data) in enumerate(loads):
        if loader == "rom":
            last = index == len(loads) - 1
            load_through_rom(emulator, host, address, data, entry=entry if last else None, verify=False)
        else:
            emulator.write_memory(address, data)
    emulator.loaded_elf_sha256 = hashlib.sha256(elf).hexdigest()
    if loader == "direct":
        emulator.write_reg(1, RETURN_ADDRESS)
        emulator.uc.reg_write(emulator.r.UC_RISCV_REG_PC, entry)
    host.detach()
    return entry


def boot_runtime(emulator, elf, loader="direct"):
    entry = load_runtime(emulator, elf, loader)
    stops = {elf_symbols(elf)["kumi_poll"]: "runtime_poll"}
    start = emulator.pc() if loader == "rom" else entry
    result = emulator.run(30_000_000, start, stops, skip_initial_stop=loader == "rom")
    host = RamHost(emulator, elf, result=result)
    host.loader = loader
    return host


def runtime_smoke(emulator, elf, vectors=(), loader="direct"):
    host = boot_runtime(emulator, elf, loader)
    return exercise(host, emulator, elf, vectors)


def native_smoke(emulator, elf, vectors, budget):
    symbols = elf_symbols(elf)
    boot = emulator.boot(budget, stops={symbols["kumi_poll"]: "runtime_poll", symbols["kumi_fault"]: "runtime_fault"})
    if not boot["outcome"] or boot["outcome"]["result"] != "runtime_poll" or boot["faults"]:
        boot["boot"] = boot["outcome"]
        return boot
    report = exercise(RamHost(emulator, elf, running=True), emulator, elf, vectors)
    report["boot"] = boot["outcome"]
    return report


def exercise(host, emulator, elf, vectors):
    descriptors = host.enumerate()
    host.drain_in(1)
    checks = []
    for vector in vectors:
        response = host.bulk_request(bytes.fromhex(vector["request"]))
        ok = response.hex() == vector["response"]
        checks.append({"name": vector["name"], "ok": ok})
        if not ok:
            raise ModelError(f"RPC vector {vector['name']} failed: {response.hex()}")
    remap = next((v for v in vectors if v["name"] == "remap_a_to_b"), None)
    if remap:
        host.bulk_request(bytes.fromhex(remap["request"]))
    for key in (4, 0):
        body = bytes([4, 0, 0, key, 0, 0, 0, 0, 0])
        host.uart(bytes([0x4b, 1, 0x64, 11]) + body + sum(body).to_bytes(2, "little"))
    packets = host.drain_in(1)
    keyboard = [p for p in packets if p[:1] == b"\x01"]
    expected = bytes([1, 0, 0, 5 if remap else 4, 0, 0, 0, 0, 0])
    passed = keyboard == [expected, bytes([1]) + bytes(8)] and not emulator.faults
    report = emulator.report()
    report.update(descriptors)
    report.update({"elf_sha256": hashlib.sha256(elf).hexdigest(), "loaded_segments": host.segments,
                   "execution_identity": host.identity or {"kind": "ram", "elf_sha256": emulator.loaded_elf_sha256,
                                                           "loader": host.loader},
                   "rpc_checks": checks, "hid_packets": [p.hex() for p in packets],
                   "outcome": {"result": "runtime_smoke_passed" if passed else "runtime_smoke_failed"}})
    return report

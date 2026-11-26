# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import json
import os
from pathlib import Path
import struct
import unittest

from phone import KUMI, SERVICE, UsbDevice
import ramload
import runtime_ram
from spv5048 import BOOT_INFO_ADDRESS
from testenv import read, requires


def make_elf(entry, segments):
    header = bytearray(52)
    header[:7] = b"\x7fELF\x01\x01\x01"
    struct.pack_into("<HHIIIIIHHHHHH", header, 16, 2, 243, 1, entry, 52, 0, 0, 52, 32, len(segments), 40, 0, 0)
    table = bytearray()
    body = bytearray()
    offset = 52 + 32 * len(segments)
    for address, data, memory in segments:
        table += struct.pack("<8I", 1, offset + len(body), address, address, len(data), memory, 5, 4)
        body += data
    return bytes(header + table + body)


class SimulatedBridge:
    def __init__(self, simulator):
        self.simulator = simulator
        self.identity = {"usb_path": "/dev/bus/usb/001/005", "descriptor_sha256": "8be4b8ef" * 8}
        self.closed = False

    def send(self, request):
        return self.simulator.send(request)

    def close(self):
        self.closed = True


class FakePhone:
    def __init__(self, appears=True):
        self.simulator = ramload.Simulator()
        self.simulator.ram[BOOT_INFO_ADDRESS:BOOT_INFO_ADDRESS + 4] = b"KBI1"
        self.device = UsbDevice(*SERVICE, 1, 5, "480", "1-1")
        self.appears = appears
        self.clock = 0.0

    def devices(self):
        if self.simulator.executed:
            return [UsbDevice(*KUMI, 1, 6, "12", "1-1")] if self.appears else []
        return [self.device]

    def bridge(self, device, seconds):
        self.bridged = SimulatedBridge(self.simulator)
        return self.bridged

    def describe(self):
        return {"transport": "fake"}

    def hold_awake(self, device):
        return "on"

    def now(self):
        return self.clock

    def sleep(self, seconds):
        self.clock += seconds


ILM = (0x2000, bytes(range(256)) * 3, 0x300)
DLM = (0x102000, b"\x13" * 100, 0x200)


class PlanTests(unittest.TestCase):
    def test_segments_load_bootinfo_clears_and_the_entry_runs(self):
        phone = FakePhone()
        elf = make_elf(0x2000, [ILM, DLM])
        report = runtime_ram.start(phone, elf, out=lambda text: None)
        ram = phone.simulator.ram
        self.assertEqual(ram[0x2000:0x2300], ILM[1])
        self.assertEqual(ram[0x102000:0x102064], DLM[1])
        self.assertEqual(ram[BOOT_INFO_ADDRESS:BOOT_INFO_ADDRESS + 32], bytes(32))
        self.assertEqual(phone.simulator.executed, [0x2000])
        self.assertTrue(phone.bridged.closed)
        self.assertTrue(report["ok"])
        self.assertEqual(report["kumi"]["id"], "1209:0001")
        self.assertEqual([s["file_bytes"] for s in report["segments"]], [0x300, 100])
        self.assertEqual(report["timeline"][0]["event"], "jumped")
        json.dumps(report)

    def test_missing_enumeration_tells_the_owner_to_replug(self):
        report = runtime_ram.start(FakePhone(appears=False), make_elf(0x2000, [ILM]), wait_seconds=2, out=lambda text: None)
        self.assertFalse(report["ok"])
        self.assertIn("unplug", report["instruction"])

    def test_wrong_entry_and_segments_outside_the_runtime_windows_are_refused(self):
        for elf in (make_elf(0x2100, [ILM]), make_elf(0x2000, [ILM, (0x130000, b"\0" * 4, 4)]),
                    make_elf(0x2000, [ILM, (0x100000, b"\0" * 4, 4)])):
            with self.subTest(), self.assertRaises((runtime_ram.RunError, ValueError)):
                runtime_ram.plan(elf)

    def test_a_wrong_chip_stops_before_any_write(self):
        phone = FakePhone()
        phone.simulator.chip = 0x12345678
        with self.assertRaises(runtime_ram.RunError):
            runtime_ram.start(phone, make_elf(0x2000, [ILM]), out=lambda text: None)
        self.assertEqual(phone.simulator.trace, [0x110])

    @requires("KUMI_RUNTIME_ELF")
    def test_the_built_runtime_fits_the_ram_windows(self):
        entry, loads = runtime_ram.plan(read("KUMI_RUNTIME_ELF"))
        self.assertEqual(entry, 0x2000)
        self.assertGreaterEqual(len(loads), 2)


@requires("KUMI_BROM_DUMP", "KUMI_STOCK_DUMP", "KUMI_RUNTIME_ELF", "KUMI_RPC_VECTORS")
class EmulatorTests(unittest.TestCase):
    def test_the_boot_rom_loads_and_starts_the_runtime_which_enumerates_and_answers(self):
        from phone_emu import EmulatedPhone
        elf = read("KUMI_RUNTIME_ELF")
        phone = EmulatedPhone(read("KUMI_BROM_DUMP"), read("KUMI_STOCK_DUMP"), runtime_elf=elf)
        report = runtime_ram.start(phone, elf, verify=False, out=lambda text: None)
        self.assertTrue(report["ok"], report)
        self.assertEqual(report["kumi"]["id"], "1209:0001")
        self.assertEqual(phone.mode, "kumi")
        self.assertEqual(phone.entries[-1]["a0"], 0x2000)
        self.assertNotEqual(phone.entries[-1]["a0"], BOOT_INFO_ADDRESS)
        self.assertEqual(phone.entries[-1]["ra"], 0x82E96)
        self.assertEqual(bytes(phone.emu.uc.mem_read(BOOT_INFO_ADDRESS, 4)), bytes(4))
        vectors = {v["name"]: v for v in json.loads(Path(os.environ["KUMI_RPC_VECTORS"]).read_text())["vectors"]}
        info = vectors["device_info"]
        self.assertEqual(phone.kumi_frame(bytes.fromhex(info["request"])).hex(), info["response"])
        self.assertFalse(phone.emu.faults)
        self.assertFalse(phone.emu.flash.writes)


if __name__ == "__main__":
    unittest.main()

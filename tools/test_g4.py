# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import contextlib
import io
import json
from pathlib import Path
import random
import struct
import tempfile
import unittest

import g4
import odt
from phone import STOCK, SERVICE, UsbDevice
from phone_emu import helper_allows
import ramload
from spv5048 import CHIP_ID, SERVICE_MODE_BIT
from testenv import read, requires

PROBE_IMAGE = bytes(0x18) + g4.TABLE_MAGIC + bytes(0x400)


class FakeCase:
    def __init__(self, flag_kept=True, ram_kept=True, reset_taken=True, comes_back=True, rom_clears=True,
                 disarm_works=True):
        self.flag_kept, self.ram_kept, self.reset_taken = flag_kept, ram_kept, reset_taken
        self.comes_back, self.rom_clears, self.disarm_works = comes_back, rom_clears, disarm_works
        self.ram = bytearray(0x200000)
        self.flag = SERVICE_MODE_BIT
        self.number = 4
        self.mode = "service"
        self.device = UsbDevice(*SERVICE, 1, self.number, "480", "1-1")
        self.requests = []
        self.refused = []
        self.boots = []

    def boot(self, flag, keep_ram, cause):
        if not keep_ram:
            self.ram = bytearray(len(self.ram))
        self.flag = flag
        self.number += 1
        self.mode = "service" if flag & SERVICE_MODE_BIT else "stock"
        if not self.comes_back:
            self.mode = "off"
            self.device = None
        else:
            self.device = UsbDevice(*(SERVICE if self.mode == "service" else STOCK), 1, self.number, "480", "1-1")
        self.boots.append((cause, self.mode))

    def record(self, kind, before, after):
        self.ram[g4.REPORT_ADDRESS:g4.REPORT_ADDRESS + g4.REPORT_BYTES] = g4.encode_report(kind, before, after, a0=0)

    def execute(self, entry):
        before = self.flag
        if entry == g4.ENTRIES["report"]:
            self.record(1, before, before)
        elif entry == g4.ENTRIES["arm"]:
            self.flag |= SERVICE_MODE_BIT
            self.record(2, before, self.flag)
        elif entry == g4.ENTRIES["disarm"]:
            if self.disarm_works:
                self.flag &= ~SERVICE_MODE_BIT
            self.record(3, before, self.flag)
        elif entry in (g4.ENTRIES["reset_kumi"], g4.ENTRIES["reset_stock"]):
            stock = entry == g4.ENTRIES["reset_stock"]
            self.record(5 if stock else 4, before, SERVICE_MODE_BIT if stock else before | SERVICE_MODE_BIT)
            if not self.reset_taken:
                self.record(6, before, before)
                return
            flag = SERVICE_MODE_BIT if stock else before | SERVICE_MODE_BIT
            self.boot(flag if self.flag_kept else 0, self.ram_kept, "software_reset")

    def stock_enter(self):
        self.boot(SERVICE_MODE_BIT, self.ram_kept, "stock_request")


class FakeBridge(odt.Channel):
    def __init__(self, case, device):
        super().__init__()
        if device != case.device or case.mode != "service":
            raise odt.ProtocolError("USB bridge closed: Expected active BROM 0666:0888")
        self.case = case
        self.device = device
        self.identity = {"usb_path": device.path, "descriptor_sha256": "8be4b8ef" * 8}
        self.area = None

    def send(self, request):
        case = self.case
        if case.device != self.device:
            raise odt.ProtocolError("USB bridge closed: USB OUT: No such device")
        if not helper_allows(request):
            case.refused.append(request[:14].hex())
            raise odt.ProtocolError("USB bridge closed: Rejected command outside the read/return allowlist")
        command, _, _, length = struct.unpack_from("<HBBH", request, 4)
        payload = request[14:14 + length]
        case.requests.append(command)
        body = b""
        if command == 0x110:
            body = bytes(40) + struct.pack("<I", CHIP_ID)
        elif command in (0x128, 0x12A):
            address, size, _, _ = struct.unpack("<4I", payload)
            self.area = [address, size]
        elif command == 0x129:
            address, left = self.area
            count = min(len(payload), left)
            case.ram[address:address + count] = payload[:count]
            self.area = [address + count, left - count]
            body = b"\0"
        elif command == 0x12B:
            address, left = self.area
            count = min(256, left)
            body = bytes(case.ram[address:address + count])
            self.area = [address + count, left - count]
        elif command == 0x12C:
            reply = odt.encode_frame(0x22C, b"")
            case.execute(struct.unpack("<I", payload)[0])
            return reply
        elif command == 0x124:
            reply = odt.encode_frame(0x224, b"")
            case.boot(case.flag & ~SERVICE_MODE_BIT if case.rom_clears else case.flag, case.ram_kept, "boot_rom_reboot")
            return reply
        return odt.encode_frame(command + 0x100, body)

    def reboot(self):
        packet = self.send(odt.encode_frame(0x124, b""))
        odt.decode_frame(packet, 0x224, 0)

    def close(self):
        pass


class FakePhone:
    def __init__(self, case, power_cycle=None):
        self.case = case
        self.clock = 0.0
        self.prompts = []
        self.power_cycle = power_cycle

    def devices(self):
        return [self.case.device] if self.case.device else []

    def bridge(self, device, seconds):
        return FakeBridge(self.case, device)

    def enter_service(self, device):
        self.case.stock_enter()
        return {"hidraw": "/dev/hidraw0"}

    def prompt(self, text):
        self.prompts.append(text)
        if self.power_cycle:
            self.power_cycle(self.case)

    def wake_lock(self):
        return True

    def helper_sha256(self):
        return "52f4fd87" * 8

    def describe(self):
        return {"transport": "fake"}

    def hold_awake(self, device):
        return "on"

    def now(self):
        return self.clock

    def sleep(self, seconds):
        self.clock += seconds


def nonce():
    generator = random.Random(7)
    return lambda length: bytes(generator.randrange(256) for _ in range(length))


def session(case, power_cycle=None):
    return g4.Session(FakePhone(case, power_cycle), PROBE_IMAGE, nonce=nonce(), out=lambda text: None)


def power_off(case):
    case.boot(0, False, "power_on")


class DecisionTests(unittest.TestCase):
    def test_retained_flag_and_ram_pass_the_gate_and_return_to_stock(self):
        case = FakeCase()
        evidence = session(case, power_off).run(list(g4.TRIALS), power_cycle=True)
        verdict = evidence["verdict"]
        self.assertEqual(verdict["gate_g4"], "passed")
        self.assertTrue(verdict["returned_to_stock"])
        self.assertEqual(verdict["software_reset_stock"]["outcome"], "service_mode")
        self.assertEqual(case.mode, "stock")
        self.assertFalse(case.flag & SERVICE_MODE_BIT)
        self.assertFalse(case.refused)
        self.assertIn("RAM 0x137D00..0x138000 kept", evidence["evidence_line"])
        self.assertIn("power cut: stock", evidence["evidence_line"])
        power = g4.trial(evidence, "power_cycle")
        self.assertEqual(power["outcome"], "stock")
        self.assertFalse(power["markers"]["diag"]["intact"])
        self.assertEqual(power["markers"]["diag"]["zero_bytes"], 0x300)
        self.assertIn("stock firmware ran", power["markers_note"])
        kumi = g4.trial(evidence, "software_reset_kumi")
        self.assertEqual(kumi["record_kept_in_ilm"]["kind"], "reset_kumi")
        self.assertTrue(kumi["registers_after"]["bit31_after"])
        self.assertEqual([event["event"] for event in kumi["timeline"]][:2], ["markers_written", "reset_entry_called"])
        json.dumps(evidence)

    def test_lost_ram_with_a_kept_flag_is_recorded_as_such(self):
        evidence = session(FakeCase(ram_kept=False)).run(["software_reset_kumi"])
        kumi = g4.trial(evidence, "software_reset_kumi")
        self.assertEqual(evidence["verdict"]["gate_g4"], "bit31_kept_ram_lost")
        self.assertEqual(kumi["markers"]["diag"]["zero_bytes"], 0x300)
        self.assertIsNone(kumi["record_kept_in_ilm"])
        self.assertIn("RAM 0x137D00..0x138000 lost", evidence["evidence_line"])

    def test_a_lost_flag_fails_the_gate_without_an_evidence_line(self):
        case = FakeCase(flag_kept=False)
        evidence = session(case).run(["no_reset", "software_reset_kumi", "software_reset_stock"])
        self.assertEqual(evidence["verdict"]["gate_g4"], "failed")
        self.assertIsNone(evidence["evidence_line"])
        kumi = g4.trial(evidence, "software_reset_kumi")
        self.assertEqual(kumi["outcome"], "stock")
        self.assertIn("stock_service_request", [event["event"] for event in kumi["timeline"]])
        self.assertTrue(evidence["verdict"]["returned_to_stock"])
        self.assertEqual(case.mode, "stock")

    def test_a_reset_that_is_not_taken_is_read_from_the_payload_record(self):
        evidence = session(FakeCase(reset_taken=False)).run(["software_reset_kumi"])
        kumi = g4.trial(evidence, "software_reset_kumi")
        self.assertEqual(kumi["outcome"], "reset_not_taken")
        self.assertEqual(kumi["record"]["kind"], "reset_not_taken")
        self.assertEqual(evidence["verdict"]["gate_g4"], "failed")
        self.assertTrue(evidence["verdict"]["returned_to_stock"])

    def test_a_case_that_never_returns_aborts_with_the_replug_instruction(self):
        case = FakeCase(comes_back=False)
        evidence = session(case).run(["no_reset", "software_reset_kumi"])
        self.assertIn("did not come back", evidence["aborted"])
        self.assertFalse(evidence["return_to_stock"]["returned"])
        self.assertIn("unplug", evidence["return_to_stock"]["instruction"])
        self.assertEqual(evidence["verdict"]["gate_g4"], "incomplete")

    def test_return_to_stock_clears_bit_31_itself_when_the_reboot_would_not(self):
        case = FakeCase(rom_clears=False)
        evidence = session(case).run(["no_reset"])
        attempt = evidence["return_to_stock"]["attempts"][0]
        self.assertTrue(attempt["disarm"]["bit31_before"])
        self.assertFalse(attempt["bit31_before_reboot"])
        self.assertEqual(case.mode, "stock")

    def test_a_stuck_bit_31_is_reported_instead_of_looping(self):
        case = FakeCase(rom_clears=False, disarm_works=False)
        evidence = session(case).run(["no_reset"])
        result = evidence["return_to_stock"]
        self.assertFalse(result["returned"])
        self.assertEqual(len(result["attempts"]), 2)
        self.assertIn("unplug", result["instruction"])
        self.assertEqual(case.requests.count(0x124), 2)

    def test_the_recovery_command_path_returns_a_case_left_in_service_mode(self):
        case = FakeCase(rom_clears=False)
        case.flag = SERVICE_MODE_BIT
        result = session(case).return_to_stock()
        self.assertTrue(result["returned"])
        self.assertEqual(case.mode, "stock")
        self.assertTrue(session(case).return_to_stock()["returned"])

    def test_idle_trial_reports_whether_the_case_stayed(self):
        evidence = session(FakeCase()).run([], idle_seconds=30)
        idle = g4.trial(evidence, "idle")
        self.assertFalse(idle["left_usb"])
        self.assertTrue(idle["markers"]["diag"]["intact"])

    def test_session_refuses_a_case_outside_service_mode(self):
        case = FakeCase()
        case.boot(0, True, "power_on")
        evidence = session(case).run(["no_reset"])
        self.assertIn("not in boot ROM service mode", evidence["aborted"])
        self.assertTrue(evidence["return_to_stock"]["returned"])


class FormatTests(unittest.TestCase):
    def test_record_round_trip_and_foreign_memory(self):
        raw = g4.encode_report(4, 0x80000000, 0x80000000, a0=0x2010)
        record = g4.decode_report(raw)
        self.assertEqual(record["kind"], "reset_kumi")
        self.assertEqual(record["a0"], "0x00002010")
        self.assertEqual(record["registers"]["0x2100801c"], "0x80000000")
        self.assertTrue(record["bit31_after"])
        self.assertIsNone(g4.decode_report(bytes(128)))
        trapped = bytearray(raw)
        struct.pack_into("<I", trapped, 32, 1)
        self.assertIsNone(g4.decode_report(bytes(trapped))["registers"]["0x20003000"])
        with self.assertRaises(ValueError):
            g4.decode_report(raw[:64])

    def test_image_must_carry_the_entry_table(self):
        self.assertEqual(g4.check_image(PROBE_IMAGE), PROBE_IMAGE)
        with self.assertRaises(ValueError):
            g4.check_image(bytes(0x400))
        with self.assertRaises(ValueError):
            g4.check_image(PROBE_IMAGE + bytes(0x2000))

    def test_marker_comparison_counts_patterns(self):
        result = g4.compare(b"\x01\x02\x03\x04", b"\x01\x00\xff\x04")
        self.assertEqual((result["intact"], result["matching_bytes"], result["first_difference"]), (False, 2, 1))
        self.assertEqual((result["zero_bytes"], result["erased_bytes"]), (1, 1))

    def test_marker_areas_avoid_the_boot_rom_regions_and_the_probe(self):
        for address, length in g4.MARKERS.values():
            ramload.validate_ram_range(address, length)
            self.assertFalse(address < g4.LOAD_ADDRESS + g4.IMAGE_LIMIT and g4.LOAD_ADDRESS < address + length)
            self.assertFalse(address < g4.REPORT_ADDRESS + g4.REPORT_BYTES and g4.REPORT_ADDRESS < address + length)
        self.assertEqual(g4.MARKERS["diag"], (0x137D00, 0x300))

    def test_trial_names_are_checked(self):
        self.assertEqual(g4.parse_trials("no_reset,software_reset_kumi"), ["no_reset", "software_reset_kumi"])
        with self.assertRaises(Exception):
            g4.parse_trials("watchdog")

    def test_cli_decodes_a_saved_record(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "record.bin"
            path.write_bytes(g4.encode_report(3, 0x80000000, 0))
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                self.assertEqual(g4.main(["decode", str(path)]), 0)
            self.assertEqual(json.loads(output.getvalue())["kind"], "disarm")


class EmulatedG4:
    ram = None
    flag = "kept"
    power = False

    @classmethod
    def setUpClass(cls):
        from phone_emu import EmulatedPhone
        cls.phone = EmulatedPhone(read("KUMI_BROM_DUMP"), read("KUMI_STOCK_DUMP"), cls.ram, cls.flag)
        cls.phone.on_prompt = cls.phone.power_cycle
        image = ramload.ram_image(read("KUMI_PROBE_ELF"), g4.LOAD_ADDRESS)
        trials = list(g4.TRIALS) if cls.power else ["software_reset_kumi"]
        cls.evidence = g4.Session(cls.phone, image, nonce=nonce(), out=lambda text: None).run(trials,
                                                                                            power_cycle=cls.power)

    def test_session_returns_the_case_to_stock(self):
        self.assertNotIn("aborted", self.evidence)
        self.assertTrue(self.evidence["verdict"]["returned_to_stock"])
        self.assertEqual(self.phone.mode, "stock")
        self.assertFalse(self.phone.emu.boot_flag() & SERVICE_MODE_BIT)
        self.assertEqual(self.evidence["initial"]["identity"]["descriptor_sha256"],
                         "8be4b8ef57904b71332cb5a5cc8dee850cd2aaf13695fd5459332e3e69712e19")
        self.assertFalse(self.phone.emu.faults)

    def test_payload_runs_on_the_boot_rom_stack_and_returns(self):
        registers = self.evidence["initial"]["registers"]
        self.assertEqual(registers["ra"], "0x00082e96")
        self.assertEqual(registers["a0"], "0x00002004")
        self.assertTrue(registers["bit31_before"])


@requires("KUMI_BROM_DUMP", "KUMI_STOCK_DUMP", "KUMI_PROBE_ELF")
class EmulatedRetentionKeptTests(EmulatedG4, unittest.TestCase):
    ram = "kept"
    power = True

    def test_kept_ram_and_flag_pass_and_the_power_cut_returns_to_stock(self):
        verdict = self.evidence["verdict"]
        self.assertEqual(verdict["gate_g4"], "passed")
        self.assertEqual(verdict["software_reset_stock"]["outcome"], "service_mode")
        self.assertTrue(self.evidence["evidence_line"].startswith("tools/g4.py on emulator"))
        power = g4.trial(self.evidence, "power_cycle")
        self.assertEqual(power["outcome"], "stock")
        self.assertTrue(power["arm"]["bit31_after"])
        kumi = g4.trial(self.evidence, "software_reset_kumi")
        self.assertEqual(kumi["record_kept_in_ilm"]["kind"], "reset_kumi")
        self.assertEqual(kumi["record_kept_in_ilm"]["boot_flag_after"], "0x80000000")


@requires("KUMI_BROM_DUMP", "KUMI_STOCK_DUMP", "KUMI_PROBE_ELF")
class EmulatedRetentionLostTests(EmulatedG4, unittest.TestCase):
    ram = "lost"

    def test_lost_ram_is_measured_and_the_flag_still_selects_service_mode(self):
        kumi = g4.trial(self.evidence, "software_reset_kumi")
        self.assertEqual(kumi["outcome"], "service_mode")
        self.assertEqual(kumi["markers"]["diag"]["zero_bytes"], 0x300)
        self.assertEqual(self.evidence["verdict"]["gate_g4"], "bit31_kept_ram_lost")
        self.assertIn("lost", self.evidence["evidence_line"])


@requires("KUMI_BROM_DUMP", "KUMI_STOCK_DUMP", "KUMI_PROBE_ELF")
class EmulatedFlagLostTests(EmulatedG4, unittest.TestCase):
    ram = "kept"
    flag = "lost"

    def test_a_reset_that_drops_bit_31_comes_back_as_stock_and_fails(self):
        kumi = g4.trial(self.evidence, "software_reset_kumi")
        self.assertEqual(kumi["outcome"], "stock")
        self.assertEqual(self.evidence["verdict"]["gate_g4"], "failed")
        self.assertIsNone(self.evidence["evidence_line"])


if __name__ == "__main__":
    unittest.main()

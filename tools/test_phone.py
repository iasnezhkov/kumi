# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import subprocess
import unittest

import phone
from phone import SERVICE, STOCK, AdbPhone, Timeline, UsbDevice, parse_devices, parse_hidraw, wait_for

LISTING = """0666 0888 1 59 480 1-1
1d6b 0002 1 1 480 usb1
garbage line
0661 0881 1 60 480 1-1
"""


class Runner:
    def __init__(self, outputs):
        self.outputs = list(outputs)
        self.calls = []

    def __call__(self, argv, **kwargs):
        self.calls.append(argv)
        code, output = self.outputs.pop(0)
        return subprocess.CompletedProcess(argv, code, output, "denied" if code else "")


class ScriptedPhone:
    def __init__(self, listings):
        self.listings = list(listings)
        self.clock = 0.0

    def devices(self):
        return self.listings.pop(0) if len(self.listings) > 1 else self.listings[0]

    def now(self):
        return self.clock

    def sleep(self, seconds):
        self.clock += seconds


class ParseTests(unittest.TestCase):
    def test_sysfs_listing_and_paths(self):
        devices = parse_devices(LISTING)
        self.assertEqual(len(devices), 3)
        self.assertEqual(devices[0].path, "/dev/bus/usb/001/059")
        self.assertEqual(devices[0].kind, "boot_rom_service")
        self.assertEqual(devices[2].describe()["id"], "0661:0881")
        self.assertEqual(devices[1].kind, "other")

    def test_hidraw_belongs_to_the_device_by_sysfs_name(self):
        text = ("hidraw0 /sys/devices/platform/musb-hdrc.1/usb1/1-1/1-1:1.0/0003:0661:0881.0007/hidraw/hidraw0\n"
                "hidraw1 /sys/devices/platform/other/usb2/2-1/2-1:1.0/0003:1234:5678.0001/hidraw/hidraw1\n")
        stock = UsbDevice(*STOCK, 1, 60, "480", "1-1")
        self.assertEqual(parse_hidraw(text, stock), "/dev/hidraw0")
        self.assertIsNone(parse_hidraw(text, UsbDevice(*STOCK, 3, 2, "480", "3-1")))


class AdbPhoneTests(unittest.TestCase):
    def test_commands_go_through_adb_shell_with_the_serial(self):
        runner = Runner([(0, LISTING), (0, "on\n"), (0, "kumi-case other\n"), (1, "")])
        device = AdbPhone("SERIAL", "/data/local/tmp/spv40-usb", adb="fake-adb", runner=runner)
        listed = device.devices()
        self.assertEqual(runner.calls[0][:4], ["fake-adb", "-s", "SERIAL", "shell"])
        self.assertEqual(device.hold_awake(listed[0]), "on")
        self.assertIn("/sys/bus/usb/devices/1-1/power/control", runner.calls[1][4])
        self.assertTrue(device.wake_lock())
        self.assertIsNone(device.wake_lock())

    def test_enter_service_uses_the_hidraw_node_of_the_stock_device(self):
        runner = Runner([(0, "hidraw3 /sys/x/1-1/1-1:1.0/0003:0661:0881.0001/hidraw/hidraw3\n"),
                         (0, "Sent fixed 4b 01 ff runtime request\n")])
        device = AdbPhone("S", "/data/local/tmp/h", runner=runner)
        result = device.enter_service(UsbDevice(*STOCK, 1, 7, "480", "1-1"))
        self.assertEqual(result["hidraw"], "/dev/hidraw3")
        self.assertIn("/data/local/tmp/h enter /dev/hidraw3", runner.calls[1][4])
        with self.assertRaises(phone.PhoneError):
            AdbPhone("S", "h", runner=Runner([(0, "")])).enter_service(UsbDevice(*STOCK, 1, 7, "480", "1-1"))


class WaitTests(unittest.TestCase):
    def test_waits_for_the_old_device_to_leave_and_a_new_one_to_arrive(self):
        old = UsbDevice(*SERVICE, 1, 5)
        new = UsbDevice(*SERVICE, 1, 6)
        scripted = ScriptedPhone([[old], [old], [], [new]])
        timeline = Timeline(scripted.now)
        self.assertEqual(wait_for(scripted, {SERVICE}, 10, gone=old, timeline=timeline), new)
        self.assertEqual([len(event["devices"]) for event in timeline.events], [1, 0, 1])
        self.assertEqual(timeline.events[-1]["t"], 1.5)

    def test_a_quick_reconnect_is_seen_as_a_new_device_number(self):
        old = UsbDevice(*SERVICE, 1, 5)
        new = UsbDevice(*STOCK, 1, 6)
        self.assertEqual(wait_for(ScriptedPhone([[new]]), {SERVICE, STOCK}, 5, gone=old), new)

    def test_gives_up_at_the_deadline(self):
        old = UsbDevice(*SERVICE, 1, 5)
        scripted = ScriptedPhone([[old]])
        self.assertIsNone(wait_for(scripted, {SERVICE}, 3, gone=old))
        self.assertGreaterEqual(scripted.clock, 3)


class DiagTests(unittest.TestCase):
    def test_crash_record_bootinfo_and_stage0_report_decode(self):
        import struct
        import zlib
        import diag_read
        area = bytearray(768)
        crash = bytearray(256)
        crash[:5] = b"KDIA\x01"
        crash[5] = 2
        struct.pack_into("<H", crash, 6, 0x01F0)
        struct.pack_into("<I", crash, 8, 3)
        struct.pack_into("<6I", crash, 12, 2, 0x2468, 0, 0x2400, 0x127000, 1234)
        crash[36] = 7
        crash[40:47] = b"illegal"
        for i, stage in enumerate((0x0001, 0x0100, 0x01F0)):
            struct.pack_into("<H", crash, 136 + 2 * i, stage)
        crash[168] = 3
        struct.pack_into("<I", crash, 252, zlib.crc32(bytes(crash[:252])))
        area[0:256] = crash
        result = diag_read.decode(bytes(area))
        self.assertEqual(result["last_crash"]["reason"], "trap")
        self.assertEqual(result["last_crash"]["registers"]["mepc"], "0x00002468")
        self.assertEqual(result["last_crash"]["message"], "illegal")
        self.assertEqual(result["last_crash"]["trail"], ["0x0001", "0x0100", "0x01f0"])
        self.assertIsNone(result["current"])
        self.assertIsNone(result["boot_info"])
        self.assertIsNone(result["stage0_report"])
        area[5] ^= 1
        self.assertIsNone(diag_read.decode(bytes(area))["last_crash"])
        with self.assertRaises(ValueError):
            diag_read.decode(bytes(10))


if __name__ == "__main__":
    unittest.main()

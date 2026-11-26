# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest

from testenv import REQUIRE

TOOLS = Path(__file__).resolve().parent
HELPER = TOOLS / "android" / "spv40-usb.c"
HOST = TOOLS / "android" / "host"
COMPILER = shutil.which("cc") or shutil.which("clang")


def messages(raw):
    out = []
    while raw:
        length = struct.unpack_from("<H", raw)[0]
        out.append(raw[2:2 + length])
        raw = raw[2 + length:]
    return out


def framed(frames):
    return b"".join(struct.pack("<H", len(frame)) + frame for frame in frames)


@unittest.skipUnless(COMPILER or REQUIRE, "no C compiler")
class HelperOnFakeUsbfsTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.binary = Path(cls.directory.name) / "spv40-usb"
        cls.device = Path(cls.directory.name) / "usb-device"
        cls.device.write_bytes(b"")
        subprocess.run([COMPILER, "-std=c11", "-D_DEFAULT_SOURCE", "-D_DARWIN_C_SOURCE", "-Wall", "-Wextra", "-Werror",
                        "-I", str(HOST / "include"), "-o", str(cls.binary), str(HELPER), str(HOST / "fake_usbfs.c")],
                       check=True, capture_output=True, timeout=120)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def bridge(self, data, max_packet=64, **env):
        log = Path(self.directory.name) / f"usb-{max_packet}.log"
        result = subprocess.run([str(self.binary), "kumi-bridge", str(self.device)], input=data, capture_output=True,
                                timeout=30, env={**os.environ, "FAKE_USB_MAX": str(max_packet), "FAKE_USB_LOG": str(log),
                                                 **env})
        return result, messages(result.stdout), log.read_text().splitlines() if log.exists() else []

    def test_frames_cross_with_the_endpoint_packet_size_and_zero_length_packets(self):
        lengths = [1, 63, 64, 65, 127, 128, 511, 512, 513, 1024, 1500, 4096]
        frames = [bytes((7 * i + n) & 255 for i in range(n)) for n in lengths]
        for size in (64, 512):
            with self.subTest(max_packet=size):
                result, received, log = self.bridge(framed(frames), size)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertIn(f"max {size}", result.stderr.decode())
                identity, event, *echoes = received
                self.assertEqual(identity[8:12], bytes.fromhex("09120100"))
                self.assertEqual(len(identity), 18 + 48)
                self.assertEqual(event, bytes((0xE0 + i) & 255 for i in range(size)))
                self.assertEqual(echoes, [bytes(b ^ 0x5A for b in frame) for frame in frames])
                out = [int(line.split()[1]) for line in log if line.startswith("OUT")]
                expected = []
                for n in lengths:
                    expected += [size] * (n // size) + ([n % size] if n % size else [0])
                self.assertEqual(out, expected)
                self.assertEqual([line for line in log if not line.startswith(("OUT", "IN"))], ["CLAIM 1", "RELEASE 1"])

    def test_foreign_device_and_bad_host_frames_stop_the_helper(self):
        result, received, _ = self.bridge(b"", FAKE_USB_PID="0x0002")
        self.assertEqual(result.returncode, 1)
        self.assertIn(b"Expected configured Kumi 1209:0001", result.stderr)
        self.assertEqual(received, [])
        for data in (struct.pack("<H", 4097) + bytes(4097), struct.pack("<H", 0), struct.pack("<H", 10) + bytes(3)):
            with self.subTest(data=data[:4].hex()):
                result, received, _ = self.bridge(data)
                self.assertEqual(result.returncode, 1)
                self.assertEqual(len(received), 2)


if __name__ == "__main__":
    unittest.main()

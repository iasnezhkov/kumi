# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import os
from pathlib import Path
import shutil
import struct
import subprocess
import tempfile
import unittest
import zlib

TOOLS = Path(__file__).resolve().parent
SOURCE = TOOLS / "android" / "spv40-usb.c"
INCLUDE = TOOLS / "android" / "host" / "include"
COMPILER = shutil.which("cc") or shutil.which("clang")
FLASH = 0x48000000


def request(command, payload=b"", flags=0):
    head = b"BUXX" + struct.pack("<HBBH", command, 0, flags, len(payload))
    frame = head + struct.pack("<I", zlib.crc32(head))
    if payload:
        frame += payload + struct.pack("<I", zlib.crc32(payload))
    return frame.ljust(512, b"\0")


def area(command, address, size):
    return request(command, struct.pack("<4I", address, size, 256, 0))


@unittest.skipUnless(COMPILER, "no C compiler")
class HelperModeTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.directory = tempfile.TemporaryDirectory()
        cls.helper = Path(cls.directory.name) / "spv40-usb"
        subprocess.run([COMPILER, "-O1", "-Wall", "-Wextra", "-Werror", f"-I{INCLUDE}", "-o", str(cls.helper),
                        str(SOURCE)], check=True, timeout=120)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def allowed(self, packets, mode=None):
        args = [str(self.helper), "validate"] + ([mode] if mode else [])
        results = []
        for packet in packets:
            result = subprocess.run(args, input=packet, capture_output=True, timeout=10, env=dict(os.environ))
            results.append(result.returncode == 0)
        return results

    def test_reads_are_allowed_in_every_mode_and_writes_only_where_the_mode_says(self):
        read_area = area(0x122, FLASH, 0x1000)
        erase_head = area(0x120, FLASH, 0x1000)
        erase_scratch = area(0x120, FLASH + 0x3F0000, 0x1000)
        for mode in (None, "scratch", "install"):
            self.assertEqual(self.allowed([read_area, request(0x123)], mode), [True, True], mode)
        self.assertEqual(self.allowed([erase_head, erase_scratch]), [False, False])
        self.assertEqual(self.allowed([erase_head, erase_scratch], "scratch"), [False, True])
        self.assertEqual(self.allowed([erase_head, erase_scratch], "install"), [True, True])

    def test_install_mode_erases_one_sector_at_a_time_and_never_writes_the_status_register(self):
        self.assertEqual(self.allowed([area(0x120, FLASH, 0x2000), area(0x120, FLASH + 0x800, 0x1000),
                                       area(0x120, FLASH + 0x400000, 0x1000), request(0x127, b"\0")], "install"),
                         [False, False, False, False])

    def test_an_unknown_validation_mode_is_a_usage_error(self):
        result = subprocess.run([str(self.helper), "validate", "everything"], input=bytes(512), capture_output=True,
                                timeout=10)
        self.assertEqual(result.returncode, 2)


if __name__ == "__main__":
    unittest.main()

# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import struct
import unittest
import zlib

import stage0_dry

GOLDEN_REPORT = bytes.fromhex(
    "4b53305202000000400000004b424931010000000100000003000000010203000100000000000000e5f3478f00010002010000000000000000000000631c2854"
)


def seal(body):
    raw = bytearray(64)
    raw[:len(body)] = body
    struct.pack_into("<I", raw, 60, zlib.crc32(bytes(raw[:60])))
    return bytes(raw)


class DecodeTest(unittest.TestCase):
    def test_golden_report_shared_with_rust(self):
        result = stage0_dry.decode(GOLDEN_REPORT)
        self.assertEqual(result["outcome"], "would_jump")
        self.assertEqual(result["entry"], "0x00000040")
        self.assertEqual(result["boot_info"], {
            "slot": "B", "safe_mode": False, "boot_count": 1, "generation": 3,
            "image_version": "1.2.3", "stage0_version": "0.1.0",
        })
        self.assertEqual(result["slot_checks"], {"A": "not_checked", "B": "valid"})
        self.assertEqual(result["trial_attempts_left"], {"A": 0, "B": 2})
        self.assertFalse(result["journal_found"])
        self.assertFalse(result["previous_boot_crashed"])
        self.assertEqual(result["flash_head"], "0x00000000")

    def test_flash_head_proves_the_rom_read_path(self):
        body = bytearray(b"KS0R" + bytes([10]))
        body.extend(bytes(56 - len(body)))
        body[56:60] = struct.pack("<I", 0x0A40006F)
        result = stage0_dry.decode(seal(body))
        self.assertEqual(result["flash_head"], "0x0a40006f")

    def test_loop_breaker_flags_flash_mismatch_and_unhealthy_starts(self):
        body = bytearray(b"KS0R" + bytes([17, 1, 0, 6]))
        body.extend(bytes(53 - len(body)))
        body[52] = 3
        result = stage0_dry.decode(seal(body))
        self.assertEqual(result["outcome"], "flash_mismatch")
        self.assertEqual(result["flags"], ["boot_rom_marker_stored", "disarmed"])
        self.assertEqual(result["unhealthy_starts"], 3)
        self.assertEqual(stage0_dry.decode(GOLDEN_REPORT)["flags"], [])

    def test_bootrom_outcome_on_stock_flash(self):
        raw = seal(b"KS0R" + bytes([10]))
        result = stage0_dry.decode(raw)
        self.assertEqual(result["outcome"], "no_bootable_slot")
        self.assertNotIn("entry", result)
        self.assertEqual(result["slot_checks"], {"A": "not_checked", "B": "not_checked"})

    def test_trap_and_panic_records(self):
        trap = stage0_dry.decode(seal(b"KS0T" + struct.pack("<5I", 2, 0x128100, 0, 0x128200, 0x137000)))
        self.assertEqual(trap, {"record": "trap", "mcause": "0x00000002", "mepc": "0x00128100", "mtval": "0x00000000",
                                "ra": "0x00128200", "sp": "0x00137000"})
        panic = stage0_dry.decode(seal(b"KS0P" + bytes([5]) + b"index"))
        self.assertEqual(panic, {"record": "panic", "message": "index"})

    def test_damage_and_foreign_memory_are_refused(self):
        damaged = bytearray(GOLDEN_REPORT)
        damaged[20] ^= 1
        with self.assertRaises(ValueError):
            stage0_dry.decode(bytes(damaged))
        with self.assertRaises(ValueError):
            stage0_dry.decode(bytes(64))
        with self.assertRaises(ValueError):
            stage0_dry.decode(seal(b"XXXX"))

    def test_ram_image_comes_from_a_packed_stage0(self):
        image = bytearray(0x1000)
        struct.pack_into("<I", image, 4, stage0_dry.SPV_MAGIC)
        image[0x200:0x204] = b"\x6f\x00\x00\x00"
        self.assertEqual(stage0_dry.ram_image(bytes(image))[:4], b"\x6f\x00\x00\x00")
        with self.assertRaises(ValueError):
            stage0_dry.ram_image(bytes(0x1000))
        huge = bytearray(0x200 + stage0_dry.RAM_IMAGE_LIMIT + 4)
        struct.pack_into("<I", huge, 4, stage0_dry.SPV_MAGIC)
        with self.assertRaises(ValueError):
            stage0_dry.ram_image(bytes(huge))


if __name__ == "__main__":
    unittest.main()

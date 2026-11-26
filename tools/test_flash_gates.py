# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import struct
import unittest

import flash_gates
from odt import encode_frame
from spv5048 import CHIP_ID


class RomModel:
    def __init__(self, flash, ignore_address=False, drop_base=False, jedec=bytes.fromhex("1665c800"),
                 status=bytes.fromhex("00020000")):
        self.flash = bytearray(flash)
        self.ignore_address = ignore_address
        self.drop_base = drop_base
        self.jedec = jedec
        self.status = status
        self.address = self.left = 0
        self.commands = []

    def target(self, address, writing):
        offset = address - flash_gates.FLASH
        if writing and self.ignore_address:
            return 0
        if writing and self.drop_base:
            return offset & 0xFFFF
        return offset

    def send(self, packet):
        command, _, flags, length = struct.unpack_from("<HBBH", packet, 4)
        payload = bytes(packet[14:14 + length])
        self.commands.append(command)
        reply = b""
        if command == flash_gates.HANDSHAKE:
            reply = bytes(40) + struct.pack("<I", CHIP_ID)
        elif command == flash_gates.JEDEC:
            reply = self.jedec
        elif command == flash_gates.STATUS:
            reply = self.status
        elif command == flash_gates.AREA:
            address, size, _, _ = struct.unpack("<4I", payload)
            self.read_at = address - flash_gates.FLASH
            self.address = self.target(address, True)
            self.left = size
        elif command == flash_gates.ERASE:
            address, size, _, _ = struct.unpack("<4I", payload)
            at = self.target(address, True)
            self.flash[at:at + size] = b"\xff" * size
        elif command == flash_gates.READ:
            n = min(flash_gates.CHUNK, self.left)
            reply = bytes(self.flash[self.read_at:self.read_at + n])
            self.read_at += n
            self.left -= n
        elif command == flash_gates.PROGRAM:
            for i, byte in enumerate(payload):
                self.flash[self.address + i] &= byte
            self.address += len(payload)
            reply = b"\0"
        return encode_frame((command & 0xFF) | 0x200, reply)


def stock():
    image = bytearray(b"\xff" * flash_gates.FLASH_SIZE)
    image[:0x30000] = bytes((i * 7) & 0xFF for i in range(0x30000))
    image[0x12C:0x130] = bytes(4)
    image[0x200000:0x200010] = b"stock settings!!"
    return bytes(image)


class GatesTest(unittest.TestCase):
    def test_all_gates_pass_on_a_correct_boot_rom(self):
        model = RomModel(stock())
        gates = flash_gates.Gates(model, stock())
        gates.run()
        steps = [s["step"] for s in gates.steps]
        self.assertEqual(steps[0], "identity")
        self.assertEqual(steps[1], "flash_before")
        self.assertEqual(steps[-1], "flash_after")
        self.assertEqual(gates.steps[1]["sha256"], gates.steps[-1]["sha256"])
        self.assertEqual(bytes(model.flash), stock())
        self.assertLess(model.commands.index(flash_gates.PROGRAM), model.commands.index(flash_gates.ERASE))

    def test_program_patterns_are_no_ops_wherever_a_faulty_rom_lands_them(self):
        gates = flash_gates.Gates(RomModel(stock()), stock())
        gates.before = stock()
        for offset, length in ((flash_gates.WINDOW, 256), (flash_gates.FOUR_BYTES_OFFSET, 4),
                               (flash_gates.TAIL_OFFSET, 256)):
            pattern = gates.pattern(offset, length)
            self.assertNotEqual(pattern, b"\xff" * length)
            for at in flash_gates.landing_offsets(offset):
                with self.subTest(offset=offset, landing=at):
                    old = stock()[at:at + length]
                    self.assertEqual(bytes(a & b for a, b in zip(old, pattern)), old)
        self.assertEqual(flash_gates.landing_offsets(flash_gates.FOUR_BYTES_OFFSET), [0, 0x2C, 0x12C])

    def faulty_rom_fails_g6_without_changing_flash(self, model):
        gates = flash_gates.Gates(model, stock())
        with self.assertRaisesRegex(flash_gates.GateFailure, "G6 page read-back differs"):
            gates.run()
        self.assertEqual([s["step"] for s in gates.steps], ["identity", "flash_before", "scratch_block_erased"])
        self.assertEqual(bytes(model.flash), stock())
        self.assertNotIn(flash_gates.ERASE, model.commands)

    def test_a_rom_that_ignores_the_write_address_fails_g6_without_changing_flash(self):
        self.faulty_rom_fails_g6_without_changing_flash(RomModel(stock(), ignore_address=True))

    def test_a_rom_that_drops_the_window_base_fails_g6_without_changing_flash(self):
        self.faulty_rom_fails_g6_without_changing_flash(RomModel(stock(), drop_base=True))

    def test_flash_changed_elsewhere_during_the_gates_fails(self):
        model = RomModel(stock())
        gates = flash_gates.Gates(model, stock())
        erase = gates.erase

        def erase_and_disturb(offset, length):
            model.flash[0x200000] = 0x00
            return erase(offset, length)

        gates.erase = erase_and_disturb
        with self.assertRaisesRegex(flash_gates.GateFailure, "flash changed during the gates"):
            gates.run()
        self.assertEqual(gates.steps[-2]["step"], "G7_block_erase")

    def test_identity_and_preconditions_refuse_before_any_write(self):
        cases = (
            (RomModel(stock(), jedec=bytes.fromhex("1640ef00")), "flash identity"),
            (RomModel(stock(), jedec=bytes.fromhex("1565c800")), "flash identity"),
            (RomModel(stock(), status=bytes.fromhex("1c020000")), "block protection"),
        )
        for model, message in cases:
            with self.subTest(message=message):
                with self.assertRaisesRegex(flash_gates.GateFailure, message):
                    flash_gates.Gates(model, stock()).run()
                self.assertNotIn(flash_gates.PROGRAM, model.commands)
        dirty = bytearray(stock())
        dirty[flash_gates.WINDOW + 5] = 0
        model = RomModel(dirty)
        with self.assertRaisesRegex(flash_gates.GateFailure, "not erased"):
            flash_gates.Gates(model, stock()).run()
        changed = bytearray(stock())
        changed[0x100] ^= 1
        model = RomModel(changed)
        with self.assertRaisesRegex(flash_gates.GateFailure, "verified dump"):
            flash_gates.Gates(model, stock()).run()
        self.assertNotIn(flash_gates.PROGRAM, model.commands)

    def test_writes_outside_the_window_are_refused(self):
        gates = flash_gates.Gates(RomModel(stock()), stock())
        with self.assertRaises(flash_gates.GateFailure):
            gates.program(0, b"\0")
        with self.assertRaises(flash_gates.GateFailure):
            gates.erase(flash_gates.WINDOW - flash_gates.SECTOR, flash_gates.SECTOR)

    def test_default_deadline_covers_two_whole_flash_reads(self):
        exchanges = 2 * (flash_gates.FLASH_SIZE // flash_gates.CHUNK)
        self.assertGreaterEqual(flash_gates.DEFAULT_DEADLINE_SECONDS, exchanges * 0.02)


if __name__ == "__main__":
    unittest.main()

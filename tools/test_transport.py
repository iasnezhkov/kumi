# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import hashlib
import json
from pathlib import Path
import struct
import tempfile
import unittest

import adb_bridge
import odt
import ramload
from spv5048 import CHIP_ID


def reference_crc(data):
    value = 0xFFFFFFFF
    for byte in data:
        value ^= byte
        for _ in range(8):
            value = (value >> 1) ^ (0xEDB88320 if value & 1 else 0)
    return value ^ 0xFFFFFFFF


def reply(command, body=b"", sequence=0):
    header = b"BUXX" + struct.pack("<HBBH", command, sequence, 0, len(body))
    frame = header + struct.pack("<I", reference_crc(header))
    if body:
        frame += body + struct.pack("<I", reference_crc(body))
    return frame.ljust(512, b"\0")


class FakeRom(odt.Channel):
    def __init__(self, chip=CHIP_ID, changed_second_pass=False):
        super().__init__()
        self.requests = []
        self.chip = chip
        self.changed_second_pass = changed_second_pass
        self.pass_number = 0

    def send(self, packet):
        if len(packet) != 512:
            raise AssertionError("BUXX requests are 512-byte packets")
        header = packet[:10]
        command, sequence, flags, length = struct.unpack_from("<HBBH", header, 4)
        if header[:4] != b"BUXX" or reference_crc(header) != struct.unpack_from("<I", packet, 10)[0]:
            raise AssertionError("invalid request CRC")
        payload = packet[14:14 + length]
        if length and reference_crc(payload) != struct.unpack_from("<I", packet, 14 + length)[0]:
            raise AssertionError("invalid request payload CRC")
        if sequence != 0 or flags != (1 if command == 0x110 else 0):
            raise AssertionError("invalid request sequence or flags")
        self.requests.append(command)
        if command == 0x110:
            body = struct.pack("<11I", *([0] * 10 + [self.chip]))
        elif command == 0x122:
            self.address, self.length, chunk, reserved = struct.unpack("<4I", payload)
            if chunk != 256 or reserved != 0:
                raise AssertionError("invalid read area")
            self.offset = 0
            self.pass_number += 1
            body = b""
        elif command == 0x123:
            size = min(256, self.length - self.offset)
            body = bytes((self.address + self.offset + i) & 255 for i in range(size))
            if self.changed_second_pass and self.pass_number == 2:
                body = bytes([body[0] ^ 1]) + body[1:]
            self.offset += size
        else:
            raise AssertionError("unexpected command")
        return reply(command + 0x100, body)


class FramingTests(unittest.TestCase):
    def test_vendor_request_vectors(self):
        self.assertEqual(odt.encode_request(0x110)[:14].hex(), "425558581001000100006db58de8")
        area = struct.pack("<4I", 0x48000000, 512, 256, 0)
        self.assertEqual(odt.encode_request(0x122, area)[:34].hex(),
                         "42555858220100001000ad6b3fea00000048000200000001000000000000e6bb2ef5")
        self.assertEqual(odt.encode_request(0x123)[:14].hex(), "4255585823010000000059aaa16b")

    def test_read_allowlist_excludes_writes_and_returns(self):
        for command in (0, 1, 0x120, 0x121, 0x124, 0x127, 0x128, 0x12C, 0x130, 0xFFFF):
            with self.subTest(command=command), self.assertRaises(odt.ProtocolError):
                odt.encode_request(command)
        for command in (odt.READ_JEDEC, odt.READ_STATUS):
            self.assertEqual(odt.encode_request(command)[:4], b"BUXX")
            with self.assertRaises(odt.ProtocolError):
                odt.encode_request(command, b"\0")

    def test_corruption_and_wrong_replies(self):
        valid = reply(0x223, bytes(range(256)))
        self.assertEqual(odt.decode_reply(valid, 0x123, 256)[0], bytes(range(256)))
        for position in (0, 4, 6, 8, 10, 15, 270):
            damaged = bytearray(valid)
            damaged[position] ^= 1
            with self.subTest(position=position), self.assertRaises(odt.ProtocolError):
                odt.decode_reply(damaged, 0x123, 256)
        for bad in (valid[:13], valid[:273], reply(0x222, bytes(range(256))),
                    reply(0x223, bytes(range(256)), sequence=1), reply(0x223, b"short")):
            with self.assertRaises(odt.ProtocolError):
                odt.decode_reply(bad, 0x123, 256)


class ReadbackTests(unittest.TestCase):
    def test_two_complete_reads_with_short_last_chunk(self):
        rom = FakeRom()
        data, info = odt.verified_read(rom.exchange, 0x48000000, 513, CHIP_ID)
        self.assertEqual(data, bytes(range(256)) * 2 + b"\0")
        self.assertEqual(len(info), 44)
        self.assertEqual(rom.requests, [0x110, 0x122, 0x123, 0x123, 0x123, 0x122, 0x123, 0x123, 0x123])

    def test_chip_mismatch_prevents_flash_read(self):
        rom = FakeRom(chip=0x12345678)
        with self.assertRaisesRegex(odt.ProtocolError, "Chip mismatch"):
            odt.verified_read(rom.exchange, 0x48000000, 256, CHIP_ID)
        self.assertEqual(rom.requests, [0x110])

    def test_unstable_flash_fails(self):
        with self.assertRaisesRegex(odt.ProtocolError, "reads differ"):
            odt.verified_read(FakeRom(changed_second_pass=True).exchange, 0x48000000, 512, CHIP_ID)

    def test_invalid_ranges_prevent_io(self):
        for address, length in ((0, 256), (0x48000000, 0), (0x48000000, -1),
                                (0x48000000, 0x400001), (0x483FFFFF, 2)):
            rom = FakeRom()
            with self.subTest(address=address, length=length), self.assertRaises(odt.ProtocolError):
                odt.verified_read(rom.exchange, address, length, CHIP_ID)
            self.assertEqual(rom.requests, [])

    def test_save_hash_and_no_overwrite(self):
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "flash.bin"
            digest = odt.save_result(output, b"reference", {})
            self.assertEqual(digest, hashlib.sha256(b"reference").hexdigest())
            manifest = json.loads(Path(str(output) + ".json").read_text())
            self.assertEqual(manifest["identical_reads"], 2)
            with self.assertRaises(FileExistsError):
                odt.save_result(output, b"changed", {})
            self.assertEqual(output.read_bytes(), b"reference")

    def test_shared_validators(self):
        self.assertEqual(odt.validate_descriptor_sha256("0123456789abcdef" * 4), "0123456789abcdef" * 4)
        for value in ("a" * 63, "a" * 65, "A" * 64, "g" * 64):
            with self.subTest(value=value), self.assertRaises(odt.ProtocolError):
                odt.validate_descriptor_sha256(value)
        for value in (-1, 0x100000000):
            with self.subTest(value=value), self.assertRaises(odt.ProtocolError):
                odt.validate_chip_id(value)
        for deadline, timeout in ((0, 1000), (301, 1000), (120, 0), (120, 5001), (0, None), (301, None)):
            with self.subTest(deadline=deadline, timeout=timeout), self.assertRaises(odt.ProtocolError):
                odt.validate_timing(deadline, timeout)


class BridgeTests(unittest.TestCase):
    def test_return_to_runtime_frame(self):
        bridge = object.__new__(adb_bridge.Bridge)
        sent = []

        def send(request):
            sent.append(request)
            return reply(0x224)

        bridge.send = send
        bridge.reboot()
        self.assertEqual(sent[0][:14], b"BUXX" + struct.pack("<HBBH", 0x124, 0, 0, 0)
                         + struct.pack("<I", reference_crc(sent[0][:10])))
        self.assertEqual(sent[0][14:], bytes(498))
        damaged = bytearray(reply(0x224))
        damaged[10] ^= 1
        for bad in (reply(0x224)[:13], reply(0x225), reply(0x224, sequence=1), reply(0x224, b"\0"),
                    b"XXXX" + reply(0x224)[4:], bytes(damaged)):
            bridge.send = lambda request, bad=bad: bad
            with self.subTest(bad=bad[:14].hex()), self.assertRaises(odt.ProtocolError):
                bridge.reboot()


class RamLoadTests(unittest.TestCase):
    def test_describe_is_flash_free(self):
        info = ramload.describe()
        self.assertFalse(info["touches_flash"])
        self.assertFalse(info["device_verified"])

    def test_upload_readback_and_exec_roundtrip(self):
        image = bytes((i * 7 + 1) & 0xFF for i in range(600))
        sim = ramload.Simulator()
        channel = ramload.SimulatorChannel(sim)
        digest, info = ramload.load_and_run(channel.exchange, 0x2000, image, 0x2000, expected_chip=CHIP_ID)
        self.assertEqual(sim.ram[0x2000:0x2000 + len(image)], image)
        self.assertEqual(ramload.readback(channel.exchange, 0x2000, len(image)), image)
        self.assertEqual(sim.executed, [0x2000])
        self.assertEqual(odt.chip_id(info), CHIP_ID)
        self.assertEqual(digest, hashlib.sha256(image).hexdigest())

    def test_simulate_reports_matching_readback(self):
        result = ramload.simulate(b"\x90" * 300, 0x3000, 0x3004)
        self.assertTrue(result["readback_matches"])
        self.assertEqual(result["executed"], ["0x00003004"])
        self.assertEqual(result["plan"]["write_slices"], 2)

    def test_reserved_and_bounds_refused(self):
        for address, length in ((0x100000, 0x10), (0x13F000, 0x10), (0x1FFFF0, 0x40), (0x4003FFF0, 0x20)):
            with self.subTest(address=address), self.assertRaises(odt.ProtocolError):
                ramload.validate_ram_range(address, length)
        ramload.validate_ram_range(0x100000, 0x10, allow_reserved=True)
        ramload.validate_ram_range(0x40000000, 0x40000)

    def test_rt40_upload_and_readback(self):
        exchange = ramload.exchange_over(ramload.Simulator())
        image = bytes((i * 13 + 5) & 0xFF for i in range(512))
        ramload.upload(exchange, 0x40000000, image)
        self.assertEqual(ramload.readback(exchange, 0x40000000, len(image)), image)

    def test_area_slice_and_status_checks(self):
        for command, payload in ((ramload.WRITE_RAM_AREA, struct.pack("<4I", 0x2000, 0x10, 128, 0)),
                                 (ramload.WRITE_RAM_DATA, b""), (ramload.WRITE_RAM_DATA, bytes(257)),
                                 (ramload.EXEC, bytes(3)), (0x0199, b"")):
            with self.subTest(command=command), self.assertRaises(odt.ProtocolError):
                ramload.encode_request(command, payload)
        with self.assertRaises(odt.ProtocolError):
            ramload.decode_reply(odt.encode_frame(0x0229, b"\x02"), ramload.WRITE_RAM_DATA, 1)

    def test_exchange_over_drives_a_raw_send_channel(self):
        exchange = ramload.exchange_over(ramload.Simulator())
        image = bytes(range(32))
        ramload.upload(exchange, 0x2000, image)
        self.assertEqual(ramload.readback(exchange, 0x2000, 32), image)


if __name__ == "__main__":
    unittest.main()

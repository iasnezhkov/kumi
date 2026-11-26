# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import struct
import unittest

from brom_emu import FLASH_SIZE, Emulator
import musb
from musb import UsbStall
from ram_host import RamHost
import spv5048
from test_runtime import RUNTIME, keys, rom_abi_fixture, vectors
from testenv import read, requires

CONTROL, STREAMING = 2, 3
OUT, FEEDBACK = 7, 4
CLOCK, FEATURE = 1, 10
CUR, RANGE = 1, 2
FREQUENCY, VALID, MUTE, VOLUME = 1, 2, 1, 2


def descriptors(config):
    parts, at = [], 0
    while at < len(config):
        parts.append(bytes(config[at:at + config[at]]))
        at += config[at]
    return parts


@requires(*RUNTIME)
class UsbAudioTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.elf = read('KUMI_RUNTIME_ELF')
        cls.vectors, cls.by_name = vectors('KUMI_RPC_VECTORS')

    def setUp(self):
        self.emu = Emulator(rom_abi_fixture(), b'\xff' * FLASH_SIZE, boot_flag=spv5048.SERVICE_MODE_BIT)
        self.host = RamHost(self.emu, self.elf)
        self.config = bytes.fromhex(self.host.enumerate()['configuration'])
        self.host.drain_in(1)

    def tearDown(self):
        self.assertFalse(self.emu.faults)
        self.assertFalse(self.emu.guest_traps)
        self.assertFalse(self.emu.flash.writes)

    def request(self, name):
        vector = self.by_name[name]
        self.assertEqual(self.host.bulk_request(bytes.fromhex(vector['request'])).hex(), vector['response'], name)

    def get(self, entity, selector, length, request=CUR, channel=0):
        setup = struct.pack('<BBHHH', 0xA1, request, selector << 8 | channel, entity << 8 | CONTROL, length)
        return self.host.control(setup)

    def set(self, entity, selector, payload, channel=0):
        setup = struct.pack('<BBHHH', 0x21, CUR, selector << 8 | channel, entity << 8 | CONTROL, len(payload))
        self.host.control(setup, payload)

    def select(self, alternate):
        self.host.control(struct.pack('<BBHHH', 0x01, 11, alternate, STREAMING, 0))
        self.host.pump(2)

    def feedback(self):
        self.host.pump()
        return self.host.iso_in(FEEDBACK)

    def test_the_audio_function_follows_the_keyboard_and_vendor_interfaces_with_one_packet_per_millisecond(self):
        parts = descriptors(self.config)
        self.assertEqual(self.config[4], 4)
        self.assertIn(bytes([8, 0x0B, CONTROL, 2, 1, 0, 0x20, 0]), parts)
        interfaces = [tuple(p[2:7]) for p in parts if p[1] == 4]
        self.assertEqual(interfaces[0][0], 0)
        self.assertEqual(interfaces[1][:2], (1, 0))
        self.assertEqual(
            interfaces[2:],
            [(2, 0, 0, 1, 1), (3, 0, 0, 1, 2), (3, 1, 2, 1, 2), (3, 2, 2, 1, 2), (3, 3, 2, 1, 2), (3, 4, 2, 1, 2)],
        )
        self.assertIn(bytes([8, 0x24, 0x0A, CLOCK, 0x03, 0x07, 0, 0]), parts)
        endpoints = [p for p in parts if p[1] == 5 and p[2] in (OUT, 0x80 | FEEDBACK)]
        expected = []
        for size in (388, 582, 776, 776):
            expected += [bytes([7, 5, OUT, 0x05]) + struct.pack('<H', size) + b'\x02', bytes([7, 5, 0x84, 0x11, 4, 0, 4])]
        self.assertEqual(endpoints, expected)
        self.assertEqual(self.host.max_packet(OUT, receive=True), 776)
        self.assertEqual(self.host.max_packet(FEEDBACK, receive=False), 4)
        self.assertEqual(self.emu.usb.endpoints[OUT].rx_fifo_size, musb.FIFO_DOUBLE | 7)
        self.assertEqual((self.host.max_packet(3, receive=True), self.host.max_packet(2, receive=False)), (512, 512))

    def test_clock_and_feature_unit_requests_reach_the_uac2_control(self):
        self.assertEqual(self.get(CLOCK, FREQUENCY, 4), struct.pack('<I', 48_000))
        rates = self.get(CLOCK, FREQUENCY, 255, request=RANGE)
        self.assertEqual(struct.unpack_from('<H', rates)[0], 8)
        self.assertEqual([struct.unpack_from('<I', rates, 2 + 12 * i)[0] for i in range(8)],
                         [44_100, 48_000, 88_200, 96_000, 176_400, 192_000, 352_800, 384_000])
        self.assertEqual(self.get(CLOCK, VALID, 1), b'\x01')
        self.set(CLOCK, FREQUENCY, struct.pack('<I', 44_100))
        self.assertEqual(self.get(CLOCK, FREQUENCY, 4), struct.pack('<I', 44_100))
        self.set(CLOCK, FREQUENCY, struct.pack('<I', 384_000))
        self.assertEqual(self.get(CLOCK, FREQUENCY, 4), struct.pack('<I', 384_000))
        with self.assertRaises(UsbStall):
            self.set(CLOCK, FREQUENCY, struct.pack('<I', 768_000))
        self.assertEqual(self.get(FEATURE, VOLUME, 8, request=RANGE, channel=1), bytes([1, 0, 0, 0xC4, 0, 0, 0x80, 0]))
        self.set(FEATURE, VOLUME, struct.pack('<h', -10 * 256), channel=2)
        self.assertEqual(self.get(FEATURE, VOLUME, 2, channel=2), struct.pack('<h', -10 * 256))
        self.set(FEATURE, MUTE, b'\x01')
        self.assertEqual(self.get(FEATURE, MUTE, 1), b'\x01')
        with self.assertRaises(UsbStall):
            self.get(FEATURE, 3, 1)
        self.request('device_info')

    def test_a_stream_without_a_dac_sends_nominal_feedback_drains_packets_and_keeps_hid_and_rpc_working(self):
        self.set(CLOCK, FREQUENCY, struct.pack('<I', 44_100))
        self.select(1)
        self.assertTrue(self.emu.usb.endpoints[OUT].rx_csr & musb.RX_ISO)
        self.assertTrue(self.emu.usb.endpoints[FEEDBACK].tx_csr & musb.TX_ISO)
        nominal = struct.pack('<I', (44_100 << 16) // 8_000)
        self.assertEqual(self.feedback(), nominal)
        for _ in range(8):
            self.host.iso_out(OUT, bytes(4 * 45))
            self.host.iso_out(OUT, bytes(4 * 44))
            self.host.pump()
        self.assertEqual(self.emu.usb.iso_dropped, 0)
        self.assertFalse(self.emu.usb.endpoints[OUT].rx_csr & (musb.RX_PKTRDY | musb.RX_OVERRUN))
        self.assertEqual(self.feedback(), nominal)
        self.assertEqual(self.get(CLOCK, VALID, 1), b'\x00')
        self.request('device_info')
        self.host.uart(keys(4) + keys())
        self.assertIn(bytes.fromhex('010000040000000000'), self.host.drain_in(1))
        self.select(0)
        self.assertEqual(self.feedback(), b'')
        self.select(3)
        self.assertEqual(self.feedback(), nominal)
        self.set(CLOCK, FREQUENCY, struct.pack('<I', 176_400))
        self.select(4)
        self.assertEqual(self.feedback(), struct.pack('<I', (176_400 << 16) // 8_000))
        self.host.enumerate()
        self.assertEqual(self.get(CLOCK, VALID, 1), b'\x01')
        self.assertEqual(self.feedback(), b'')
        self.request('device_info')


if __name__ == '__main__':
    unittest.main()

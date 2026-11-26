# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import io
import json
import os
from pathlib import Path
import struct
import unittest
from unittest.mock import patch
import zlib

from brom_emu import BROM_SIZE, FLASH_SIZE, Emulator, ModelError, elf_symbols, firmware_flash, service_boot
import emulator_server
import musb
from ram_host import RamHost, RuntimeStopped, UsbStall, boot_runtime
import spv5048
from testenv import read, requires

RUNTIME = ('KUMI_RUNTIME_ELF', 'KUMI_RPC_VECTORS')
DUMPS = ('KUMI_BROM_DUMP', 'KUMI_STOCK_DUMP')
NATIVE = DUMPS + ('KUMI_RUNTIME_ELF', 'KUMI_STAGE0_BIN', 'KUMI_RUNTIME_IMAGE', 'KUMI_NATIVE_RPC_VECTORS')


def rom_abi_fixture():
    rom = bytearray(BROM_SIZE)
    struct.pack_into('<I', rom, 0x2356, 0x00008067)
    for address in (0x272e, 0x27ec):
        struct.pack_into('<2I', rom, address, 0x00000513, 0x00008067)
    struct.pack_into('<3I', rom, 0x20C6, 0x00C86537, 0x51650513, 0x00008067)
    struct.pack_into('<10I', rom, 0x225E, 0x480002B7, 0x00550533, 0x00060E63, 0x00054283, 0x00558023,
                     0x00150513, 0x00158593, 0xFFF60613, 0xFE0616E3, 0x00008067)
    return rom


def uart_frame(body, command=0x64, group=1):
    return bytes([0x4B, group, command, len(body) + 2]) + body + sum(body).to_bytes(2, 'little')


def keys(*usages, modifiers=0):
    return uart_frame(bytes([4, modifiers, 0]) + bytes(usages).ljust(6, b'\0'))


def vectors(name):
    items = json.loads(Path(os.environ[name]).read_text())['vectors']
    return items, {v['name']: v for v in items}


class RuntimeScenarios:
    @classmethod
    def setUpClass(cls):
        cls.elf = read('KUMI_RUNTIME_ELF')
        cls.vectors, cls.by_name = vectors('KUMI_RPC_VECTORS')

    def tearDown(self):
        self.assertFalse(self.emu.faults)
        self.assertFalse(self.emu.guest_traps)
        self.assertFalse(self.emu.flash.writes)

    def request(self, name):
        vector = self.by_name[name]
        actual = self.host.bulk_request(bytes.fromhex(vector['request']))
        self.assertEqual(actual.hex(), vector['response'], name)
        return actual

    def test_client_vectors_run_on_the_actual_rv32_binary(self):
        for vector in self.vectors:
            with self.subTest(vector=vector['name']):
                self.request(vector['name'])

    def test_the_first_keyboard_link_frames_and_replies_are_logged_for_dev_peek(self):
        from kbdlink_log import SYMBOL, decode
        self.host.uart(uart_frame(bytes(5), command=0x11, group=0x80) + keys(4))
        self.host.drain_in(1)
        address = elf_symbols(self.elf)[SYMBOL]
        records = decode(bytes(self.emu.uc.mem_read(address, 64)))
        self.assertEqual([(r['dir'], r['group'], r['command']) for r in records],
                         [('from_mcu', '0x80', '0x11'), ('to_mcu', '0x80', '0x11'), ('from_mcu', '0x01', '0x64')])
        self.assertEqual(bytes.fromhex(records[1]['body']), b'0.0.1')
        self.assertEqual(records[2]['body'], '040000040000000000')

    def test_uart_remap_press_and_release_in_one_poll_are_not_lost(self):
        self.request('remap_a_to_b')
        self.host.uart(keys(4) + keys())
        packets = self.host.drain_in(1)
        self.assertEqual([p for p in packets if p[0] == 1],
                         [bytes.fromhex('010000050000000000'), bytes.fromhex('010000000000000000')])

    def test_layer_key_and_regular_key_in_one_uart_report(self):
        self.request('layer_hold')
        self.request('layer_one_a_to_c')
        self.host.uart(keys(4, 57))
        self.assertIn(bytes.fromhex('010000060000000000'), self.host.drain_in(1))
        self.host.uart(keys(4))
        self.assertEqual(self.host.drain_in(1), [])
        self.host.uart(keys())
        self.assertIn(bytes.fromhex('010000000000000000'), self.host.drain_in(1))
        self.host.uart(keys(4))
        self.assertIn(bytes.fromhex('010000040000000000'), self.host.drain_in(1))

    def test_consumer_usages_and_modifiers(self):
        self.host.uart(uart_frame(bytes([3, 1])))
        self.assertIn(bytes.fromhex('02e900'), self.host.drain_in(1))
        self.host.uart(keys(4, modifiers=2))
        self.assertIn(bytes.fromhex('010200040000000000'), self.host.drain_in(1))

    def test_bad_uart_checksums_are_ignored_and_a_query_gets_a_reply(self):
        damaged = bytearray(keys(4))
        damaged[-1] ^= 1
        self.host.uart(damaged)
        self.assertEqual(self.host.drain_in(1), [])
        self.host.uart(uart_frame(bytes(5), command=0x11, group=0x80))
        self.host.pump(20)
        self.assertEqual(bytes(self.emu.uart.tx), uart_frame(b'0.0.1', command=0x11, group=0x80))

    def test_boot_reports_leds_and_reset_restore_report_protocol(self):
        self.host.control(bytes.fromhex('210b000000000000'))
        self.assertEqual(self.host.control(bytes.fromhex('a103000000000100')), b'\0')
        self.host.drain_in(1)
        self.host.uart(keys(4))
        self.assertIn(bytes.fromhex('0000040000000000'), self.host.drain_in(1))
        self.host.control(bytes.fromhex('2109000200000100'), b'\x02')
        self.assertEqual(self.host.control(bytes.fromhex('a101000200000100')), b'\x02')
        self.host.enumerate()
        self.assertEqual(self.host.control(bytes.fromhex('a103000000000100')), b'\x01')
        self.assertEqual(self.host.control(bytes.fromhex('a101010200000200')), b'\x01\x00')
        self.assertIn(bytes.fromhex('010000040000000000'), self.host.drain_in(1))

    def test_report_ids_control_reports_and_idle(self):
        self.host.uart(keys(4))
        self.assertEqual(self.host.control(bytes.fromhex('a101010100000900')), bytes.fromhex('010000040000000000'))
        self.host.control(bytes.fromhex('2109010200000200'), b'\x01\x03')
        self.assertEqual(self.host.control(bytes.fromhex('a101010200000200')), b'\x01\x03')
        self.host.drain_in(1)
        self.host.control(bytes.fromhex('210a010500000000'))
        self.assertEqual(self.host.control(bytes.fromhex('a102010000000100')), b'\x05')
        self.host.drain_in(1)
        self.assertEqual(self.host.drain_in(1, limit=4), [])
        self.host.idle(25)
        self.assertIn(bytes.fromhex('010000040000000000'), self.host.drain_in(1, limit=8))

    def test_protocol_change_flushes_an_unread_old_format_report(self):
        self.host.uart(keys(4))
        self.assertTrue(self.emu.usb.endpoints[1].tx_packets)
        self.host.control(bytes.fromhex('210b000000000000'))
        reports = self.host.drain_in(1)
        self.assertTrue(reports)
        self.assertTrue(all(len(report) == 8 for report in reports))
        self.assertIn(bytes.fromhex('0000040000000000'), reports)
        self.host.control(bytes.fromhex('210b010000000000'))
        reports = self.host.drain_in(1)
        self.assertIn(bytes.fromhex('010000040000000000'), reports)
        self.assertIn(bytes.fromhex('020000'), reports)

    def test_control_stall_and_abandoned_data_stage_recover(self):
        with self.assertRaises(UsbStall):
            self.host.control(bytes.fromhex('8006009900001200'))
        self.assertEqual(len(self.host.control(bytes.fromhex('8006000100001200'))), 18)
        self.host.inject_setup(bytes.fromhex('8006000200004900'))
        self.host.pump(2)
        self.assertTrue(self.emu.usb.endpoints[0].tx_csr & musb.CSR0_TXPKTRDY)
        self.assertEqual(len(self.host.control(bytes.fromhex('8006000100001200'))), 18)
        self.host.inject_setup(bytes.fromhex('8006000100001200'))
        self.host.pump(2)
        self.assertEqual(len(self.host.take_in(0)), 18)
        self.host.pump()
        self.assertEqual(self.host.control(bytes.fromhex('8008000000000100')), b'\x01')

    def test_set_address_waits_for_the_status_stage_and_can_be_cancelled(self):
        self.host.inject_setup(bytes.fromhex('0005070000000000'))
        self.host.pump(2)
        self.assertEqual(self.emu.usb.faddr, 1)
        self.assertTrue(self.emu.usb.host_status())
        self.host.pump(2)
        self.assertEqual(self.emu.usb.faddr, 7)
        self.host.address = 7
        self.host.inject_setup(bytes.fromhex('0005090000000000'))
        self.host.pump(2)
        self.host.control(bytes.fromhex('8006000100001200'))
        self.assertEqual(self.emu.usb.faddr, 7)

    def test_partial_bulk_frame_is_discarded_on_reset_and_deconfiguration(self):
        self.host.out_packet(3, bytes.fromhex(self.by_name['full_packet_request']['request']))
        self.host.enumerate()
        self.request('device_info')
        self.host.out_packet(3, bytes(self.host.max_packet(3, receive=True)))
        self.host.control(bytes.fromhex('0009000000000000'))
        self.host.control(bytes.fromhex('0009010000000000'))
        self.request('device_info')

    def test_oversized_and_malformed_bulk_frames_do_not_poison_the_next_request(self):
        maximum = self.host.max_packet(3, receive=True)
        for _ in range(768 // maximum + 2):
            self.host.out_packet(3, bytes(maximum))
        self.host.out_packet(3, b'')
        self.assertIsNone(self.host.read_in(2, polls=4))
        self.host.out_packet(3, b'\xff')
        self.assertIsNone(self.host.read_in(2, polls=4))
        self.request('device_info')

    def test_a_slow_hid_host_does_not_block_rpc_or_uart(self):
        for _ in range(40):
            self.host.uart(keys(4) + keys())
        self.request('device_info')
        reports = self.host.drain_in(1, limit=100)
        self.assertEqual([p for p in reports if p[0] == 1][-1], bytes.fromhex('010000000000000000'))

    def test_endpoint_halt_and_clear(self):
        self.host.control(bytes.fromhex('0203000082000000'))
        self.assertEqual(self.host.control(bytes.fromhex('8200000082000200')), b'\x01\x00')
        self.host.control(bytes.fromhex('0201000082000000'))
        self.assertEqual(self.host.control(bytes.fromhex('8200000082000200')), b'\x00\x00')
        self.request('device_info')

    def test_detached_device_cannot_enumerate_or_transfer(self):
        self.emu.usb.power &= ~musb.POWER_SOFTCONN
        with self.assertRaisesRegex(ModelError, 'disconnected'):
            self.host.enumerate()
        with self.assertRaisesRegex(ModelError, 'disconnected'):
            self.request('device_info')

    def test_halted_out_does_not_accept_or_dispatch_a_packet(self):
        self.host.control(bytes.fromhex('0203000003000000'))
        with self.assertRaises(UsbStall):
            self.request('device_info')
        self.assertFalse(self.emu.usb.endpoints[3].rx_data)
        self.host.control(bytes.fromhex('0201000003000000'))
        self.request('device_info')

    def test_missing_undersized_and_overlapping_endpoint_fifos_are_rejected(self):
        endpoint = self.emu.usb.endpoints[2]
        for name, value in (('tx_maxp', 0), ('tx_fifo_size', 0), ('tx_fifo_address', 0)):
            saved = getattr(endpoint, name)
            setattr(endpoint, name, value)
            with self.subTest(name=name), self.assertRaises(ModelError):
                self.request('device_info')
            setattr(endpoint, name, saved)
            self.host.drain_in(2)
        self.request('device_info')

    def test_host_rejects_an_unexpected_device_address(self):
        self.emu.usb.faddr ^= 1
        with self.assertRaisesRegex(ModelError, 'address'):
            self.request('device_info')

    def test_high_speed_descriptors_hid_and_winusb_discovery(self):
        self.assertEqual(self.emu.usb.power & (musb.POWER_HSENAB | musb.POWER_HSMODE), musb.POWER_HSENAB | musb.POWER_HSMODE)
        qualifier = self.host.control(struct.pack('<BBHHH', 0x80, 6, 6 << 8, 0, 10))
        self.assertEqual((len(qualifier), qualifier[1], qualifier[7], qualifier[8]), (10, 6, 64, 1))
        with self.assertRaises(UsbStall):
            self.host.control(struct.pack('<BBHHH', 0x80, 6, 7 << 8, 0, 64))
        head = self.host.control(bytes.fromhex('8006000200000900'))
        config = self.host.control(struct.pack('<BBHHH', 0x80, 6, 0x200, 0, int.from_bytes(head[2:4], 'little')))
        endpoints, at = {}, 0
        while at < len(config):
            if config[at + 1] == 5:
                endpoints[config[at + 2]] = int.from_bytes(config[at + 4:at + 6], 'little')
            at += config[at]
        self.assertEqual((endpoints[0x82], endpoints[0x03]), (512, 512))
        self.assertEqual((self.host.max_packet(3, receive=True), self.host.max_packet(2, receive=False)), (512, 512))
        report = self.host.control(bytes.fromhex('8106002200009100'))
        self.assertEqual(len(report), 91)
        self.assertIn(b'\x85\x01', report)
        self.assertIn(b'\x85\x02', report)
        self.assertNotIn(b'\x85\x03', report)
        bos = self.host.control(bytes.fromhex('8006000f00004000'))
        self.assertEqual(int.from_bytes(bos[2:4], 'little'), len(bos))
        self.assertIn(bytes.fromhex('38b60834a909a0478bfda0768815b665'), bos)
        msos = self.host.control(bytes.fromhex('c020000007000004'))
        self.assertIn(b'WINUSB\0\0', msos)
        self.assertEqual(int.from_bytes(msos[8:10], 'little'), len(msos))


@requires(*RUNTIME)
class FixtureRomRuntimeTests(RuntimeScenarios, unittest.TestCase):
    def setUp(self):
        self.emu = Emulator(rom_abi_fixture(), b'\xff' * FLASH_SIZE, boot_flag=spv5048.SERVICE_MODE_BIT)
        self.host = RamHost(self.emu, self.elf)
        self.descriptors = self.host.enumerate()
        self.host.drain_in(1)

    def test_clock_initialization_failure_records_a_panic_and_resets_before_usb_connect(self):
        self.emu.guest_traps.clear()
        rom = rom_abi_fixture()
        struct.pack_into('<I', rom, 0x27ec, 0xfff00513)
        emu = Emulator(rom, b'\xff' * FLASH_SIZE)
        with self.assertRaises(RuntimeStopped) as stopped:
            RamHost(emu, self.elf)
        self.assertEqual(stopped.exception.outcome['result'], 'reset_requested')
        self.assertFalse(emu.report()['usb_softconnect'])
        self.assertFalse(emu.boot_flag() & spv5048.SERVICE_MODE_BIT)
        record = bytes(emu.uc.mem_read(spv5048.DIAG_RECORD_ADDRESS, spv5048.DIAG_RECORD_SIZE))
        self.assertEqual(record[:4], b'KDIA')
        self.assertEqual(zlib.crc32(record[:252]), int.from_bytes(record[252:], 'little'))
        self.assertEqual(record[5], 1)
        self.assertEqual(record[40:40 + record[36]], b'USB clock initialization failed')


@requires(*RUNTIME, *DUMPS)
class RealRomRuntimeTests(RuntimeScenarios, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        super().setUpClass()
        cls.rom = read('KUMI_BROM_DUMP')
        cls.stock = read('KUMI_STOCK_DUMP')

    def setUp(self):
        self.emu = Emulator(self.rom, self.stock, boot_flag=spv5048.SERVICE_MODE_BIT)
        self.host = boot_runtime(self.emu, self.elf, loader='direct')
        self.descriptors = self.host.enumerate()
        self.host.drain_in(1)

    def test_runtime_loaded_and_started_through_the_boot_rom(self):
        emu = Emulator(self.rom, self.stock, boot_flag=spv5048.SERVICE_MODE_BIT)
        host = boot_runtime(emu, self.elf, loader='rom')
        self.assertEqual(host.loader, 'rom')
        host.enumerate()
        self.assertEqual(host.bulk_request(bytes.fromhex(self.by_name['device_info']['request'])).hex(),
                         self.by_name['device_info']['response'])
        self.assertFalse(emu.faults)


@requires(*NATIVE)
class FullBootTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rom = read('KUMI_BROM_DUMP')
        cls.stock = read('KUMI_STOCK_DUMP')
        cls.elf = read('KUMI_RUNTIME_ELF')
        cls.stage0 = read('KUMI_STAGE0_BIN')
        cls.image = read('KUMI_RUNTIME_IMAGE')
        cls.vectors, cls.by_name = vectors('KUMI_NATIVE_RPC_VECTORS')
        cls.symbols = elf_symbols(cls.elf)

    def boot(self, image):
        emu = Emulator(self.rom, firmware_flash(self.stock, self.stage0, image), writable=True, stop_at_image=False)
        result = emu.boot(100_000_000, stops={self.symbols['kumi_poll']: 'runtime_poll', self.symbols['kumi_fault']: 'runtime_fault'})
        self.assertFalse(result['faults'])
        self.assertFalse(result['guest_traps'])
        return emu, result

    def test_rom_stage0_signed_image_and_runtime_without_host_ram_loading(self):
        emu, result = self.boot(self.image)
        self.assertEqual(result['outcome']['result'], 'runtime_poll')
        mutated = bytearray(self.elf)
        table = struct.unpack_from('<I', mutated, 28)[0]
        offset = struct.unpack_from('<I', mutated, table + 4)[0]
        mutated[offset] ^= 2
        with self.assertRaisesRegex(ModelError, 'ELF bytes'):
            RamHost(emu, mutated, running=True)
        host = RamHost(emu, self.elf, running=True)
        self.assertEqual(host.identity['kind'], 'signed_slot')
        host.enumerate()
        before = len(emu.flash.writes)
        for vector in self.vectors:
            self.assertEqual(host.bulk_request(bytes.fromhex(vector['request'])).hex(), vector['response'], vector['name'])
        self.assertGreater(before, 0)
        self.assertGreater(len(emu.flash.writes), before)
        allowed = (spv5048.JOURNAL_REGION, spv5048.SETTINGS)
        for write in emu.flash.writes:
            address = int(write['address'], 16)
            self.assertTrue(any(start <= address < start + size for start, size in allowed), write)
        self.assertFalse(emu.flash.ignored)

    def test_damaged_signed_image_enters_the_rom_recovery_path(self):
        damaged = bytearray(self.image)
        damaged[600] ^= 1
        emu, result = self.boot(damaged)
        self.assertEqual(result['outcome'], {'result': 'reset_requested', 'boot_flag': '0x80000000'})
        reboot = Emulator(self.rom, emu.flash.image, boot_flag=emu.boot_flag())
        _, device, _ = service_boot(reboot)
        self.assertEqual(device[8:12], bytes.fromhex('66068808'))


@requires(*NATIVE)
class ServerTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = {'brom': os.environ['KUMI_BROM_DUMP'], 'flash': os.environ['KUMI_STOCK_DUMP'],
                      'elf': os.environ['KUMI_RUNTIME_ELF'], 'stage0': os.environ['KUMI_STAGE0_BIN'],
                      'image': os.environ['KUMI_RUNTIME_IMAGE']}
        cls.vectors, cls.by_name = vectors('KUMI_NATIVE_RPC_VECTORS')

    def faults(self, retention):
        session = emulator_server.Session({**self.config, 'retention': retention})
        session.send(bytes.fromhex(self.by_name['confirm_boot']['request']))
        self.assertEqual(session.receive().hex(), self.by_name['confirm_boot']['response'])
        modes, records = [], []
        for _ in range(5):
            if session.host is None:
                break
            session.command({'op': 'fault', 'kind': 'illegal'})
            modes.append(session.mode)
            records.append(bytes(session.emu.uc.mem_read(spv5048.LAST_CRASH_ADDRESS, 4)))
        report = session.report()
        self.assertEqual(report['retention'], retention)
        self.assertEqual(report['model']['reset_retention'], retention)
        self.assertEqual(set(report['inputs']), {'brom_sha256', 'flash_sha256', 'elf_sha256', 'stage0_sha256', 'image_sha256'})
        return modes, records, report

    def test_kept_ram_keeps_the_crash_records_and_the_fifth_unhealthy_start_enters_the_boot_rom(self):
        modes, records, report = self.faults('kept')
        self.assertEqual(modes, ['runtime'] * 4 + ['bootrom'])
        self.assertTrue(all(record == b'KDIA' for record in records))
        self.assertEqual(report['restarts'], 5)

    def test_lost_ram_loses_the_crash_records_but_the_journal_still_reaches_the_boot_rom(self):
        modes, records, report = self.faults('lost')
        self.assertEqual(modes, ['runtime'] * 4 + ['bootrom'])
        self.assertEqual(records, [bytes(4)] * 5)
        self.assertEqual(report['restarts'], 5)

    def test_requests_fail_cleanly_without_killing_the_server(self):
        session = emulator_server.Session({**self.config, 'retention': 'lost'})
        replies = exchange(session, [3, b'{"op":"uart"}'], [3, b'{"op":"nope"}'], [3, b'[1]'], [2, b''],
                           [3, b'{"op":"reset","kind":"power"}'], [3, b'{"op":"report"}'])
        self.assertEqual([r[0] for r in replies], [1, 1, 1, 1, 0, 0])
        self.assertIn(b'KeyError', replies[0])
        self.assertIn(b'IN endpoint 2 timed out', replies[3])
        report = json.loads(replies[5][1:])
        self.assertEqual(report['mode'], 'runtime')
        self.assertEqual(report['model']['reset_retention'], 'lost')
        text, offset = '', 0
        while True:
            reply = exchange(session, [3, json.dumps({'op': 'model', 'offset': offset}).encode()])[0]
            self.assertEqual(reply[0], 0)
            part = json.loads(reply[1:])
            text += part['text']
            offset += len(part['text'])
            if offset >= part['total']:
                break
        self.assertEqual(json.loads(text), session.report()['model'])
        self.assertEqual(report['model']['assumption_count'], len(json.loads(text)['assumptions']))


class ServeTests(unittest.TestCase):
    def test_a_session_that_cannot_start_answers_every_request_with_its_error(self):
        replies = exchange({'brom': '/nonexistent'}, [3, b'{"op":"report"}'], [1, b'frame'])
        self.assertEqual([r[0] for r in replies], [1, 1])
        self.assertIn(b"retention", replies[0])

    def test_resets_during_boot_are_followed_with_the_chosen_retention(self):
        class FakeFlash:
            image = bytes(FLASH_SIZE)

        class FakeEmulator:
            created = []

            def __init__(self, rom, flash, boot_flag=0, writable=False, stop_at_image=True, ram=None):
                self.ram = ram
                self.boot_flag_value = boot_flag
                self.flash = FakeFlash()
                FakeEmulator.created.append(self)

            def boot_flag(self):
                return 0x80000000

            def ram_image(self):
                return b'retained'

        outcomes = iter(['reset_requested', 'reset_requested', 'runtime_poll'])
        session = object.__new__(emulator_server.Session)
        session.native, session.retention, session.max_resets = True, 'kept', 8
        session.rom, session.resets, session.restarts, session.boots = b'', [], 0, []
        with patch.object(emulator_server, 'Emulator', FakeEmulator), \
                patch.object(emulator_server.Session, 'boot', lambda self: (next(outcomes), None)):
            session.start(bytes(FLASH_SIZE), 0, None, 'power_on')
        self.assertEqual([e.ram for e in FakeEmulator.created], [None, b'retained', b'retained'])
        self.assertEqual([e.boot_flag_value for e in FakeEmulator.created], [0, 0x80000000, 0x80000000])
        self.assertEqual([r['after'] for r in session.resets], ['power_on', 'software_reset'])
        session.retention = 'lost'
        outcomes = iter(['reset_requested'] * 20)
        FakeEmulator.created.clear()
        with patch.object(emulator_server, 'Emulator', FakeEmulator), \
                patch.object(emulator_server.Session, 'boot', lambda self: (next(outcomes), None)):
            with self.assertRaisesRegex(ModelError, 'consecutive resets'):
                session.start(bytes(FLASH_SIZE), 0, None, 'power_on')
        self.assertEqual({e.ram for e in FakeEmulator.created}, {None})


def exchange(session_or_config, *messages):
    source = io.BytesIO(b''.join(struct.pack('<I', len(bytes([op]) + data)) + bytes([op]) + data for op, data in messages))
    sink = io.BytesIO()
    if isinstance(session_or_config, dict):
        emulator_server.serve(session_or_config, source, sink)
    else:
        with patch.object(emulator_server, 'Session', lambda config: session_or_config):
            emulator_server.serve({}, source, sink)
    raw, replies = sink.getvalue(), []
    while raw:
        length = struct.unpack_from('<I', raw)[0]
        replies.append(raw[4:4 + length])
        raw = raw[4 + length:]
    return replies


if __name__ == '__main__':
    unittest.main()

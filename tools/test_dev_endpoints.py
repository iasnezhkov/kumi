# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import json
import os
from pathlib import Path
import struct
import unittest
import zlib

from brom_emu import FLASH_SIZE, Emulator, ModelError
import emulator_server
from ram_host import RamHost, RuntimeStopped, boot_runtime
import spv5048
from test_runtime import keys, rom_abi_fixture
from testenv import read, requires

RUNTIME = ('KUMI_RUNTIME_ELF', 'KUMI_RPC_VECTORS')
LAB = RUNTIME + ('KUMI_LAB_RUNTIME_ELF',)
DUMPS = ('KUMI_BROM_DUMP', 'KUMI_STOCK_DUMP')
NATIVE = DUMPS + ('KUMI_RUNTIME_ELF', 'KUMI_STAGE0_BIN', 'KUMI_RUNTIME_IMAGE', 'KUMI_NATIVE_RPC_VECTORS')
NATIVE_LAB = DUMPS + ('KUMI_LAB_RUNTIME_ELF', 'KUMI_STAGE0_BIN', 'KUMI_LAB_RUNTIME_IMAGE', 'KUMI_RPC_VECTORS')
WINDOW = (0x3F0000, 0x400000)
HEAD = 0x1000
MIRROR_QUEUE = 8
BYTE_BLOCK = 64
UNIMP = 0xC0001073
EBREAK = 0x00100073
FAULTS = {'illegal_instruction': (2, 'runtime fault'), 'breakpoint': (3, 'runtime fault'), 'panic': (0, 'developer fault')}
NOR_BUSY_CYCLES = {'page_program_cycles': 60_000, 'sector_erase_cycles': 1_920_000, 'block_erase_cycles': 9_480_000}
REPORT_FLAGS = ('window_was_erased', 'page_matches', 'unaligned_matches', 'sector_erased', 'block_erased')
REPORT_WORDS = ('page_program_cycles', 'sector_erase_cycles', 'block_erase_cycles', 'stock_head_crc_before',
                'stock_head_crc_after')
CASE_ERRORS = {3: 'NotUnlocked', 4: 'HardwareFault', 7: 'WrongState', 0: 'Unsupported'}
BENCH_STATES = ('idle', 'starting', 'running', 'failed')
AUDIO_REQUESTS = ('audio_status', 'audio_calibrate', 'audio_stop', 'audio_start_uncalibrated', 'audio_bad_rate')
AUDIO_REGISTERS = ((0x20002060, 0x20002068), (0x20002078, 0x2000207C), (0x200020F8, 0x200020FC), (0x20010000, 0x20011000),
                   (0x20020000, 0x20022000), (0x2000801C, 0x20008020), (0x20070000, 0x20071000), (0x20110000, 0x2011000C),
                   (0x20110014, 0x20110020), (0x20110088, 0x2011008C), (0x20110300, 0x20110314), (0x20140000, 0x20141000),
                   (0x21000010, 0x21000014), (0x21001010, 0x21001018))


def developer(name):
    return json.loads(Path(os.environ[name]).read_text())['developer']


def varint(data, at):
    value = shift = 0
    while True:
        byte = data[at]
        at += 1
        value |= (byte & 0x7F) << shift
        shift += 7
        if not byte & 0x80:
            return value, at


def result(body):
    tag, at = varint(body, 0)
    if tag:
        return CASE_ERRORS.get(varint(body, at)[0], 'other'), None
    return None, at


def flash_report(body):
    error, at = result(body)
    if error:
        return {'error': error}
    report = {}
    for name in REPORT_FLAGS:
        report[name] = body[at] == 1
        at += 1
    for name in REPORT_WORDS:
        report[name], at = varint(body, at)
    if at != len(body):
        raise ValueError('trailing bytes after the flash test report')
    return report


def diagnostics(body):
    error, at = result(body)
    if error:
        return {'error': error}
    out = {}
    out['boot_count'], at = varint(body, at)
    out['safe_mode'], at = body[at] == 1, at + 1
    out['stage'], at = varint(body, at)
    out['uptime_ms'], at = varint(body, at)
    out['dropped_events'], at = varint(body, at)
    out['watchdog_armed'], at = body[at] == 1, at + 1
    present, at = body[at], at + 1
    out['last_crash'] = None
    if present:
        crash = {}
        for name in ('boot_count', 'reason', 'stage', 'mcause', 'mepc', 'mtval', 'ra', 'sp', 'uptime_ms'):
            crash[name], at = varint(body, at)
        length, at = varint(body, at)
        crash['reason'] = ('panic', 'trap')[crash['reason']]
        crash['message'], at = body[at:at + length].decode(), at + length
        out['last_crash'] = crash
    return out


class Link:
    def __init__(self, send, receive, vectors):
        self.send = send
        self.receive = receive
        self.vectors = vectors
        topics = vectors['topics']
        self.topics = {bytes.fromhex(header): name for name, header in topics.items()}

    def call(self, entry):
        self.send(bytes.fromhex(entry['request']))
        header = bytes.fromhex(entry['response_header'])
        events = []
        for _ in range(64):
            frame = self.receive()
            if frame.startswith(header):
                return frame, frame[len(header):], events
            name = self.topics.get(frame[:13])
            if name is None:
                raise ModelError(f'unexpected frame {frame.hex()}')
            events.append((name, frame[13:]))
        raise ModelError('no reply after 64 frames')

    def expect(self, entry, response):
        frame, _, events = self.call(entry)
        if frame.hex() != entry[response]:
            raise AssertionError(f'expected {response}: {entry[response]}, got {frame.hex()}')
        return events


def bench(body):
    error, at = result(body)
    if error:
        return {'error': error}
    state, at = varint(body, at)
    present, at = body[at], at + 1
    cpu_hz = None
    if present:
        cpu_hz, at = varint(body, at)
    return {'state': BENCH_STATES[state], 'cpu_hz': cpu_hz, 'calibration_stream_failure_halt': body[at:].hex()}


def audio_registers(emu):
    words = emu.system.words
    return {address: value for address, value in words.items()
            if any(start <= address < end for start, end in AUDIO_REGISTERS)}


def mirrored(events):
    blocks = []
    for name, body in events:
        if name == 'uart_mirror':
            length, at = varint(body, 0)
            if at + length != len(body):
                raise AssertionError('malformed mirror block')
            blocks.append(body[at:])
    return blocks


def stage0_boot_info():
    raw = bytearray(spv5048.BOOT_INFO_SIZE)
    raw[:4] = b'KBI1'
    struct.pack_into('<II', raw, 8, 1, 1)
    raw[16:19] = bytes((0, 1, 0))
    struct.pack_into('<I', raw, 28, zlib.crc32(raw[:28]))
    return bytes(raw)


def record_at(emu, address):
    record = bytes(emu.uc.mem_read(address, spv5048.DIAG_RECORD_SIZE))
    if record[:4] != b'KDIA' or zlib.crc32(record[:252]) != int.from_bytes(record[252:], 'little'):
        return None
    words = [int.from_bytes(record[at:at + 4], 'little') for at in range(12, 36, 4)]
    return {'reason': record[5], 'mcause': words[0], 'mepc': words[1], 'mtval': words[2],
            'message': record[40:40 + record[36]].decode()}


class RamScenarios:
    def link(self):
        return Link(self.host.send_frame, self.host.receive_frame, self.dev)

    def unlock(self, link):
        link.expect(self.dev['unlock'], 'ok')

    def test_locked_developer_mode_refuses_faults_flash_test_and_the_uart_mirror(self):
        link = self.link()
        for name in FAULTS:
            link.expect(self.dev['fault'][name], 'not_unlocked')
        link.expect(self.dev['flash_test'], 'not_unlocked')
        link.expect(self.dev['subscribe_keys_uart'], 'locked')
        self.host.uart(keys(4) + keys())
        self.assertEqual(mirrored(link.expect(self.dev['ping'], 'ok')), [])
        self.assertFalse(self.emu.guest_traps)
        self.assertFalse(self.emu.flash.writes)

    def test_peek_and_poke_need_developer_mode_and_reach_the_registers(self):
        link = self.link()
        link.expect(self.dev['peek_chip_id'], 'not_unlocked')
        link.expect(self.dev['poke_gpio'], 'not_unlocked')
        self.unlock(link)
        link.expect(self.dev['peek_chip_id'], 'ok')
        link.expect(self.dev['poke_gpio'], 'ok')
        self.assertFalse(self.emu.guest_traps)
        self.assertFalse(self.emu.flash.writes)

    def test_the_audio_bench_refuses_locked_uncalibrated_and_invalid_requests_without_touching_audio_blocks(self):
        link = self.link()
        before = audio_registers(self.emu)
        for name in AUDIO_REQUESTS:
            link.expect(self.dev[name], 'not_unlocked')
        self.unlock(link)
        link.expect(self.dev['audio_bad_rate'], 'invalid_argument')
        link.expect(self.dev['audio_start_uncalibrated'], 'wrong_state')
        _, body, _ = link.call(self.dev['audio_status'])
        status = bench(body)
        self.assertEqual(status['state'], 'idle', status)
        self.assertEqual(status['calibration_stream_failure_halt'], '00000000')
        self.assertTrue(status['cpu_hz'] is None or status['cpu_hz'] > 0)
        self.assertEqual(audio_registers(self.emu), before)
        self.assertFalse(self.emu.guest_traps)
        self.assertFalse(self.emu.flash.writes)

    def test_each_fault_kind_replies_first_records_the_crash_and_resets_without_the_boot_rom(self):
        for name, (mcause, message) in FAULTS.items():
            with self.subTest(name):
                self.restart()
                link = self.link()
                self.unlock(link)
                link.expect(self.dev['fault'][name], 'ok')
                with self.assertRaises(RuntimeStopped) as stopped:
                    self.host.pump(4)
                self.assertEqual(stopped.exception.outcome['result'], 'reset_requested')
                self.assertFalse(self.emu.boot_flag() & spv5048.SERVICE_MODE_BIT)
                record = record_at(self.emu, spv5048.DIAG_RECORD_ADDRESS)
                self.assertEqual(record['reason'], 1 if name == 'panic' else 2)
                self.assertEqual(record['mcause'], mcause)
                self.assertEqual(record['message'], message)
                self.assertFalse(self.emu.flash.writes)
                if name == 'panic':
                    self.assertEqual(self.emu.guest_traps, [])
                    continue
                self.assertEqual([trap['cause'] for trap in self.emu.guest_traps], [mcause])
                word = int.from_bytes(self.emu.uc.mem_read(record['mepc'], 4), 'little')
                self.assertEqual(word, UNIMP if name == 'illegal_instruction' else EBREAK)
                self.assertEqual(record['mtval'], UNIMP if name == 'illegal_instruction' else 0)

    def test_uart_mirror_forwards_raw_bytes_and_keys_keep_working(self):
        link = self.link()
        self.unlock(link)
        link.expect(self.dev['subscribe_keys_uart'], 'unlocked')
        sent = keys(4) + bytes([0x00, 0x13, 0x37]) + keys()
        self.host.uart(sent)
        reports = [p for p in self.host.drain_in(1) if p[0] == 1]
        self.assertEqual(reports, [bytes.fromhex('010000040000000000'), bytes.fromhex('010000000000000000')])
        events = link.expect(self.dev['ping'], 'ok')
        blocks = mirrored(events)
        self.assertEqual(b''.join(blocks), sent)
        self.assertTrue(all(1 <= len(block) <= BYTE_BLOCK for block in blocks))
        self.assertEqual([name for name, _ in events].count('key_events'), 2)
        self.host.uart(keys(5))
        self.assertIn(bytes.fromhex('010000050000000000'), self.host.drain_in(1))
        self.assertEqual(b''.join(mirrored(link.expect(self.dev['ping'], 'ok'))), keys(5))
        self.assertFalse(self.emu.guest_traps)

    def test_a_flood_is_cut_into_64_byte_blocks_and_overflow_counts_as_dropped_events(self):
        link = self.link()
        self.unlock(link)
        link.expect(self.dev['subscribe_keys_uart'], 'unlocked')
        flood = bytes(value for value in range(256) if value != 0x4B) * 5
        self.host.uart(flood)
        self.host.uart(keys(6))
        self.assertIn(bytes.fromhex('010000060000000000'), self.host.drain_in(1))
        blocks = mirrored(link.expect(self.dev['ping'], 'ok'))
        self.assertTrue(all(1 <= len(block) <= BYTE_BLOCK for block in blocks))
        self.assertEqual(blocks[0], flood[:len(blocks[0])])
        self.assertLess(len(blocks), len(flood) // BYTE_BLOCK)
        self.assertLessEqual(len(blocks), MIRROR_QUEUE + 3)
        _, body, _ = link.call(self.dev['diagnostics'])
        self.assertGreaterEqual(diagnostics(body)['dropped_events'], len(flood) // BYTE_BLOCK - len(blocks))

    def test_the_mirror_stops_when_developer_mode_expires_and_a_bus_reset_locks_it(self):
        link = self.link()
        link.expect(self.dev['unlock_one_second'], 'ok')
        link.expect(self.dev['subscribe_keys_uart'], 'unlocked')
        self.host.uart(b'\x01\x02')
        self.assertEqual(mirrored(link.expect(self.dev['ping'], 'ok')), [b'\x01\x02'])
        self.host.idle(1100)
        self.host.uart(b'\x03\x04')
        self.assertEqual(mirrored(link.expect(self.dev['ping'], 'ok')), [])
        link.expect(self.dev['fault']['panic'], 'not_unlocked')
        self.unlock(link)
        self.host.enumerate()
        self.host.drain_in(1)
        link.expect(self.dev['fault']['breakpoint'], 'not_unlocked')
        link.expect(self.dev['subscribe_keys_uart'], 'locked')
        self.assertFalse(self.emu.guest_traps)


@requires(*RUNTIME)
class FixtureRomDeveloperTests(RamScenarios, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.elf = read('KUMI_RUNTIME_ELF')
        cls.dev = developer('KUMI_RPC_VECTORS')

    def setUp(self):
        self.restart()

    def restart(self):
        self.emu = Emulator(rom_abi_fixture(), b'\xff' * FLASH_SIZE, boot_flag=spv5048.SERVICE_MODE_BIT)
        self.host = RamHost(self.emu, self.elf)
        self.host.enumerate()
        self.host.drain_in(1)


@requires(*RUNTIME, *DUMPS)
class RealRomDeveloperTests(RamScenarios, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.elf = read('KUMI_RUNTIME_ELF')
        cls.dev = developer('KUMI_RPC_VECTORS')
        cls.rom = read('KUMI_BROM_DUMP')
        cls.stock = read('KUMI_STOCK_DUMP')

    def setUp(self):
        self.restart()

    def restart(self, flash=None):
        self.emu = Emulator(self.rom, flash or self.stock, boot_flag=spv5048.SERVICE_MODE_BIT, writable=True)
        self.host = boot_runtime(self.emu, self.elf, loader='direct')
        self.host.enumerate()
        self.host.drain_in(1)

    def flash_test(self):
        link = self.link()
        self.unlock(link)
        _, body, events = link.call(self.dev['flash_test'])
        self.assertEqual(events, [])
        return flash_report(body)

    def test_flash_test_from_ram_writes_only_the_window_reports_everything_and_leaves_it_erased(self):
        before = bytes(self.emu.flash.image)
        self.assertEqual(before[WINDOW[0]:WINDOW[1]], b'\xff' * (WINDOW[1] - WINDOW[0]))
        report = self.flash_test()
        self.assertTrue(all(report[name] for name in REPORT_FLAGS), report)
        for name, busy in NOR_BUSY_CYCLES.items():
            self.assertGreaterEqual(report[name], busy, name)
            self.assertLess(report[name], 2 * busy, name)
        self.assertEqual(report['stock_head_crc_before'], zlib.crc32(before[:HEAD]))
        self.assertEqual(report['stock_head_crc_after'], report['stock_head_crc_before'])
        self.assertEqual(bytes(self.emu.flash.image), before)
        writes = [(w['command'], int(w['address'], 16)) for w in self.emu.flash.writes]
        self.assertEqual(writes, [('0x02', 0x3F0000), ('0x02', 0x3F012C), ('0x20', 0x3F0000), ('0x02', 0x3FF000),
                                  ('0xd8', 0x3F0000)])
        self.assertFalse(self.emu.flash.ignored)
        self.assertFalse(self.emu.guest_traps)
        self.link().expect(self.dev['ping'], 'ok')

    def test_a_window_that_is_not_erased_is_reported_and_never_written(self):
        flash = bytearray(self.stock)
        flash[WINDOW[1] - 1] = 0x5A
        self.restart(bytes(flash))
        report = self.flash_test()
        self.assertEqual({name: report[name] for name in REPORT_FLAGS}, dict.fromkeys(REPORT_FLAGS, False))
        self.assertEqual(report['stock_head_crc_after'], zlib.crc32(self.stock[:HEAD]))
        self.assertEqual(self.emu.flash.writes, [])
        self.assertEqual(bytes(self.emu.flash.image), bytes(flash))


@requires(*NATIVE)
class SlotDeveloperTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = {'brom': os.environ['KUMI_BROM_DUMP'], 'flash': os.environ['KUMI_STOCK_DUMP'],
                      'elf': os.environ['KUMI_RUNTIME_ELF'], 'stage0': os.environ['KUMI_STAGE0_BIN'],
                      'image': os.environ['KUMI_RUNTIME_IMAGE'], 'retention': 'kept'}
        cls.dev = developer('KUMI_NATIVE_RPC_VECTORS')

    def setUp(self):
        self.session = emulator_server.Session(self.config)
        self.link = Link(self.session.send, self.session.receive, self.dev)

    def window_writes(self):
        return [w for w in self.session.emu.flash.writes if WINDOW[0] <= int(w['address'], 16) < WINDOW[1]]

    def test_flash_test_is_refused_from_a_slot_even_when_unlocked(self):
        self.link.expect(self.dev['flash_test'], 'wrong_state')
        self.link.expect(self.dev['unlock'], 'ok')
        self.link.expect(self.dev['flash_test'], 'wrong_state')
        self.assertEqual(self.window_writes(), [])
        self.assertFalse(self.session.emu.guest_traps)

    def test_the_audio_bench_is_refused_from_a_slot_even_when_unlocked(self):
        self.link.expect(self.dev['audio_status'], 'wrong_state')
        self.link.expect(self.dev['unlock'], 'ok')
        for name in AUDIO_REQUESTS:
            self.link.expect(self.dev[name], 'wrong_state')
        self.assertFalse(self.session.emu.guest_traps)

    def test_a_fault_in_a_slot_boot_survives_the_restart_as_the_last_crash(self):
        self.link.expect(self.dev['unlock'], 'ok')
        self.link.expect(self.dev['fault']['breakpoint'], 'ok')
        self.session.advance()
        self.assertEqual(self.session.restarts, 1)
        self.assertEqual(self.session.mode, 'runtime')
        self.assertEqual(self.session.resets[-1]['boot_flag'], '0x00000000')
        _, body, _ = self.link.call(self.dev['diagnostics'])
        crash = diagnostics(body)['last_crash']
        self.assertEqual((crash['reason'], crash['mcause'], crash['mtval']), ('trap', 3, 0))
        self.link.expect(self.dev['fault']['panic'], 'not_unlocked')
        self.assertEqual(self.window_writes(), [])


class SlotGates:
    def assert_lab_gates(self, link, slot):
        link.expect(slot['poke_gpio'], 'not_unlocked')
        link.expect(slot['permitted_i2c']['probe_bus1'], 'not_unlocked')
        link.expect(self.dev['unlock'], 'ok')
        before = dict(self.emu.system.words)
        for name, entry in slot['refused_pokes'].items():
            with self.subTest(name):
                link.expect(entry, 'invalid_argument')
        for name, entry in slot['refused_i2c'].items():
            with self.subTest(name):
                link.expect(entry, 'invalid_argument')
        self.assertEqual(dict(self.emu.system.words), before)
        link.expect(slot['poke_gpio'], 'ok')
        link.expect(slot['poke_i2c_select'], 'ok')
        _, body, _ = link.call(slot['poke_i2c_clock'])
        self.assertIsNone(result(body)[0])
        self.assertEqual(self.emu.system.words[0x20002000] & 0x1000, 0x1000)
        for name, entry in slot['permitted_i2c'].items():
            with self.subTest(name):
                link.expect(entry, 'hardware_fault')
        link.expect(self.dev['flash_test'], 'wrong_state')
        audio_before = audio_registers(self.emu)
        link.expect(self.dev['audio_bad_rate'], 'invalid_argument')
        link.expect(self.dev['audio_start_uncalibrated'], 'wrong_state')
        _, body, _ = link.call(self.dev['audio_status'])
        self.assertEqual(bench(body)['state'], 'idle')
        self.assertEqual(audio_registers(self.emu), audio_before)
        self.assertFalse(self.emu.guest_traps)


@requires(*LAB)
class FixtureSlotDeveloperTests(SlotGates, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.release = read('KUMI_RUNTIME_ELF')
        cls.lab = read('KUMI_LAB_RUNTIME_ELF')
        cls.dev = developer('KUMI_RPC_VECTORS')

    def boot(self, elf, from_slot=True):
        self.emu = Emulator(rom_abi_fixture(), b'\xff' * FLASH_SIZE, boot_flag=spv5048.SERVICE_MODE_BIT)
        if from_slot:
            self.emu.write_memory(spv5048.BOOT_INFO_ADDRESS, stage0_boot_info())
            self.emu.write_reg(10, spv5048.BOOT_INFO_ADDRESS)
        self.host = RamHost(self.emu, elf)
        self.host.enumerate()
        self.host.drain_in(1)
        return Link(self.host.send_frame, self.host.receive_frame, self.dev)

    def test_a_release_runtime_started_by_stage0_refuses_poke_and_i2c_even_when_unlocked(self):
        link = self.boot(self.release)
        self.assertEqual(self.emu.system.words.get(0x20110014), 0x2D0)
        slot = self.dev['slot']
        link.expect(slot['device_info'], 'release')
        link.expect(slot['poke_gpio'], 'wrong_state')
        link.expect(self.dev['unlock'], 'ok')
        link.expect(slot['poke_gpio'], 'wrong_state')
        link.expect(self.dev['uart_send'], 'unsupported')
        for table in ('permitted_i2c', 'refused_i2c'):
            for name, entry in slot[table].items():
                with self.subTest(name):
                    link.expect(entry, 'wrong_state')
        self.assertFalse(self.emu.guest_traps)
        self.assertFalse(self.emu.flash.writes)

    def test_a_lab_runtime_keeps_gpio5_high_and_sends_dev_uart_bytes(self):
        link = self.boot(self.lab)
        self.assertEqual(self.emu.system.words.get(0x20110014), 0x2D0)
        self.assertEqual(self.emu.system.words.get(0x20003020), 0x3FB)
        _, diag_body, _ = link.call(self.dev['diagnostics'])
        self.assertTrue(diagnostics(diag_body)['watchdog_armed'])
        self.host.suspend(True)
        self.host.pump(8)
        self.assertEqual(self.emu.system.words.get(0x20110014), 0xD0)
        self.host.suspend(False)
        self.host.pump(8)
        self.assertEqual(self.emu.system.words.get(0x20110014), 0x2D0)
        link.expect(self.dev['uart_send'], 'not_unlocked')
        link.expect(self.dev['peek_flash_head'], 'not_unlocked')
        link.expect(self.dev['unlock'], 'ok')
        link.expect(self.dev['peek_flash_head'], 'erased')
        link.expect(self.dev['uart_send_empty'], 'invalid_argument')
        link.expect(self.dev['uart_send'], 'ok')
        self.host.pump(8)
        self.assertEqual(bytes(self.emu.uart.tx), b'record:1\n')
        self.assertFalse(self.emu.guest_traps)
        self.assertFalse(self.emu.flash.writes)

    def test_a_lab_runtime_started_by_stage0_reports_itself_and_serves_poke_and_i2c_behind_the_denylist(self):
        link = self.boot(self.lab)
        slot = self.dev['slot']
        link.expect(slot['device_info'], 'lab')
        self.assert_lab_gates(link, slot)
        self.assertFalse(self.emu.flash.writes)

    def test_a_bus_reset_locks_the_lab_endpoints_again(self):
        link = self.boot(self.lab)
        slot = self.dev['slot']
        link.expect(self.dev['unlock'], 'ok')
        link.expect(slot['poke_gpio'], 'ok')
        self.host.enumerate()
        self.host.drain_in(1)
        link.expect(slot['poke_gpio'], 'not_unlocked')
        link.expect(slot['permitted_i2c']['pointer_read'], 'not_unlocked')

    def test_a_lab_runtime_loaded_into_ram_keeps_the_ram_rules(self):
        link = self.boot(self.lab, from_slot=False)
        slot = self.dev['slot']
        link.expect(slot['device_info'], 'ram_lab')
        link.expect(self.dev['unlock'], 'ok')
        link.expect(slot['refused_pokes']['dma_enable'], 'ok')
        link.expect(slot['refused_pokes']['serial_flash'], 'invalid_argument')
        link.expect(slot['refused_i2c']['write'], 'hardware_fault')
        self.assertFalse(self.emu.guest_traps)

    def test_a_lab_slot_boot_captures_the_startup_snapshot_and_logs_link_frames(self):
        from brom_emu import elf_symbols
        self.boot(self.lab)
        syms = elf_symbols(self.lab)

        def word(address):
            return int.from_bytes(self.emu.uc.mem_read(address, 4), 'little')

        self.assertEqual(word(syms['KUMI_LAB_BOOT']), int.from_bytes(b'KLBT', 'little'))
        self.assertEqual(word(syms['KUMI_LAB_JOURNAL']), int.from_bytes(b'KLJR', 'little'))
        self.assertEqual(word(syms['KUMI_LAB_CONTROL']), int.from_bytes(b'KLCT', 'little'))
        self.assertGreater(word(syms['KUMI_LAB_BOOT'] + 4 * 7), 0)
        before = word(syms['KUMI_LAB_JOURNAL'] + 4)
        self.host.uart(keys(4) + keys())
        self.host.drain_in(1)
        self.assertGreater(word(syms['KUMI_LAB_JOURNAL'] + 4), before)
        self.assertFalse(self.emu.guest_traps)

    def test_boot_enables_external_dcdc_90mhz_serial_suspend_30mhz_and_remote_wakeup(self):
        self.boot(self.lab)
        self.assertEqual(self.emu.system.peek(0x20002010), 0x24)
        self.assertEqual(self.emu.system.peek(0x21001000), 0x000003C3)
        pmu = self.emu.system.peek(0x21004000)
        self.assertTrue(pmu & (1 << 25))
        self.assertFalse(pmu & (1 << 10))
        device = self.host.control(bytes.fromhex('8006000100001200'))
        self.assertNotEqual(device[16], 0)
        serial_desc = self.host.control(struct.pack('<BBHHH', 0x80, 6, 0x0300 | device[16], 0x0409, 64))
        serial = serial_desc[2:].decode('utf-16le')
        self.assertEqual(len(serial), 16)
        self.assertTrue(all(c in '0123456789ABCDEF' for c in serial))
        head = self.host.control(bytes.fromhex('8006000200000900'))
        self.assertTrue(head[7] & 0x20)
        self.host.control(bytes.fromhex('0003010000000000'))
        self.host.suspend(True)
        self.host.pump(4)
        self.assertEqual(self.emu.system.peek(0x20002010), 0x44)
        self.assertEqual(self.emu.system.peek(0x21001000), 0x000003C3)
        self.assertEqual(self.emu.usb.remote_wakeups, 0)
        self.host.uart(keys(4))
        self.assertEqual(self.emu.system.peek(0x20002010), 0x24)
        self.assertEqual(self.emu.usb.remote_wakeups, 1)
        self.assertFalse(self.emu.guest_traps)

    def test_release_slot_auto_enables_uac2_only_when_headphones_are_plugged(self):
        self.boot(self.release)
        unplugged_cfg = self.host.control(bytes.fromhex('8006000200000900'))
        self.assertEqual(unplugged_cfg[4], 2)
        self.emu = Emulator(rom_abi_fixture(), b'\xff' * FLASH_SIZE, boot_flag=spv5048.SERVICE_MODE_BIT)
        self.emu.system.words[0x21010050] = 0
        self.emu.write_memory(spv5048.BOOT_INFO_ADDRESS, stage0_boot_info())
        self.emu.write_reg(10, spv5048.BOOT_INFO_ADDRESS)
        self.host = RamHost(self.emu, self.release)
        self.host.enumerate()
        self.host.drain_in(1)
        plugged_cfg = self.host.control(bytes.fromhex('8006000200000900'))
        self.assertEqual(plugged_cfg[4], 4)
        self.assertFalse(self.emu.guest_traps)

    def test_idle_enters_wfi_and_wakes_on_uart_usb_and_timer3(self):
        from brom_emu import elf_symbols
        link = self.boot(self.lab)
        self.assertEqual(self.emu.plic.peek(0x10002000), 0x08000800)
        self.assertEqual(self.emu.plic.peek(0x10002004), 0x00000004)
        self.assertTrue(self.emu.system.timer3_irq_enabled())
        self.host.pump(8)
        self.assertGreater(self.emu.wfi_halts, 0)
        res = self.emu.resume(50_000_000, self.host.stops)
        self.assertEqual(res['outcome']['result'], 'wfi')
        self.emu.uart.push_rx(keys(4))
        woke_uart = self.emu.resume(50_000_000, self.host.stops)
        self.assertEqual(woke_uart['outcome']['result'], 'runtime_poll')
        self.host.pump(4)
        reports = [p for p in self.host.drain_in(1) if p[:1] == b'\x01']
        self.assertIn(bytes.fromhex('010000040000000000'), reports)
        self.host.pump(8)
        res = self.emu.resume(50_000_000, self.host.stops)
        self.assertEqual(res['outcome']['result'], 'wfi')
        self.emu.advance_ms(20)
        woke_timer = self.emu.resume(50_000_000, self.host.stops)
        self.assertEqual(woke_timer['outcome']['result'], 'runtime_poll')
        link.expect(self.dev['ping'], 'ok')
        syms = elf_symbols(self.lab)
        stats = struct.unpack('<4I', bytes(self.emu.uc.mem_read(syms['KUMI_IDLE_STATS'], 16)))
        self.assertEqual(stats[0], int.from_bytes(b'KWFI', 'little'))
        self.assertGreater(stats[1], 0)
        self.assertGreater(stats[2], 0)
        self.assertFalse(self.emu.guest_traps)


@requires(*NATIVE_LAB)
class LabImageDeveloperTests(SlotGates, unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.config = {'brom': os.environ['KUMI_BROM_DUMP'], 'flash': os.environ['KUMI_STOCK_DUMP'],
                      'elf': os.environ['KUMI_LAB_RUNTIME_ELF'], 'stage0': os.environ['KUMI_STAGE0_BIN'],
                      'image': os.environ['KUMI_LAB_RUNTIME_IMAGE'], 'retention': 'kept'}
        cls.dev = developer('KUMI_RPC_VECTORS')

    def setUp(self):
        self.session = emulator_server.Session(self.config)
        self.emu = self.session.emu
        self.link = Link(self.session.send, self.session.receive, self.dev)

    def test_a_signed_lab_image_booted_by_stage0_serves_poke_and_i2c_behind_the_denylist(self):
        self.assert_lab_gates(self.link, self.dev['slot'])
        allowed = (spv5048.JOURNAL_REGION, spv5048.SETTINGS)
        for write in self.emu.flash.writes:
            address = int(write['address'], 16)
            self.assertTrue(any(start <= address < start + size for start, size in allowed), write)


if __name__ == '__main__':
    unittest.main()

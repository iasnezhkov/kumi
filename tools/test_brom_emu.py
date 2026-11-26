# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

from collections import deque
import contextlib
import io
import json
import os
from pathlib import Path
import re
import struct
import tempfile
import unittest
from unittest.mock import patch
import zlib

import andes
import brom_emu
from brom_emu import BROM_SIZE, FLASH_SIZE, Emulator, ModelError, scenarios, stock_smoke
import musb
from musb import UsbHost, UsbProtocolError
from nor import Gd25, SpiController
from peripherals import RegisterFile
import spv5048
from testenv import read, requires

USB = musb.BASE


class Clock:
    def __init__(self):
        self.now = 0

    def cycles(self):
        return self.now

    def cycles_floor(self):
        return self.now

    def cycles_per_ms(self):
        return 60_000

    def skip_to(self, target):
        self.now = max(self.now, target)


def machine():
    return Emulator(bytes(BROM_SIZE), b'\xff' * FLASH_SIZE)


def program(emu, address, *words):
    emu.write_memory(address, struct.pack(f'<{len(words)}I', *words))


class AndesTests(unittest.TestCase):
    def test_stock_gp_offset_and_load(self):
        emu = machine()
        emu.write_reg(3, 0x104000)
        emu.store(0x104000 - 0x658, 4, 0xFEDCBA98)
        name, pc = andes.execute(emu, 0x2000, 0x9ABFA72B)
        self.assertEqual((name, pc, emu.read_reg(14)), ('lwgp', 0x2004, 0xFEDCBA98))

    def test_bitfield_extract_insert_and_sign(self):
        self.assertEqual(andes.bitfield(0x12345678, 15, 8, False), 0x56)
        self.assertEqual(andes.bitfield(3, 4, 5, False), 0x30)
        self.assertEqual(andes.bitfield(1, 0, 31, True), 0x80000000)
        self.assertEqual(andes.bitfield(0x80, 7, 0, True), 0xFFFFFF80)
        self.assertEqual(andes.bitfield(2, 4, 5, True), 0xFFFFFFE0)

    def test_branch_backwards(self):
        emu = machine()
        for value, next_pc in [(0, 0x80EE6), (32, 0x80EEC)]:
            emu.write_reg(15, value)
            self.assertEqual(andes.execute(emu, 0x80EE8, 0xBE57FF5B), ('bbc', next_pc))

    def test_unsigned_multiply_and_carry_across_register_pair(self):
        emu = machine()
        emu.write_reg(15, 0xFFFFFFFF)
        emu.write_reg(8, 2)
        andes.execute(emu, 0x2000, 0xF087947F)
        self.assertEqual((emu.read_reg(8), emu.read_reg(9)), (0xFFFFFFFE, 1))
        emu.write_reg(10, 3)
        emu.write_reg(11, 5)
        andes.execute(emu, 0x2004, 0xC0A4147F)
        self.assertEqual((emu.read_reg(8), emu.read_reg(9)), (1, 7))

    def test_pack_top_halves(self):
        emu = machine()
        emu.write_reg(10, 0x12345678)
        emu.write_reg(8, 0x9ABCDEF0)
        andes.execute(emu, 0x2000, 0x2E85157F)
        self.assertEqual(emu.read_reg(10), 0x12349ABC)

    def test_unknown_custom_instruction_is_not_accepted(self):
        self.assertIsNone(andes.decode(0xFE00007F))
        self.assertIsNone(andes.decode(0xBE57FFDB))


class NorTests(unittest.TestCase):
    def flash(self, image=None, writable=True):
        self.clock = Clock()
        return Gd25(image or b'\xff' * FLASH_SIZE, self.clock, writable)

    def transaction(self, flash, frame):
        flash.begin()
        values = [flash.exchange(b) for b in frame]
        flash.end()
        return values

    def test_read_and_measured_identity(self):
        image = bytearray(b'\xff' * FLASH_SIZE)
        image[0x123:0x127] = b'Kumi'
        flash = self.flash(image, writable=False)
        self.assertEqual(bytes(self.transaction(flash, b'\x03\x00\x01\x23\0\0\0\0')[4:]), b'Kumi')
        self.assertEqual(bytes(self.transaction(flash, b'\x9f\0\0\0')[1:]), b'\xc8\x65\x16')
        self.assertEqual(self.transaction(flash, b'\x35\0')[1], 2)

    def test_read_only_runs_refuse_and_writes_need_wren(self):
        read_only = self.flash(writable=False)
        self.transaction(read_only, b'\x06')
        with self.assertRaises(ModelError):
            self.transaction(read_only, b'\x02\0\0\0a')
        flash = self.flash()
        self.transaction(flash, b'\x02\0\x01\x23\x55')
        self.assertEqual(flash.image[0x123], 0xFF)
        self.assertEqual(flash.ignored[-1]['reason'], 'write without WREN')
        self.transaction(flash, b'\x06')
        self.transaction(flash, b'\x02\0\x01\x23\x55')
        self.assertEqual(flash.image[0x123], 0x55)

    def test_malformed_erase_frames_are_ignored_like_the_chip(self):
        flash = self.flash(bytes(FLASH_SIZE))
        for frame in (b'\x20\x00\x10', b'\x20\x00\x10\x00\x00', b'\xd8\x00'):
            self.transaction(flash, b'\x06')
            self.transaction(flash, frame)
        self.assertEqual(flash.image, bytes(FLASH_SIZE))
        self.assertEqual(len(flash.ignored), 3)
        self.transaction(flash, b'\x04')
        self.transaction(flash, b'\x06\x00')
        self.assertFalse(flash.wel)
        self.assertEqual(flash.ignored[-1]['reason'], 'single-byte command with extra bytes')
        self.transaction(flash, b'\x06')
        self.transaction(flash, b'\x02\x00\x00\x00')
        self.assertEqual(flash.ignored[-1]['reason'], 'page program without data')

    def test_page_program_wraps_inside_the_page_and_keeps_the_last_256_bytes(self):
        flash = self.flash()
        self.transaction(flash, b'\x06')
        self.transaction(flash, b'\x02\x00\x01\xfe' + b'\x00\x11\x22')
        self.assertEqual(flash.image[0x1FE:0x200], b'\x00\x11')
        self.assertEqual(flash.image[0x100], 0x22)
        self.assertEqual(flash.image[0x200], 0xFF)
        flash.busy_until = 0
        self.transaction(flash, b'\x06')
        self.transaction(flash, b'\x02\x00\x02\x00' + bytes(range(256)) + b'\x77')
        self.assertEqual(flash.image[0x200], 0x77)
        self.assertEqual(flash.image[0x201:0x300], bytes(range(1, 256)))

    def test_busy_times_are_the_measured_reply_times(self):
        flash = self.flash()
        for command, ms in ((b'\x20', 32), (b'\xd8', 158)):
            start = self.clock.now
            self.transaction(flash, b'\x06')
            self.transaction(flash, command + b'\x00\x00\x00')
            self.assertEqual(self.transaction(flash, b'\x05\0')[1] & 1, 1)
            self.assertEqual(self.clock.now - start, ms * 60_000)
            self.assertEqual(self.transaction(flash, b'\x05\0')[1] & 1, 0)

    def test_commands_during_busy_and_deep_power_down_are_ignored(self):
        flash = self.flash()
        self.transaction(flash, b'\x06')
        self.transaction(flash, b'\x20\x00\x00\x00')
        self.assertEqual(self.transaction(flash, b'\x9f\0\0\0')[1:], [0xFF] * 3)
        self.assertEqual(flash.ignored[-1]['reason'], 'write in progress')
        self.clock.now = flash.busy_until
        self.transaction(flash, b'\xb9')
        self.assertEqual(self.transaction(flash, b'\x03\0\0\0\0')[4], 0xFF)
        self.assertEqual(flash.ignored[-1]['reason'], 'deep power-down')
        self.assertEqual(self.transaction(flash, b'\xab\0\0\0\0')[4], 0x15)
        self.assertEqual(bytes(self.transaction(flash, b'\x9f\0\0\0')[1:]), b'\xc8\x65\x16')

    def test_unmodeled_commands_fail(self):
        for frame in (b'\x5a\0', b'\xc7', b'\x01\x00'):
            flash = self.flash()
            self.transaction(flash, b'\x06')
            with self.subTest(frame=frame.hex()), self.assertRaises(ModelError):
                self.transaction(flash, frame)

    def test_spi_status_is_the_measured_idle_value_with_rx_pending_in_bit_30(self):
        flash = self.flash(writable=False)
        spi = SpiController(flash, RegisterFile())
        self.assertEqual(spi.read(spi.STATUS, 4), 0x40500101)
        with self.assertRaises(ModelError):
            spi.write(spi.TX, 4, 0x9F)
        spi.write(spi.CONTROL, 4, 2)
        spi.write(spi.TX, 4, 0x9F)
        self.assertEqual(spi.read(spi.STATUS, 4), 0x00500101)
        spi.read(spi.RX, 4)
        self.assertEqual(spi.read(spi.STATUS, 4), 0x40500101)


class CpuTests(unittest.TestCase):
    def test_rv32_function_one_cycle_per_instruction_and_budget(self):
        emu = machine()
        program(emu, 0x2000, 0x02A00513, RET)
        self.assertEqual(emu.call(0x2000)['outcome']['a0'], 42)
        self.assertEqual(emu.steps, 2)
        program(emu, 0x3000, csrr(A0, 0xB00), csrr(A1, 0xB00), RET)
        emu.call(0x3000)
        self.assertEqual(emu.read_reg(A1) - emu.read_reg(A0), brom_emu.CYCLES_PER_INSTRUCTION)
        program(emu, 0x4000, 0x0000006F)
        self.assertEqual(emu.run(23, 0x4000)['outcome']['result'], 'budget_exhausted')

    def test_unmapped_access_fails_without_mapping_fake_ram(self):
        emu = machine()
        program(emu, 0x2000, 0x300005B7, 0x0005A503)
        result = emu.call(0x2000)
        self.assertTrue(result['faults'])
        self.assertEqual(result['unmapped_accesses'][0]['address'], '0x30000000')

    def test_compressed_return_at_the_end_of_ilm(self):
        emu = machine()
        emu.write_memory(0xfffe, b'\x82\x80')
        result = emu.call(0xfffe)
        self.assertEqual(result['outcome']['result'], 'returned')
        self.assertFalse(result['faults'])

    def test_measured_csrs_and_traps(self):
        emu = machine()
        for csr, value in ((0x301, 0x40901124), (0xF11, 0x31E), (0xF12, 0x80000025), (0xF13, 0x240),
                           (0xFC0, 0x592DA), (0xFC1, 0x592CA), (0xFC2, 0x2007F038), (0x7C0, 1), (0x7C1, 0x100001),
                           (0x7D0, 0x48), (0x7CA, 0)):
            program(emu, 0x2000, csrr(A0, csr), RET)
            with self.subTest(csr=hex(csr)):
                self.assertEqual(emu.call(0x2000)['outcome']['a0'], value)
        for csr in (0xFC3, 0xC01, 0xC81, 0x7FF):
            emu = machine()
            program(emu, 0x2000, csrr(A0, csr), RET)
            with self.subTest(csr=hex(csr)):
                result = emu.call(0x2000)
                self.assertEqual(result['guest_traps'][0]['cause'], 2)

    def test_atomic_and_double_precision_instructions_trap(self):
        for word in (0x00B6252F, 0x02000053):
            emu = machine()
            program(emu, 0x2000, word)
            with self.subTest(word=hex(word)):
                result = emu.call(0x2000)
                self.assertEqual(result['guest_traps'][0]['cause'], 2)
        self.assertFalse(Emulator.in_ram(0x10000, 4))
        self.assertFalse(Emulator.in_ram(0x140000, 4))

    def test_ecall_and_ebreak_trap_to_mtvec_with_their_causes(self):
        for code, cause in ((struct.pack('<I', 0x00000073), 11), (struct.pack('<I', 0x00100073), 3), (b'\x02\x90', 3)):
            emu = machine()
            program(emu, 0x2100, 0x34202573, 0x00008067)
            emu.uc.reg_write(emu.r.UC_RISCV_REG_MTVEC, 0x2100)
            emu.write_memory(0x2000, code)
            result = emu.call(0x2000)
            self.assertEqual(result['outcome']['a0'], cause)

    def test_shared_aliases_and_byte_accurate_register_writes(self):
        emu = machine()
        emu.store(0x60000000, 4, 0x12345678)
        self.assertEqual(emu.load(0x40000000, 4), 0x12345678)
        emu.store(0x20110000, 4, 0xAABBCCDD)
        emu.store(0x20110001, 1, 0x12)
        self.assertEqual(emu.load(0x20110000, 4), 0xAABB12DD)
        self.assertIn('0x20110000', emu.report()['unmodeled_register_writes'])

    def test_timer_window_reads_all_ones_and_refuses_writes(self):
        emu = machine()
        for address in (0x12000000, 0x12000004, 0x12000008, 0x1200000C):
            self.assertEqual(emu.load(address, 4), 0xFFFFFFFF)
        with self.assertRaises(ModelError):
            emu.store(0x12000008, 4, 0)


def csrr(rd, csr):
    return (csr << 20) | (2 << 12) | (rd << 7) | 0x73


def lui(rd, upper):
    return (upper << 12) | (rd << 7) | 0x37


def add(rd, rs1, rs2):
    return (rs2 << 20) | (rs1 << 15) | (rd << 7) | 0x33


def load(rd, rs1, offset, funct3=2):
    return (offset << 20) | (rs1 << 15) | (funct3 << 12) | (rd << 7) | 0x03


def store(rs2, rs1, offset):
    return ((offset >> 5) << 25) | (rs2 << 20) | (rs1 << 15) | (2 << 12) | ((offset & 31) << 7) | 0x23


def bltu(rs1, rs2, offset):
    offset &= 0x1FFF
    return (((offset >> 12) & 1) << 31) | (((offset >> 5) & 0x3F) << 25) | (rs2 << 20) | (rs1 << 15) | (6 << 12) \
        | (((offset >> 1) & 0xF) << 8) | (((offset >> 11) & 1) << 7) | 0x63


RET, MRET, NOP = 0x00008067, 0x30200073, 0x00000013
T0, T1, T2, A0, A1, A2, A3 = 5, 6, 7, 10, 11, 12, 13


class TimeTests(unittest.TestCase):
    def test_a_pure_mcycle_wait_is_fast_forwarded_to_its_deadline(self):
        emu = machine()
        program(emu, 0x2000, csrr(T0, 0xB00), lui(T1, 0x3C00), add(T0, T0, T1), csrr(T1, 0xB00), bltu(T1, T0, -4),
                csrr(A0, 0xB02), RET)
        result = emu.call(0x2000, budget=200_000)
        self.assertEqual(result['outcome']['result'], 'returned')
        self.assertGreaterEqual(emu.cycles(), 0x3C00000)
        self.assertLess(emu.cycles(), 0x3C00000 + brom_emu.WAIT_SKIP_CYCLES + 20)
        self.assertLess(emu.steps, 100_000)
        self.assertGreater(emu.read_reg(A0), 0x3C00000 // 2 - 1000)

    def test_a_loop_that_stores_is_executed_instruction_by_instruction(self):
        emu = machine()
        program(emu, 0x2000, csrr(T0, 0xB00), lui(T1, 0x10), add(T0, T0, T1), store(A0, 0, 0x100), csrr(T1, 0xB00),
                bltu(T1, T0, -8), RET)
        result = emu.call(0x2000, budget=200_000)
        self.assertEqual(result['outcome']['result'], 'returned')
        self.assertEqual(emu.waits_skipped, 0)
        self.assertGreater(emu.steps, 0x10000 // 4)

    def test_idle_time_advances_the_clock_only(self):
        emu = machine()
        emu.advance_ms(5)
        self.assertEqual(emu.cycles(), 5 * 60_000)
        self.assertEqual(emu.steps, 0)


def counting_program(emu):
    program(emu, 0x2000, lui(T0, 0x20040), lui(A1, 0x10), csrr(A2, 0xB00), 0x00160613, store(A2, T0, 0x20),
            0xFFF58593, store(A1, 0, 0x100), 0x00B03533, bltu(0, A1, -24), csrr(A3, 0xB02), RET)
    return emu


class BlockExecutionTests(unittest.TestCase):
    def test_hooked_instructions_keep_exact_counts_inside_blocks(self):
        emu = counting_program(machine())
        result = emu.call(0x2000, budget=1_000_000)
        self.assertEqual(result['outcome']['result'], 'returned')
        self.assertEqual(emu.steps, 2 + 7 * 0x10000 + 2)
        self.assertEqual(len(emu.uart.tx), 0x10000)
        self.assertEqual(emu.read_reg(A3), emu.steps - 1)

    def test_budget_stops_at_the_exact_instruction_inside_a_block(self):
        for budget in (1, 5, 6, 7, 1000, 1001):
            emu = counting_program(machine())
            emu.write_reg(1, brom_emu.RETURN_ADDRESS)
            result = emu.run(budget, 0x2000)
            with self.subTest(budget=budget):
                self.assertEqual(result['outcome']['result'], 'budget_exhausted')
                self.assertEqual(emu.steps, budget)
                self.assertEqual(int(result['outcome']['pc'], 16), emu.pc())
                resumed = emu.resume(1_000_000)
                self.assertEqual(resumed['outcome']['result'], 'returned')
                self.assertEqual(emu.steps, 2 + 7 * 0x10000 + 2)

    def test_rewritten_code_is_decoded_again(self):
        emu = machine()
        program(emu, 0x2000, 0x02A00513, RET)
        self.assertEqual(emu.call(0x2000)['outcome']['a0'], 42)
        program(emu, 0x2000, 0x02B00513, RET)
        self.assertEqual(emu.call(0x2000)['outcome']['a0'], 43)
        program(emu, 0x2000, 0xFFFFFFFF, RET)
        emu.uc.reg_write(emu.r.UC_RISCV_REG_MTVEC, 0x2100)
        program(emu, 0x2100, csrr(A0, 0x342), RET)
        self.assertEqual(emu.call(0x2000)['outcome']['a0'], 2)
        self.assertEqual(emu.guest_traps[0]['pc'], '0x00002000')


class SnapshotTests(unittest.TestCase):
    def setUp(self):
        brom_emu.SNAPSHOTS.memory.clear()

    def compare(self, first, second):
        self.assertEqual(first.steps, second.steps)
        self.assertEqual(first.cycles(), second.cycles())
        self.assertEqual([first.read_reg(i) for i in range(32)], [second.read_reg(i) for i in range(32)])
        self.assertEqual(first.pc(), second.pc())
        self.assertEqual(first.memory_state(), second.memory_state())
        self.assertEqual(bytes(first.uart.tx), bytes(second.uart.tx))
        first_report, second_report = first.report(), second.report()
        for report in (first_report, second_report):
            report.pop('elapsed_seconds')
            report.pop('last_run_seconds')
        self.assertEqual(first_report, second_report)

    def test_a_restored_boot_continues_exactly_like_the_original(self):
        original = counting_program(machine())
        original.write_reg(1, brom_emu.RETURN_ADDRESS)
        report = original.boot(70_003, start=0x2000)
        self.assertEqual(report['outcome']['result'], 'budget_exhausted')
        restored = counting_program(machine())
        restored.write_reg(1, brom_emu.RETURN_ADDRESS)
        with patch.object(Emulator, 'run', side_effect=AssertionError('the boot was executed again')):
            self.assertEqual(restored.boot(70_003, start=0x2000)['outcome'], report['outcome'])
        self.compare(original, restored)
        for emu in (original, restored):
            emu.resume(1_000_000)
        self.compare(original, restored)

    def test_snapshots_survive_the_disk_and_differ_by_input(self):
        with tempfile.TemporaryDirectory() as directory, patch.dict(os.environ, {brom_emu.SNAPSHOT_DIRECTORY: directory}):
            original = counting_program(machine())
            original.write_reg(1, brom_emu.RETURN_ADDRESS)
            original.boot(5_000, start=0x2000)
            self.assertEqual(len(list(Path(directory).glob('*.snapshot'))), 1)
            brom_emu.SNAPSHOTS.memory.clear()
            restored = counting_program(machine())
            restored.write_reg(1, brom_emu.RETURN_ADDRESS)
            with patch.object(Emulator, 'run', side_effect=AssertionError('the boot was executed again')):
                restored.boot(5_000, start=0x2000)
            self.compare(original, restored)
            different = counting_program(machine())
            different.write_reg(1, brom_emu.RETURN_ADDRESS)
            different.write_memory(0x3000, b'\x01')
            different.boot(5_000, start=0x2000)
            self.assertEqual(len(list(Path(directory).glob('*.snapshot'))), 2)
        with self.assertRaises(ValueError):
            restored.boot(5_000, start=0x2000)


class PlicTests(unittest.TestCase):
    HANDLER = (csrr(A0, 0x342), lui(A2, 0x200D0), load(A3, A2, 4, funct3=5), lui(T2, 0x10200), load(A1, T2, 4),
               store(A1, T2, 4), MRET)

    def setup(self, mie=0x800, status=0x8, enable=True, priority=None, threshold=0, vectored=False):
        emu = machine()
        program(emu, 0x3000, *self.HANDLER)
        program(emu, 0x2000, *([NOP] * 8), RET)
        emu.uc.reg_write(emu.r.UC_RISCV_REG_MTVEC, 0x3000)
        emu.uc.reg_write(emu.r.UC_RISCV_REG_MIE, mie)
        emu.uc.reg_write(emu.r.UC_RISCV_REG_MSTATUS, status)
        if enable:
            emu.store(0x10002004, 4, 1 << 2)
        if priority is not None:
            emu.store(0x10000004 + 4 * 33, 4, priority)
        emu.store(0x10200000, 4, threshold)
        if vectored:
            emu.store(0x10000000, 4, 2)
            emu.csrs[0x7D0] |= 2
            emu.uc.reg_write(emu.r.UC_RISCV_REG_MTVEC, 0x4000)
            emu.store(0x4000 + 4 * 34, 4, 0x3000)
        return emu

    def test_delivery_needs_mie_meie_enable_and_priority_above_threshold(self):
        cases = {"delivered": ({}, True), "mie clear": ({"status": 0}, False), "meie clear": ({"mie": 0}, False),
                 "disabled": ({"enable": False}, False), "priority zero": ({"priority": 0}, False),
                 "at threshold": ({"threshold": 1}, False)}
        for name, (options, delivered) in cases.items():
            emu = self.setup(**options)
            emu.usb.raise_rx(2)
            with self.subTest(name):
                result = emu.call(0x2000)
                self.assertEqual(result['outcome']['result'], 'returned')
                self.assertEqual(bool(emu.interrupts), delivered)
                if delivered:
                    self.assertEqual(emu.read_reg(10), 0x8000000B)
                    self.assertEqual(emu.read_reg(11), 34)

    def test_vectored_mode_reads_the_handler_relative_to_mtvec(self):
        emu = self.setup(vectored=True)
        emu.usb.raise_rx(2)
        emu.call(0x2000)
        self.assertEqual(emu.interrupts[34], 1)
        emu = self.setup()
        emu.store(0x10000000, 4, 2)
        emu.usb.raise_rx(2)
        self.assertTrue(emu.call(0x2000)['faults'])

    def test_level_triggered_source_is_pending_again_after_completion(self):
        emu = self.setup(status=0)
        emu.usb.raise_rx(2)
        self.assertEqual(emu.load(0x10200004, 4), 34)
        self.assertEqual(emu.load(0x10200004, 4), 0)
        emu.store(0x10200004, 4, 34)
        self.assertEqual(emu.load(0x10001004, 4), 1 << 2)
        emu.usb.read(USB + musb.INTRRX, 2)
        emu.store(0x10200004, 4, emu.load(0x10200004, 4))
        self.assertEqual(emu.load(0x10001004, 4), 0)


class MusbTests(unittest.TestCase):
    def device(self):
        return musb.Musb(Clock())

    def test_word_writes_merge_bytes_and_leave_read_only_flags(self):
        usb = self.device()
        usb.write(USB + musb.INTRTXE, 2, 0x000A)
        usb.write(USB + musb.INTRRXE, 2, 0x0024)
        usb.write(USB + musb.INTRUSBE, 1, 0x07)
        usb.host_attach()
        usb.power = musb.POWER_SOFTCONN
        usb.host_bus_reset()
        usb.write(USB + 0x08, 4, 0x07000024)
        self.assertEqual((usb.intr_tx_enable, usb.intr_rx_enable, usb.intr_usb_enable), (0x000A, 0x0024, 0x07))
        self.assertEqual(usb.read(USB + 0x08, 4) & 0x00FF0000, musb.USB_RESET << 16)
        self.assertEqual(usb.read(USB + 0x08, 4) & 0x00FF0000, 0)
        usb.write(USB, 4, 0x6005)
        usb.write(USB + 1, 1, 0x40)
        self.assertEqual((usb.faddr, usb.power), (5, 0x40))

    def test_frame_counter_runs_only_while_connected_and_not_suspended(self):
        usb = self.device()
        usb.clock.now = 10 * 60_000
        self.assertEqual(usb.frame(), 0)
        usb.host_attach()
        usb.write(USB + musb.POWER, 1, musb.POWER_SOFTCONN)
        usb.clock.now += 3 * 60_000
        self.assertEqual(usb.frame(), 3)
        usb.host_suspend(True)
        usb.clock.now += 5 * 60_000
        self.assertEqual(usb.frame(), 3)
        usb.host_suspend(False)
        usb.clock.now += 60_000
        self.assertEqual(usb.frame(), 4)

    def test_status_stage_completes_in_the_core_after_data_end(self):
        usb = self.device()
        usb.host_attach()
        usb.power = musb.POWER_SOFTCONN
        usb.host_setup(bytes.fromhex('8006000100001200'))
        self.assertEqual(usb.read(USB + musb.INTRTX, 2), 1)
        self.assertEqual(usb.read(USB + musb.COUNT, 2), 8)
        usb.read(USB + 0x20, 4)
        usb.read(USB + 0x20, 4)
        usb.write(USB + musb.CSR0, 2, musb.CSR0_SERVICEDRX)
        usb.write(USB + 0x20, 4, 0x01020304)
        usb.write(USB + musb.CSR0, 2, musb.CSR0_TXPKTRDY | musb.CSR0_DATAEND)
        self.assertEqual(usb.host_in(0), bytes.fromhex('04030201'))
        self.assertEqual(usb.read(USB + musb.INTRTX, 2), 1)
        self.assertTrue(usb.host_status())
        self.assertEqual(usb.read(USB + musb.INTRTX, 2), 1)
        self.assertEqual(usb.read(USB + musb.CSR0, 2) & musb.CSR0_RXPKTRDY, 0)
        self.assertEqual(usb.ep0_phase, 'idle')

    def test_no_data_request_waits_for_data_end_and_rejects_a_zero_length_in_packet(self):
        usb = self.device()
        usb.host_attach()
        usb.host_setup(bytes.fromhex('0009010000000000'))
        usb.write(USB + musb.CSR0, 2, musb.CSR0_SERVICEDRX)
        self.assertFalse(usb.host_status())
        usb.write(USB + musb.CSR0, 2, musb.CSR0_TXPKTRDY | musb.CSR0_DATAEND)
        with self.assertRaisesRegex(UsbProtocolError, 'status handshake itself'):
            usb.host_status()
        usb.host_setup(bytes.fromhex('0009010000000000'))
        self.assertTrue(usb.read(USB + musb.CSR0, 2) & musb.CSR0_SETUPEND)
        usb.write(USB + musb.CSR0, 2, musb.CSR0_SERVICEDRX | musb.CSR0_DATAEND | musb.CSR0_SERVICEDSETUPEND)
        self.assertTrue(usb.host_status())

    def test_host_checks_fifo_layout(self):
        usb = self.device()
        usb.host_attach()
        usb.power = musb.POWER_SOFTCONN
        host = UsbHost(usb, lambda: None)
        with self.assertRaisesRegex(UsbProtocolError, 'no MAXP'):
            host.max_packet(2, receive=False)
        for index in (2, 3):
            usb.index = index
            for offset, value in ((musb.TXMAXP, 64), (musb.TXFIFOSZ, 3), (musb.TXFIFOADD, 8)):
                usb.write_register(offset, value)
            if index == 2:
                self.assertEqual(host.max_packet(2, receive=False), 64)
        with self.assertRaisesRegex(UsbProtocolError, 'overlap'):
            host.max_packet(2, receive=False)


class ImageIdentityTests(unittest.TestCase):
    def images(self):
        elf = bytearray(0x104)
        elf[:6] = b'\x7fELF\x01\x01'
        struct.pack_into('<H', elf, 18, 243)
        struct.pack_into('<II', elf, 24, 0x2000, 52)
        struct.pack_into('<HH', elf, 42, 32, 1)
        struct.pack_into('<8I', elf, 52, 1, 0x100, 0x2000, 0x2000, 4, 8, 5, 4)
        elf[0x100:] = bytes.fromhex('67800000')
        image = bytearray(260)
        image[:6] = b'KUMI\x01\x00'
        image[11] = 2
        struct.pack_into('<II', image, 16, 4, 0x2000)
        struct.pack_into('<4I', image, 24, 1, 0x2000, 0, 4)
        struct.pack_into('<4I', image, 40, 2, 0x2004, 0, 4)
        image[256:] = elf[0x100:]
        struct.pack_into('<I', image, 252, zlib.crc32(image[:252]))
        return image, elf

    def test_image_matches_copy_zero_and_entry(self):
        image, elf = self.images()
        brom_emu.verify_image_elf(image, elf)

    def test_different_code_entry_or_zero_region_is_rejected(self):
        for offset in (0x100, 24, 52 + 20):
            image, elf = self.images()
            elf[offset] ^= 2
            with self.subTest(offset=offset), self.assertRaises((ModelError, ValueError)):
                brom_emu.verify_image_elf(image, elf)


class LayoutTests(unittest.TestCase):
    def test_python_addresses_match_kumi_layout(self):
        source = (Path(__file__).resolve().parents[1] / 'crates/kumi-layout/src/lib.rs').read_text()
        constants = {name: int(value.replace('_', ''), 16) for name, value in
                     re.findall(r'pub const (\w+): u32 = (0x[0-9A-Fa-f_]+);', source)}
        windows = {name: (int(a.replace('_', ''), 16), int(b.replace('_', ''), 16)) for name, a, b in
                   re.findall(r'pub const (\w+): RamWindow = RamWindow \{ name: "[^"]+", start: (0x[0-9A-Fa-f_]+), end: (0x[0-9A-Fa-f_]+) \}', source)}
        regions = {name: (int(a.replace('_', ''), 16), int(b.replace('_', ''), 16)) for name, a, b in
                   re.findall(r'pub const (\w+): Region = Region \{ name: "[^"]+", offset: (0x[0-9A-Fa-f_]+), size: (0x[0-9A-Fa-f_]+|2 \* SECTOR)', source.replace('2 * SECTOR', '0x2000'))}
        self.assertEqual(constants['LAST_CRASH_ADDRESS'], spv5048.LAST_CRASH_ADDRESS)
        self.assertEqual(constants['BOOT_INFO_ADDRESS'], spv5048.BOOT_INFO_ADDRESS)
        self.assertEqual(constants['DIAG_RECORD_ADDRESS'], spv5048.DIAG_RECORD_ADDRESS)
        self.assertEqual(windows['STAGE0_RAM'], spv5048.STAGE0_RAM)
        self.assertEqual(windows['BOOTROM_STACK'], spv5048.BOOTROM_STACK)
        self.assertEqual(windows['BOOTROM_DATA'], spv5048.BOOTROM_DATA)
        for name, value in (('STAGE0', spv5048.STAGE0_REGION), ('BOOT_JOURNAL', spv5048.JOURNAL_REGION),
                            ('SLOT_A', spv5048.SLOT_A), ('SLOT_B', spv5048.SLOT_B),
                            ('STOCK_COPY', spv5048.STOCK_COPY), ('SETTINGS', spv5048.SETTINGS)):
            self.assertEqual(regions[name], value, name)


@requires('KUMI_BROM_DUMP', 'KUMI_STOCK_DUMP')
class RealBinaryTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.brom = read('KUMI_BROM_DUMP')
        cls.flash = read('KUMI_STOCK_DUMP')

    def service(self):
        emu = Emulator(self.brom, self.flash, spv5048.SERVICE_MODE_BIT)
        host, device, config = brom_emu.service_boot(emu)
        return emu, host, device, config

    def test_recovery_matrix_runs_the_actual_rom_and_usb_handler(self):
        reports, _ = brom_emu.g3_reports(self.brom, self.flash, {}, [], 100_000_000)
        self.assertEqual(len(reports), 2 * len(scenarios(self.flash)[0]))
        for name, report in reports.items():
            with self.subTest(name):
                self.assertTrue(report['ok'], report['outcome'])
                self.assertFalse(report['flash_writes'])
                if report['expected'] == 'service_ready':
                    self.assertTrue(report['usb_device_descriptor'].startswith('1201000200000040'))

    def test_rom_and_stock_complete_the_status_stage_in_the_core(self):
        emu, host, _, _ = self.service()
        writes = []
        original = emu.usb.write_csr0

        def record(command):
            writes.append(command)
            original(command)

        emu.usb.write_csr0 = record
        self.assertEqual(len(host.control(bytes.fromhex('8006000100001200'))), 18)
        self.assertNotIn(musb.CSR0_SENDSTALL, [w & musb.CSR0_SENDSTALL for w in writes])
        self.assertEqual(writes, [musb.CSR0_SERVICEDRX, musb.CSR0_TXPKTRDY | musb.CSR0_DATAEND])
        emu.usb.host_setup(bytes.fromhex('8006000100001200'))
        host.retry(lambda: host.take_in(0), 'IN')
        for _ in range(3):
            host.pump()
        writes.clear()
        endpoint = emu.usb.endpoints[0]
        endpoint.rx_data, endpoint.rx_count = deque(), 0
        endpoint.tx_csr |= musb.CSR0_RXPKTRDY
        emu.usb.ep0_phase = 'idle'
        emu.usb.raise_tx(0)
        host.retry(lambda: writes or None, 'boot ROM reaction')
        self.assertEqual(writes, [musb.CSR0_SERVICEDRX | musb.CSR0_SENDSTALL])
        stock = Emulator(self.brom, self.flash, stop_at_image=False)
        self.assertEqual(stock.run(5_000_000, stops={0x480014A4: 'stock_main'})['outcome']['result'], 'stock_main')
        stock_host = brom_emu.usb_host(stock)
        stock_host.attach()
        stock_host.control(bytes.fromhex('8006000100001200'))
        addresses = []
        original_faddr = stock.usb.write_register

        def watch(offset, value):
            if offset == musb.FADDR:
                addresses.append((stock.usb.ep0_phase, value))
            original_faddr(offset, value)

        stock.usb.write_register = watch
        stock_host.control(bytes.fromhex('0005070000000000'))
        self.assertEqual(addresses, [('idle', 7)])

    def test_stock_runtime_enumerates_and_answers_version(self):
        result = stock_smoke(Emulator(self.brom, self.flash, stop_at_image=False), 5_000_000)
        self.assertEqual(result['outcome']['result'], 'stock_smoke_passed')
        self.assertEqual(result['stock_version'], 'V0.1')
        self.assertFalse(result['faults'])

    def test_boot_rom_buxx_service_matches_the_hardware_replies(self):
        import odt
        import ramload
        emu, host, device, config = self.service()
        self.assertEqual(device[8:12], bytes.fromhex('66068808'))
        channel = odt.Channel()
        channel.send = brom_emu.RomBuxx(emu, host).send
        self.assertEqual(odt.chip_id(channel.exchange(odt.HANDSHAKE, b'', 44)), spv5048.CHIP_ID)
        self.assertEqual(channel.exchange(odt.READ_JEDEC, b'', 4).hex(), '1665c800')
        self.assertEqual(channel.exchange(odt.READ_STATUS, b'', 4).hex(), '00020000')
        self.assertEqual(odt.read_flash(channel.exchange, odt.FLASH_BASE, 512), self.flash[:512])
        blob = bytes((7 * i) & 255 for i in range(600))
        brom_emu.load_through_rom(emu, host, 0x3000, blob)
        self.assertEqual(bytes(emu.uc.mem_read(0x3000, 600)), blob)
        exchange = brom_emu.rom_exchange(emu, host)
        with self.assertRaises(odt.ProtocolError):
            ramload.upload(exchange, 0x138000, b'\0')
        self.assertFalse(emu.faults)

    def test_g3_cli_reports_counters_model_and_assumptions(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
            code = brom_emu.main(['--brom', os.environ['KUMI_BROM_DUMP'], '--flash', os.environ['KUMI_STOCK_DUMP'],
                                  '--only', 'header_sector_erased'])
        self.assertEqual(code, 0)
        report = json.loads(output.getvalue())
        self.assertEqual(set(report['scenarios']), {'header_sector_erased', 'header_sector_erased+boot_flag'})
        self.assertIn('assumptions', report['model'])
        self.assertTrue(any('0x200020F4' in a for a in report['model']['assumptions']))
        for result in report['scenarios'].values():
            self.assertEqual(result['outcome']['result'], 'service_ready')
            self.assertIn('unmodeled_register_writes', result)

    @requires('KUMI_STAGE0_BIN', 'KUMI_G2_REPORT')
    def test_stage0_through_the_rom_matches_the_hardware_report_byte_for_byte(self):
        import stage0_dry
        emu, host, _, _ = self.service()
        ram = stage0_dry.ram_image(read('KUMI_STAGE0_BIN'))
        brom_emu.load_through_rom(emu, host, stage0_dry.LOAD_ADDRESS, ram, entry=stage0_dry.DRY_ENTRY)
        raw = ramload_readback(emu, host, stage0_dry.REPORT_ADDRESS, stage0_dry.REPORT_BYTES)
        self.assertEqual(raw, read('KUMI_G2_REPORT'))
        self.assertEqual(stage0_dry.decode(raw)['outcome'], 'no_bootable_slot')
        self.assertFalse(emu.faults)
        self.assertFalse(emu.flash.writes)

    @requires('KUMI_PROBE_ELF')
    def test_m1_probe_loaded_and_called_through_the_rom_reports_the_model(self):
        import probe
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
            code = brom_emu.main(['--mode', 'ram', '--brom', os.environ['KUMI_BROM_DUMP'], '--flash',
                                  os.environ['KUMI_STOCK_DUMP'], '--elf', os.environ['KUMI_PROBE_ELF'],
                                  '--read', f'0x{probe.REPORT_ADDRESS:x}:{probe.REPORT_BYTES}'])
        self.assertEqual(code, 0)
        scenario = json.loads(output.getvalue())['scenarios']['ram']
        self.assertEqual(scenario['loader'], 'rom')
        report = probe.decode(bytes.fromhex(scenario['memory'][f'0x{probe.REPORT_ADDRESS:x}:{probe.REPORT_BYTES}']))
        self.assertEqual(report['layout'], 3)
        self.assertEqual(report['isa_extensions'], 'CFIMUX')
        self.assertEqual(report['csr']['mmisc_ctl'], '0x00000048')
        self.assertIsNone(report['csr']['mmsc_cfg2'])
        self.assertIn('dlm_exec_ok', report['status'])
        self.assertIn('amo_trapped', report['status'])
        self.assertAlmostEqual(report['cpu_mhz'], 60.0, delta=0.01)
        self.assertIsNone(report['mtime_mhz'])
        self.assertEqual(report['cpi_loop']['cycles_per_instruction'], 1.0)
        self.assertEqual(report['registers']['0x12000008'], '0xffffffff')
        self.assertEqual(report['registers']['0x200a0004'], '0x40500101')


def ramload_readback(emu, host, address, length):
    import ramload
    return ramload.readback(brom_emu.rom_exchange(emu, host), address, length)


if __name__ == '__main__':
    unittest.main()

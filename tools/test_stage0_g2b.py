# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

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

import brom_emu
from brom_emu import Emulator
import odt
import ramload
import spv5048
import stage0_dry
from testenv import read, requires

PASSED_STEPS = list(stage0_dry.G2B_STEPS.values())
NORC_CONTROL, NORC_PAUSE = 0x20090000, 1 << 24
NOR_SECTOR_ERASE_CYCLES = 32 * 60_000


def seal(body):
    raw = bytearray(64)
    raw[:len(body)] = body
    struct.pack_into('<I', raw, 60, zlib.crc32(bytes(raw[:60])))
    return bytes(raw)


def g2b_report(outcome, steps, before=0x1234ABCD, after=0x1234ABCD, cycles=3_900_000):
    body = bytearray(b'KS0W' + bytes([outcome, steps]))
    body.extend(bytes(28 - len(body)))
    struct.pack_into('<III', body, 12, before, after, 0)
    struct.pack_into('<I', body, 24, cycles)
    return seal(body)


class G2bDecodeTests(unittest.TestCase):
    def test_a_passed_session_lists_every_step(self):
        result = stage0_dry.decode(g2b_report(0, stage0_dry.G2B_ALL_STEPS))
        self.assertEqual(result['record'], 'g2b')
        self.assertTrue(result['passed'])
        self.assertEqual(result['outcome'], 'passed')
        self.assertEqual(result['steps'], PASSED_STEPS)
        self.assertEqual(result['missing_steps'], [])
        self.assertEqual(result['scratch_sector'], '0x3f0000')
        self.assertEqual(result['head_crc_before'], '0x1234abcd')
        self.assertEqual(result['session_ms_at_60mhz'], 65.0)

    def test_failures_name_the_outcome_and_the_missing_steps(self):
        cases = {15: 0x40, 17: 0x41, 18: 0x43, 19: 0x7F & ~0x10, 20: 0x3F}
        names = {15: 'flash_unavailable', 17: 'flash_mismatch', 18: 'window_not_erased', 19: 'write_failed',
                 20: 'head_changed'}
        for outcome, steps in cases.items():
            with self.subTest(outcome):
                result = stage0_dry.decode(g2b_report(outcome, steps, after=0x1111 if outcome == 20 else 0x1234ABCD))
                self.assertFalse(result['passed'])
                self.assertEqual(result['outcome'], names[outcome])
        self.assertEqual(stage0_dry.decode(g2b_report(19, 0x6F))['missing_steps'], ['page_programmed'])
        self.assertFalse(stage0_dry.decode(g2b_report(0, stage0_dry.G2B_ALL_STEPS, after=1))['passed'])
        self.assertEqual(stage0_dry.decode(g2b_report(42, 0))['outcome'], 'unknown_42')

    def test_traps_panics_and_damage_use_the_dry_run_rules(self):
        trap = stage0_dry.decode(seal(b'KS0T' + struct.pack('<5I', 5, 0x82200, 0, 0x128400, 0x137800)))
        self.assertEqual((trap['record'], trap['mcause']), ('trap', '0x00000005'))
        damaged = bytearray(g2b_report(0, stage0_dry.G2B_ALL_STEPS))
        damaged[24] ^= 1
        with self.assertRaises(ValueError):
            stage0_dry.decode(bytes(damaged))

    def test_entry_and_scratch_window_match_kumi_layout(self):
        source = (Path(__file__).resolve().parents[1] / 'crates/kumi-layout/src/lib.rs').read_text()
        offset = re.search(r'pub const STAGE0_G2B_ENTRY: u32 = STAGE0_RAM\.start \+ (\d+);', source)
        self.assertEqual(stage0_dry.G2B_ENTRY, spv5048.STAGE0_RAM[0] + int(offset.group(1)))
        self.assertEqual(stage0_dry.G2B_ENTRY, 0x128010)
        scratch = re.search(r'pub const SCRATCH: Region = Region \{ name: "scratch", offset: (0x[0-9A-F_]+), '
                            r'size: (0x[0-9A-F_]+)', source)
        self.assertEqual(stage0_dry.SCRATCH, tuple(int(v.replace('_', ''), 16) for v in scratch.groups()))

    def test_the_g2b_command_calls_the_g2b_entry_over_the_dry_run_transport(self):
        image = bytearray(0x1000)
        struct.pack_into('<I', image, 4, stage0_dry.SPV_MAGIC)
        calls = []

        def device(args, address, data, entry, report_address, report_bytes):
            calls.append((address, len(data), entry, report_address, report_bytes, args.serial))
            return 'digest', g2b_report(0, stage0_dry.G2B_ALL_STEPS)

        with tempfile.TemporaryDirectory() as directory:
            stage0 = Path(directory) / 'stage0.bin'
            stage0.write_bytes(bytes(image))
            output = Path(directory) / 'g2b.bin'
            argv = ['g2b', '--image', str(stage0), '--serial', 'case', '--helper', '/data/local/tmp/spv40-usb',
                    '--usb-path', '/dev/bus/usb/001/002', '--output', str(output)]
            with patch.object(ramload, 'run_on_device', device), contextlib.redirect_stdout(io.StringIO()):
                self.assertEqual(stage0_dry.main(argv), 0)
            saved = json.loads(Path(str(output) + '.json').read_text())
            self.assertEqual(output.read_bytes(), g2b_report(0, stage0_dry.G2B_ALL_STEPS))
            with patch.object(ramload, 'run_on_device', device), self.assertRaises(SystemExit):
                stage0_dry.main(argv)
        self.assertEqual(calls, [(0x128000, 0xE00, 0x128010, 0x137E40, 64, 'case')])
        self.assertTrue(saved['passed'])
        self.assertEqual(saved['called_entry'], '0x00128010')
        self.assertEqual(saved['ram_image_sha256'], 'digest')


@requires('KUMI_BROM_DUMP', 'KUMI_STOCK_DUMP', 'KUMI_STAGE0_BIN')
class G2bEmulatorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.brom = read('KUMI_BROM_DUMP')
        cls.stock = read('KUMI_STOCK_DUMP')
        cls.ram = stage0_dry.ram_image(read('KUMI_STAGE0_BIN'))

    def session(self, flash):
        emu = Emulator(self.brom, flash, spv5048.SERVICE_MODE_BIT, writable=True)
        host, _, _ = brom_emu.service_boot(emu)
        areas = bytes(emu.uc.mem_read(spv5048.LAST_CRASH_ADDRESS, 0x300))
        brom_emu.load_through_rom(emu, host, stage0_dry.LOAD_ADDRESS, self.ram, entry=stage0_dry.G2B_ENTRY)
        exchange = brom_emu.rom_exchange(emu, host)
        raw = ramload.readback(exchange, stage0_dry.REPORT_ADDRESS, stage0_dry.REPORT_BYTES)
        after = bytearray(emu.uc.mem_read(spv5048.LAST_CRASH_ADDRESS, 0x300))
        report_at = stage0_dry.REPORT_ADDRESS - spv5048.LAST_CRASH_ADDRESS
        after[report_at:report_at + stage0_dry.REPORT_BYTES] = areas[report_at:report_at + stage0_dry.REPORT_BYTES]
        self.assertEqual(bytes(after), areas)
        self.assertFalse(emu.faults)
        self.assertFalse(emu.guest_traps)
        self.assertFalse(emu.system.peek(NORC_CONTROL) & NORC_PAUSE)
        return emu, host, exchange, stage0_dry.decode(raw)

    def test_g2b_runs_the_boot_flash_session_and_writes_only_the_scratch_sector(self):
        emu, host, exchange, report = self.session(self.stock)
        self.assertTrue(report['passed'], report)
        self.assertEqual(report['steps'], PASSED_STEPS)
        self.assertEqual(report['head_crc_before'], f'0x{zlib.crc32(self.stock[:0x1000]):08x}')
        self.assertGreater(report['session_cycles'], 2 * NOR_SECTOR_ERASE_CYCLES)
        writes = [(w['command'], int(w['address'], 16)) for w in emu.flash.writes]
        self.assertEqual(writes, [('0x20', 0x3F0000), ('0x02', 0x3F0000), ('0x20', 0x3F0000)])
        self.assertFalse(emu.flash.ignored)
        self.assertEqual(bytes(emu.flash.image), self.stock)
        channel = odt.Channel()
        channel.send = brom_emu.RomBuxx(emu, host).send
        self.assertEqual(odt.read_flash(channel.exchange, odt.FLASH_BASE + 0x3F0000, 512), b'\xff' * 512)
        self.assertEqual(odt.read_flash(channel.exchange, odt.FLASH_BASE, 512), self.stock[:512])
        ramload.run(exchange, stage0_dry.DRY_ENTRY)
        raw = ramload.readback(exchange, stage0_dry.REPORT_ADDRESS, stage0_dry.REPORT_BYTES)
        self.assertEqual(stage0_dry.decode(raw)['outcome'], 'no_bootable_slot')
        if os.environ.get('KUMI_G2_REPORT'):
            self.assertEqual(raw, read('KUMI_G2_REPORT'))
        self.assertEqual(len(emu.flash.writes), 3)

    def test_a_scratch_sector_that_is_not_erased_is_left_alone(self):
        flash = bytearray(self.stock)
        flash[0x3F0FFF] = 0x5A
        emu, _, _, report = self.session(bytes(flash))
        self.assertEqual(report['outcome'], 'window_not_erased')
        self.assertFalse(report['passed'])
        self.assertEqual(report['steps'], ['controller_paused', 'identity_matched', 'head_unchanged'])
        self.assertEqual(emu.flash.writes, [])
        self.assertEqual(bytes(emu.flash.image), bytes(flash))


if __name__ == '__main__':
    unittest.main()

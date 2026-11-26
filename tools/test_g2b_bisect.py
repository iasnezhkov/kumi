# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import unittest

import g2b_bisect
import probe
import ramload
from testenv import read, requires


class DecodeTests(unittest.TestCase):
    def test_a_record_lists_the_steps_done_and_the_timings(self):
        words = [g2b_bisect.RECORD_MAGIC, 4, 1, 4, 0, 0x00C86516, 0, 60, 600, 6_000, 60_000] + [0] * 5
        raw = b"".join(word.to_bytes(4, "little") for word in words)
        report = g2b_bisect.decode(raw)
        self.assertEqual(report["steps_done"], ["pause", "spi_enable", "init", "identity"])
        self.assertTrue(report["reached_limit"])
        self.assertTrue(report["rom_interrupts"])
        self.assertEqual(report["jedec"], "0x00c86516")
        self.assertEqual(report["ms_at_60mhz"], [0.001, 0.01, 0.1, 1.0])

    def test_a_missing_request_is_not_a_record(self):
        self.assertIsNone(g2b_bisect.decode(bytes(4 * g2b_bisect.WORDS))["record"])


@requires('KUMI_BROM_DUMP', 'KUMI_STOCK_DUMP', 'KUMI_PROBE_ELF')
class EmulatorTests(unittest.TestCase):
    def run_bisect(self, limit, flags):
        import brom_emu
        import spv5048
        emu = brom_emu.Emulator(read('KUMI_BROM_DUMP'), read('KUMI_STOCK_DUMP'), spv5048.SERVICE_MODE_BIT, writable=True)
        host, _, _ = brom_emu.service_boot(emu)
        image = ramload.ram_image(read('KUMI_PROBE_ELF'), probe.LOAD_ADDRESS)
        _, raw = g2b_bisect.exchange_bisect(brom_emu.rom_exchange(emu, host), image, limit, flags)
        self.assertFalse(emu.faults)
        return emu, g2b_bisect.decode(raw)

    def test_every_step_runs_through_the_boot_rom_and_writes_only_the_scratch_sector(self):
        emu, report = self.run_bisect(len(g2b_bisect.STEPS), 0)
        self.assertEqual(report["steps_done"], g2b_bisect.STEPS)
        self.assertEqual(report["jedec"], "0x00c86516")
        self.assertEqual([(w['command'], w['address']) for w in emu.flash.writes],
                         [('0x20', '0x3f0000'), ('0x02', '0x3f0000'), ('0x20', '0x3f0000')])

    def test_a_limit_stops_early_and_the_rom_context_variant_runs(self):
        emu, report = self.run_bisect(3, g2b_bisect.ROM_INTERRUPTS)
        self.assertEqual(report["steps_done"], ["pause", "spi_enable", "init"])
        self.assertTrue(report["rom_interrupts"])
        self.assertFalse(emu.flash.writes)


if __name__ == '__main__':
    unittest.main()

# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import contextlib
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest

import probe
from testenv import read, requires

MEASURED = bytes.fromhex(
    "4b554d310100000025000000c0cd1300962e080088600080b400080000080000241190401e0300002500008040020000"
    "00000000da920500ca92050038f007200000000001000000010010000000000048000000000000000000000000010000"
    "61ea00000000000020000000314c1d000000000000000000000000001e00000000000020804401491000002002000d40"
    "2400002000000000002000208f0010081020002004000000202000203000000028200020000000003020002000000000"
    "602000200000000064200020000000007820002000000000003000208f00100820300020000000002430002024730205"
    "0000092000f7000104000920000000000800092030000000300009200000000034000920000000003800092000000000"
    "3c0009200000000000000a200d00000004000a200101504000000d200470000060000d20990006001c80002100000080"
    "104000210f12020008000012ffffffff0c000012ffffffff00100220"
).ljust(512, b"\0")


def layout2(**overrides):
    words = list(struct.unpack("<128I", MEASURED))
    words[1] = 2
    words[probe.W_LOOP_CYCLES] = 8195
    words[probe.W_LOOP_INSTRET] = 8193
    words[probe.W_WAIT_INSTRET] = 1_000_000
    for index, value in overrides.items():
        words[int(index[1:])] = value
    return struct.pack("<128I", *words)


def layout3(**overrides):
    words = list(struct.unpack("<128I", layout2())) + [0] * 128
    words[1] = 3
    words[probe.W_V3_STATUS] = 1 | 2 | 4 | 8 | 16 | 32
    words[probe.W_TIMER_CYCLES] = 120_000
    for i in range(probe.TIMER_WORDS):
        words[probe.W_TIMER_A + i] = words[probe.W_TIMER_B + i] = 0x10000 + i
    words[probe.W_TIMER_B + 10] = words[probe.W_TIMER_A + 10] - 2000
    words[probe.W_TIMER_TRAPS] = 1 << 3
    words[probe.W_T0_SAVED:probe.W_T0_SAVED + 3] = [0, 0, 0]
    words[probe.W_T0_SAMPLES:probe.W_T0_SAMPLES + 6] = [1000, 120_000, 31_000, 90_000, 61_000, 60_000]
    words[probe.W_PLIC + 7] = 1 << 27
    words[probe.W_PLIC + 8] = 1 << 2
    for source in range(1, probe.PLIC_SOURCES + 1):
        words[probe.W_PLIC_PRIORITY + source - 1] = 1
    for index, value in overrides.items():
        words[int(index[1:])] = value
    return struct.pack("<256I", *words)


class DecodeTest(unittest.TestCase):
    def test_measured_report_from_the_case(self):
        result = probe.decode(MEASURED)
        self.assertEqual(result["layout"], 1)
        self.assertEqual(result["status"], ["done", "fpu_in_misa", "dlm_exec_ok"])
        self.assertEqual(result["isa_extensions"], "CFIMUX")
        self.assertEqual(result["csr"]["misa"], "0x40901124")
        self.assertEqual(result["csr"]["mmisc_ctl"], "0x00000048")
        self.assertIsNone(result["csr"]["mmsc_cfg2"])
        self.assertTrue(result["fpu_usable"])
        self.assertTrue(result["dlm_executable"])
        self.assertEqual(result["cpu_mhz"], 60.001)
        self.assertIsNone(result["mtime_mhz"])
        self.assertEqual(result["usb_frames_measured"], 32)
        self.assertEqual(result["bootrom_mstatus"], "0x80006088")
        self.assertEqual(result["bootrom_mie"], "0x00000800")
        self.assertEqual(result["dlm_base_per_andes_spec"], {"enabled": True, "base": "0x00100000"})
        self.assertEqual(result["registers"]["0x20000000"], "0x49014480")
        self.assertEqual(result["registers"]["0x200a0004"], "0x40500101")
        self.assertEqual(result["registers"]["0x12000008"], "0xffffffff")
        self.assertEqual(len(result["registers"]), 30)
        self.assertNotIn("cpi_loop", result)

    def test_layout_2_reports_cycles_per_instruction(self):
        result = probe.decode(layout2())
        self.assertEqual(result["cpi_loop"], {"cycles": 8195, "instructions": 8193, "cycles_per_instruction": 1.0})
        self.assertEqual(result["cpi_frame_wait"]["cycles_per_instruction"], 1.92)
        self.assertIsNone(result["minstret_trap_mcause"])
        trapped = probe.decode(layout2(w121=0, w122=0, w123=0x102))
        self.assertIsNone(trapped["cpi_loop"]["cycles_per_instruction"])
        self.assertEqual(trapped["minstret_trap_mcause"], 2)

    def test_layout_3_timer_block_timer0_and_plic(self):
        result = probe.decode(layout3())
        self.assertEqual(result["layout"], 3)
        self.assertEqual(result["cpi_frame_wait"]["cycles_per_instruction"], 1.92)
        block = result["timer_block"]
        self.assertEqual(block["ticking"], ["0x20010028"])
        self.assertEqual(block["words"]["0x20010028"]["delta"], -2000)
        self.assertEqual(block["words"]["0x20010028"]["per_mcycle"], round(-2000 / 120_000, 6))
        self.assertEqual(block["trapped"], ["0x2001000c"])
        self.assertNotIn("0x2001000c", block["words"])
        timer0 = result["timer0_experiment"]
        self.assertTrue(timer0["counted"] and timer0["restored_ok"])
        self.assertEqual(timer0["counts_per_mcycle"], -1.0)
        plic = result["plic"]
        self.assertEqual(plic["enabled_sources"], [27, 34])
        self.assertFalse(plic["claim_register_read"])
        self.assertEqual(plic["priorities"]["34"], 1)
        self.assertEqual(result["v3_status"][-1], "plic_read")

    def test_layout_3_without_the_timer_experiment_and_with_plic_traps(self):
        result = probe.decode(layout3(w128=1 | 32, w255=(1 << 7) | probe.PRIORITY_TRAPPED))
        self.assertEqual(result["timer0_experiment"], {"run": False})
        self.assertIsNone(result["plic"]["registers"]["enable_0"])
        self.assertEqual(result["plic"]["enabled_sources"], [34])
        self.assertIsNone(result["plic"]["priorities"])

    def test_layout_3_needs_the_whole_kilobyte(self):
        with self.assertRaises(ValueError):
            probe.decode(layout3()[:512])
        self.assertEqual(probe.decode(layout2() + bytes(512))["layout"], 2)

    def test_decode_cli_reads_a_saved_report(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory) / "report.bin"
            path.write_bytes(layout3())
            output = io.StringIO()
            with contextlib.redirect_stdout(output):
                probe.main(["decode", str(path)])
            self.assertEqual(json.loads(output.getvalue())["layout"], 3)


    def test_trapped_fpu_and_missing_frames(self):
        words = list(struct.unpack("<128I", MEASURED))
        words[2], words[24], words[25] = 1 | 2 | 4 | 8, 0, 0
        result = probe.decode(struct.pack("<128I", *words))
        self.assertFalse(result["fpu_usable"])
        self.assertIn("frame_timeout", result["status"])
        self.assertIsNone(result["cpu_mhz"])

    def test_rejects_foreign_memory(self):
        with self.assertRaises(ValueError):
            probe.decode(bytes(512))
        with self.assertRaises(ValueError):
            probe.decode(MEASURED[:100])
        with self.assertRaises(ValueError):
            probe.decode(layout2(w1=4))


@requires("KUMI_BROM_DUMP", "KUMI_STOCK_DUMP", "KUMI_PROBE_ELF")
class EmulatorTest(unittest.TestCase):
    def test_layout_3_runs_through_the_boot_rom_restores_and_returns(self):
        import brom_emu
        import odt
        import ramload
        import spv5048
        emu = brom_emu.Emulator(read("KUMI_BROM_DUMP"), read("KUMI_STOCK_DUMP"), spv5048.SERVICE_MODE_BIT)
        host, _, _ = brom_emu.service_boot(emu)
        exchange = brom_emu.rom_exchange(emu, host)
        image = ramload.ram_image(read("KUMI_PROBE_ELF"), probe.LOAD_ADDRESS)
        self.assertLessEqual(len(image), probe.IMAGE_LIMIT)
        ramload.load_and_run(exchange, probe.LOAD_ADDRESS, image, probe.LOAD_ADDRESS, expected_chip=spv5048.CHIP_ID)
        report = probe.decode(ramload.readback(exchange, probe.REPORT_ADDRESS, probe.REPORT_BYTES))
        self.assertEqual(report["layout"], 3)
        self.assertEqual(report["cpi_loop"]["cycles_per_instruction"], 1.0)
        self.assertEqual(report["cpi_frame_wait"]["cycles_per_instruction"], 1.0)
        self.assertEqual(report["v3_status"], ["timer_block_sampled", "timer0_experiment_run", "timer0_restored", "plic_read"])
        self.assertEqual(report["timer_block"]["ticking"], [])
        self.assertGreaterEqual(report["timer_block"]["window_cycles"], 120_000)
        self.assertIn(34, report["plic"]["enabled_sources"])
        self.assertEqual(report["timer0_experiment"]["restored"], report["timer0_experiment"]["saved"])
        self.assertEqual(odt.chip_id(exchange(odt.HANDSHAKE, b"", odt.HANDSHAKE_LENGTH)), spv5048.CHIP_ID)
        self.assertFalse(emu.faults)
        self.assertNotIn("0x10200004", emu.report()["registers_defaulted_to_zero"])


if __name__ == "__main__":
    unittest.main()

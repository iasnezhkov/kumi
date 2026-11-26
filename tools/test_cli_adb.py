# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest

from test_bridge import framed, messages
from testenv import requires

TOOLS = Path(__file__).resolve().parent
ROOT = TOOLS.parent


def ram_config(directory, elf_path):
    config = {"python": sys.executable, "server": str(TOOLS / "emulator_server.py"), "brom": os.environ["KUMI_BROM_DUMP"],
              "flash": os.environ["KUMI_STOCK_DUMP"], "elf": elf_path, "retention": "kept", "native": False}
    path = Path(directory) / "emulator.json"
    path.write_text(json.dumps(config))
    return path


def vectors():
    return {v["name"]: v for v in json.loads(Path(os.environ["KUMI_RPC_VECTORS"]).read_text())["vectors"]}


@requires("KUMI_BROM_DUMP", "KUMI_STOCK_DUMP", "KUMI_RUNTIME_ELF", "KUMI_RPC_VECTORS")
class LoopbackTests(unittest.TestCase):
    def test_the_emulated_bridge_speaks_the_helper_framing(self):
        with tempfile.TemporaryDirectory() as directory:
            config = ram_config(directory, os.environ["KUMI_RUNTIME_ELF"])
            by_name = vectors()
            names = ["device_info", "ping", "diagnostics"]
            names = [name for name in names if name in by_name] or list(by_name)[:3]
            data = framed([bytes.fromhex(by_name[name]["request"]) for name in names])
            result = subprocess.run([sys.executable, str(TOOLS / "kumi_bridge_emu.py"), "--config", str(config)],
                                    input=data, capture_output=True, timeout=300, cwd=TOOLS)
            self.assertEqual(result.returncode, 0, result.stderr)
            identity, *replies = messages(result.stdout)
            self.assertEqual(identity[8:12], bytes.fromhex("09120100"))
            self.assertEqual([reply.hex() for reply in replies], [by_name[name]["response"] for name in names])


def build_cli():
    if os.environ.get("KUMI_CLI"):
        return Path(os.environ["KUMI_CLI"])
    cargo = ["cargo"]
    env = dict(os.environ)
    if shutil.which("rustup"):
        which = subprocess.run(["rustup", "which", "cargo"], cwd=ROOT, capture_output=True, text=True)
        if which.returncode == 0 and which.stdout.strip():
            tc_bin = str(Path(which.stdout.strip()).parent)
            env["PATH"] = tc_bin + os.pathsep + env.get("PATH", "")
            cargo = [which.stdout.strip()]
        else:
            cargo = ["rustup", "run", "stable", "cargo"]
    subprocess.run(cargo + ["build", "-q", "-p", "kumi-cli", "--bin", "kumi"], cwd=ROOT, env=env, check=True, timeout=900)
    return ROOT / "target" / "debug" / "kumi"


@requires("KUMI_BROM_DUMP", "KUMI_STOCK_DUMP", "KUMI_RUNTIME_ELF", "KUMI_RPC_VECTORS")
class CliOverAdbTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.cli = build_cli()
        cls.directory = tempfile.TemporaryDirectory()
        cls.config = ram_config(cls.directory.name, os.environ["KUMI_RUNTIME_ELF"])
        cls.args = Path(cls.directory.name) / "adb-args.txt"
        cls.adb = Path(cls.directory.name) / "adb"
        cls.adb.write_text(f"#!/bin/sh\nprintf '%s\\n' \"$@\" > '{cls.args}'\n"
                           f"exec '{sys.executable}' '{TOOLS / 'kumi_bridge_emu.py'}' --config '{cls.config}'\n")
        cls.adb.chmod(0o755)

    @classmethod
    def tearDownClass(cls):
        cls.directory.cleanup()

    def kumi(self, *command, transport="adb"):
        if transport == "adb":
            prefix = ["--adb", "SERIAL1", "--helper", "/data/local/tmp/spv40-usb", "--usb-path", "/dev/bus/usb/001/012",
                      "--deadline", "120"]
        else:
            prefix = ["--emulator", str(self.config)]
        result = subprocess.run([str(self.cli), *prefix, *command], capture_output=True, text=True, timeout=600,
                                env={**os.environ, "KUMI_ADB": str(self.adb)}, cwd=TOOLS)
        return result

    def test_info_and_diagnostics_match_over_both_transports(self):
        adb = self.kumi("info")
        self.assertEqual(adb.returncode, 0, adb.stderr)
        self.assertEqual(self.args.read_text().split("\n")[:5],
                         ["-s", "SERIAL1", "shell", "-T",
                          "timeout -s KILL 120 /data/local/tmp/spv40-usb kumi-bridge /dev/bus/usb/001/012"])
        emulator = self.kumi("info", transport="emulator")
        self.assertEqual(emulator.returncode, 0, emulator.stderr)
        self.assertEqual(json.loads(adb.stdout), json.loads(emulator.stdout))
        diagnostics = self.kumi("diagnostics")
        self.assertEqual(diagnostics.returncode, 0, diagnostics.stderr)
        self.assertIn("boot_count", json.loads(diagnostics.stdout))
        keys = self.kumi("keys")
        self.assertEqual(keys.returncode, 0, keys.stderr)
        self.assertIsInstance(json.loads(keys.stdout), list)

    def test_events_stream_subscribes_and_ends_cleanly(self):
        result = self.kumi("events", "1")
        self.assertEqual(result.returncode, 0, result.stderr)
        summary = json.loads(result.stdout[result.stdout.index("{\n"):])
        self.assertTrue(summary["granted"]["keys"])
        self.assertEqual(summary["seconds"], 1)

    def test_developer_commands_answer_or_refuse_cleanly(self):
        for transport in ("adb", "emulator"):
            for command, path in ((("dev", "unlock", "30"), "dev/unlock"), (("dev", "flash-test"), "dev/flash/test"),
                                  (("dev", "fault", "panic"), "dev/fault"), (("uart-mirror", "1"), "dev/uart/mirror")):
                with self.subTest(transport=transport, command=command):
                    result = self.kumi(*command, transport=transport)
                    if result.returncode == 3:
                        self.assertIn(path, result.stderr)
                        self.assertTrue("Unsupported" in result.stderr or "dev unlock" in result.stderr
                                        or "does not serve" in result.stderr, result.stderr)
                    else:
                        self.assertEqual(result.returncode, 0, result.stderr)
        usage = self.kumi("dev", "fault", "explode")
        self.assertEqual(usage.returncode, 2)
        self.assertEqual(self.kumi("dev", "unlock", "601").returncode, 2)
        self.assertEqual(self.kumi("exercise-recovery").returncode, 1)


if __name__ == "__main__":
    unittest.main()

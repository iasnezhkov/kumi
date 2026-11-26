# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
from pathlib import Path
import struct

import ramload

LOAD_ADDRESS = 0x2000
REPORT_ADDRESS = 0x8000
REPORT_BYTES = 1024
LEGACY_REPORT_BYTES = 512
IMAGE_LIMIT = 0x2000
MAGIC = struct.unpack("<I", b"KUM1")[0]
LAYOUTS = (1, 2, 3)
CSR_NAMES = ["misa", "mvendorid", "marchid", "mimpid", "mhartid", "micm_cfg", "mdcm_cfg", "mmsc_cfg",
             "mmsc_cfg2", "milmb", "mdlmb", "mcache_ctl", "mmisc_ctl", "mxstatus", "mdcause"]
STATUS_BITS = {1: "done", 2: "frame_timeout", 4: "fpu_in_misa", 8: "fpu_instruction_trapped", 16: "mcycle_stuck",
               32: "dlm_exec_ok", 64: "dlm_fetch_trapped", 128: "a_in_misa", 256: "amo_trapped", 512: "amo_ok",
               1024: "lrsc_trapped", 2048: "lrsc_ok"}
REGISTER_LIMIT = {1: 47, 2: 44, 3: 44}
W_LOOP_CYCLES, W_LOOP_INSTRET, W_WAIT_INSTRET, W_INSTRET_TRAP = 120, 121, 122, 123
W_V3_STATUS, W_TIMER_CYCLES, W_TIMER_A, W_TIMER_B, W_TIMER_TRAPS = 128, 129, 130, 163, 196
W_T0_SAVED, W_T0_SAMPLES, W_T0_RESTORED, W_PLIC, W_PLIC_PRIORITY, W_PLIC_TRAPS = 198, 201, 207, 210, 221, 255
V3_STATUS_BITS = {1: "timer_block_sampled", 2: "timer_block_changed", 4: "timer0_experiment_run", 8: "timer0_counted",
                  16: "timer0_restored", 32: "plic_read"}
TIMER_BASE = 0x20010000
TIMER_WORDS = 33
PLIC_NAMES = ["feature", "pending_0", "pending_1", "trigger_type_0", "trigger_type_1", "configuration", "version",
              "enable_0", "enable_1", "threshold", "preempted_stack"]
PLIC_ADDRESSES = [0x10000000, 0x10001000, 0x10001004, 0x10001080, 0x10001084, 0x10001100, 0x10001104, 0x10002000,
                  0x10002004, 0x10200000, 0x10200400]
PLIC_SOURCES = 34
PRIORITY_TRAPPED = 1 << 16
UNREAD = 0xDEAD0BAD


def local_memory_size(cfg):
    code = (cfg >> 15) & 0x1F
    return 0 if code == 0 else 1024 << (code - 1)


def local_memory_base(reg):
    return {"enabled": bool(reg & 1), "base": f"0x{reg & ~0x3FF & 0xFFFFFFFF:08x}"}


def cpi(cycles, instructions):
    return round(cycles / instructions, 3) if cycles and instructions else None


def rate(delta, cycles):
    return round(delta / cycles, 6) if cycles else None


def timer_block(words):
    traps = words[W_TIMER_TRAPS] | words[W_TIMER_TRAPS + 1] << 32
    cycles = words[W_TIMER_CYCLES]
    block, ticking, trapped = {}, [], []
    for i in range(TIMER_WORDS):
        address = f"0x{TIMER_BASE + 4 * i:08x}"
        if traps >> i & 1:
            trapped.append(address)
            continue
        first, second = words[W_TIMER_A + i], words[W_TIMER_B + i]
        entry = {"first": f"0x{first:08x}", "second": f"0x{second:08x}"}
        if first != second:
            delta = (second - first) & 0xFFFFFFFF
            if delta >= 1 << 31:
                delta -= 1 << 32
            entry.update({"delta": delta, "per_mcycle": rate(delta, cycles)})
            ticking.append(address)
        block[address] = entry
    return {"window_cycles": cycles, "words": block, "ticking": ticking, "trapped": trapped}


def timer0(words, status):
    if not status & 4:
        return {"run": False}
    samples = [{"mcycle": words[W_T0_SAMPLES + 2 * i], "count": None if words[W_T0_SAMPLES + 2 * i + 1] == UNREAD
                else words[W_T0_SAMPLES + 2 * i + 1]} for i in range(3)]
    result = {"run": True, "touched": {"0x20010004": "OR 0x403", "0x20010024": 120000, "0x20010020": 3},
              "saved": {name: f"0x{words[W_T0_SAVED + i]:08x}" for i, name in enumerate(("0x20010004", "0x20010020", "0x20010024"))},
              "restored": {name: f"0x{words[W_T0_RESTORED + i]:08x}" for i, name in enumerate(("0x20010004", "0x20010020", "0x20010024"))},
              "restored_ok": bool(status & 16), "samples": samples, "counted": bool(status & 8)}
    first, last = samples[0], samples[-1]
    if result["counted"] and first["count"] is not None and last["count"] is not None:
        delta = (last["count"] - first["count"]) & 0xFFFFFFFF
        if delta >= 1 << 31:
            delta -= 1 << 32
        result["counts_per_mcycle"] = rate(delta, (last["mcycle"] - first["mcycle"]) & 0xFFFFFFFF)
    return result


def plic(words):
    traps = words[W_PLIC_TRAPS]
    registers = {}
    for i, (name, address) in enumerate(zip(PLIC_NAMES, PLIC_ADDRESSES)):
        registers[name] = None if traps >> i & 1 else f"0x{words[W_PLIC + i]:08x}"
    enabled = []
    for half in (0, 1):
        if not traps >> (7 + half) & 1:
            word = words[W_PLIC + 7 + half]
            enabled.extend(32 * half + bit for bit in range(32) if word >> bit & 1)
    priorities = None if traps & PRIORITY_TRAPPED else {
        str(source): words[W_PLIC_PRIORITY + source - 1] for source in range(1, PLIC_SOURCES + 1)}
    return {"addresses": {name: f"0x{address:08x}" for name, address in zip(PLIC_NAMES, PLIC_ADDRESSES)},
            "registers": registers, "enabled_sources": enabled, "priorities": priorities,
            "claim_register_read": False}


def decode(raw):
    if len(raw) < LEGACY_REPORT_BYTES:
        raise ValueError("report must be at least 512 bytes")
    layout = struct.unpack_from("<I", raw, 4)[0]
    if struct.unpack_from("<I", raw, 0)[0] != MAGIC or layout not in LAYOUTS:
        raise ValueError("not a kumi-probe layout 1, 2 or 3 report")
    if layout >= 3 and len(raw) < REPORT_BYTES:
        raise ValueError("a layout 3 report is 1024 bytes")
    words = struct.unpack(f"<{(REPORT_BYTES if layout >= 3 else LEGACY_REPORT_BYTES) // 4}I",
                          raw[:REPORT_BYTES if layout >= 3 else LEGACY_REPORT_BYTES])
    status = words[2]
    traps = words[23]
    csrs = {}
    for index, name in enumerate(CSR_NAMES):
        csrs[name] = None if traps & (1 << index) else f"0x{words[8 + index]:08x}"
    misa = words[8]
    extensions = "".join(chr(ord("A") + bit) for bit in range(26) if misa & (1 << bit)) if not traps & 1 else None
    cycles_per_ms = words[24]
    mtime_per_ms = words[25]
    registers = {}
    for i in range(min(words[31], REGISTER_LIMIT[layout])):
        address, value = words[32 + 2 * i], words[33 + 2 * i]
        registers[f"0x{address:08x}"] = None if value == 0xDEAD0BAD else f"0x{value:08x}"
    result = {
        "layout": layout,
        "status": [name for bit, name in STATUS_BITS.items() if status & bit],
        "entry_sp_after_prologue": f"0x{words[3]:08x}",
        "entry_ra": f"0x{words[4]:08x}",
        "bootrom_mstatus": f"0x{words[5]:08x}",
        "bootrom_mtvec": f"0x{words[6]:08x}",
        "bootrom_mie": f"0x{words[7]:08x}",
        "csr": csrs,
        "isa_extensions": extensions,
        "fpu_usable": bool(status & 4) and not status & 8,
        "dlm_executable": bool(status & 32) and not status & 64,
        "amo_usable": bool(status & 512) and not status & 256,
        "lrsc_usable": bool(status & 2048) and not status & 1024,
        "amo_trap_mcause": words[29] & 0xFF if words[29] else None,
        "lrsc_trap_mcause": words[30] & 0xFF if words[30] else None,
        "usb_frames_measured": words[26],
        "cpu_mhz": round(cycles_per_ms / 1000, 3) if cycles_per_ms else None,
        "mtime_mhz": round(mtime_per_ms / 1000, 3) if mtime_per_ms else None,
        "registers": registers,
    }
    if layout >= 2:
        result.update({
            "cpi_loop": {"cycles": words[W_LOOP_CYCLES], "instructions": words[W_LOOP_INSTRET],
                         "cycles_per_instruction": cpi(words[W_LOOP_CYCLES], words[W_LOOP_INSTRET])},
            "cpi_frame_wait": {"cycles": words[27], "instructions": words[W_WAIT_INSTRET],
                               "cycles_per_instruction": cpi(words[27], words[W_WAIT_INSTRET])},
            "minstret_trap_mcause": words[W_INSTRET_TRAP] & 0xFF if words[W_INSTRET_TRAP] else None,
        })
    if layout >= 3:
        status3 = words[W_V3_STATUS]
        result.update({
            "v3_status": [name for bit, name in V3_STATUS_BITS.items() if status3 & bit],
            "timer_block": timer_block(words),
            "timer0_experiment": timer0(words, status3),
            "plic": plic(words),
        })
    if csrs["micm_cfg"] is not None:
        result["ilm_size_bytes_per_andes_spec"] = local_memory_size(int(csrs["micm_cfg"], 16))
    if csrs["mdcm_cfg"] is not None:
        result["dlm_size_bytes_per_andes_spec"] = local_memory_size(int(csrs["mdcm_cfg"], 16))
    if csrs["milmb"] is not None:
        result["ilm_base_per_andes_spec"] = local_memory_base(int(csrs["milmb"], 16))
    if csrs["mdlmb"] is not None:
        result["dlm_base_per_andes_spec"] = local_memory_base(int(csrs["mdlmb"], 16))
    return result


def run(args):
    output = ramload.require_new_report(args.output)
    image = ramload.ram_image(Path(args.image).read_bytes(), LOAD_ADDRESS)
    if len(image) > IMAGE_LIMIT:
        raise SystemExit("probe image exceeds its 8 KiB window")
    digest, raw = ramload.run_on_device(args, LOAD_ADDRESS, image, LOAD_ADDRESS, REPORT_ADDRESS, REPORT_BYTES)
    report = decode(raw)
    report["image_sha256"] = digest
    ramload.save_report(output, raw, report)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Kumi M1 probe: load into case RAM through BROM, run, read the report")
    sub = parser.add_subparsers(dest="action", required=True)
    node = sub.add_parser("run", help="load and run the probe through the phone bridge")
    node.add_argument("--image", required=True)
    ramload.add_bridge_arguments(node)
    node = sub.add_parser("decode", help="decode a saved report (512 bytes for layouts 1 and 2, 1024 for layout 3)")
    node.add_argument("report")
    args = parser.parse_args(argv)
    if args.action == "run":
        run(args)
    else:
        print(json.dumps(decode(Path(args.report).read_bytes()), indent=2))


if __name__ == "__main__":
    main()

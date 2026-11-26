# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
from collections import Counter, OrderedDict, deque
import copy
import ctypes
import hashlib
import json
import os
from pathlib import Path
import pickle
import struct
import sys
import time
import zlib

import andes
from musb import ASSUMPTIONS as USB_ASSUMPTIONS, BASE as USB_BASE, SIZE as USB_SIZE, Musb, UsbHost, UsbProtocolError
from nor import FLASH_SIZE, Gd25, SpiController
from peripherals import ModelError, Plic, SdController, SystemRegisters, TimerWindow, Uart
import spv5048
from spv5048 import SERVICE_MODE_BIT

BROM_BASE = 0x80000
BROM_SIZE = 0x10000
ILM_SIZE = 0x10000
DLM_BASE = 0x100000
DLM_SIZE = 0x40000
XIP_ALIASES = (0x48000000, 0x4C000000, 0x6C000000)
RT_ALIASES = (0x40000000, 0x60000000)
RT_SIZE = 0x40000
MMIO_WINDOWS = ((0x10000000, 0x10000), (0x10200000, 0x10000), (0x12000000, 0x10000), (0x20000000, 0x1200000))
BUXX_LOOP = 0x82988
RETURN_ADDRESS = 0xFFFFF000
CPU_HZ = 60_000_000
CYCLES_PER_INSTRUCTION = 1
WAIT_SKIP_CYCLES = CPU_HZ // 10_000
WAIT_STREAK = 3
USB_IRQ = 34
MSTATUS_MIE, MSTATUS_MPIE, MIE_MEIE, MMISC_VEC_PLIC = 0x8, 0x80, 0x800, 0x2
RAM_WINDOWS = ((0, ILM_SIZE), (DLM_BASE, DLM_SIZE), *((a, RT_SIZE) for a in RT_ALIASES))
MEASURED_CSRS = {
    0x301: 0x40901124, 0xF11: 0x0000031E, 0xF12: 0x80000025, 0xF13: 0x00000240, 0xF14: 0,
    0xFC0: 0x000592DA, 0xFC1: 0x000592CA, 0xFC2: 0x2007F038, 0x7C0: 0x00000001, 0x7C1: 0x00100001,
    0x7CA: 0, 0x7D0: 0x00000048, 0x7C4: 0, 0x7C9: 0,
}
READ_ONLY_CSRS = {0x301, 0xF11, 0xF12, 0xF13, 0xF14, 0xFC0, 0xFC1, 0xFC2}
UNMEASURED_CSRS = {0x801}
COUNTER_CSRS = {0xB00, 0xB80, 0xC00, 0xC80, 0xB02, 0xB82, 0xC02, 0xC82}
NAMED_CSRS = {0x001: "FFLAGS", 0x002: "FRM", 0x003: "FCSR", 0x300: "MSTATUS", 0x304: "MIE", 0x305: "MTVEC",
              0x340: "MSCRATCH", 0x341: "MEPC", 0x342: "MCAUSE", 0x343: "MTVAL", 0x344: "MIP"}
NAMED_REGISTERS = {}
CORE_ASSUMPTIONS = (
    "CPU: one cycle per instruction (in-order core executing from ILM/DLM; minstret against mcycle is not yet "
    "measured, the M1 probe layout 2 measures it)",
    "CPU: CSRs misa, mvendorid, marchid, mimpid, mhartid, micm_cfg, mdcm_cfg, mmsc_cfg, milmb, mdlmb, mcache_ctl, "
    "mmisc_ctl, mxstatus and mdcause hold the values the M1 probe read; mmsc_cfg2, time and timeh trap as measured "
    "or because no time source was measured; CSR 0x801 (saved by the boot ROM and stock interrupt handlers) is "
    "plain storage reading zero; any other CSR traps",
    "time: a loop that reads mcycle with no memory or register side effects in between is fast-forwarded in steps "
    "of at most 100 us of virtual time, so its exit is seen up to 100 us late; minstret counts the skipped "
    "instructions",
    "time: idle time the host requests (advance_ms) passes without executing instructions",
    "RAM: ILM 64 KiB at 0, DLM 256 KiB at 0x100000 with aliases at 0x40000000 and 0x60000000 (probe and stock "
    "usage); after a software reset RAM is either kept or zeroed, as the run's retention setting says (G4 has not "
    "measured it); after power-on it reads zero",
)
NOT_MODELED = ["physical USB link and electrical behaviour, including the boot ROM stall after a USB resume",
               "DMA, I2S, DAC, ADC and audio", "power consumption and sleep", "caches and their timing"]


STOPPED, REDIRECTED = "stopped", "redirected"
CSR, ECALL, EBREAK, MRET, WFI, TRAP, CUSTOM, NATIVE_CSR = "csr", "ecall", "ebreak", "mret", "wfi", "trap", "custom", "native"
NATIVE_CSRS = (0x300, 0x304)
CUSTOM_OPCODES = (0x0B, 0x2B, 0x5B, 0x7F)


def is_double_precision(word):
    opcode = word & 0x7F
    return ((opcode in (0x07, 0x27) and ((word >> 12) & 7) == 3)
            or (opcode in (0x43, 0x47, 0x4B, 0x4F, 0x53) and ((word >> 25) & 3) == 1)
            or (opcode == 0x53 and (word >> 25) == 0x20 and ((word >> 20) & 31) == 1))


def is_memory(word, length):
    if length == 2:
        return (word >> 13) & 7 in (1, 2, 3, 5, 6, 7) and word & 3 in (0, 2)
    return word & 0x7F in (0x03, 0x07, 0x23, 0x27, 0x2F)


def classify(word, length):
    if length == 2:
        funct3, op = (word >> 13) & 7, word & 3
        if word == 0x9002:
            return EBREAK, False
        if funct3 in (1, 5) and op in (0, 2):
            return TRAP, False
        return None, funct3 in (6, 7) and op in (0, 2)
    opcode = word & 0x7F
    if opcode in CUSTOM_OPCODES:
        return CUSTOM, False
    if opcode == 0x73:
        if (word >> 12) & 3:
            return (NATIVE_CSR if word >> 20 in NATIVE_CSRS else CSR), False
        return {0x00000073: ECALL, 0x00100073: EBREAK, 0x30200073: MRET, 0x10500073: WFI}.get(word), False
    if opcode == 0x2F or is_double_precision(word):
        return TRAP, False
    return None, opcode == 0x23 or opcode == 0x27


class Block:
    def __init__(self, start, size, data):
        self.start = start
        self.size = size
        self.addrs = []
        self.lengths = []
        self.words = []
        self.index = {}
        self.specials = {}
        self.store_prefix = [0]
        self.memory = []
        self.decoded = []
        self.ready = False
        at = 0
        while at < size:
            half = int.from_bytes(data[at:at + 2], "little")
            length = 4 if half & 3 == 3 else 2
            word = int.from_bytes(data[at:at + length], "little") if at + length <= size else half
            kind, store = classify(word, length)
            address = start + at
            self.index[address] = len(self.addrs)
            self.addrs.append(address)
            self.lengths.append(length)
            self.words.append(word if length == 4 else word & 0xFFFF)
            if kind is not None:
                self.specials[address] = kind
            self.store_prefix.append(self.store_prefix[-1] + (1 if store else 0))
            self.memory.append(is_memory(word, length))
            self.decoded.append(andes.compile_op(andes.decode(word)) if kind == CUSTOM else None)
            at += length
        self.count = len(self.addrs)


EMPTY_BLOCK = Block(0, 0, b"")
DEVICE_NAMES = ("system", "plic", "uart", "spi", "sd", "usb", "timer_window", "flash")
DEVICE_LINKS = frozenset({"clock", "flash", "fallback", "image", "pins"})
EXECUTION_STATE = frozenset({
    "uc", "cpu", "r", "u", "hook_exception", "code_function", "peripherals", "pages", "blocks",
    "page_blocks", "code_pages", "hooked", "oneshot", "block", "flash_view", "rt_buffer", "image_entries", "done",
    "cut", "entered", "quiet", "resume_at", "resume_quiet", "mmio_next", "block_count", "deadline", "inputs",
    "located", "mmio_active",
    *DEVICE_NAMES,
})
TIMING_STATE = frozenset({"total_elapsed", "elapsed"})
CODE_FILES = ("andes.py", "brom_emu.py", "musb.py", "nor.py", "peripherals.py", "spv5048.py")
SNAPSHOT_DIRECTORY = "KUMI_EMULATOR_CACHE"
SNAPSHOT_MEMORY = 16


def code_digest():
    if not CODE_DIGEST:
        import unicorn
        digest = hashlib.sha256(unicorn.__version__.encode())
        for name in CODE_FILES:
            digest.update((Path(__file__).resolve().parent / name).read_bytes())
        CODE_DIGEST.append(digest.hexdigest())
    return CODE_DIGEST[0]


CODE_DIGEST = []


class SnapshotCache:
    def __init__(self):
        self.memory = OrderedDict()

    @staticmethod
    def directory():
        location = os.environ.get(SNAPSHOT_DIRECTORY)
        return Path(location) if location else None

    def get(self, key):
        if key in self.memory:
            self.memory.move_to_end(key)
            return self.memory[key]
        directory = self.directory()
        path = directory / f"{key}.snapshot" if directory else None
        if path is None or not path.exists():
            return None
        value = pickle.loads(zlib.decompress(path.read_bytes()))
        self.remember(key, value)
        return value

    def remember(self, key, value):
        self.memory[key] = value
        while len(self.memory) > SNAPSHOT_MEMORY:
            self.memory.popitem(last=False)

    def put(self, key, value):
        self.remember(key, value)
        directory = self.directory()
        if directory is None:
            return
        directory.mkdir(parents=True, exist_ok=True)
        temporary = directory / f"{key}.{os.getpid()}.partial"
        temporary.write_bytes(zlib.compress(pickle.dumps(value, protocol=pickle.HIGHEST_PROTOCOL), 1))
        temporary.replace(directory / f"{key}.snapshot")


SNAPSHOTS = SnapshotCache()


class Cpu:
    def __init__(self, uc):
        from unicorn.unicorn_py3 import unicorn as binding
        self.binding = binding
        self.lib = binding.uclib
        self.handle = uc._uch
        self.value = ctypes.c_uint32()
        self.ref = ctypes.byref(self.value)
        self.functions = []

    def reg(self, register):
        self.lib.uc_reg_read(self.handle, register, self.ref)
        return self.value.value

    def set_reg(self, register, value):
        self.value.value = value & 0xFFFFFFFF
        self.lib.uc_reg_write(self.handle, register, self.ref)

    def stop(self):
        self.lib.uc_emu_stop(self.handle)

    def keep(self, kind, function):
        cfunc = kind(function)
        self.functions.append(cfunc)
        return cfunc

    def hook(self, kind, cfunc_type, function, begin=1, end=0):
        handle = self.binding.uc_hook_h()
        status = self.lib.uc_hook_add(self.handle, ctypes.byref(handle), kind, self.keep(cfunc_type, function), None,
                                      ctypes.c_uint64(begin), ctypes.c_uint64(end))
        if status:
            raise self.binding.UcError(status)
        return handle.value

    def unhook(self, handle):
        status = self.lib.uc_hook_del(self.handle, self.binding.uc_hook_h(handle))
        if status:
            raise self.binding.UcError(status)

    def mmio(self, base, size, read, write):
        status = self.lib.uc_mmio_map(self.handle, base, size, self.keep(self.binding.MMIO_READ_CFUNC, read),
                                      ctypes.c_void_p(base), self.keep(self.binding.MMIO_WRITE_CFUNC, write),
                                      ctypes.c_void_p(base))
        if status:
            raise self.binding.UcError(status)


class Emulator(andes.Machine):
    def __init__(self, brom, flash_image, boot_flag=0, writable=False, stop_at_image=True, ram=None,
                 host_high_speed=True):
        import unicorn as u
        from unicorn import riscv_const as r
        if len(brom) != BROM_SIZE:
            raise ValueError("boot ROM dump must be exactly 64 KiB")
        self.inputs = (hashlib.sha256(bytes(brom)).hexdigest(), writable, stop_at_image, host_high_speed)
        self.u = u
        self.uc = u.Uc(u.UC_ARCH_RISCV, u.UC_MODE_RISCV32)
        self.cpu = Cpu(self.uc)
        self.r = r
        if not NAMED_REGISTERS:
            NAMED_REGISTERS.update({csr: getattr(r, "UC_RISCV_REG_" + name) for csr, name in NAMED_CSRS.items()})
        self.hook_exception = None
        self.steps = 0
        self.skipped_cycles = 0
        self.skipped_instret = 0
        self.cycle_offset = 0
        self.instret_offset = 0
        self.flash = Gd25(flash_image, self, writable)
        self.flash.on_change = self.flash_changed
        self.plic = Plic(self.update_irq)
        self.system = SystemRegisters(self.request_reset, boot_flag,
                                      on_timer3_irq=lambda level: self.plic.set_level(11, level))
        self.uart = Uart(self.system, on_rx_change=lambda level: self.plic.set_level(27, level))
        self.spi = SpiController(self.flash, self.system)
        self.sd = SdController()
        self.usb = Musb(self)
        self.usb.host_high_speed = host_high_speed
        self.usb.on_line_change = lambda level: self.plic.set_level(USB_IRQ, level)
        self.usb.on_device_event = self.device_event
        self.timer_window = TimerWindow()
        self.peripherals = (self.system, self.plic, self.uart, self.spi, self.sd, self.usb, self.timer_window, self.flash)
        self.pages = {}
        for base, size, device in ((0x10000000, 0x10000, self.plic), (0x10200000, 0x10000, self.plic),
                                   (0x12000000, 0x10000, self.timer_window), (0x20040000, 0x1000, self.uart),
                                   (0x200A0000, 0x1000, self.spi), (0x200C0000, 0x1000, self.sd),
                                   (USB_BASE, USB_SIZE, self.usb)):
            for page in range(base >> 12, (base + size) >> 12):
                self.pages[page] = device
        self.csrs = dict(MEASURED_CSRS)
        self.csrs_touched = set()
        self.mmio_log = deque(maxlen=128)
        self.custom = Counter()
        self.hot = Counter()
        self.outcome = None
        self.faults = []
        self.guest_traps = []
        self.unmapped = []
        self.interrupts = Counter()
        self.total_elapsed = 0
        self.last_pc = BROM_BASE
        self.blocks = {}
        self.page_blocks = {}
        self.code_pages = {}
        self.hooked = set()
        self.oneshot = None
        self.block = EMPTY_BLOCK
        self.done = 0
        self.cut = True
        self.entered = None
        self.quiet = None
        self.resume_at = None
        self.resume_quiet = False
        self.mmio_next = None
        self.located = False
        self.mmio_active = False
        self.irq_check = False
        self.block_count = 0
        self.breakpoints = {}
        self.stop_at_image = stop_at_image
        self.stop_after_irq = False
        self.stop_on_usb = False
        self.usb_event = False
        self.after_mret = False
        self.irq_armed = False
        self.attention = False
        self.image_entries = frozenset(XIP_ALIASES[:2])
        self.impure = 0
        self.wait = (None, 0, 0, 0, 0)
        self.waits_skipped = 0
        self.wfi_halts = 0
        self.xip_entries = []
        self.elf_symbols = {}
        self.loaded_elf_sha256 = None
        self.limit = 0
        self.deadline = 0
        self.skip_initial_stop = False
        self.uc.mem_map(0, ILM_SIZE)
        self.uc.mem_map(DLM_BASE, DLM_SIZE)
        if ram is None:
            self.uc.mem_write(0, bytes(ILM_SIZE))
            self.uc.mem_write(DLM_BASE, bytes(DLM_SIZE))
        self.uc.mem_map(BROM_BASE, BROM_SIZE, u.UC_PROT_READ | u.UC_PROT_EXEC)
        self.uc.mem_write(BROM_BASE, bytes(brom))
        self.flash_view = (ctypes.c_char * FLASH_SIZE).from_buffer(self.flash.image)
        for address in XIP_ALIASES:
            self.uc.mem_map_ptr(address, FLASH_SIZE, u.UC_PROT_READ | u.UC_PROT_EXEC, ctypes.addressof(self.flash_view))
        self.rt_buffer = ctypes.create_string_buffer(RT_SIZE)
        for address in RT_ALIASES:
            self.uc.mem_map_ptr(address, RT_SIZE, u.UC_PROT_ALL, ctypes.addressof(self.rt_buffer))
        self.uc.mem_map(RETURN_ADDRESS, 0x1000, u.UC_PROT_READ | u.UC_PROT_EXEC)
        binding = self.cpu.binding
        for base, size in MMIO_WINDOWS:
            self.cpu.mmio(base, size, self.mmio_read_hook, self.mmio_write_hook)
        if ram is not None:
            self.restore_ram(ram)
        self.code_function = self.cpu.keep(binding.HOOK_CODE_CFUNC, self.on_hooked)
        self.cpu.hook(u.UC_HOOK_BLOCK, binding.HOOK_CODE_CFUNC, self.on_block)
        self.cpu.hook(u.UC_HOOK_INTR, binding.HOOK_INTR_CFUNC, self.guard(self.on_exception))
        self.cpu.hook(u.UC_HOOK_MEM_INVALID, binding.HOOK_MEM_INVALID_CFUNC, self.guard(self.on_invalid_memory, False))
        for base, size in MMIO_WINDOWS:
            self.cpu.hook(u.UC_HOOK_MEM_READ | u.UC_HOOK_MEM_WRITE, binding.HOOK_MEM_ACCESS_CFUNC, self.sync_pc,
                          begin=base, end=base + size - 1)
        self.ensure_hooks(self.image_entries)

    def guard(self, function, failed=None):
        def guarded(*args):
            try:
                return function(*args)
            except BaseException as error:
                return self.hook_failed(error, failed)
        return guarded

    def hook_failed(self, error, value=None):
        if self.hook_exception is None:
            self.hook_exception = error
        self.cpu.stop()
        return value

    @staticmethod
    def sync_pc(handle, access, address, size, value, user):
        return None

    @property
    def registers(self):
        return self.system.words

    def boot_flag(self):
        return self.system.boot_flag

    def cycles(self):
        if self.mmio_active and not self.located:
            raise ModelError("a device needed exact time during an MMIO access that was not located")
        return self.steps * CYCLES_PER_INSTRUCTION + self.skipped_cycles

    def cycles_floor(self):
        return self.steps * CYCLES_PER_INSTRUCTION + self.skipped_cycles

    def cycles_per_ms(self):
        return CPU_HZ // 1000

    def skip_to(self, target):
        if target > self.cycles():
            self.skipped_cycles += target - self.cycles()

    def advance_ms(self, milliseconds):
        self.skipped_cycles += int(milliseconds * self.cycles_per_ms())
        if milliseconds > 0 and self.system.timer3_irq_enabled():
            self.system.raise_timer3()

    def ram_image(self):
        return bytes(self.uc.mem_read(0, ILM_SIZE)) + bytes(self.uc.mem_read(DLM_BASE, DLM_SIZE))

    def restore_ram(self, ram):
        if len(ram) != ILM_SIZE + DLM_SIZE:
            raise ValueError("retained RAM must cover ILM and DLM")
        self.uc.mem_write(0, bytes(ram[:ILM_SIZE]))
        self.uc.mem_write(DLM_BASE, bytes(ram[ILM_SIZE:]))

    def read_reg(self, index):
        return 0 if index == 0 else self.cpu.reg(self.r.UC_RISCV_REG_X0 + index)

    def write_reg(self, index, value):
        if index:
            self.cpu.set_reg(self.r.UC_RISCV_REG_X0 + index, value)

    def pc(self):
        return self.cpu.reg(self.r.UC_RISCV_REG_PC)

    @staticmethod
    def in_mmio(address):
        return any(base <= address < base + length for base, length in MMIO_WINDOWS)

    def load(self, address, size, signed=False):
        if self.in_mmio(address):
            value = self.mmio_access(address, size)
        else:
            value = int.from_bytes(self.uc.mem_read(address, size), "little")
        if signed and value >> (8 * size - 1):
            value -= 1 << (8 * size)
        return value & 0xFFFFFFFF

    def store(self, address, size, value):
        if self.in_mmio(address):
            self.mmio_access(address, size, value)
            return
        if not self.in_ram(address, size):
            raise ModelError(f"write outside RAM: 0x{address:08x}")
        self.impure += 1
        self.uc.mem_write(address, (value & ((1 << (8 * size)) - 1)).to_bytes(size, "little"))
        self.code_written(address, size)

    @staticmethod
    def in_ram(address, size):
        return any(base <= address and address + size <= base + length for base, length in RAM_WINDOWS)

    @staticmethod
    def aliases(address):
        for alias in RT_ALIASES:
            if alias <= address < alias + RT_SIZE:
                return [address - alias + other for other in RT_ALIASES]
        return [address]

    def drop_blocks(self, address, size):
        dropped = False
        for page in range(address >> 12, ((address + size - 1) >> 12) + 1):
            for key in list(self.page_blocks.get(page, ())):
                start, length = key >> 16, key & 0xFFFF
                if start < address + size and address < start + length:
                    self.page_blocks[page].discard(key)
                    self.blocks.pop(key, None)
                    dropped = True
        return dropped

    def invalidate(self, address, size):
        for start in self.aliases(address):
            self.drop_blocks(start, size)
            self.uc.ctl_remove_cache(start, start + size)

    def code_written(self, address, size):
        if any(page in self.code_pages for start in self.aliases(address)
               for page in range(start >> 12, ((start + size - 1) >> 12) + 1)):
            self.invalidate(address, size)

    def flash_changed(self, offset, size):
        for alias in XIP_ALIASES:
            self.drop_blocks(alias + offset, size)
            self.uc.ctl_remove_cache(alias + offset, alias + offset + size)

    def memory_state(self):
        return (bytes(self.uc.mem_read(0, ILM_SIZE)), bytes(self.uc.mem_read(DLM_BASE, DLM_SIZE)), self.rt_buffer.raw,
                bytes(self.flash.image))

    def model_state(self):
        state = {key: value for key, value in vars(self).items() if key not in EXECUTION_STATE}
        devices = {name: {key: value for key, value in vars(getattr(self, name)).items()
                          if key not in DEVICE_LINKS and not callable(value)} for name in DEVICE_NAMES}
        return state, devices

    def snapshot(self):
        state, devices = self.model_state()
        return {"context": pickle.dumps(self.uc.context_save()), "memory": self.memory_state(),
                "state": copy.deepcopy(state), "devices": copy.deepcopy(devices)}

    def restore(self, snapshot):
        if self.steps or self.faults:
            raise ValueError("a snapshot can only be restored into a fresh emulator")
        self.uc.context_restore(pickle.loads(snapshot["context"]))
        ilm, dlm, rt, flash = snapshot["memory"]
        self.uc.mem_write(0, ilm)
        self.uc.mem_write(DLM_BASE, dlm)
        ctypes.memmove(self.rt_buffer, rt, RT_SIZE)
        self.flash.image[:] = flash
        for key, value in copy.deepcopy(snapshot["state"]).items():
            setattr(self, key, value)
        for name, values in copy.deepcopy(snapshot["devices"]).items():
            for key, value in values.items():
                setattr(getattr(self, name), key, value)

    def snapshot_key(self, *arguments):
        state, devices = self.model_state()
        digest = hashlib.sha256(code_digest().encode())
        registers = [self.cpu.reg(self.r.UC_RISCV_REG_X0 + i) for i in range(32)]
        registers += [self.cpu.reg(register) for register in (self.r.UC_RISCV_REG_PC, *NAMED_REGISTERS.values())]
        digest.update(pickle.dumps((self.inputs, registers, arguments,
                                    {k: v for k, v in state.items() if k not in TIMING_STATE}, devices)))
        for part in self.memory_state():
            digest.update(hashlib.sha256(part).digest())
        return digest.hexdigest()

    def boot(self, budget=100_000_000, stops=None, start=None):
        if self.steps or self.faults:
            raise ValueError("boot snapshots need a fresh emulator")
        key = self.snapshot_key(budget, sorted((stops or {}).items()), start)
        cached = SNAPSHOTS.get(key)
        if cached is not None:
            self.restore(cached["snapshot"])
            return copy.deepcopy(cached["report"])
        report = self.run(budget, start, stops)
        SNAPSHOTS.put(key, {"snapshot": self.snapshot(), "report": copy.deepcopy(report)})
        return report

    def write_memory(self, address, data):
        self.uc.mem_write(address, bytes(data))
        self.invalidate(address, len(data))

    def on_code_write(self, handle, access, address, size, value, user):
        for start in self.aliases(address):
            self.drop_blocks(start, size)

    def watch_code(self, block):
        if not self.in_ram(block.start, 1):
            return
        for start in self.aliases(block.start):
            for page in range(start >> 12, ((start + block.size - 1) >> 12) + 1):
                if page not in self.code_pages:
                    self.code_pages[page] = self.cpu.hook(self.u.UC_HOOK_MEM_WRITE, self.cpu.binding.HOOK_MEM_ACCESS_CFUNC,
                                                          self.guard(self.on_code_write), page << 12, (page << 12) + 0xFFF)

    def device_for(self, address):
        return self.pages.get(address >> 12, self.system)

    def mmio_access(self, address, size, value=None):
        device = self.device_for(address)
        if value is None:
            result = device.read(address, size)
            if not device.pure_read(address):
                self.impure += 1
            self.mmio_log.append(f"r{size} 0x{address:08x}=0x{result:08x}")
            return result
        self.impure += 1
        self.mmio_log.append(f"w{size} 0x{address:08x}=0x{value:08x}")
        device.write(address, size, value)
        return None

    def locate(self, address):
        if self.located:
            return True
        pc = self.cpu.reg(self.r.UC_RISCV_REG_PC)
        block = self.block
        index = block.index.get(pc)
        if index is None or not block.memory[index]:
            self.fail(f"MMIO access at 0x{address:08x} without a known instruction position (pc 0x{pc:08x})")
            return False
        self.account(index + 1)
        self.last_pc = pc
        self.mmio_next = pc + block.lengths[index]
        self.located = True
        return True

    def mmio_read_hook(self, handle, offset, size, base):
        try:
            if self.faults:
                return 0
            address = base + offset
            device = self.pages.get(address >> 12, self.system)
            self.located = False
            if device.needs_time(address, size, None, self.cycles_floor()) and not self.locate(address):
                return 0
            self.mmio_active = True
            try:
                return self.mmio_access(address, size)
            except ModelError as error:
                self.locate(address)
                self.fail(str(error))
                return 0
            finally:
                self.mmio_active = False
        except BaseException as error:
            return self.hook_failed(error, 0)

    def mmio_write_hook(self, handle, offset, size, value, base):
        try:
            if self.faults:
                return
            address = base + offset
            device = self.pages.get(address >> 12, self.system)
            self.located = False
            if device.needs_time(address, size, value, self.cycles_floor()) and not self.locate(address):
                return
            armed = self.irq_armed
            self.mmio_active = True
            try:
                self.mmio_access(address, size, value)
            except ModelError as error:
                self.locate(address)
                self.fail(str(error))
                return
            finally:
                self.mmio_active = False
            if self.outcome is not None:
                self.locate(address)
            elif self.usb_event or (self.irq_armed and not armed):
                if self.locate(address):
                    self.restart(self.mmio_next, quiet=False)
        except BaseException as error:
            self.hook_failed(error)

    def device_event(self):
        if self.stop_on_usb:
            self.usb_event = True
            self.attention = True

    def request_reset(self):
        self.outcome = {"result": "reset_requested", "boot_flag": f"0x{self.system.boot_flag:08x}"}
        self.cpu.stop()

    def fail(self, message, **fields):
        self.faults.append({"error": message, "pc": f"0x{self.last_pc:08x}", **fields})
        self.cut = True
        self.cpu.stop()

    def restart(self, address, quiet):
        self.cut = True
        self.resume_at = address
        self.resume_quiet = quiet
        self.cpu.stop()

    def on_invalid_memory(self, handle, access, address, size, value, user):
        self.unmapped.append({"access": access, "address": f"0x{address:08x}", "size": size})
        self.fail("unmapped or protected memory", address=f"0x{address:08x}")
        return False

    def enter_trap(self, cause, value, pc, vector):
        r, cpu = self.r, self.cpu
        status = cpu.reg(r.UC_RISCV_REG_MSTATUS)
        cpu.set_reg(r.UC_RISCV_REG_MSTATUS, (status & ~0x1888) | ((status & MSTATUS_MIE) << 4) | 0x1800)
        cpu.set_reg(r.UC_RISCV_REG_MEPC, pc)
        cpu.set_reg(r.UC_RISCV_REG_MCAUSE, cause)
        cpu.set_reg(r.UC_RISCV_REG_MTVAL, value)
        cpu.set_reg(r.UC_RISCV_REG_PC, vector)
        self.impure += 1
        self.update_irq()

    def trap(self, cause, word):
        vector = self.cpu.reg(self.r.UC_RISCV_REG_MTVEC) & ~3
        self.guest_traps.append({"pc": f"0x{self.last_pc:08x}", "cause": cause, "word": f"0x{word:08x}"})
        if not vector or len(self.guest_traps) > 32:
            self.fail("unhandled guest trap", cause=cause, word=f"0x{word:08x}")
            return
        self.enter_trap(cause, word, self.last_pc, vector)

    def on_exception(self, handle, number, user):
        block = self.block
        if block.addrs:
            self.last_pc = block.addrs[-1]
            self.account(len(block.addrs))
        self.cut = True
        word = int.from_bytes(self.uc.mem_read(self.last_pc, 4), "little")
        self.trap(number, word)
        if not self.faults:
            self.restart(self.pc(), quiet=False)

    def custom_instruction(self, address, word, operation):
        if operation is None:
            self.trap(2, word)
            return
        name, target = operation(self, address)
        self.custom[name] += 1
        self.hot[address] += 1
        self.cpu.set_reg(self.r.UC_RISCV_REG_PC, target)

    def update_irq(self):
        if not self.plic.pending or self.plic.best() is None:
            self.irq_armed = False
            return
        status = self.cpu.reg(self.r.UC_RISCV_REG_MSTATUS)
        enabled = self.cpu.reg(self.r.UC_RISCV_REG_MIE)
        self.irq_armed = bool(status & MSTATUS_MIE and enabled & MIE_MEIE)
        if self.irq_armed:
            self.attention = True

    def take_interrupt(self, pc):
        mtvec = self.cpu.reg(self.r.UC_RISCV_REG_MTVEC)
        if self.csrs[0x7D0] & MMISC_VEC_PLIC:
            if not self.plic.vectored():
                raise ModelError("mmisc_ctl.VEC_PLIC set while the PLIC is not in vectored mode")
            source = self.plic.claim()
            vector = self.load((mtvec & ~3) + 4 * source, 4)
        else:
            if self.plic.vectored():
                raise ModelError("PLIC vectored mode without mmisc_ctl.VEC_PLIC is not modeled")
            source = self.plic.best()
            vector = (mtvec & ~3) + (4 * 11 if mtvec & 3 == 1 else 0)
        self.interrupts[source] += 1
        self.enter_trap(0x8000000B, 0, pc, vector)

    def counter(self, csr):
        cycles = self.cycles() + self.cycle_offset
        instret = self.steps + self.skipped_instret + self.instret_offset
        value = cycles if csr in (0xB00, 0xB80, 0xC00, 0xC80) else instret
        return (value >> 32) & 0xFFFFFFFF if csr in (0xB80, 0xC80, 0xB82, 0xC82) else value & 0xFFFFFFFF

    def set_counter(self, csr, new):
        if csr in (0xB00, 0xB80):
            current = self.cycles() + self.cycle_offset
            target = ((current & ~0xFFFFFFFF) | new) if csr == 0xB00 else ((current & 0xFFFFFFFF) | (new << 32))
            self.cycle_offset += target - current
        else:
            current = self.steps + self.skipped_instret + self.instret_offset
            target = ((current & ~0xFFFFFFFF) | new) if csr == 0xB02 else ((current & 0xFFFFFFFF) | (new << 32))
            self.instret_offset += target - current

    def fast_forward_wait(self):
        pc, steps, impure, period, streak = self.wait
        if pc == self.last_pc and impure == self.impure and self.steps - steps == period and period > 0:
            streak += 1
        else:
            streak, period = 0, self.steps - steps
        if streak >= WAIT_STREAK:
            iterations = max(1, WAIT_SKIP_CYCLES // (period * CYCLES_PER_INSTRUCTION))
            self.skipped_cycles += iterations * period * CYCLES_PER_INSTRUCTION
            self.skipped_instret += iterations * period
            self.waits_skipped += iterations
        self.wait = (self.last_pc, self.steps, self.impure, period, streak)

    def csr(self, word):
        funct3 = (word >> 12) & 7
        rd, rs1, csr = (word >> 7) & 31, (word >> 15) & 31, word >> 20
        self.csrs_touched.add(csr)
        if csr in (0xB00, 0xC00):
            self.fast_forward_wait()
        if csr in COUNTER_CSRS:
            old = self.counter(csr)
        elif csr in NAMED_CSRS:
            old = self.cpu.reg(NAMED_REGISTERS[csr])
        elif csr in self.csrs or csr in UNMEASURED_CSRS:
            old = self.csrs.get(csr, 0)
        else:
            self.trap(2, word)
            return
        operand = rs1 if funct3 & 4 else self.read_reg(rs1)
        kind = funct3 & 3
        new = (operand if kind == 1 else old | operand if kind == 2 else old & ~operand) & 0xFFFFFFFF
        if kind == 1 or rs1:
            if csr >> 10 == 3:
                self.trap(2, word)
                return
            self.impure += 1
            if csr in COUNTER_CSRS:
                self.set_counter(csr, new)
            elif csr in NAMED_CSRS:
                self.cpu.set_reg(NAMED_REGISTERS[csr], new)
            elif csr not in READ_ONLY_CSRS:
                self.csrs[csr] = new
            if csr in (0x300, 0x304, 0x7D0):
                self.update_irq()
        self.write_reg(rd, old)
        self.cpu.set_reg(self.r.UC_RISCV_REG_PC, self.last_pc + 4)

    def refresh_attention(self):
        self.attention = self.skip_initial_stop or self.usb_event or self.after_mret or self.irq_armed

    def attend(self, address):
        if address in self.breakpoints and not self.skip_initial_stop:
            self.outcome = {"result": self.breakpoints[address], "pc": f"0x{address:08x}", "a0": self.read_reg(10)}
            self.cpu.stop()
            return STOPPED
        self.skip_initial_stop = False
        if self.usb_event:
            self.usb_event = False
            self.outcome = {"result": "usb_event", "pc": f"0x{address:08x}"}
            self.cpu.stop()
            self.refresh_attention()
            return STOPPED
        if self.after_mret:
            self.after_mret = False
            self.update_irq()
            if self.stop_after_irq and not self.irq_armed:
                self.outcome = {"result": "irq_returned", "pc": f"0x{address:08x}"}
                self.cpu.stop()
                self.refresh_attention()
                return STOPPED
        self.refresh_attention()
        if self.irq_armed and self.steps < self.limit:
            try:
                self.take_interrupt(address)
            except ModelError as error:
                self.fail(str(error))
                return STOPPED
            return REDIRECTED
        return None

    def account(self, upto):
        done = self.done
        if upto > done:
            block = self.block
            self.steps += upto - done
            if block.store_prefix[upto] != block.store_prefix[done]:
                self.impure += 1
            self.done = upto

    def ensure_hooks(self, addresses):
        added = False
        for address in addresses:
            if address not in self.hooked:
                self.add_code_hook(address)
                self.uc.ctl_remove_cache(address, address + 4)
                self.hooked.add(address)
                added = True
        return added

    def add_code_hook(self, address):
        handle = self.cpu.binding.uc_hook_h()
        status = self.cpu.lib.uc_hook_add(self.cpu.handle, ctypes.byref(handle), self.u.UC_HOOK_CODE, self.code_function, None,
                                          ctypes.c_uint64(address), ctypes.c_uint64(address))
        if status:
            raise self.cpu.binding.UcError(status)
        return handle.value

    def decode_block(self, address, size):
        block = Block(address, size, bytes(self.uc.mem_read(address, size)))
        self.blocks[(address << 16) | size] = block
        for page in range(address >> 12, ((address + size - 1) >> 12) + 1):
            self.page_blocks.setdefault(page, set()).add((address << 16) | size)
        self.watch_code(block)
        return block

    def clear_oneshot(self):
        if self.oneshot is not None:
            address, handle = self.oneshot
            self.cpu.unhook(handle)
            self.uc.ctl_remove_cache(address, address + 4)
            self.oneshot = None

    def on_block(self, handle, address, size, user):
        try:
            if not self.cut:
                block = self.block
                upto = block.count
                done = self.done
                if upto > done:
                    self.steps += upto - done
                    prefix = block.store_prefix
                    if prefix[upto] != prefix[done]:
                        self.impure += 1
            block = self.blocks.get((address << 16) | size)
            if block is None:
                block = self.decode_block(address, size)
            self.block = block
            self.done = 0
            self.cut = False
            self.entered = address
            self.last_pc = address
            if self.irq_check:
                self.irq_check = False
                self.update_irq()
            if self.quiet == address:
                self.quiet = None
            else:
                self.quiet = None
                if self.attention or address in self.breakpoints:
                    action = self.attend(address)
                    if action is STOPPED:
                        self.cut = True
                        return
                    if action is REDIRECTED:
                        self.restart(self.pc(), quiet=False)
                        return
            if not block.ready:
                block.ready = True
                if self.ensure_hooks(block.specials):
                    self.uc.ctl_remove_cache(address, address + size)
                    self.restart(address, quiet=True)
                    return
            remaining = self.limit - self.steps
            self.block_count += 1
            if remaining < block.count or not self.block_count & 0xFF:
                self.check_budget(block, address, size, remaining)
        except BaseException as error:
            self.hook_failed(error)

    def check_budget(self, block, address, size, remaining):
        if remaining <= 0 or (not self.block_count & 0xFF and time.monotonic() >= self.deadline):
            self.outcome = {"result": "budget_exhausted", "pc": f"0x{address:08x}"}
            self.cut = True
            self.cpu.stop()
            return
        if remaining < block.count:
            target = block.addrs[remaining]
            if target not in self.hooked and (self.oneshot is None or self.oneshot[0] != target):
                self.clear_oneshot()
                self.oneshot = (target, self.add_code_hook(target))
                self.uc.ctl_remove_cache(address, address + size)
                self.restart(address, quiet=True)

    def on_hooked(self, handle, address, size, user):
        try:
            block = self.block
            index = block.index.get(address)
            if index is None:
                return
            self.last_pc = address
            if address != self.entered:
                self.account(index)
                if (self.attention or address in self.breakpoints):
                    action = self.attend(address)
                    if action is not None:
                        self.cut = True
                        return
                if self.steps >= self.limit:
                    self.outcome = {"result": "budget_exhausted", "pc": f"0x{address:08x}"}
                    self.cut = True
                    self.cpu.stop()
                    return
            self.account(index + 1)
            if address in self.image_entries:
                self.xip_entries.append(f"0x{address:08x}")
                if self.stop_at_image:
                    self.outcome = {"result": "boot_image", "pc": f"0x{address:08x}"}
                    self.cut = True
                    self.cpu.stop()
                    return
            kind = block.specials.get(address)
            if kind is NATIVE_CSR:
                self.native_csr(block.words[index])
            elif kind is not None:
                if kind != MRET:
                    self.cut = True
                try:
                    self.special(address, kind, block, index)
                except (ModelError, ValueError) as error:
                    self.fail(str(error))
        except BaseException as error:
            self.hook_failed(error)

    def native_csr(self, word):
        self.csrs_touched.add(word >> 20)
        if (word >> 12) & 3 == 1 or (word >> 15) & 31:
            self.impure += 1
            self.irq_check = True

    def special(self, address, kind, block, index):
        word = block.words[index]
        if kind == CUSTOM:
            self.custom_instruction(address, word, block.decoded[index])
        elif kind == CSR:
            self.csr(word)
        elif kind == ECALL:
            self.trap(11, 0)
        elif kind == EBREAK:
            self.trap(3, 0)
        elif kind == MRET:
            self.after_mret = True
            self.attention = True
            self.impure += 1
        elif kind == WFI:
            if self.plic.best() is not None:
                self.cpu.set_reg(self.r.UC_RISCV_REG_PC, address + 4)
            else:
                self.wfi_halts += 1
                self.outcome = {"result": "wfi", "pc": f"0x{address:08x}"}
                self.cpu.stop()
        else:
            self.trap(2, word)

    def run(self, budget=50_000_000, start=None, stops=None, seconds=900, skip_initial_stop=False, stop_after_irq=False,
            stop_on_usb=False):
        if budget <= 0 or seconds <= 0:
            raise ValueError("budgets must be positive")
        self.outcome = None
        self.breakpoints = {BUXX_LOOP: "service_ready", RETURN_ADDRESS: "returned", **(stops or {})}
        self.skip_initial_stop = skip_initial_stop
        self.stop_after_irq = stop_after_irq
        self.stop_on_usb = stop_on_usb
        self.usb_event = False
        self.after_mret = False
        self.limit = self.steps + budget
        self.deadline = time.monotonic() + seconds
        started = time.monotonic()
        self.update_irq()
        self.refresh_attention()
        self.ensure_hooks(self.breakpoints)
        address = BROM_BASE if start is None else start
        self.block = EMPTY_BLOCK
        self.cut = True
        self.quiet = None
        while True:
            self.resume_at = None
            self.hook_exception = None
            try:
                self.uc.emu_start(address, 0xFFFFFFFF, timeout=int(seconds * 1_000_000))
            except Exception as error:
                if not self.faults:
                    self.fail(str(error))
            if self.hook_exception is not None and not self.faults:
                self.fail(str(self.hook_exception))
            if self.resume_at is None or self.outcome is not None or self.faults:
                break
            address = self.resume_at
            self.quiet = address if self.resume_quiet else None
        if not self.cut:
            self.account(self.block.count)
            self.cut = True
        self.clear_oneshot()
        if self.outcome is None and not self.faults:
            self.outcome = {"result": "budget_exhausted", "pc": f"0x{self.pc():08x}"}
        self.elapsed = time.monotonic() - started
        self.total_elapsed += self.elapsed
        return self.report()

    def resume(self, budget, stops=None, stop_after_irq=False, stop_on_usb=False):
        return self.run(budget, self.pc(), stops, skip_initial_stop=True, stop_after_irq=stop_after_irq,
                        stop_on_usb=stop_on_usb)

    def call(self, entry, args=(), budget=2_000_000):
        if not self.read_reg(2):
            self.write_reg(2, 0x13F000)
        self.write_reg(1, RETURN_ADDRESS)
        for i, value in enumerate(args):
            self.write_reg(10 + i, value)
        return self.run(budget, entry)

    def load_elf(self, data, load=True):
        entry, segments = elf_segments(data)
        loaded = []
        for address, contents, memory in segments:
            if any(base <= address and address + memory <= base + FLASH_SIZE for base in XIP_ALIASES):
                continue
            if not self.in_ram(address, memory):
                raise ValueError(f"ELF segment outside RAM: 0x{address:08x}")
            if load:
                self.write_memory(address, contents + b"\x00" * (memory - len(contents)))
            loaded.append({"address": f"0x{address:08x}", "file_bytes": len(contents), "memory_bytes": memory})
        if not loaded:
            raise ValueError("ELF has no RAM segments")
        self.elf_symbols = elf_symbols(data)
        if load:
            self.loaded_elf_sha256 = hashlib.sha256(data).hexdigest()
        return entry, loaded

    def running_identity(self, elf):
        info = spv5048.boot_info(self.uc.mem_read(spv5048.BOOT_INFO_ADDRESS, spv5048.BOOT_INFO_SIZE))
        if info is not None:
            base = spv5048.SLOTS[info["slot"]][0]
            header = self.flash.image[base:base + 256]
            length = 256 + int.from_bytes(header[16:20], "little")
            if length > spv5048.SLOT_A[1]:
                raise ModelError("booted image exceeds its slot")
            image = bytes(self.flash.image[base:base + length])
            verify_image_elf(image, elf)
            if tuple(header[8:11]) != info["image_version"] or int.from_bytes(header[12:16], "little") != info["generation"]:
                raise ModelError("boot information does not match the image")
            return {"kind": "signed_slot", "slot": info["slot"], "image_sha256": hashlib.sha256(image).hexdigest()}
        digest = hashlib.sha256(elf).hexdigest()
        if self.loaded_elf_sha256 != digest:
            raise ModelError("running ELF has no matching RAM load or booted image")
        return {"kind": "ram", "elf_sha256": digest}

    def report(self):
        registers = self.system.report()
        usb = self.usb.report()
        return {
            "outcome": self.outcome,
            "faults": self.faults,
            "instructions": self.steps,
            "virtual_ms": round(self.cycles() / self.cycles_per_ms(), 3),
            "elapsed_seconds": round(self.total_elapsed, 3),
            "last_run_seconds": round(getattr(self, "elapsed", 0), 3),
            "xip_entries": self.xip_entries,
            "flash_commands": self.flash.transactions,
            "flash_writes": self.flash.writes,
            "flash_ignored": self.flash.ignored,
            "flash_jedec": self.flash.report()["jedec"],
            "usb_register_writes": usb["register_writes"],
            "usb_softconnect": usb["softconnect"],
            "usb_fifo_overreads": usb["fifo_overreads"],
            "interrupts": {str(s): n for s, n in sorted(self.interrupts.items())},
            "andes_instructions": dict(self.custom),
            "wait_iterations_skipped": self.waits_skipped,
            "unmapped_accesses": self.unmapped,
            "guest_traps": self.guest_traps,
            "csrs_touched": sorted(f"0x{c:03x}" for c in self.csrs_touched),
            "registers_defaulted_to_zero": {**registers["registers_defaulted_to_zero"], **usb["registers_defaulted_to_zero"],
                                            **self.plic.report()["registers_defaulted_to_zero"],
                                            **self.sd.report()["registers_defaulted_to_zero"],
                                            **self.uart.report()["registers_defaulted_to_zero"]},
            "unmodeled_register_writes": {**registers["unmodeled_register_writes"], **usb["unmodeled_register_writes"],
                                          **self.plic.report()["unmodeled_register_writes"],
                                          **self.sd.report()["unmodeled_register_writes"],
                                          **self.uart.report()["unmodeled_register_writes"]},
            "hot_andes_pcs": [f"0x{pc:08x}:{n}" for pc, n in self.hot.most_common(6)],
            "mmio_tail": list(self.mmio_log)[-12:],
        }


def model_description(retention=None):
    import unicorn
    assumptions = list(CORE_ASSUMPTIONS)
    for device in (SystemRegisters, Plic, Uart, SpiController, SdController, TimerWindow):
        assumptions.extend(device.assumptions)
    assumptions.extend(USB_ASSUMPTIONS)
    assumptions.extend(Gd25.assumptions)
    description = {"unicorn": unicorn.__version__, "cpu": "RV32IMCFUX with Andes custom instructions; A and D trap",
                   "virtual_cpu_hz": CPU_HZ, "virtual_cycles_per_instruction": CYCLES_PER_INSTRUCTION,
                   "ilm_bytes": ILM_SIZE, "dlm_bytes": DLM_SIZE,
                   "csrs": {f"0x{c:03x}": f"0x{v:08x}" for c, v in sorted(MEASURED_CSRS.items())},
                   "assumptions": assumptions, "not_modeled": NOT_MODELED}
    if retention is not None:
        description["reset_retention"] = retention
    return description


def scenarios(stock):
    flags, length, stored = (struct.unpack_from("<I", stock, at)[0] for at in (0xC, 0x28, 0x2C))
    out = {"stock": bytes(stock), "all_erased": b"\xff" * FLASH_SIZE, "all_zero": b"\x00" * FLASH_SIZE}
    edits = {
        "header_sector_erased": (0, b"\xff" * 0x1000),
        "one_body_bit_flipped": (0x1000, bytes([stock[0x1000] ^ 1])),
        "stored_sum_off_by_one": (0x2C, struct.pack("<I", (stored + 1) & 0xFFFFFFFF)),
        "magic_zeroed": (4, b"\x00" * 4),
        "header_page_torn": (0x80, b"\xff" * 0x80),
        "stored_sum_erased": (0x2C, b"\xff" * 4),
    }
    for name, (at, value) in edits.items():
        image = bytearray(stock)
        image[at:at + len(value)] = value
        out[name] = bytes(image)
    return out, {"flags": f"0x{flags:08x}", "checksum_length": length, "stored_sum": f"0x{stored:08x}"}


def elf_segments(data):
    if len(data) < 52 or data[:6] != b"\x7fELF\x01\x01" or struct.unpack_from("<H", data, 18)[0] != 243:
        raise ValueError("expected a little-endian RV32 ELF")
    entry, table = struct.unpack_from("<II", data, 24)
    stride, count = struct.unpack_from("<HH", data, 42)
    if stride < 32 or table + stride * count > len(data):
        raise ValueError("truncated ELF program headers")
    segments = []
    for i in range(count):
        kind, offset, address, _, size, memory, _, _ = struct.unpack_from("<8I", data, table + i * stride)
        if kind != 1 or not memory:
            continue
        if size > memory or offset + size > len(data):
            raise ValueError("invalid ELF segment")
        if not Emulator.in_ram(address, memory) and not any(base <= address and address + memory <= base + FLASH_SIZE for base in XIP_ALIASES):
            raise ValueError("ELF image segment outside RAM")
        if any(address < other + length and other < address + memory for other, _, length in segments):
            raise ValueError("overlapping ELF segments")
        segments.append((address, data[offset:offset + size], memory))
    if not any(address <= entry < address + len(contents) for address, contents, _ in segments):
        raise ValueError("ELF entry is not in a loaded segment")
    return entry, segments


def verify_image_elf(image, elf):
    entry, segments = elf_segments(elf)
    if len(image) < 256 or image[:6] != b"KUMI\x01\x00" or zlib.crc32(image[:252]) != int.from_bytes(image[252:256], "little"):
        raise ModelError("invalid signed image header")
    payload = bytearray()
    expected = []
    for address, contents, memory in segments:
        if contents:
            expected.append((1, address, len(payload), len(contents)))
            payload.extend(contents)
        if memory > len(contents):
            expected.append((2, address + len(contents), 0, memory - len(contents)))
    if image[11] != len(expected) or len(expected) > 4 or int.from_bytes(image[20:24], "little") != entry:
        raise ModelError("ELF entry or segments do not match the signed image")
    table = [struct.unpack_from("<4I", image, 24 + 16 * i) for i in range(image[11])]
    length = int.from_bytes(image[16:20], "little")
    if table != expected or length != len(payload) or image[256:256 + length] != payload:
        raise ModelError("ELF bytes do not match the signed image")


def elf_symbols(data):
    table = struct.unpack_from("<I", data, 32)[0]
    stride, count = struct.unpack_from("<HH", data, 46)
    if stride < 40 or table + stride * count > len(data):
        raise ValueError("invalid ELF section table")
    sections = [struct.unpack_from("<10I", data, table + i * stride) for i in range(count)]
    symbols = {}
    for section in sections:
        if section[1] != 2:
            continue
        offset, size, link, entry_size = section[4], section[5], section[6], section[9]
        if link >= len(sections) or entry_size < 16 or offset + size > len(data):
            raise ValueError("invalid ELF symbols")
        strings = sections[link]
        names = data[strings[4]:strings[4] + strings[5]]
        for at in range(offset, offset + size, entry_size):
            name, value, _, _, _, index = struct.unpack_from("<IIIBBH", data, at)
            if name >= len(names) or not index:
                continue
            end = names.find(b"\0", name)
            if end < 0:
                raise ValueError("unterminated ELF symbol")
            symbols[names[name:end].decode("utf-8")] = value
    return symbols


def firmware_flash(stock, stage0, image):
    if len(stock) != FLASH_SIZE or not 0x200 <= len(stage0) <= 0x10000 or not 512 <= len(image) <= 0x100000:
        raise ValueError("invalid stock, stage0 or firmware image size")
    flash = bytearray(stock)
    stage, journal, slot_a, slot_b, copy = (spv5048.STAGE0_REGION, spv5048.JOURNAL_REGION, spv5048.SLOT_A,
                                            spv5048.SLOT_B, spv5048.STOCK_COPY)
    flash[stage[0]:journal[0] + journal[1]] = b"\xff" * (journal[0] + journal[1] - stage[0])
    flash[:len(stage0)] = stage0
    flash[slot_a[0]:slot_a[0] + slot_a[1]] = b"\xff" * slot_a[1]
    flash[slot_a[0]:slot_a[0] + len(image)] = image
    flash[slot_b[0]:slot_b[0] + slot_b[1]] = b"\xff" * slot_b[1]
    flash[copy[0]:copy[0] + copy[1]] = stock[:copy[1]]
    return flash


class InterruptPump:
    def __init__(self, emulator, slice_instructions=200_000):
        self.emu = emulator
        self.slice = slice_instructions

    def __call__(self):
        result = self.emu.resume(self.slice, stop_after_irq=True, stop_on_usb=True)
        if self.emu.faults or result["outcome"]["result"] not in ("irq_returned", "usb_event", "budget_exhausted",
                                                                  "service_ready", "wfi"):
            raise ModelError(f"guest stopped while serving USB: {result['outcome']}, faults={self.emu.faults}")


def usb_host(emulator, pump=None):
    return UsbHost(emulator.usb, pump or InterruptPump(emulator), emulator.system.usb_clock_ready)


class RomBuxx:
    def __init__(self, emulator, host):
        self.emu = emulator
        self.host = host

    def send(self, request):
        self.host.transfer_out(2, request, zero_length_packet=False)
        reply = bytearray()
        while len(reply) < len(request):
            packet = self.host.read_in(1)
            if packet is None:
                raise UsbProtocolError("boot ROM did not answer a BUXX request")
            reply.extend(packet)
            if len(packet) < self.host.max_packet(1, receive=False):
                break
        return bytes(reply)


def service_boot(emulator, budget=60_000_000):
    result = emulator.run(budget)
    if emulator.faults or not result["outcome"] or result["outcome"]["result"] != "service_ready":
        raise ModelError(f"boot ROM did not reach service mode: {result['outcome']}, faults={emulator.faults}")
    host = usb_host(emulator)
    host.attach()
    device, config = host.enumerate()
    return host, device, config


def rom_exchange(emulator, host):
    import ramload
    return ramload.exchange_over(RomBuxx(emulator, host))


def load_through_rom(emulator, host, address, data, entry=None, verify=True):
    import odt
    import ramload
    exchange = rom_exchange(emulator, host)
    if entry is not None and verify:
        return ramload.load_and_run(exchange, address, data, entry, expected_chip=spv5048.CHIP_ID)[0]
    if odt.chip_id(exchange(odt.HANDSHAKE, b"", odt.HANDSHAKE_LENGTH)) != spv5048.CHIP_ID:
        raise ModelError("boot ROM handshake returned another chip ID")
    if verify:
        digest = ramload.load_and_verify(exchange, address, data)
    else:
        ramload.upload(exchange, address, data)
        digest = hashlib.sha256(data).hexdigest()
    if entry is not None:
        ramload.run(exchange, entry)
    return digest


def stock_smoke(emu, budget):
    boot = emu.run(budget, stops={0x480014A4: "stock_main"})
    if emu.faults or boot["outcome"]["result"] != "stock_main":
        return boot
    host = usb_host(emu)
    host.attach()
    descriptor, configuration = host.enumerate(configuration=1)
    configured = host.control(bytes.fromhex("8008000000000100"))
    host.out_packet(5, bytes.fromhex("4b800c00").ljust(64, b"\x00"))
    replies = []
    for _ in range(64):
        packet = host.take_in(6)
        if packet is not None:
            replies.append(packet)
            break
        host.pump()
    versions = [p for p in replies if p.startswith(b"\x4b\x80\x0c\x00")]
    version = versions[0][4:].split(b"\x00", 1)[0].decode("ascii", "replace") if versions else None
    report = emu.report()
    passed = (descriptor[8:12] == bytes.fromhex("61068108") and configured == b"\x01" and version == "V0.1"
              and not emu.faults)
    report.update({"boot": boot["outcome"], "device_descriptor": descriptor.hex(), "configuration": configuration.hex(),
                   "configured": configured.hex(), "stock_version": version,
                   "outcome": {"result": "stock_smoke_passed" if passed else "stock_smoke_failed"}})
    return report


def service_check(emu, budget):
    boot = emu.run(budget)
    report = emu.report()
    if emu.faults or not boot["outcome"] or boot["outcome"]["result"] != "service_ready":
        return report
    host = usb_host(emu)
    host.attach()
    descriptor, _ = host.enumerate(configuration=None)
    report = emu.report()
    report["outcome"] = boot["outcome"]
    report["usb_device_descriptor"] = descriptor.hex()
    report["service_vid_pid"] = f"{int.from_bytes(descriptor[8:10], 'little'):04x}:{int.from_bytes(descriptor[10:12], 'little'):04x}"
    return report


def g3_reports(brom, stock, extra_images, only, budget):
    images, header = scenarios(stock)
    for name, data in extra_images.items():
        if name in images or len(data) > FLASH_SIZE:
            raise ValueError("duplicate image name or image exceeds 4 MiB")
        images[name] = data + b"\xff" * (FLASH_SIZE - len(data))
    if set(only) - images.keys():
        raise ValueError("unknown scenario")
    reports = {}
    for name, image in images.items():
        if only and name not in only:
            continue
        for flag in (0, SERVICE_MODE_BIT):
            emu = Emulator(brom, image, flag)
            expected = "boot_image" if name == "stock" and not flag else "service_ready"
            report = emu.run(budget) if expected == "boot_image" else service_check(emu, budget)
            report["expected"] = expected
            report["ok"] = bool(report["outcome"] and report["outcome"]["result"] == expected and not report["faults"])
            if expected == "service_ready":
                report["ok"] &= report.get("service_vid_pid") == "0666:0888" and report["usb_softconnect"]
            reports[name if not flag else name + "+boot_flag"] = report
    return reports, header


def ram_report(brom, stock, elf, entry, reads):
    import ramload
    emu = Emulator(brom, stock, SERVICE_MODE_BIT)
    host, _, _ = service_boot(emu)
    start, segments = elf_segments(elf)
    for address, contents, memory in segments:
        if Emulator.in_ram(address, memory):
            load_through_rom(emu, host, address, contents + b"\x00" * (memory - len(contents)))
    exchange = rom_exchange(emu, host)
    ramload.run(exchange, start if entry is None else entry)
    memory = {area: ramload.readback(exchange, *[int(s, 0) for s in area.split(":")]).hex() for area in reads}
    report = emu.report()
    report["loader"] = "rom"
    report["memory"] = memory
    report["ok"] = not report["faults"]
    return report


def main(argv=None):
    parser = argparse.ArgumentParser(description="Execute the case's RV32 boot ROM, stock firmware or RAM ELF on the host")
    parser.add_argument("--mode", choices=("g3", "stock", "ram", "runtime", "firmware"), default="g3")
    parser.add_argument("--brom", required=True)
    parser.add_argument("--flash", required=True)
    parser.add_argument("--elf")
    parser.add_argument("--rpc-vectors", help="JSON vectors produced by the kumi-client emulator-vectors example")
    parser.add_argument("--stage0", help="packed stage0 for an in-memory firmware boot fixture")
    parser.add_argument("--firmware-image", help="signed image to place in slot A of the boot fixture")
    parser.add_argument("--entry", type=lambda s: int(s, 0))
    parser.add_argument("--read", action="append", default=[], metavar="ADDRESS:LENGTH")
    parser.add_argument("--image", action="append", default=[], metavar="NAME=PATH")
    parser.add_argument("--only", action="append", default=[])
    parser.add_argument("--budget", type=int, default=100_000_000)
    parser.add_argument("--allow-flash-writes", action="store_true", help="allow writes to the in-memory NOR model")
    parser.add_argument("--output")
    args = parser.parse_args(argv)
    try:
        if args.output and Path(args.output).exists():
            raise ValueError("output already exists")
        brom, stock = Path(args.brom).read_bytes(), Path(args.flash).read_bytes()
        if len(brom) != BROM_SIZE or len(stock) != FLASH_SIZE:
            raise ValueError("expected a 64 KiB boot ROM and a 4 MiB flash dump")
        result = {"brom_sha256": hashlib.sha256(brom).hexdigest(), "flash_sha256": hashlib.sha256(stock).hexdigest(),
                  "model": model_description(), "mode": args.mode, "scenarios": {}}
        if args.mode in ("ram", "runtime", "firmware") and not args.elf:
            raise ValueError(f"{args.mode} mode requires --elf")
        elf = Path(args.elf).read_bytes() if args.elf else None
        if elf is not None:
            result["elf_sha256"] = hashlib.sha256(elf).hexdigest()
        vectors = json.loads(Path(args.rpc_vectors).read_text())["vectors"] if args.rpc_vectors else ()
        if args.mode == "g3":
            extra = {}
            for spec in args.image:
                name, path = spec.split("=", 1)
                extra[name] = Path(path).read_bytes()
            result["scenarios"], result["stock_header"] = g3_reports(brom, stock, extra, args.only, args.budget)
        elif args.mode == "stock":
            result["scenarios"]["stock"] = stock_smoke(Emulator(brom, stock, stop_at_image=False), args.budget)
        elif args.mode == "ram":
            result["scenarios"]["ram"] = ram_report(brom, stock, elf, args.entry, args.read)
        elif args.mode == "runtime":
            from ram_host import runtime_smoke
            result["scenarios"]["runtime"] = runtime_smoke(Emulator(brom, stock, SERVICE_MODE_BIT), elf, vectors)
        else:
            from ram_host import native_smoke
            if not args.stage0 or not args.firmware_image or not args.allow_flash_writes:
                raise ValueError("firmware mode requires --elf, --stage0, --firmware-image and --allow-flash-writes")
            stage0, image = Path(args.stage0).read_bytes(), Path(args.firmware_image).read_bytes()
            flash = firmware_flash(stock, stage0, image)
            result["fixture"] = {"stage0_sha256": hashlib.sha256(stage0).hexdigest(),
                                 "image_sha256": hashlib.sha256(image).hexdigest(),
                                 "flash_sha256": hashlib.sha256(flash).hexdigest(),
                                 "scope": "host-only boot fixture; not an installation or recovery image"}
            result["scenarios"]["firmware"] = native_smoke(Emulator(brom, flash, writable=True, stop_at_image=False),
                                                           elf, vectors, args.budget)
        ok = True
        for key, report in result["scenarios"].items():
            if "ok" not in report:
                report["ok"] = (not report["faults"] and bool(report["outcome"])
                                and report["outcome"]["result"] in ("stock_smoke_passed", "runtime_smoke_passed"))
            ok &= report["ok"]
            print(key, report["outcome"], f"ok={report['ok']}", file=sys.stderr)
        result["ok"] = ok
        encoded = json.dumps(result, indent=2) + "\n"
        if args.output:
            with Path(args.output).open("x") as output:
                output.write(encoded)
        print(encoded)
        return 0 if ok else 1
    except (OSError, ValueError, ModelError) as error:
        parser.exit(2, f"error: {error}\n")


if __name__ == "__main__":
    sys.modules.setdefault("brom_emu", sys.modules[__name__])
    sys.exit(main())

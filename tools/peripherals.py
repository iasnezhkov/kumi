# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

from collections import Counter, deque

from spv5048 import BOOT_FLAG, CHIP_ID, RESET_CONTROL, RESET_REQUEST_BIT


class ModelError(Exception):
    pass


def mask(size):
    return (1 << (8 * size)) - 1


class RegisterFile:
    name = "registers"
    assumptions = ()

    def __init__(self, initial=None, modeled=()):
        self.words = {}
        self.initial = dict(initial or {})
        self.modeled = set(modeled) | set(self.initial)
        self.defaulted = Counter()
        self.unmodeled_writes = Counter()

    def peek(self, aligned):
        return self.words.get(aligned, self.initial.get(aligned, 0))

    def pure_read(self, address):
        return True

    def needs_time(self, address, size, value, floor):
        return False

    def read_word(self, aligned):
        if aligned not in self.words and aligned not in self.initial:
            self.defaulted[aligned] += 1
        return self.peek(aligned)

    def read(self, address, size):
        return (self.read_word(address & ~3) >> (8 * (address & 3))) & mask(size)

    def write(self, address, size, value):
        aligned = address & ~3
        shift = 8 * (address & 3)
        bits = mask(size) << shift
        if aligned not in self.modeled:
            self.unmodeled_writes[aligned] += 1
        self.write_word(aligned, (self.peek(aligned) & ~bits) | ((value << shift) & bits), bits)

    def write_word(self, aligned, value, bits):
        self.words[aligned] = value

    def report(self):
        return {"registers_defaulted_to_zero": {f"0x{a:08x}": n for a, n in sorted(self.defaulted.items())},
                "unmodeled_register_writes": {f"0x{a:08x}": n for a, n in sorted(self.unmodeled_writes.items())}}


class SystemRegisters(RegisterFile):
    name = "system"
    SELF_CLEARING = {0x20090004: 0x4, 0x20160008: 0x1}
    USB_CLOCK, USB_RESET, USB_BIT = 0x20002000, 0x20003000, 1 << 20
    CLOCK_STATUS, CLOCK_READY, CLOCK_BLOCK = 0x200020F4, 0x0000000A, 1 << 27
    USB_PHY, PHY_MASK, PHY_ON = 0x20006000, 0x7, 0x6
    assumptions = (
        "system: 0x20000000=0x49014480, 0x20000010=0x400D0002, 0x20090000=0x0100F700, 0x20090008=0x00000030 and "
        "0x21004010=0x0002120F are values the M1 probe read after boot ROM initialization that the boot ROM never "
        "writes, used as reset values",
        "system: 0x20090004 bit 2 and 0x20160008 bit 0 clear immediately after being set; the boot ROM sets each and "
        "waits for it to clear",
        "system: clock status 0x200020F4 reads 0x0000000A while bit 27 of 0x20002000 and of 0x20003000 are set, "
        "and 0 otherwise: the boot ROM sets both bits before it polls bits 1 and 3 for service-mode USB "
        "(0x0810008F measured in service mode), and a runtime booted from flash without them failed its USB "
        "clock start on the case on 05.11; that bit 27 alone is enough is unmeasured",
        "system: 0x21004000=0x00001700, 0x21004004=0x012D6425, 0x2101004C=0x00000001 and 0x21010050=1023 match "
        "measured boot-time PMU and unplugged LRADC0 reset values on the case",
        "system: setting bit 1 of 0x20003024 requests a reset (stock usage)",
        "system: bit 31 of the boot flag 0x2100801C forces service mode (stock usage); the flag keeps its value "
        "across a software reset (implied by stock usage, not measured by G4) and is zero after power-on",
        "system: the USB controller answers the host only while bit 20 of 0x20002000 and of 0x20003000 are set "
        "(both set after boot ROM initialization in service mode, measured 0x0810008F) and bits 2..0 of 0x20006000 "
        "read 110: the boot ROM sets bits 1 and 2 and clears bit 0 before it connects in service mode, and a runtime "
        "booted from flash that never wrote 0x20006000 did not appear on USB on the case on 05.11 (which bits the "
        "PHY needs is unmeasured)",
        "system: all other registers in the peripheral windows are plain storage reading zero until written; reads "
        "and writes of them are listed in every report",
    )

    def __init__(self, on_reset, boot_flag=0, on_timer3_irq=None):
        super().__init__({BOOT_FLAG: boot_flag, 0x20000000: CHIP_ID, 0x20000010: 0x400D0002, 0x20005000: 0x0000FFFF,
                          0x20090000: 0x0100F700, 0x20090008: 0x00000030, 0x21004000: 0x00001700,
                          0x21004004: 0x012D6425, 0x21004010: 0x0002120F, 0x2101004C: 0x00000001, 0x21010050: 1023},
                         modeled={RESET_CONTROL, *self.SELF_CLEARING, self.USB_CLOCK, self.USB_RESET,
                                  self.CLOCK_STATUS, self.USB_PHY, 0x20002010, 0x2000206C, 0x20005000,
                                  0x20010004, 0x20010008, 0x20010080, 0x20010084, 0x21000004, 0x21001000,
                                  0x21004000, 0x21004004, 0x21010050})
        self.on_reset = on_reset
        self.on_timer3_irq = on_timer3_irq

    def read_word(self, aligned):
        if aligned == self.CLOCK_STATUS:
            return self.CLOCK_READY if self.peek(self.USB_CLOCK) & self.peek(self.USB_RESET) & self.CLOCK_BLOCK else 0
        return super().read_word(aligned) & ~self.SELF_CLEARING.get(aligned, 0)

    def write_word(self, aligned, value, bits):
        if aligned == 0x20010008:
            self.words[aligned] = self.peek(aligned) & ~(value & bits)
            if self.on_timer3_irq is not None:
                self.on_timer3_irq(bool(self.words[aligned] & 0x8))
            return
        super().write_word(aligned, value, bits)
        if aligned == RESET_CONTROL and value & bits & RESET_REQUEST_BIT:
            self.on_reset()

    def timer3_irq_enabled(self):
        return bool(
            (self.peek(self.USB_CLOCK) & (1 << 5))
            and (self.peek(self.USB_RESET) & (1 << 5))
            and (self.peek(0x20010080) & 1)
            and (self.peek(0x20010004) & 8)
        )

    def raise_timer3(self):
        self.words[0x20010008] = self.peek(0x20010008) | 0x8
        if self.on_timer3_irq is not None:
            self.on_timer3_irq(True)

    @property
    def boot_flag(self):
        return self.peek(BOOT_FLAG)

    def usb_clock_ready(self):
        powered = self.peek(self.USB_PHY) & self.PHY_MASK == self.PHY_ON
        return powered and bool(self.peek(self.USB_CLOCK) & self.peek(self.USB_RESET) & self.USB_BIT)


class SdController(RegisterFile):
    name = "sd"
    COMMAND, STATUS = 0x200C002C, 0x200C0044
    assumptions = (
        "SD controller: writing the command register 0x200C002C clears its bit 31 and sets bit 8 of 0x200C0044 "
        "(command sent); 0x200C0044 is write-one-to-clear; no card ever answers, so the boot ROM's response poll "
        "times out",
    )

    def __init__(self):
        super().__init__(modeled={self.COMMAND, self.STATUS})

    def write_word(self, aligned, value, bits):
        if aligned == self.COMMAND:
            super().write_word(aligned, value & ~0x80000000, bits)
            self.words[self.STATUS] = self.peek(self.STATUS) | 0x100
        elif aligned == self.STATUS:
            self.words[aligned] = self.peek(aligned) & ~(value & bits)
        else:
            super().write_word(aligned, value, bits)


class Uart(RegisterFile):
    name = "uart"
    STATUS, TX_STATUS, TX, RX = 0x20040014, 0x20040018, 0x20040020, 0x20040024
    assumptions = (
        "UART: 0x20040014 bit 1 reports a received byte in 0x20040024; the TX FIFO never fills (0x20040018 bit 6 "
        "reads zero) and transmission takes no time",
        "UART: a byte from the keyboard MCU arrives intact only while bits 7..6 of the RX pin register 0x2011003C "
        "are set; otherwise it reads 0xFE (measured 05.11: clearing them in a running runtime turned the keyboard "
        "link into a flood of 0xFE, setting them back restored the frames; that they are a pull-up is a guess)",
    )
    RX_PIN, RX_PULL = 0x2011003C, 0xC0

    def __init__(self, pins=None, on_rx_change=None):
        super().__init__(modeled={self.STATUS, self.TX_STATUS, self.TX, self.RX})
        self.rx = deque()
        self.tx = bytearray()
        self.pins = pins
        self.on_rx_change = on_rx_change

    def push_rx(self, data):
        self.rx.extend(data)
        if self.on_rx_change is not None:
            self.on_rx_change(bool(self.rx))

    def pure_read(self, address):
        return address != self.RX

    def read(self, address, size):
        if address == self.STATUS:
            return 2 if self.rx else 0
        if address == self.TX_STATUS:
            return 0
        if address == self.RX:
            if not self.rx:
                return 0
            byte = self.rx.popleft()
            if self.on_rx_change is not None:
                self.on_rx_change(bool(self.rx))
            pulled = self.pins is None or self.pins.peek(self.RX_PIN) & self.RX_PULL == self.RX_PULL
            return byte if pulled else 0xFE
        return super().read(address, size)

    def write(self, address, size, value):
        if address == self.TX:
            self.tx.append(value & 255)
        elif address not in (self.STATUS, self.TX_STATUS):
            super().write(address, size, value)


class TimerWindow:
    name = "timer_window"
    base = 0x12000000
    size = 0x10000

    def needs_time(self, address, size, value, floor):
        return False
    assumptions = (
        "0x12000000 window: the M1 probe read 0xFFFFFFFF at +8 and +0xC and counted no mtime ticks at +0/+4; every "
        "word of the window reads all ones (measured only at +8 and +0xC) and every write fails as unmeasured",
    )

    def pure_read(self, address):
        return True

    def read(self, address, size):
        return mask(size)

    def write(self, address, size, value):
        raise ModelError(f"unmeasured write to 0x{address:08x}")

    def report(self):
        return {}


class Plic(RegisterFile):
    name = "plic"
    FEATURE, PRIORITY, PENDING, ENABLE, THRESHOLD, CLAIM = 0x10000000, 0x10000004, 0x10001000, 0x10002000, 0x10200000, 0x10200004
    SOURCES = 64
    RESET_PRIORITY = 1
    assumptions = (
        "PLIC: Andes layout (feature 0x10000000, priorities from 0x10000004, pending 0x10001000, target 0 enables "
        "0x10002000, threshold 0x10200000, claim and complete 0x10200004); a source is delivered when enabled, "
        "its priority exceeds the threshold and mstatus.MIE and mie.MEIE are set",
        "PLIC: priorities reset to 1 and the threshold to 0: the boot ROM and the stock firmware enable sources "
        "without writing a priority or threshold and still receive interrupts on hardware",
        "PLIC: in vectored mode (mmisc_ctl.VEC_PLIC, set by the stock firmware; measured clear after boot ROM "
        "initialization) the handler address is read from mtvec + 4 x source and the source is claimed by hardware; "
        "mcause is 0x8000000B in both modes",
        "PLIC: sources are level-triggered; an interrupt line still asserted after completion is pending again",
    )

    def __init__(self, on_change):
        super().__init__(modeled={self.FEATURE, self.THRESHOLD, self.CLAIM})
        self.levels = set()
        self.pending = set()
        self.claimed = set()
        self.on_change = on_change

    def priority(self, source):
        return self.peek(self.PRIORITY + 4 * (source - 1)) if (self.PRIORITY + 4 * (source - 1)) in self.words else self.RESET_PRIORITY

    def enabled(self, source):
        return bool(self.peek(self.ENABLE + 4 * (source // 32)) >> (source % 32) & 1)

    def vectored(self):
        return bool(self.peek(self.FEATURE) & 2)

    def best(self):
        threshold = self.peek(self.THRESHOLD)
        ready = [s for s in self.pending if s not in self.claimed and self.enabled(s) and self.priority(s) > threshold]
        return max(ready, key=lambda s: (self.priority(s), -s)) if ready else None

    def set_level(self, source, level):
        if level:
            self.levels.add(source)
            if source not in self.claimed:
                self.pending.add(source)
        else:
            self.levels.discard(source)
        self.on_change()

    def claim(self):
        source = self.best()
        if source is None:
            return 0
        self.pending.discard(source)
        self.claimed.add(source)
        self.on_change()
        return source

    def complete(self, source):
        self.claimed.discard(source)
        if source in self.levels:
            self.pending.add(source)
        self.on_change()

    def pure_read(self, address):
        return address & ~3 != self.CLAIM

    def is_modeled(self, aligned):
        return (aligned in self.modeled or self.PRIORITY <= aligned < self.PRIORITY + 4 * (self.SOURCES - 1)
                or self.ENABLE <= aligned < self.ENABLE + 4 * (self.SOURCES // 32)
                or self.PENDING <= aligned < self.PENDING + 4 * (self.SOURCES // 32))

    def read_word(self, aligned):
        if aligned == self.CLAIM:
            return self.claim()
        if self.PENDING <= aligned < self.PENDING + 4 * (self.SOURCES // 32):
            base = 32 * ((aligned - self.PENDING) // 4)
            return sum(1 << (s - base) for s in self.pending if base <= s < base + 32)
        if self.PRIORITY <= aligned < self.PRIORITY + 4 * (self.SOURCES - 1):
            return self.priority((aligned - self.PRIORITY) // 4 + 1)
        if self.is_modeled(aligned):
            return self.peek(aligned)
        return super().read_word(aligned)

    def write(self, address, size, value):
        aligned = address & ~3
        if self.is_modeled(aligned):
            self.modeled.add(aligned)
        super().write(address, size, value)

    def write_word(self, aligned, value, bits):
        if aligned == self.CLAIM:
            self.complete(value & 0x3FF)
            return
        if self.PENDING <= aligned < self.PENDING + 4 * (self.SOURCES // 32):
            return
        super().write_word(aligned, value, bits)
        self.on_change()

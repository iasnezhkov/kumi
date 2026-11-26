# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

from collections import deque

from peripherals import ModelError

FLASH_SIZE = 0x400000
PAGE = 256
JEDEC = (0xC8, 0x65, 0x16)
STATUS2 = 0x02
DEVICE_ID = 0x15
READS = {0x03: 4, 0x0B: 5}
ERASES = {0x20: 0x1000, 0x52: 0x8000, 0xD8: 0x10000}
BUSY_MS = {0x02: 1.0, 0x20: 32.0, 0x52: 158.0, 0xD8: 158.0}
SINGLE_BYTE = (0x06, 0x04, 0xB9, 0x66, 0x99)
STATUS_READS = (0x05, 0x35)
KNOWN = set(READS) | set(ERASES) | set(SINGLE_BYTE) | set(STATUS_READS) | {0x02, 0x9F, 0xAB}
DESTRUCTIVE = {0x02, *ERASES, 0x01, 0x31, 0x60, 0xC7}
ASSUMPTIONS = (
    "NOR: GD25 command set and framing per the GigaDevice datasheet: erase needs exactly four bytes, page program "
    "five or more with data wrapping inside the page and only the last 256 bytes kept, single-byte commands exactly "
    "one byte; malformed frames are ignored and listed",
    "NOR: identity C8 65 16 and status-2 0x02 measured through the boot ROM (0125, 0126); device ID 0x15 for 0xAB is "
    "from the datasheet",
    "NOR: busy times are the measured boot ROM reply times, 32 ms for a 4 KiB erase and 158 ms for 64 KiB; 32 KiB "
    "erase is assumed to take as long as 64 KiB and a page program 1 ms (upper bound from the 0121 reply)",
    "NOR: a status read during a busy period returns WIP and advances virtual time to the end of the operation; "
    "other commands during WIP or deep power-down are ignored and listed",
)


class FlashFault(ModelError):
    pass


class Gd25:
    name = "nor"
    assumptions = ASSUMPTIONS

    def __init__(self, image, clock, writable=False):
        if len(image) != FLASH_SIZE:
            raise ValueError("flash image must be exactly 4 MiB")
        self.image = bytearray(image)
        self.clock = clock
        self.writable = writable
        self.transactions = []
        self.writes = []
        self.ignored = []
        self.wel = False
        self.busy_until = 0
        self.powered_down = False
        self.reset_enabled = False
        self.on_change = None
        self.frame = bytearray()

    def busy(self):
        return self.busy_until > self.clock.cycles_floor() and self.clock.cycles() < self.busy_until

    def needs_time(self, floor):
        return self.busy_until > floor or bool(self.frame and self.frame[0] in DESTRUCTIVE)

    def begin(self):
        self.frame = bytearray()

    def status(self):
        busy = self.busy()
        if busy:
            self.clock.skip_to(self.busy_until)
        return (1 if busy else 0) | (2 if self.wel else 0)

    def exchange(self, value):
        self.frame.append(value & 255)
        command, position = self.frame[0], len(self.frame) - 1
        if position == 0 and command not in KNOWN and command not in DESTRUCTIVE:
            raise FlashFault(f"unmodeled flash command 0x{command:02x}")
        if self.powered_down and command != 0xAB:
            return 0xFF
        if command in STATUS_READS and position >= 1:
            return self.status() if command == 0x05 else STATUS2
        if self.busy():
            return 0xFF
        if command in READS and position >= READS[command]:
            address = int.from_bytes(self.frame[1:4], "big") + position - READS[command]
            return self.image[address % FLASH_SIZE]
        if command == 0x9F and 1 <= position <= 3:
            return JEDEC[position - 1]
        if command == 0xAB and position >= 4:
            return DEVICE_ID
        return 0xFF

    def ignore(self, data, reason):
        self.ignored.append({"command": f"0x{data[0]:02x}", "bytes": len(data), "reason": reason})

    def end(self):
        data = bytes(self.frame)
        self.frame = bytearray()
        if not data:
            return
        command = data[0]
        entry = {"command": f"0x{command:02x}", "bytes": len(data)}
        if command in (*READS, 0x02, *ERASES) and len(data) >= 4:
            entry["address"] = f"0x{int.from_bytes(data[1:4], 'big') % FLASH_SIZE:06x}"
        self.transactions.append(entry)
        if self.powered_down:
            if command == 0xAB:
                self.powered_down = False
            else:
                self.ignore(data, "deep power-down")
            return
        if command in STATUS_READS:
            return
        if self.busy():
            self.ignore(data, "write in progress")
            return
        if command in SINGLE_BYTE and len(data) != 1:
            self.ignore(data, "single-byte command with extra bytes")
            return
        if command == 0x06:
            self.wel = True
        elif command == 0x04:
            self.wel = False
        elif command == 0xB9:
            self.powered_down = True
        elif command == 0x66:
            self.reset_enabled = True
            return
        elif command == 0x99:
            if self.reset_enabled:
                self.wel = False
        elif command in DESTRUCTIVE:
            self.destructive(data, entry)
        self.reset_enabled = False

    def destructive(self, data, entry):
        command = data[0]
        if command in (0x01, 0x31, 0x60, 0xC7):
            raise FlashFault(f"unmodeled destructive flash command 0x{command:02x}")
        if command in ERASES and len(data) != 4:
            self.ignore(data, "erase frame is not exactly four bytes")
            return
        if command == 0x02 and len(data) < 5:
            self.ignore(data, "page program without data")
            return
        self.writes.append(entry)
        if not self.writable:
            raise FlashFault("guest attempted a flash write in a read-only run")
        if not self.wel:
            self.ignore(data, "write without WREN")
            return
        address = int.from_bytes(data[1:4], "big") % FLASH_SIZE
        if command == 0x02:
            page = address & ~(PAGE - 1)
            payload = data[4:][-PAGE:]
            start = (address + len(data[4:]) - len(payload)) & (PAGE - 1)
            for i, value in enumerate(payload):
                self.image[page | ((start + i) & (PAGE - 1))] &= value
            changed = (page, PAGE)
        else:
            size = ERASES[command]
            at = address & ~(size - 1)
            self.image[at:at + size] = b"\xff" * size
            changed = (at, size)
        self.wel = False
        self.busy_until = self.clock.cycles() + int(BUSY_MS[command] * self.clock.cycles_per_ms())
        if self.on_change:
            self.on_change(*changed)

    def report(self):
        return {"commands": self.transactions, "writes": self.writes, "ignored": self.ignored,
                "jedec": bytes(JEDEC).hex()}


class SpiController:
    name = "spi"
    base = 0x200A0000
    size = 0x1000
    CONTROL, STATUS, TX, RX = 0x200A0000, 0x200A0004, 0x200A0010, 0x200A0014
    IDLE_STATUS = 0x40500101
    RX_EMPTY = 0x40000000
    assumptions = (
        "SPI controller: status reads the idle value 0x40500101 measured in service mode; bit 30 clears while a "
        "received byte waits in RX, because the boot ROM's transfer routine waits for bit 30 to clear after each TX "
        "write and then reads RX; transfers take no time",
        "SPI controller: control bit 1 drives chip select of the NOR",
    )

    def __init__(self, flash, fallback):
        self.flash = flash
        self.fallback = fallback
        self.selected = False
        self.rx = deque()
        self.max_depth = 0

    def pure_read(self, address):
        return address != self.RX

    def needs_time(self, address, size, value, floor):
        if value is None:
            return False
        if address == self.TX:
            return self.flash.needs_time(floor)
        return address == self.CONTROL and self.selected and not value & 2 and self.flash.needs_time(floor)

    def read(self, address, size):
        if address == self.STATUS:
            value = self.IDLE_STATUS & ~(self.RX_EMPTY if self.rx else 0)
        elif address == self.RX:
            value = self.rx.popleft() if self.rx else 0xFF
        else:
            return self.fallback.read(address, size)
        return value & ((1 << (8 * size)) - 1)

    def write(self, address, size, value):
        if address == self.CONTROL:
            selected = bool(value & 2)
            if selected and not self.selected:
                self.flash.begin()
            elif self.selected and not selected:
                self.flash.end()
            self.selected = selected
            self.fallback.write(address, size, value)
        elif address == self.TX:
            if not self.selected:
                raise FlashFault("SPI transfer with chip select inactive")
            self.rx.append(self.flash.exchange(value))
            self.max_depth = max(self.max_depth, len(self.rx))
        else:
            self.fallback.write(address, size, value)

    def report(self):
        return {"max_rx_depth": self.max_depth}

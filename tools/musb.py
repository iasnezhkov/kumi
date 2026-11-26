# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

from collections import Counter, deque
import struct

from peripherals import ModelError

BASE = 0x200D0000
SIZE = 0x10000
FADDR, POWER, INTRTX, INTRRX, INTRTXE, INTRRXE, INTRUSB, INTRUSBE, FRAME, INDEX = (
    0x00, 0x01, 0x02, 0x04, 0x06, 0x08, 0x0A, 0x0B, 0x0C, 0x0E)
TXMAXP, CSR0, RXMAXP, RXCSR, COUNT, DEVCTL, TXFIFOSZ, RXFIFOSZ, TXFIFOADD, RXFIFOADD = (
    0x10, 0x12, 0x14, 0x16, 0x18, 0x60, 0x62, 0x63, 0x64, 0x66)
FIFO_START, FIFO_END = 0x20, 0x60
POWER_SOFTCONN, POWER_HSENAB, POWER_HSMODE, POWER_RESUME = 0x40, 0x20, 0x10, 0x04
CSR0_RXPKTRDY, CSR0_TXPKTRDY, CSR0_SENTSTALL, CSR0_DATAEND, CSR0_SETUPEND = 0x01, 0x02, 0x04, 0x08, 0x10
CSR0_SENDSTALL, CSR0_SERVICEDRX, CSR0_SERVICEDSETUPEND, CSR0_FLUSH = 0x20, 0x40, 0x80, 0x100
TX_PKTRDY, TX_UNDERRUN, TX_FLUSH, TX_SENDSTALL, TX_SENTSTALL, TX_CLRDATATOG, TX_INCOMP = 0x01, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80
RX_PKTRDY, RX_FLUSH, RX_SENDSTALL, RX_SENTSTALL, RX_CLRDATATOG = 0x01, 0x10, 0x20, 0x40, 0x80
RX_OVERRUN, RX_DATAERROR = 0x04, 0x08
TX_MODE = 0x2000
TX_ISO = RX_ISO = 0x4000
FIFO_DOUBLE = 0x10
USB_SUSPEND, USB_RESUME, USB_RESET = 0x01, 0x02, 0x04
DEVCTL_ATTACHED = 0x99
INDEXED_WORDS = {TXMAXP: 2, CSR0: 2, RXMAXP: 2, RXCSR: 2, COUNT: 2}
INDEXED_FIFO = {TXFIFOSZ: 1, RXFIFOSZ: 1, TXFIFOADD: 2, RXFIFOADD: 2}
MODELED = {FADDR: 1, POWER: 1, INTRTX: 2, INTRRX: 2, INTRTXE: 2, INTRRXE: 2, INTRUSB: 1, INTRUSBE: 1,
           FRAME: 2, INDEX: 1, DEVCTL: 1, **INDEXED_WORDS, **INDEXED_FIFO}
ASSUMPTIONS = (
    "MUSB core: the USB interrupt line (PLIC source 34) is asserted while any IntrTx, IntrRx or IntrUSB flag is "
    "pending; enable registers are stored but not applied, because the boot ROM (IntrTxE=0x0002) and the stock "
    "firmware (IntrTxE=0x004A) clear the EP0 enable bit yet service EP0 only from their interrupt handlers",
    "MUSB core: no SOF interrupts are generated; the frame number counts milliseconds of virtual time while the "
    "host is attached, the device is soft-connected and the bus is not suspended",
    "MUSB core: a bus reset clears FAddr, the CSRs, FIFOs and endpoint FIFO size, address and MaxP registers",
    "MUSB core: the size of the FIFO RAM is not modeled (the stock firmware configures FIFOs up to byte 4552); "
    "overlapping endpoint FIFOs and a MaxP larger than the FIFO are rejected",
    "MUSB core: DevCtl reads 0x99 while a host supplies VBUS (measured in service mode), 0 otherwise",
    "MUSB core: an endpoint number serves one direction; it answers IN tokens only with TxCSR.Mode set and OUT tokens "
    "only with it clear, otherwise the host sees no handshake (the boot ROM sets Mode on its IN endpoint 1; Kumi "
    "without Mode got RX protocol errors on every IN endpoint on the case, 05.11)",
    "MUSB core: the host negotiates high speed when Power.HSEnab is set (measured Power 0x70 in service mode)",
    "MUSB core (Mentor layout, unmeasured on the case): TxCSR bit 14 and RxCSR bit 14 select isochronous transfers; an "
    "isochronous OUT packet is never NAKed: it is loaded, queued as the second packet when RxFIFOsz has the double "
    "buffering bit 4, or dropped with RxCSR.OverRun (bit 2, cleared by writing 0); an isochronous IN token with no "
    "loaded packet gets a zero-length packet and sets TxCSR.UnderRun (bit 2)",
)


class UsbStall(ModelError):
    pass


class UsbProtocolError(ModelError):
    pass


def fifo_capacity(code):
    return (8 << (code & 15)) * (2 if code & 16 else 1)


class Endpoint:
    def __init__(self):
        self.tx_maxp = self.rx_maxp = 0
        self.tx_csr = self.rx_csr = 0
        self.tx_fifo_size = self.rx_fifo_size = 0
        self.tx_fifo_address = self.rx_fifo_address = 0
        self.tx_fill = bytearray()
        self.tx_packets = deque()
        self.rx_data = deque()
        self.rx_pending = deque()
        self.rx_count = 0


class Musb:
    name = "musb"
    assumptions = ASSUMPTIONS

    def __init__(self, clock):
        self.clock = clock
        self.faddr = 0
        self.power = 0
        self.index = 0
        self.intr_tx = self.intr_rx = self.intr_usb = 0
        self.intr_tx_enable = self.intr_rx_enable = self.intr_usb_enable = 0
        self.devctl_written = 0
        self.endpoints = [Endpoint() for _ in range(16)]
        self.attached = False
        self.suspended = False
        self.host_high_speed = True
        self.ep0_dataend = False
        self.ep0_stall = False
        self.ep0_phase = "idle"
        self.frames = 0
        self.sof_since = None
        self.register_writes = 0
        self.fifo_overreads = 0
        self.iso_dropped = 0
        self.remote_wakeups = 0
        self.unmodeled = {}
        self.unmodeled_writes = Counter()
        self.defaulted = Counter()
        self.on_line_change = None
        self.on_device_event = None

    def sof_running(self):
        return self.attached and bool(self.power & POWER_SOFTCONN) and not self.suspended

    def update_sof(self):
        now = self.clock.cycles()
        if self.sof_since is not None:
            self.frames += (now - self.sof_since) // self.clock.cycles_per_ms()
            self.sof_since = self.sof_since + ((now - self.sof_since) // self.clock.cycles_per_ms()) * self.clock.cycles_per_ms()
        if self.sof_running():
            if self.sof_since is None:
                self.sof_since = now
        else:
            self.sof_since = None

    def frame(self):
        self.update_sof()
        return self.frames & 0x7FF

    def line(self):
        return bool(self.intr_tx or self.intr_rx or self.intr_usb)

    def raise_tx(self, ep):
        self.intr_tx |= 1 << ep
        self.notify()

    def raise_rx(self, ep):
        self.intr_rx |= 1 << ep
        self.notify()

    def raise_usb(self, bits):
        self.intr_usb |= bits
        self.notify()

    def device_event(self):
        if self.on_device_event:
            self.on_device_event()

    def pure_read(self, address):
        return address - BASE in (FRAME, FRAME + 1, POWER, FADDR, INDEX)

    def needs_time(self, address, size, value, floor):
        offset = address - BASE
        if value is None:
            return offset < FRAME + 2 and FRAME < offset + size
        return offset <= POWER < offset + size

    def notify(self):
        if self.on_line_change:
            self.on_line_change(self.line())

    def selected(self):
        return self.endpoints[self.index]

    def csr(self, ep, receive=False):
        endpoint = self.endpoints[ep]
        return endpoint.rx_csr if receive else endpoint.tx_csr

    def read_register(self, offset, size):
        ep = self.selected()
        if offset == FADDR:
            return self.faddr
        if offset == POWER:
            return self.power
        if offset == INTRTX:
            value, self.intr_tx = self.intr_tx, 0
            self.notify()
            return value
        if offset == INTRRX:
            value, self.intr_rx = self.intr_rx, 0
            self.notify()
            return value
        if offset == INTRUSB:
            value, self.intr_usb = self.intr_usb, 0
            self.notify()
            return value
        if offset == INTRTXE:
            return self.intr_tx_enable
        if offset == INTRRXE:
            return self.intr_rx_enable
        if offset == INTRUSBE:
            return self.intr_usb_enable
        if offset == FRAME:
            return self.frame()
        if offset == INDEX:
            return self.index
        if offset == DEVCTL:
            return DEVCTL_ATTACHED if self.attached else 0
        if offset == TXMAXP:
            return ep.tx_maxp
        if offset == CSR0:
            return ep.tx_csr
        if offset == RXMAXP:
            return ep.rx_maxp
        if offset == RXCSR:
            return ep.rx_csr
        if offset == COUNT:
            return ep.rx_count
        if offset == TXFIFOSZ:
            return ep.tx_fifo_size
        if offset == RXFIFOSZ:
            return ep.rx_fifo_size
        if offset == TXFIFOADD:
            return ep.tx_fifo_address
        if offset == RXFIFOADD:
            return ep.rx_fifo_address
        raise AssertionError(offset)

    def write_register(self, offset, value):
        ep = self.selected()
        if offset == FADDR:
            changed = self.faddr != value & 0x7F
            self.faddr = value & 0x7F
            if changed:
                self.device_event()
        elif offset == POWER:
            if value & POWER_RESUME and not self.power & POWER_RESUME and self.suspended:
                self.remote_wakeups += 1
            changed = (self.power ^ value) & POWER_SOFTCONN
            self.power = (value & ~(0x02 | 0x08 | POWER_HSMODE)) | (self.power & POWER_HSMODE)
            self.update_sof()
            if changed:
                self.device_event()
        elif offset in (INTRTX, INTRRX, INTRUSB, FRAME):
            pass
        elif offset == INTRTXE:
            self.intr_tx_enable = value
        elif offset == INTRRXE:
            self.intr_rx_enable = value
        elif offset == INTRUSBE:
            self.intr_usb_enable = value
        elif offset == INDEX:
            self.index = value & 15
        elif offset == DEVCTL:
            self.devctl_written = value
        elif offset == TXMAXP:
            ep.tx_maxp = value
        elif offset == CSR0:
            if self.index == 0:
                self.write_csr0(value)
            else:
                self.write_tx_csr(self.index, value)
        elif offset == RXMAXP:
            ep.rx_maxp = value
        elif offset == RXCSR:
            self.write_rx_csr(self.index, value)
        elif offset == COUNT:
            pass
        elif offset == TXFIFOSZ:
            ep.tx_fifo_size = value
        elif offset == RXFIFOSZ:
            ep.rx_fifo_size = value
        elif offset == TXFIFOADD:
            ep.tx_fifo_address = value
        elif offset == RXFIFOADD:
            ep.rx_fifo_address = value

    def write_csr0(self, command):
        ep = self.endpoints[0]
        csr = ep.tx_csr
        if not command & CSR0_SENTSTALL:
            csr &= ~CSR0_SENTSTALL
        if command & CSR0_SERVICEDRX:
            csr &= ~CSR0_RXPKTRDY
            ep.rx_data.clear()
        if command & CSR0_SERVICEDSETUPEND:
            csr &= ~CSR0_SETUPEND
        if command & CSR0_FLUSH:
            ep.tx_fill.clear()
            ep.tx_packets.clear()
            csr &= ~CSR0_TXPKTRDY
        if command & CSR0_SENDSTALL:
            self.ep0_stall = True
        if command & CSR0_TXPKTRDY and not csr & CSR0_TXPKTRDY:
            ep.tx_packets.append((bytes(ep.tx_fill), bool(command & CSR0_DATAEND)))
            ep.tx_fill.clear()
            csr |= CSR0_TXPKTRDY
        if command & CSR0_DATAEND:
            self.ep0_dataend = True
        ep.tx_csr = csr
        if command & (CSR0_TXPKTRDY | CSR0_DATAEND | CSR0_SENDSTALL | CSR0_SERVICEDRX):
            self.device_event()

    def write_tx_csr(self, index, command):
        ep = self.endpoints[index]
        keep = ep.tx_csr & (TX_PKTRDY | TX_UNDERRUN | TX_SENTSTALL | TX_INCOMP | 0x02)
        csr = (command & 0xFF00) | (command & TX_SENDSTALL) | keep
        for bit in (TX_UNDERRUN, TX_SENTSTALL, TX_INCOMP):
            if not command & bit:
                csr &= ~bit
        if command & TX_FLUSH:
            ep.tx_fill.clear()
            ep.tx_packets.clear()
            csr &= ~TX_PKTRDY
        if command & TX_PKTRDY and not csr & TX_PKTRDY:
            ep.tx_packets.append((bytes(ep.tx_fill), False))
            ep.tx_fill.clear()
            csr |= TX_PKTRDY
            self.device_event()
        ep.tx_csr = csr

    def write_rx_csr(self, index, command):
        ep = self.endpoints[index]
        keep = ep.rx_csr & (RX_PKTRDY | RX_SENTSTALL | 0x0E)
        csr = (command & 0xFF00) | (command & RX_SENDSTALL) | keep
        for bit in (RX_PKTRDY, RX_SENTSTALL, RX_OVERRUN):
            if not command & bit:
                csr &= ~bit
        if command & RX_FLUSH or not csr & RX_PKTRDY:
            ep.rx_data.clear()
        if command & RX_FLUSH:
            csr &= ~RX_PKTRDY
            ep.rx_pending.clear()
        if ep.rx_csr & RX_PKTRDY and not csr & RX_PKTRDY:
            self.device_event()
        ep.rx_csr = csr
        if not csr & RX_PKTRDY and ep.rx_pending:
            self.load_rx(index, ep.rx_pending.popleft())

    def load_rx(self, ep_number, data):
        ep = self.endpoints[ep_number]
        ep.rx_data = deque(data)
        ep.rx_count = len(data)
        ep.rx_csr |= RX_PKTRDY
        self.raise_rx(ep_number)

    def iso_out(self, ep_number, data):
        ep = self.endpoints[ep_number]
        if not ep.rx_csr & RX_PKTRDY:
            self.load_rx(ep_number, data)
        elif ep.rx_fifo_size & FIFO_DOUBLE and not ep.rx_pending:
            ep.rx_pending.append(bytes(data))
        else:
            ep.rx_csr |= RX_OVERRUN
            self.iso_dropped += 1
        return True

    def read(self, address, size):
        offset = address - BASE
        if FIFO_START <= offset < FIFO_END:
            ep = self.endpoints[(offset - FIFO_START) // 4]
            value = 0
            for i in range(size):
                if ep.rx_data:
                    value |= ep.rx_data.popleft() << (8 * i)
                else:
                    self.fifo_overreads += 1
            return value
        value = 0
        at = offset
        while at < offset + size:
            register, width = self.register_at(at)
            if register is None:
                key = BASE + at
                if key not in self.unmodeled:
                    self.defaulted[key] += 1
                byte = self.unmodeled.get(key, 0)
                value |= byte << (8 * (at - offset))
                at += 1
                continue
            word = self.read_register(register, width)
            for i in range(width):
                if offset <= register + i < offset + size:
                    value |= ((word >> (8 * i)) & 0xFF) << (8 * (register + i - offset))
            at = register + width
        return value

    def write(self, address, size, value):
        offset = address - BASE
        self.register_writes += 1
        data = value.to_bytes(size, "little")
        if FIFO_START <= offset < FIFO_END:
            self.endpoints[(offset - FIFO_START) // 4].tx_fill.extend(data)
            return
        writes = []
        at = offset
        while at < offset + size:
            register, width = self.register_at(at)
            if register is None:
                self.unmodeled[BASE + at] = data[at - offset]
                self.unmodeled_writes[BASE + at] += 1
                at += 1
                continue
            covered = [register + i for i in range(width) if offset <= register + i < offset + size]
            if len(covered) == width:
                word = int.from_bytes(data[register - offset:register - offset + width], "little")
            else:
                word = self.read_register_quiet(register, width)
                for position in covered:
                    shift = 8 * (position - register)
                    word = (word & ~(0xFF << shift)) | (data[position - offset] << shift)
            writes.append((register, word))
            at = register + width
        for register, word in sorted(writes, key=lambda item: item[0] == INDEX, reverse=True):
            self.write_register(register, word)

    def read_register_quiet(self, register, width):
        if register in (INTRTX, INTRRX, INTRUSB, FRAME):
            return 0
        return self.read_register(register, width)

    @staticmethod
    def register_at(offset):
        for register, width in MODELED.items():
            if register <= offset < register + width:
                return register, width
        return None, 1

    def report(self):
        return {"register_writes": self.register_writes, "fifo_overreads": self.fifo_overreads,
                "isochronous_packets_dropped": self.iso_dropped, "remote_wakeups": self.remote_wakeups,
                "softconnect": bool(self.power & POWER_SOFTCONN), "address": self.faddr,
                "unmodeled_register_writes": {f"0x{a:08x}": n for a, n in sorted(self.unmodeled_writes.items())},
                "registers_defaulted_to_zero": {f"0x{a:08x}": n for a, n in sorted(self.defaulted.items())}}

    def host_attach(self, attached=True):
        self.update_sof()
        self.attached = attached
        self.update_sof()

    def host_bus_reset(self):
        self.update_sof()
        self.suspended = False
        self.faddr = 0
        self.index = 0
        self.endpoints = [Endpoint() for _ in range(16)]
        self.ep0_dataend = False
        self.ep0_stall = False
        self.ep0_phase = "idle"
        hs = bool(self.power & POWER_HSENAB) and self.host_high_speed
        self.power = (self.power & ~POWER_HSMODE) | (POWER_HSMODE if hs else 0)
        self.update_sof()
        self.raise_usb(USB_RESET)

    def host_suspend(self, suspended):
        self.update_sof()
        self.suspended = suspended
        self.update_sof()
        self.raise_usb(USB_SUSPEND if suspended else USB_RESUME)

    def host_setup(self, setup):
        ep = self.endpoints[0]
        if self.ep0_phase != "idle":
            ep.tx_csr |= CSR0_SETUPEND
        ep.tx_csr &= ~(CSR0_TXPKTRDY | CSR0_SENTSTALL)
        ep.tx_packets.clear()
        ep.tx_fill.clear()
        ep.rx_data = deque(setup)
        ep.rx_count = len(setup)
        ep.tx_csr |= CSR0_RXPKTRDY
        self.ep0_dataend = False
        self.ep0_stall = False
        length = struct.unpack_from("<H", setup, 6)[0]
        self.ep0_phase = "in" if setup[0] & 0x80 and length else "out" if length else "nodata"
        self.raise_tx(0)

    def host_in(self, ep_number):
        ep = self.endpoints[ep_number]
        if ep_number == 0:
            if self.ep0_stall:
                return self.send_ep0_stall()
            if ep.tx_csr & CSR0_RXPKTRDY or not ep.tx_packets:
                return None
            data, last = ep.tx_packets.popleft()
            ep.tx_csr &= ~CSR0_TXPKTRDY
            if last:
                self.ep0_phase = "status_out"
            self.raise_tx(0)
            return data
        if not ep.tx_csr & TX_MODE:
            raise UsbProtocolError(f"IN token to endpoint {ep_number} with TxCSR.Mode clear: no handshake")
        if ep.tx_csr & TX_ISO and not ep.tx_packets:
            ep.tx_csr |= TX_UNDERRUN
            return b""
        if ep.tx_csr & TX_SENDSTALL:
            ep.tx_csr |= TX_SENTSTALL
            self.raise_tx(ep_number)
            raise UsbStall(f"endpoint {ep_number} IN is stalled")
        if not ep.tx_packets:
            return None
        data, _ = ep.tx_packets.popleft()
        ep.tx_csr &= ~TX_PKTRDY
        self.raise_tx(ep_number)
        return data

    def host_out(self, ep_number, data):
        ep = self.endpoints[ep_number]
        if ep_number == 0:
            if self.ep0_stall:
                return self.send_ep0_stall()
            if ep.tx_csr & CSR0_RXPKTRDY:
                return False
            ep.rx_data = deque(data)
            ep.rx_count = len(data)
            ep.tx_csr |= CSR0_RXPKTRDY
            self.raise_tx(0)
            return True
        if ep.tx_csr & TX_MODE:
            raise UsbProtocolError(f"OUT token to endpoint {ep_number} with TxCSR.Mode set: no handshake")
        if ep.rx_csr & RX_ISO:
            return self.iso_out(ep_number, data)
        if ep.rx_csr & RX_SENDSTALL:
            ep.rx_csr |= RX_SENTSTALL
            self.raise_rx(ep_number)
            raise UsbStall(f"endpoint {ep_number} OUT is stalled")
        if ep.rx_csr & RX_PKTRDY:
            return False
        ep.rx_data = deque(data)
        ep.rx_count = len(data)
        ep.rx_csr |= RX_PKTRDY
        self.raise_rx(ep_number)
        return True

    def send_ep0_stall(self):
        self.ep0_stall = False
        self.ep0_phase = "idle"
        self.endpoints[0].tx_csr |= CSR0_SENTSTALL
        self.raise_tx(0)
        raise UsbStall("control request stalled")

    def ep0_state(self):
        ep = self.endpoints[0]
        return (f"phase={self.ep0_phase} dataend={self.ep0_dataend} csr0=0x{ep.tx_csr:04x} "
                f"queued_in={len(ep.tx_packets)}")

    def host_status(self):
        ep = self.endpoints[0]
        if self.ep0_stall:
            return self.send_ep0_stall()
        if self.ep0_phase in ("nodata", "out") and ep.tx_packets:
            raise UsbProtocolError("device armed an IN packet in a control transfer without an IN data stage; "
                                   "the MUSB core sends the status handshake itself once DataEnd is set")
        if ep.tx_csr & CSR0_RXPKTRDY or not self.ep0_dataend:
            return False
        self.ep0_dataend = False
        self.ep0_phase = "idle"
        self.raise_tx(0)
        return True

    def host_abandon(self):
        self.ep0_phase = "abandoned"


class UsbHost:
    def __init__(self, device, pump, clock_ready=lambda: True, polls=256):
        self.device = device
        self.pump = pump
        self.clock_ready = clock_ready
        self.polls = polls
        self.address = 0
        self.pending_address = None
        self.control_active = False

    def check_connected(self, address=True):
        if not self.device.power & POWER_SOFTCONN or not self.device.attached:
            raise UsbProtocolError("USB device is disconnected")
        if not self.clock_ready():
            raise UsbProtocolError("USB clock and reset gates are not initialized")
        if address and self.device.faddr != self.address:
            raise UsbProtocolError("USB device address does not match the host")

    def attach(self):
        self.device.host_attach(True)
        self.reset()

    def detach(self):
        self.device.host_attach(False)

    def reset(self):
        self.check_connected(address=False)
        self.device.host_bus_reset()
        self.address = 0
        self.pending_address = None
        self.control_active = False
        self.pump()

    def suspend(self, suspended=True):
        self.device.host_suspend(suspended)
        self.pump()

    def max_packet(self, ep, receive):
        self.check_connected()
        if not 0 <= ep <= 7:
            raise ValueError("invalid endpoint")
        if ep == 0:
            return 64
        windows = [(0, 64)]
        selected = None
        for index in range(1, 8):
            endpoint = self.device.endpoints[index]
            for rx in (False, True):
                maximum = (endpoint.rx_maxp if rx else endpoint.tx_maxp) & 0x7FF
                if not maximum:
                    continue
                code = endpoint.rx_fifo_size if rx else endpoint.tx_fifo_size
                capacity = fifo_capacity(code)
                start = (endpoint.rx_fifo_address if rx else endpoint.tx_fifo_address) * 8
                if maximum > (8 << (code & 15)):
                    raise UsbProtocolError("endpoint MAXP exceeds its FIFO size")
                if any(start < end and begin < start + capacity for begin, end in windows):
                    raise UsbProtocolError("endpoint FIFOs overlap")
                windows.append((start, start + capacity))
                if index == ep and rx == receive:
                    selected = maximum
        if selected is None:
            raise UsbProtocolError(f"endpoint {ep} has no MAXP configuration")
        return selected

    def retry(self, action, what):
        for _ in range(self.polls):
            result = action()
            if result is not None and result is not False:
                return result
            self.pump()
        raise UsbProtocolError(f"{what} timed out")

    def inject_setup(self, setup):
        if len(setup) != 8:
            raise ValueError("USB setup must be exactly 8 bytes")
        self.check_connected()
        self.pending_address = setup[2] if setup[:2] == b"\x00\x05" and setup[3:] == bytes(5) else None
        self.control_active = True
        self.device.host_setup(setup)

    def take_in(self, ep):
        maximum = self.max_packet(ep, receive=False)
        packet = self.device.host_in(ep)
        if packet is not None and len(packet) > maximum:
            raise UsbProtocolError("IN packet exceeds endpoint MAXP")
        return packet

    def read_in(self, ep, polls=None):
        for _ in range(polls or self.polls):
            packet = self.take_in(ep)
            if packet is not None:
                return packet
            self.pump()
        return None

    def out_packet(self, ep, data):
        if len(data) > self.max_packet(ep, receive=True):
            raise ValueError("OUT packet exceeds endpoint MAXP")
        self.retry(lambda: self.device.host_out(ep, data), f"OUT endpoint {ep}")
        self.pump()

    def finish_status(self):
        try:
            self.retry(self.device.host_status, "control status stage")
        except UsbProtocolError as error:
            if "timed out" not in str(error):
                raise
            raise UsbProtocolError(f"control status stage timed out: {self.device.ep0_state()}") from None
        self.control_active = False
        if self.pending_address is not None:
            address, self.pending_address = self.pending_address, None
            self.retry(lambda: self.device.faddr == address or None, "device address update")
            self.address = address
        else:
            self.pump()

    def control(self, setup, payload=b""):
        request_type, _, _, _, length = struct.unpack("<BBHHH", setup)
        if not request_type & 0x80 and length != len(payload):
            raise ValueError("OUT control length does not match payload")
        self.inject_setup(setup)
        self.pump()
        if request_type & 0x80 and length:
            response = bytearray()
            while True:
                packet = self.retry(lambda: self.take_in(0), "control IN")
                response.extend(packet)
                if len(packet) < 64 or len(response) >= length:
                    break
            if len(response) > length:
                raise UsbProtocolError("control IN returned more than wLength")
            self.pump()
            self.finish_status()
            return bytes(response)
        for at in range(0, len(payload), 64):
            self.out_packet(0, payload[at:at + 64])
        self.finish_status()
        return b""

    def enumerate(self, address=1, configuration=1):
        self.reset()
        device = self.control(bytes.fromhex("8006000100001200"))
        self.control(struct.pack("<BBHHH", 0, 5, address, 0, 0))
        head = self.control(bytes.fromhex("8006000200000900"))
        size = int.from_bytes(head[2:4], "little")
        if not 9 <= size <= 4096:
            raise UsbProtocolError("invalid configuration size")
        config = self.control(struct.pack("<BBHHH", 0x80, 6, 0x200, 0, size))
        if len(config) != size or len(device) != 18:
            raise UsbProtocolError("USB enumeration returned truncated descriptors")
        if configuration is not None:
            self.control(struct.pack("<BBHHH", 0, 9, configuration, 0, 0))
        return device, config

    def iso_out(self, ep, data):
        if len(data) > self.max_packet(ep, receive=True):
            raise ValueError("OUT packet exceeds endpoint MAXP")
        self.device.host_out(ep, data)

    def iso_in(self, ep):
        return self.take_in(ep)

    def transfer_out(self, ep, data, zero_length_packet=True):
        maximum = self.max_packet(ep, receive=True)
        for at in range(0, len(data), maximum):
            self.out_packet(ep, data[at:at + maximum])
        if zero_length_packet and len(data) % maximum == 0:
            self.out_packet(ep, b"")

    def transfer_in(self, ep, limit=64):
        response = bytearray()
        maximum = self.max_packet(ep, receive=False)
        for _ in range(limit):
            packet = self.read_in(ep)
            if packet is None:
                raise UsbProtocolError(f"IN endpoint {ep} timed out")
            response.extend(packet)
            if len(packet) < maximum:
                return bytes(response)
            self.pump()
        raise UsbProtocolError(f"IN endpoint {ep} exceeded {limit} packets")

    def drain_in(self, ep, limit=64):
        packets = []
        for _ in range(limit):
            self.pump()
            packet = self.take_in(ep)
            if packet is None:
                break
            packets.append(packet)
        return packets

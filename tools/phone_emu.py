# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import hashlib
import struct
import zlib

from brom_emu import Emulator, ModelError, RomBuxx, elf_symbols, usb_host
import odt
from phone import UsbDevice
from ram_host import RamHost
from spv5048 import SERVICE_MODE_BIT

STOCK_MAIN = 0x480014A4
ROM_CALL_RETURN = 0x82E96
SETTLE_BUDGET = 40_000_000
RETENTION = ("kept", "lost")
STOCK_ENTER = bytes.fromhex("4b01ff").ljust(64, b"\0")
CONTROL_TRANSFER = (0x012C, 0x0124)


def le32(data, offset):
    return struct.unpack_from("<I", data, offset)[0]


def ram_readable(address, size):
    return ((address < 0x200000 and 0 < size <= 0x200000 - address)
            or (0x40000000 <= address < 0x40040000 and 0 < size <= 0x40040000 - address))


def ram_writable(address, size):
    if not ram_readable(address, size):
        return False
    return not (address < 0x102000 and address + size > 0x100000) and not (address < 0x140000 and address + size > 0x138000)


def helper_allows(packet):
    if len(packet) != 512 or packet[:4] != b"BUXX" or packet[6] or le32(packet, 10) != zlib.crc32(packet[:10]):
        return False
    command, length, flags = struct.unpack_from("<H", packet, 4)[0], struct.unpack_from("<H", packet, 8)[0], packet[7]
    end = 14
    if command == 0x110:
        ok = not length and flags == 1
    elif command in (0x123, 0x124, 0x125, 0x126, 0x12B):
        ok = not length and not flags
    elif command in (0x122, 0x128, 0x12A):
        end = 34
        ok = (length == 16 and not flags and le32(packet, 30) == zlib.crc32(packet[14:30]) and le32(packet, 22) == 256
              and not le32(packet, 26))
        address, size = le32(packet, 14), le32(packet, 18)
        if command == 0x122:
            ok = ok and 0x48000000 <= address < 0x48400000 and 0 < size <= 0x48400000 - address
        elif command == 0x128:
            ok = ok and ram_writable(address, size)
        else:
            ok = ok and ram_readable(address, size)
    elif command == 0x129:
        end = 18 + length
        ok = 1 <= length <= 256 and not flags and le32(packet, 14 + length) == zlib.crc32(packet[14:14 + length])
    elif command == 0x12C:
        end = 22
        ok = length == 4 and not flags and le32(packet, 18) == zlib.crc32(packet[14:18]) and ram_writable(le32(packet, 14), 1)
    else:
        ok = False
    return ok and not any(packet[end:])


class EmulatedBridge(odt.Channel):
    def __init__(self, phone, device):
        super().__init__()
        if phone.device != device or phone.mode != "service":
            raise odt.ProtocolError("USB bridge closed: Expected active BROM 0666:0888, HID interface 0, interrupt endpoints 81/02")
        self.phone = phone
        self.device = device
        raw = phone.descriptors
        self.identity = {"usb_path": device.path, "device_descriptor_hex": raw[:18].hex(),
                         "configuration_descriptor_hex": raw[18:].hex(),
                         "descriptor_sha256": hashlib.sha256(raw).hexdigest()}
        self.log = []

    def send(self, request):
        self.phone.settle()
        if self.phone.device != self.device:
            self.log.append("USB OUT: No such device")
            raise odt.ProtocolError("USB bridge closed: USB OUT: No such device")
        if not helper_allows(request):
            self.log.append("Rejected command outside the read/return allowlist")
            raise odt.ProtocolError("USB bridge closed: Rejected command outside the read/return allowlist")
        command = struct.unpack_from("<H", request, 4)[0]
        if command == 0x12C:
            self.phone.watch_entry(le32(request, 14))
        reply = RomBuxx(self.phone.emu, self.phone.host).send(request)
        if command in CONTROL_TRANSFER:
            self.phone.pending = command
        return reply

    def reboot(self):
        from adb_bridge import RETURN_COMMANDS, RETURN_REFUSAL, RETURN_TO_RUNTIME
        packet = self.send(odt.encode_request(RETURN_TO_RUNTIME, b"", RETURN_COMMANDS, RETURN_REFUSAL))
        odt.decode_reply(packet, RETURN_TO_RUNTIME, 0, RETURN_COMMANDS, RETURN_REFUSAL)

    def helper_log(self):
        return "\n".join(self.log)

    def close(self):
        self.phone.settle()


class EmulatedPhone:
    def __init__(self, rom, flash, ram_retention="kept", flag_retention="kept", service=True, runtime_elf=None,
                 budget=100_000_000):
        if ram_retention not in RETENTION or flag_retention not in RETENTION:
            raise ValueError("retention must be 'kept' or 'lost'")
        self.rom = bytes(rom)
        self.flash = bytes(flash)
        self.ram_retention = ram_retention
        self.flag_retention = flag_retention
        self.runtime_elf = runtime_elf
        self.runtime_stops = {elf_symbols(runtime_elf)["kumi_poll"]: "runtime_poll"} if runtime_elf else {}
        self.budget = budget
        self.clock = 0.0
        self.number = 1
        self.boots = []
        self.prompts = []
        self.on_prompt = None
        self.device = None
        self.runtime = None
        self.pending = None
        self.entries = []
        self.start(SERVICE_MODE_BIT if service else 0, None, "power_on")

    def start(self, flag, ram, cause):
        for _ in range(6):
            emu = Emulator(self.rom, self.flash, boot_flag=flag, stop_at_image=False, ram=ram)
            result = emu.run(self.budget, stops={STOCK_MAIN: "stock_main"})
            outcome = (result["outcome"] or {}).get("result")
            if emu.faults:
                raise ModelError(f"case faulted while booting: {emu.faults}")
            if outcome == "reset_requested":
                self.boots.append({"cause": cause, "outcome": "reset_during_boot"})
                flag, ram = self.carried(emu)
                cause = "software_reset"
                continue
            self.emu = emu
            self.host = usb_host(emu)
            self.host.attach()
            if outcome not in ("service_ready", "stock_main"):
                raise ModelError(f"case did not reach USB: {result['outcome']}")
            device, config = self.host.enumerate(configuration=1)
            self.mode = "service" if outcome == "service_ready" else "stock"
            self.attach(device + config, cause)
            return
        raise ModelError("the case kept resetting while booting")

    def watch_entry(self, entry):
        import unicorn
        record = {"entry": entry}
        self.entries.append(record)
        emu = self.emu

        def hit(uc, address, size, user):
            if "a0" not in record:
                record.update(a0=emu.read_reg(10), ra=emu.read_reg(1), sp=emu.read_reg(2))

        emu.uc.hook_add(unicorn.UC_HOOK_CODE, hit, begin=entry, end=entry)
        emu.invalidate(entry, 4)

    def carried(self, emu, flag_kept=None):
        flag = emu.boot_flag() if (self.flag_retention == "kept" if flag_kept is None else flag_kept) else 0
        ram = emu.ram_image() if self.ram_retention == "kept" else None
        return flag, ram

    def attach(self, descriptors, cause):
        self.number += 1
        vid, pid = struct.unpack_from("<HH", descriptors, 8)
        self.descriptors = bytes(descriptors)
        self.device = UsbDevice(vid, pid, 1, self.number, "480", "1-1")
        self.boots.append({"cause": cause, "device": f"{vid:04x}:{pid:04x}", "boot_flag": f"0x{self.emu.boot_flag():08x}"})

    def software_reset(self, flag_kept=None):
        flag, ram = self.carried(self.emu, flag_kept)
        self.device = None
        self.runtime = None
        self.start(flag, ram, "software_reset")

    def power_cycle(self):
        self.device = None
        self.runtime = None
        self.pending = None
        self.start(0, None, "power_on")

    def settle(self):
        command, self.pending = self.pending, None
        if command is None or self.mode != "service":
            return
        stops = {ROM_CALL_RETURN: "rom_resumed", **self.runtime_stops} if command == 0x12C else {}
        result = self.emu.resume(SETTLE_BUDGET, stops=stops)
        outcome = (result["outcome"] or {}).get("result")
        if self.emu.faults:
            raise ModelError(f"case faulted after a control transfer: {self.emu.faults}")
        if outcome == "reset_requested":
            self.software_reset()
        elif outcome == "runtime_poll":
            self.host.detach()
            self.runtime = RamHost(self.emu, self.runtime_elf, result=result)
            identity = self.runtime.enumerate()
            self.mode = "kumi"
            self.attach(bytes.fromhex(identity["device_descriptor"] + identity["configuration"]), "ram_jump")
        elif outcome not in ("rom_resumed", "service_ready", "wfi", "budget_exhausted"):
            raise ModelError(f"case stopped after a control transfer: {result['outcome']}")

    def devices(self):
        self.settle()
        return [self.device] if self.device else []

    def bridge(self, device, seconds, mode="bridge"):
        if mode != "bridge":
            raise ValueError("the emulated phone serves only the bridge mode")
        self.settle()
        return EmulatedBridge(self, device)

    def enter_service(self, device):
        if self.mode != "stock" or device != self.device:
            raise ModelError("the stock firmware is not running on that device")
        self.host.out_packet(5, STOCK_ENTER)
        for _ in range(64):
            try:
                self.host.pump()
            except ModelError:
                pass
            if (self.emu.outcome or {}).get("result") == "reset_requested":
                self.software_reset(flag_kept=True)
                return {"hidraw": "/dev/hidraw0", "helper_output": "Sent fixed 4b 01 ff runtime request"}
        raise ModelError("the stock firmware did not reset after the service request")

    def kumi_frame(self, frame):
        if self.mode != "kumi":
            raise ModelError("Kumi is not running")
        return self.runtime.bulk_request(frame)

    def prompt(self, text):
        self.prompts.append(text)
        if self.on_prompt:
            self.on_prompt()
        return ""

    def wake_lock(self):
        return True

    def hold_awake(self, device):
        return "on"

    def helper_sha256(self):
        return None

    def describe(self):
        return {"transport": "emulator", "ram_retention": self.ram_retention, "boot_flag_retention": self.flag_retention,
                "brom_sha256": hashlib.sha256(self.rom).hexdigest(), "flash_sha256": hashlib.sha256(self.flash).hexdigest(),
                "boots": self.boots}

    def now(self):
        return self.clock

    def sleep(self, seconds):
        self.clock += seconds

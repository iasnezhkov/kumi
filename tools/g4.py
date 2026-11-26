# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
from contextlib import contextmanager
import datetime
import hashlib
import json
from pathlib import Path
import secrets
import struct
import sys

import odt
import phone as phones
from phone import KUMI, SERVICE, STOCK, Timeline, matching, wait_for
import ramload
from spv5048 import CHIP_ID, SERVICE_MODE_BIT

LOAD_ADDRESS = 0x2000
IMAGE_LIMIT = 0x2000
TABLE_MAGIC_OFFSET = 0x18
TABLE_MAGIC = b"KG41"
ENTRIES = {"report": 0x2004, "arm": 0x2008, "disarm": 0x200C, "reset_kumi": 0x2010, "reset_stock": 0x2014}
REPORT_ADDRESS = 0x8400
REPORT_BYTES = 128
REPORT_MAGIC = b"KG4R"
KINDS = {1: "report", 2: "arm", 3: "disarm", 4: "reset_kumi", 5: "reset_stock", 6: "reset_not_taken"}
REGISTERS = [0x20003000, 0x20003020, 0x20003024, *range(0x21008000, 0x21008040, 4), 0x200C1000, 0x200C1004]
MARKERS = {"diag": (0x137D00, 0x300), "ilm": (0xFF00, 0x100)}
RESET_TRIALS = {"software_reset_kumi": "reset_kumi", "software_reset_stock": "reset_stock"}
TRIALS = ("no_reset", *RESET_TRIALS)
DEFAULT_TRIALS = TRIALS
NOT_AVAILABLE = {
    "watchdog_reset": "no watchdog is known to be armed in service mode and its registers are unmeasured; the M1 probe "
                      "layout 3 only samples the timer block, so a watchdog reset cannot be triggered from RAM yet",
    "sleep": "the three-minute disappearance is stock firmware behaviour with an unknown cause; the optional idle trial "
             "only shows whether the case leaves USB while it sits in service mode",
}
EVIDENCE_TRIAL = "software_reset_kumi"


class Abort(RuntimeError):
    pass


def hexword(value):
    return f"0x{value:08x}"


def decode_report(raw):
    if len(raw) < REPORT_BYTES:
        raise ValueError("a G4 record is 128 bytes")
    if raw[:4] != REPORT_MAGIC:
        return None
    words = struct.unpack("<32I", raw[:REPORT_BYTES])
    traps = words[8]
    registers = {hexword(address): None if traps >> i & 1 else hexword(words[9 + i]) for i, address in enumerate(REGISTERS)}
    return {
        "kind": KINDS.get(words[1], f"unknown_{words[1]}"),
        "a0": hexword(words[2]),
        "ra": hexword(words[3]),
        "boot_flag_before": hexword(words[4]),
        "boot_flag_after": hexword(words[5]),
        "bit31_before": bool(words[4] & SERVICE_MODE_BIT),
        "bit31_after": bool(words[5] & SERVICE_MODE_BIT),
        "reset_control": hexword(words[6]),
        "mstatus": hexword(words[7]),
        "registers": registers,
    }


def encode_report(kind, before, after, reset_control=0x05027324, a0=0, ra=0x82E96, mstatus=0x80006088, registers=None):
    words = [0] * 32
    words[0] = struct.unpack("<I", REPORT_MAGIC)[0]
    words[1:8] = [kind, a0, ra, before, after, reset_control, mstatus]
    values = dict(registers or {})
    values.setdefault(0x2100801C, before)
    values.setdefault(0x20003024, reset_control)
    for i, address in enumerate(REGISTERS):
        words[9 + i] = values.get(address, 0)
    return struct.pack("<32I", *words)


def check_image(image):
    if len(image) > IMAGE_LIMIT:
        raise ValueError("the probe image exceeds its 8 KiB window")
    if image[TABLE_MAGIC_OFFSET:TABLE_MAGIC_OFFSET + 4] != TABLE_MAGIC:
        raise ValueError("this probe image has no G4 entry table (build firmware/probe from this tree)")
    return image


def compare(expected, actual):
    same = sum(a == b for a, b in zip(expected, actual))
    first = next((i for i, (a, b) in enumerate(zip(expected, actual)) if a != b), None)
    return {"intact": expected == actual, "matching_bytes": same, "bytes": len(expected),
            "zero_bytes": actual.count(0), "erased_bytes": actual.count(0xFF), "first_difference": first,
            "expected_sha256": hashlib.sha256(expected).hexdigest(), "read_sha256": hashlib.sha256(actual).hexdigest()}


class Session:
    def __init__(self, phone, image, seconds=120, reenumerate=20.0, power_seconds=180.0, nonce=secrets.token_bytes,
                 out=None):
        self.phone = phone
        self.image = check_image(image)
        self.seconds = seconds
        self.reenumerate = reenumerate
        self.power_seconds = power_seconds
        self.nonce = nonce
        self.out = out or (lambda text: print(text, file=sys.stderr, flush=True))
        self.identity = None
        self.power_control = {}

    def service_device(self):
        found = matching(self.phone.devices(), {SERVICE})
        if len(found) > 1:
            raise Abort("more than one boot ROM device is attached")
        return found[0] if found else None

    def require_service(self):
        device = self.service_device()
        if device is None:
            raise Abort("the case is not in boot ROM service mode (0666:0888)")
        return device

    @contextmanager
    def bridge(self, device):
        self.power_control[device.path] = self.phone.hold_awake(device)
        bridge = self.phone.bridge(device, self.seconds)
        try:
            exchange = ramload.exchange_over(bridge)
            info = exchange(odt.HANDSHAKE, b"", odt.HANDSHAKE_LENGTH)
            if odt.chip_id(info) != CHIP_ID:
                raise Abort(f"chip 0x{odt.chip_id(info):08x} is not the SPV5048 0x{CHIP_ID:08x}")
            self.identity = bridge.identity
            yield bridge, exchange
        finally:
            bridge.close()

    def load(self, exchange):
        return ramload.load_and_verify(exchange, LOAD_ADDRESS, self.image)

    def call(self, exchange, name):
        ramload.upload(exchange, REPORT_ADDRESS, bytes(REPORT_BYTES))
        ramload.run(exchange, ENTRIES[name])
        report = decode_report(ramload.readback(exchange, REPORT_ADDRESS, REPORT_BYTES))
        if report is None or report["kind"] != name:
            raise Abort(f"the {name} entry left no record; the probe did not run or did not return")
        return report

    def write_markers(self, exchange):
        expected = {}
        for name, (address, length) in MARKERS.items():
            data = self.nonce(length)
            ramload.upload(exchange, address, data)
            if ramload.readback(exchange, address, length) != data:
                raise Abort(f"the {name} marker did not read back right after writing it")
            expected[name] = data
        return expected

    @staticmethod
    def read_markers(exchange):
        return {name: ramload.readback(exchange, address, length) for name, (address, length) in MARKERS.items()}

    @staticmethod
    def compare_markers(expected, actual):
        return {name: compare(expected[name], actual[name]) for name in expected}

    def enter_service(self, device, timeline):
        self.out(f"case came back as stock {device.path}; entering service mode through the stock request")
        timeline.add("stock_service_request", **self.phone.enter_service(device))
        back = wait_for(self.phone, {SERVICE}, self.reenumerate, gone=device, timeline=timeline)
        if back is None:
            raise Abort("the stock firmware did not enter service mode; unplug the case and plug it back in")
        return back

    def after_return(self, back, expected, timeline, stock_ran):
        with self.bridge(back) as (_, exchange):
            actual = self.read_markers(exchange)
            record = decode_report(ramload.readback(exchange, REPORT_ADDRESS, REPORT_BYTES))
            self.load(exchange)
            registers = self.call(exchange, "report")
        timeline.add("read_back", device=back.path)
        result = {"markers": self.compare_markers(expected, actual), "registers_after": registers,
                  "record_kept_in_ilm": record}
        if stock_ran:
            result["markers_note"] = "the stock firmware ran and a second reset happened before this read"
        return result

    def follow(self, gone, expected, timeline, seconds):
        back = wait_for(self.phone, {SERVICE, STOCK, KUMI}, seconds, gone=gone, timeline=timeline)
        if back is None:
            if gone in self.phone.devices():
                with self.bridge(gone) as (_, exchange):
                    record = decode_report(ramload.readback(exchange, REPORT_ADDRESS, REPORT_BYTES))
                return {"outcome": "reset_not_taken", "record": record}
            raise Abort("the case did not come back on USB; unplug it, plug it back in and rerun from step 1")
        timeline.add("came_back", device=back.describe())
        if back.kind == "boot_rom_service":
            return {"outcome": "service_mode", "bit31_honoured": True, "device": back.describe(),
                    **self.after_return(back, expected, timeline, False)}
        if back.kind == "stock":
            service = self.enter_service(back, timeline)
            return {"outcome": "stock", "bit31_honoured": False, "device": back.describe(),
                    **self.after_return(service, expected, timeline, True)}
        raise Abort(f"the case came back as {back.kind} {back.vid:04x}:{back.pid:04x}")

    def no_reset(self):
        device = self.require_service()
        with self.bridge(device) as (_, exchange):
            self.load(exchange)
            before = self.call(exchange, "report")
            expected = self.write_markers(exchange)
            arm = self.call(exchange, "arm")
            after = self.call(exchange, "report")
            markers = self.compare_markers(expected, self.read_markers(exchange))
        if not all(item["intact"] for item in markers.values()) or not after["bit31_after"]:
            raise Abort("without a reset the markers or bit 31 did not hold; the read and write path is not trustworthy")
        return {"name": "no_reset", "registers_before": before, "arm": arm, "registers_after": after, "markers": markers}

    def reset(self, name):
        entry = RESET_TRIALS[name]
        device = self.require_service()
        timeline = Timeline(self.phone.now)
        with self.bridge(device) as (_, exchange):
            self.load(exchange)
            before = self.call(exchange, "report")
            expected = self.write_markers(exchange)
            timeline.add("markers_written")
            try:
                ramload.run(exchange, ENTRIES[entry])
                replied = True
            except (odt.ProtocolError, TimeoutError) as error:
                replied = f"{type(error).__name__}: {error}"
        timeline.add("reset_entry_called", entry=f"0x{ENTRIES[entry]:04x}", replied=replied)
        self.out(f"{name}: reset requested, waiting up to {self.reenumerate:.0f} s for the case")
        result = self.follow(device, expected, timeline, self.reenumerate)
        return {"name": name, "entry": entry, "registers_before": before, **result, "timeline": timeline.events}

    def power_cycle(self):
        device = self.require_service()
        timeline = Timeline(self.phone.now)
        with self.bridge(device) as (_, exchange):
            self.load(exchange)
            expected = self.write_markers(exchange)
            arm = self.call(exchange, "arm")
        timeline.add("armed")
        self.phone.prompt("OWNER STEP (G4 power cut): unplug the case from the phone, wait 30 seconds so it loses "
                          "power, then plug it back in.")
        timeline.add("owner_confirmed")
        result = self.follow(device, expected, timeline, self.power_seconds)
        result["note"] = "whether the controller lost power depends on the case's own supply, which is unmeasured"
        return {"name": "power_cycle", "arm": arm, **result, "timeline": timeline.events}

    def idle(self, seconds):
        device = self.require_service()
        timeline = Timeline(self.phone.now)
        with self.bridge(device) as (_, exchange):
            self.load(exchange)
            expected = self.write_markers(exchange)
            before = self.call(exchange, "report")
        timeline.add("idle_started", seconds=seconds)
        deadline = self.phone.now() + seconds
        while self.phone.now() < deadline:
            if device not in self.phone.devices():
                timeline.add("left_usb")
                result = self.follow(device, expected, timeline, self.power_seconds)
                return {"name": "idle", "seconds": seconds, "registers_before": before, "left_usb": True, **result,
                        "timeline": timeline.events}
            self.phone.sleep(min(5.0, max(0.0, deadline - self.phone.now())))
        with self.bridge(device) as (_, exchange):
            markers = self.compare_markers(expected, self.read_markers(exchange))
            after = self.call(exchange, "report")
        return {"name": "idle", "seconds": seconds, "registers_before": before, "left_usb": False, "markers": markers,
                "registers_after": after, "timeline": timeline.events}

    def return_to_stock(self):
        attempts = []
        for _ in range(2):
            devices = self.phone.devices()
            stock = matching(devices, {STOCK})
            if stock:
                return {"returned": True, "device": stock[0].describe(), "attempts": attempts}
            device = self.service_device()
            if device is None:
                break
            timeline = Timeline(self.phone.now)
            with self.bridge(device) as (bridge, exchange):
                self.load(exchange)
                disarm = self.call(exchange, "disarm")
                check = self.call(exchange, "report")
                bridge.reboot()
            timeline.add("boot_rom_reboot_sent", bit31_before_reboot=check["bit31_after"])
            back = wait_for(self.phone, {STOCK, SERVICE}, self.reenumerate, gone=device, timeline=timeline)
            attempts.append({"disarm": disarm, "bit31_before_reboot": check["bit31_after"],
                             "came_back": back.describe() if back else None, "timeline": timeline.events})
            if back is not None and back.kind == "stock":
                return {"returned": True, "device": back.describe(), "attempts": attempts}
        return {"returned": False, "attempts": attempts,
                "instruction": "the case did not come back as stock 0661:0881: unplug it and plug it back in; a power-on "
                               "start clears bit 31 in the model and the case boots the stock firmware"}

    def initial(self):
        device = self.require_service()
        with self.bridge(device) as (_, exchange):
            leftovers = self.read_markers(exchange)
            self.load(exchange)
            registers = self.call(exchange, "report")
        return {"device": device.describe(), "identity": self.identity, "registers": registers,
                "marker_areas_before": {name: hashlib.sha256(data).hexdigest() for name, data in leftovers.items()}}

    def run(self, trials, power_cycle=False, idle_seconds=0):
        evidence = {"tool": "tools/g4.py", "format": 1, "started_utc": now_utc(), "phone": self.phone.describe(),
                    "helper_sha256": self.phone.helper_sha256(), "wake_lock_held": self.phone.wake_lock(),
                    "image": {"bytes": len(self.image), "sha256": hashlib.sha256(self.image).hexdigest(),
                              "entries": {name: f"0x{address:04x}" for name, address in ENTRIES.items()}},
                    "markers": {name: {"address": f"0x{address:06x}", "bytes": length}
                                for name, (address, length) in MARKERS.items()},
                    "not_available": NOT_AVAILABLE, "trials": []}
        reached = True
        try:
            evidence["initial"] = self.initial()
            plan = [(name, lambda name=name: self.no_reset() if name == "no_reset" else self.reset(name)) for name in trials]
            if idle_seconds:
                plan.append(("idle", lambda: self.idle(idle_seconds)))
            if power_cycle:
                plan.append(("power_cycle", self.power_cycle))
            for name, action in plan:
                self.out(f"trial {name}")
                evidence["trials"].append(action())
        except Abort as error:
            evidence["aborted"] = str(error)
            reached = self.service_device() is not None or bool(matching(self.phone.devices(), {STOCK}))
        if reached:
            try:
                evidence["return_to_stock"] = self.return_to_stock()
            except Abort as error:
                evidence["return_to_stock"] = {"returned": False, "error": str(error)}
        else:
            evidence["return_to_stock"] = {"returned": False, "instruction": "unplug the case and plug it back in"}
        evidence["usb_power_control"] = self.power_control
        evidence["finished_utc"] = now_utc()
        evidence["verdict"] = verdict(evidence)
        evidence["evidence_line"] = evidence_line(evidence)
        return evidence


def trial(evidence, name):
    return next((item for item in evidence["trials"] if item["name"] == name), None)


def verdict(evidence):
    result = {}
    for name in RESET_TRIALS:
        item = trial(evidence, name)
        if item is None:
            continue
        summary = {"outcome": item.get("outcome")}
        if item.get("outcome") == "service_mode":
            summary.update(bit31_honoured=True, diag_ram_kept=item["markers"]["diag"]["intact"],
                           ilm_kept=item["markers"]["ilm"]["intact"],
                           bit31_after_reset=item["registers_after"]["bit31_after"])
        elif item.get("outcome") == "stock":
            summary.update(bit31_honoured=False)
        result[name] = summary
    primary = result.get(EVIDENCE_TRIAL, {})
    if primary.get("bit31_honoured") and primary.get("diag_ram_kept"):
        gate = "passed"
    elif primary.get("bit31_honoured"):
        gate = "bit31_kept_ram_lost"
    elif primary.get("outcome") in ("stock", "reset_not_taken"):
        gate = "failed"
    else:
        gate = "incomplete"
    result["gate_g4"] = gate
    result["returned_to_stock"] = evidence.get("return_to_stock", {}).get("returned", False)
    return result


def evidence_line(evidence):
    primary = trial(evidence, EVIDENCE_TRIAL)
    if not primary or primary.get("outcome") != "service_mode":
        return None
    seconds = next((event["t"] for event in primary["timeline"] if event["event"] == "came_back"), None)
    diag = "kept" if primary["markers"]["diag"]["intact"] else "lost"
    ilm = "kept" if primary["markers"]["ilm"]["intact"] else "lost"
    parts = [f"tools/g4.py on {evidence['phone'].get('transport', 'unknown')} {evidence['started_utc'][:10]}: "
             f"the kumi_chip::reset register sequence (0x2100801C bit 31 set, then 0x20003024 bit 1) from boot ROM "
             f"service mode came back as 0666:0888 after {seconds} s; RAM 0x137D00..0x138000 {diag}, ILM marker {ilm}"]
    stock = trial(evidence, "software_reset_stock")
    if stock and stock.get("outcome"):
        parts.append(f"stock-style reset: {stock['outcome']}")
    power = trial(evidence, "power_cycle")
    if power and power.get("outcome"):
        parts.append(f"power cut: {power['outcome']}")
    identity = (evidence.get("initial") or {}).get("identity") or {}
    parts.append(f"boot ROM descriptor sha256 {identity.get('descriptor_sha256')}, probe image sha256 "
                 f"{evidence['image']['sha256']}")
    return "; ".join(parts)


def now_utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat(timespec="seconds")


def save(output, evidence):
    with Path(output).open("x") as stream:
        json.dump(evidence, stream, indent=2)
        stream.write("\n")
    digest = hashlib.sha256(Path(output).read_bytes()).hexdigest()
    summary = {"output": str(output), "sha256": digest, "verdict": evidence["verdict"],
               "evidence_line": evidence["evidence_line"]}
    print(json.dumps(summary, indent=2))
    return summary


def parse_trials(text):
    names = [name for name in text.split(",") if name]
    unknown = set(names) - set(TRIALS)
    if unknown:
        raise argparse.ArgumentTypeError(f"unknown trials {sorted(unknown)}; choose from {list(TRIALS)}")
    return names


def add_session_arguments(node):
    node.add_argument("--image", required=True, help="kumi-probe ELF or raw image linked at 0x2000")
    node.add_argument("--output", required=True)
    node.add_argument("--trials", type=parse_trials, default=list(DEFAULT_TRIALS))
    node.add_argument("--power-cycle", action="store_true", help="add the owner power-cut trial (prompts)")
    node.add_argument("--idle-seconds", type=int, default=0)
    node.add_argument("--reenumerate-seconds", type=float, default=20.0)


def main(argv=None):
    parser = argparse.ArgumentParser(description="Kumi gate G4: does bit 31 of 0x2100801C and the diagnostic RAM survive "
                                                 "the resets the case goes through")
    sub = parser.add_subparsers(dest="action", required=True)
    node = sub.add_parser("run", help="measure on the case through the phone helper")
    node.add_argument("--serial", required=True)
    node.add_argument("--helper", required=True)
    node.add_argument("--usb-path", help="expected path of the 0666:0888 device")
    node.add_argument("--deadline-seconds", type=int, default=120)
    node.add_argument("--allow-without-wake-lock", action="store_true")
    add_session_arguments(node)
    node = sub.add_parser("rehearse", help="run the same session against the host emulator")
    node.add_argument("--brom", required=True)
    node.add_argument("--flash", required=True)
    node.add_argument("--ram", choices=("kept", "lost"), required=True)
    node.add_argument("--boot-flag", choices=("kept", "lost"), default="kept")
    add_session_arguments(node)
    node = sub.add_parser("stock", help="recovery: clear bit 31 from service mode, send 0124 and wait for stock")
    node.add_argument("--serial", required=True)
    node.add_argument("--helper", required=True)
    node.add_argument("--image", required=True, help="kumi-probe ELF or raw image linked at 0x2000")
    node = sub.add_parser("decode", help="decode a saved 128-byte G4 record")
    node.add_argument("record")
    args = parser.parse_args(argv)
    if args.action == "decode":
        print(json.dumps(decode_report(Path(args.record).read_bytes()), indent=2))
        return 0
    if args.action == "stock":
        image = ramload.ram_image(Path(args.image).read_bytes(), LOAD_ADDRESS)
        result = Session(phones.AdbPhone(args.serial, args.helper), image).return_to_stock()
        print(json.dumps(result, indent=2))
        return 0 if result["returned"] else 1
    if Path(args.output).exists():
        parser.error(f"{args.output} exists")
    image = ramload.ram_image(Path(args.image).read_bytes(), LOAD_ADDRESS)
    if args.action == "run":
        phone = phones.AdbPhone(args.serial, args.helper)
        if args.usb_path and all(d.path != args.usb_path for d in matching(phone.devices(), {SERVICE})):
            parser.error(f"no 0666:0888 device at {args.usb_path}")
        if not args.allow_without_wake_lock and not phone.wake_lock():
            parser.error("hold a wake lock first: echo kumi-case > /sys/power/wake_lock (the boot ROM stalls after a "
                         "USB suspend)")
        session = Session(phone, image, seconds=args.deadline_seconds, reenumerate=args.reenumerate_seconds)
    else:
        from phone_emu import EmulatedPhone
        phone = EmulatedPhone(Path(args.brom).read_bytes(), Path(args.flash).read_bytes(), args.ram, args.boot_flag)
        phone.on_prompt = phone.power_cycle
        session = Session(phone, image, reenumerate=args.reenumerate_seconds)
    evidence = session.run(args.trials, power_cycle=args.power_cycle, idle_seconds=args.idle_seconds)
    save(args.output, evidence)
    return 0 if evidence["verdict"]["returned_to_stock"] and "aborted" not in evidence else 1


if __name__ == "__main__":
    raise SystemExit(main())

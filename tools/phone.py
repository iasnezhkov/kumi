# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import json
import os
import shlex
import subprocess
import time
from typing import NamedTuple

SERVICE = (0x0666, 0x0888)
STOCK = (0x0661, 0x0881)
KUMI = (0x1209, 0x0001)
NAMES = {SERVICE: "boot_rom_service", STOCK: "stock", KUMI: "kumi"}
WAKE_LOCK = "kumi-case"
LIST_DEVICES = ('for d in /sys/bus/usb/devices/*; do [ -f "$d/idVendor" ] && '
                'echo "$(cat "$d/idVendor") $(cat "$d/idProduct") $(cat "$d/busnum") $(cat "$d/devnum") '
                '$(cat "$d/speed") ${d##*/}"; done; true')
LIST_HIDRAW = 'for h in /sys/class/hidraw/hidraw*; do [ -e "$h/device" ] && echo "${h##*/} $(readlink -f "$h/device")"; done; true'


class PhoneError(RuntimeError):
    pass


class UsbDevice(NamedTuple):
    vid: int
    pid: int
    bus: int
    dev: int
    speed: str = ""
    name: str = ""

    @property
    def path(self):
        return f"/dev/bus/usb/{self.bus:03d}/{self.dev:03d}"

    @property
    def kind(self):
        return NAMES.get((self.vid, self.pid), "other")

    def describe(self):
        return {"id": f"{self.vid:04x}:{self.pid:04x}", "kind": self.kind, "path": self.path, "speed": self.speed,
                "sysfs": self.name}


def parse_devices(text):
    devices = []
    for line in text.splitlines():
        fields = line.split()
        if len(fields) < 4:
            continue
        try:
            vid, pid, bus, dev = int(fields[0], 16), int(fields[1], 16), int(fields[2]), int(fields[3])
        except ValueError:
            continue
        devices.append(UsbDevice(vid, pid, bus, dev, fields[4] if len(fields) > 4 else "",
                                 fields[5] if len(fields) > 5 else ""))
    return devices


def parse_hidraw(text, device):
    for line in text.splitlines():
        fields = line.split()
        if len(fields) == 2 and device.name and f"/{device.name}:" in fields[1]:
            return f"/dev/{fields[0]}"
    return None


def matching(devices, ids):
    return [device for device in devices if (device.vid, device.pid) in ids]


class Timeline:
    def __init__(self, clock):
        self.clock = clock
        self.started = clock()
        self.events = []

    def add(self, event, **facts):
        self.events.append({"t": round(self.clock() - self.started, 2), "event": event, **facts})


def wait_for(phone, ids, seconds, gone=None, poll=0.5, timeline=None):
    deadline = phone.now() + seconds
    seen_gone = gone is None
    last = None
    while True:
        devices = phone.devices()
        snapshot = sorted(f"{d.vid:04x}:{d.pid:04x}@{d.path}" for d in devices)
        if timeline is not None and snapshot != last:
            timeline.add("devices", devices=snapshot)
        last = snapshot
        if gone is not None and gone not in devices:
            seen_gone = True
        found = [d for d in matching(devices, ids) if d != gone]
        if seen_gone and found:
            return found[0]
        if phone.now() >= deadline:
            return None
        phone.sleep(poll)


class AdbPhone:
    def __init__(self, serial, helper, adb=None, runner=subprocess.run):
        self.serial = serial
        self.helper = helper
        self.adb = adb or os.environ.get("KUMI_ADB", "adb")
        self.runner = runner
        self.prompts = []

    def shell(self, command, seconds=30):
        result = self.runner([self.adb, "-s", self.serial, "shell", command], capture_output=True, text=True,
                             timeout=seconds)
        if result.returncode:
            raise PhoneError(f"adb shell failed ({result.returncode}): {result.stderr.strip()}")
        return result.stdout

    def devices(self):
        return parse_devices(self.shell(LIST_DEVICES))

    def bridge(self, device, seconds, mode="bridge"):
        from adb_bridge import Bridge
        return Bridge(self.serial, self.helper, device.path, seconds, mode)

    def enter_service(self, device):
        node = parse_hidraw(self.shell(LIST_HIDRAW), device)
        if node is None:
            raise PhoneError(f"no hidraw node for {device.path}")
        output = self.shell(f"{shlex.quote(self.helper)} enter {shlex.quote(node)} 2>&1")
        return {"hidraw": node, "helper_output": output.strip()}

    def hold_awake(self, device):
        if not device.name or "/" in device.name:
            return None
        control = f"/sys/bus/usb/devices/{device.name}/power/control"
        try:
            return self.shell(f"echo on > {control}; cat {control}").strip()
        except PhoneError as error:
            return str(error)

    def helper_sha256(self):
        output = self.shell(f"sha256sum {shlex.quote(self.helper)}").split()
        return output[0] if output else None

    def wake_lock(self):
        try:
            return WAKE_LOCK in self.shell("cat /sys/power/wake_lock").split()
        except PhoneError:
            return None

    def prompt(self, text):
        self.prompts.append(text)
        return input(f"\n{text}\nPress Enter when done: ")

    def describe(self):
        return {"transport": "adb", "serial": self.serial, "helper": self.helper}

    @staticmethod
    def now():
        return time.monotonic()

    @staticmethod
    def sleep(seconds):
        time.sleep(seconds)


def parse_id(text):
    vid, pid = text.split(":")
    return int(vid, 16), int(pid, 16)


def main(argv=None):
    parser = argparse.ArgumentParser(description="The case on the phone: list it, move it from stock to boot ROM "
                                                 "service mode, wait for a USB id")
    parser.add_argument("--serial", required=True)
    parser.add_argument("--helper", default="/data/local/tmp/spv40-usb")
    sub = parser.add_subparsers(dest="action", required=True)
    sub.add_parser("devices", help="list USB devices with their /dev/bus/usb paths")
    node = sub.add_parser("enter", help="send the stock firmware's service request and wait for 0666:0888")
    node.add_argument("--seconds", type=float, default=20.0)
    node = sub.add_parser("wait", help="wait for a USB id such as 1209:0001")
    node.add_argument("id", type=parse_id)
    node.add_argument("--seconds", type=float, default=30.0)
    args = parser.parse_args(argv)
    phone = AdbPhone(args.serial, args.helper)
    if args.action == "devices":
        print(json.dumps([device.describe() for device in phone.devices()], indent=2))
        return 0
    timeline = Timeline(phone.now)
    if args.action == "enter":
        stock = matching(phone.devices(), {STOCK})
        if len(stock) != 1:
            raise SystemExit("expected exactly one stock 0661:0881 case")
        timeline.add("service_request", **phone.enter_service(stock[0]))
        found = wait_for(phone, {SERVICE}, args.seconds, gone=stock[0], timeline=timeline)
    else:
        found = wait_for(phone, {args.id}, args.seconds, timeline=timeline)
    if found is not None:
        timeline.add("power_control", value=phone.hold_awake(found))
    print(json.dumps({"device": found.describe() if found else None, "timeline": timeline.events}, indent=2))
    return 0 if found else 1


if __name__ == "__main__":
    raise SystemExit(main())

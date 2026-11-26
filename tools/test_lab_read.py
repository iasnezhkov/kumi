# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import struct
import unittest

import lab_read


def record(kind, a=0, b=0, length=0, first=0, second=0):
    packed = kind | (a << 8) | (b << 16) | (length << 24)
    return [0, packed, first, second]


class DecodeRecordTests(unittest.TestCase):
    def test_usb_event_carries_the_event_power_and_devctl(self):
        out = lab_read.decode_record(record(5, a=4, b=0x55, length=0x99))
        self.assertEqual(out["kind"], "usb")
        self.assertEqual(out["event"], "configured")
        self.assertEqual((out["power"], out["devctl"]), ("0x55", "0x99"))

    def test_link_frame_carries_group_command_and_body(self):
        body = b"0.0.1"
        first, second = struct.unpack("<II", body.ljust(8, b"\0"))
        out = lab_read.decode_record(record(3, a=0x80, b=0x11, length=len(body), first=first, second=second))
        self.assertEqual((out["kind"], out["group"], out["command"]), ("link_tx", "0x80", "0x11"))
        self.assertEqual(out["body"], body.hex())

    def test_mark_decodes_the_stage_and_link_error_the_code(self):
        self.assertEqual(lab_read.decode_record(record(10, a=0x12, b=0x34))["stage"], 0x3412)
        self.assertEqual(lab_read.decode_record(record(4, a=2))["error"], "checksum")


class DecodeJournalTests(unittest.TestCase):
    def test_a_wrapped_journal_is_returned_oldest_first(self):
        written = lab_read.JOURNAL_RECORDS + 2
        words = [struct.unpack("<I", b"KLJR")[0], written, 0, 0]
        for slot in range(lab_read.JOURNAL_RECORDS):
            words += record(10, first=slot)
        journal = lab_read.decode_journal(words)
        order = [int(r["first"], 16) for r in journal["records"]]
        self.assertEqual(len(order), lab_read.JOURNAL_RECORDS)
        self.assertEqual(order[0], 2)
        self.assertEqual(order[-1], 1)


if __name__ == "__main__":
    unittest.main()

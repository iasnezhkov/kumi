# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import struct
import zlib

CHIP_ID = 0x49014480
FLASH_SIZE = 0x400000
SECTOR = 0x1000

STAGE0_REGION = (0x000000, 0x10000)
JOURNAL_REGION = (0x010000, 0x2000)
SLOT_A = (0x040000, 0x100000)
SLOT_B = (0x140000, 0x100000)
STOCK_COPY = (0x300000, 0x40000)
SETTINGS = (0x3E0000, 0x20000)
SLOTS = (SLOT_A, SLOT_B)

STAGE0_RAM = (0x128000, 0x137D00)
STAGE0_LOAD_ADDRESS = STAGE0_RAM[0]
STAGE0_DRY_ENTRY = STAGE0_RAM[0] + 4
STAGE0_RAM_IMAGE_OFFSET = 0x200
LAST_CRASH_ADDRESS = 0x137D00
BOOT_INFO_ADDRESS = 0x137E00
DIAG_RECORD_ADDRESS = 0x137F00
DIAG_RECORD_SIZE = 256
STAGE0_REPORT_ADDRESS = BOOT_INFO_ADDRESS + 64
STAGE0_REPORT_SIZE = 64
BOOT_INFO_SIZE = 32
BOOTROM_DATA = (0x100000, 0x102000)
BOOTROM_STACK = (0x138000, 0x140000)
DIAG_AREA = (LAST_CRASH_ADDRESS, DIAG_RECORD_ADDRESS + DIAG_RECORD_SIZE)

BOOT_FLAG = 0x2100801C
SERVICE_MODE_BIT = 0x80000000
RESET_CONTROL = 0x20003024
RESET_REQUEST_BIT = 0x2


def boot_info(raw):
    raw = bytes(raw[:BOOT_INFO_SIZE])
    if len(raw) < BOOT_INFO_SIZE or raw[:4] != b"KBI1" or zlib.crc32(raw[:28]) != struct.unpack_from("<I", raw, 28)[0]:
        return None
    if raw[4] > 1 or raw[5] > 1:
        return None
    return {
        "slot": raw[4],
        "safe_mode": raw[5] == 1,
        "boot_count": struct.unpack_from("<I", raw, 8)[0],
        "generation": struct.unpack_from("<I", raw, 12)[0],
        "image_version": tuple(raw[16:19]),
        "stage0_version": tuple(raw[19:22]),
    }

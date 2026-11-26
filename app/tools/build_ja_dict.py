#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
import pathlib
import struct
import sys
import urllib.request

MAGIC = b"SNAPJA01"
MOZC_BASE = "https://raw.githubusercontent.com/google/mozc/master/src/data/dictionary_oss"
MOZC_FILES = [f"dictionary{i:02d}.txt" for i in range(10)]


def is_hiragana_reading(reading: str) -> bool:
    if not (1 <= len(reading) <= 20):
        return False
    return all(("\u3041" <= ch <= "\u3096") or ch == "ー" for ch in reading)


def fetch_mozc_lines(cache_dir: pathlib.Path) -> list[str]:
    cache_dir.mkdir(parents=True, exist_ok=True)
    lines: list[str] = []
    for name in MOZC_FILES:
        local = cache_dir / name
        if not local.is_file():
            url = f"{MOZC_BASE}/{name}"
            req = urllib.request.Request(url, headers={"User-Agent": "kumi-dict-builder/1.0"})
            with urllib.request.urlopen(req, timeout=30) as resp:
                local.write_bytes(resp.read())
        lines.extend(local.read_text(encoding="utf-8", errors="ignore").splitlines())
    return lines


def compile_ja_dict(lines: list[str], dst: pathlib.Path, max_entries: int) -> int:
    best: dict[tuple[str, str], int] = {}
    for line in lines:
        parts = line.split("\t")
        if len(parts) < 5:
            continue
        reading = parts[0].strip()
        try:
            cost = int(parts[3].strip())
        except ValueError:
            continue
        surface = parts[4].strip()
        if not surface or len(surface) > 20 or not is_hiragana_reading(reading):
            continue
        if any(ord(ch) < 0x20 for ch in surface):
            continue
        if surface == reading and cost > 2500:
            continue
        key = (reading, surface)
        prev = best.get(key)
        if prev is None or cost < prev:
            best[key] = cost

    ranked = []
    for (reading, surface), cost in best.items():
        clamped = max(0, min(9000, cost))
        rank = max(1, min(255, 255 - (clamped * 254 // 9000)))
        ranked.append((cost, len(reading), reading, surface, rank))

    ranked.sort(key=lambda item: (item[0], item[1], item[2], item[3]))
    per_reading: dict[str, int] = {}
    chosen = []
    for cost, _rlen, reading, surface, rank in ranked:
        cnt = per_reading.get(reading, 0)
        if cnt >= 4:
            continue
        per_reading[reading] = cnt + 1
        chosen.append((reading, surface, rank))
        if len(chosen) >= max_entries:
            break

    chosen.sort(key=lambda item: (item[0], -item[2], item[1]))

    pool = bytearray()
    table = bytearray()
    kept = 0
    for reading, surface, rank in chosen:
        rb = reading.encode("utf-8")
        sb = surface.encode("utf-8")
        if len(rb) > 255 or len(sb) > 255:
            continue
        ro = len(pool)
        pool.extend(rb)
        so = len(pool)
        pool.extend(sb)
        table.extend(struct.pack("<IBIBB", ro, len(rb), so, len(sb), rank))
        kept += 1

    dst.parent.mkdir(parents=True, exist_ok=True)
    with dst.open("wb") as handle:
        handle.write(MAGIC)
        handle.write(struct.pack("<I", kept))
        handle.write(table)
        handle.write(pool)
    return kept


def main() -> None:
    if len(sys.argv) != 4:
        print("usage: build_ja_dict.py <cache_dir> <output.snapja> <max_entries>", file=sys.stderr)
        sys.exit(2)
    lines = fetch_mozc_lines(pathlib.Path(sys.argv[1]))
    count = compile_ja_dict(lines, pathlib.Path(sys.argv[2]), int(sys.argv[3]))
    print(f"wrote {count} entries -> {sys.argv[2]}")


if __name__ == "__main__":
    main()

#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
import math
import pathlib
import struct
import sys
import unicodedata

MAGIC = b"SNAPDCT1"

FOLD_MAP = str.maketrans({
    "ё": "е",
    "ъ": "ь",
    "й": "и",
    "ґ": "г",
    "є": "е",
    "ї": "и",
    "і": "и",
    "ß": "s",
})


def fold_key(word: str) -> str:
    lower = word.lower().translate(FOLD_MAP)
    decomposed = unicodedata.normalize("NFD", lower)
    return "".join(ch for ch in decomposed if unicodedata.category(ch) != "Mn")


def is_valid_word(word: str, script: str) -> bool:
    if not (2 <= len(word) <= 28):
        return False
    if script == "cyrillic":
        return all(("\u0400" <= ch <= "\u04ff") or ch in ("-", "'") for ch in word) and any("\u0400" <= ch <= "\u04ff" for ch in word)
    return all(ch.isalpha() or ch in ("-", "'") for ch in word) and word[0].isalpha()


def compile_dict(src: pathlib.Path, dst: pathlib.Path, max_words: int, script: str) -> int:
    raw_entries = []
    seen = set()
    for line in src.read_text(encoding="utf-8", errors="ignore").splitlines():
        parts = line.strip().split()
        if not parts:
            continue
        word = parts[0].strip()
        if not is_valid_word(word, script):
            continue
        low = word.lower()
        if low in seen:
            continue
        seen.add(low)
        freq = int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else 1
        raw_entries.append((low, max(1, freq)))
        if len(raw_entries) >= max_words:
            break

    if not raw_entries:
        raise RuntimeError(f"no valid words parsed from {src}")

    max_log = math.log(raw_entries[0][1] + 1.0)
    min_log = math.log(raw_entries[-1][1] + 1.0)
    span = max(0.001, max_log - min_log)

    entries = []
    for word, freq in raw_entries:
        norm = (math.log(freq + 1.0) - min_log) / span
        rank = max(1, min(255, int(round(1 + norm * 254))))
        entries.append((fold_key(word), -rank, word, rank))

    entries.sort(key=lambda item: (item[0], item[1], item[2]))

    pool = bytearray()
    table = bytearray()
    for _, _, word, rank in entries:
        encoded = word.encode("utf-8")
        if len(encoded) > 255:
            continue
        offset = len(pool)
        pool.extend(encoded)
        table.extend(struct.pack("<IBB", offset, len(encoded), rank))

    count = len(table) // 6
    dst.parent.mkdir(parents=True, exist_ok=True)
    with dst.open("wb") as handle:
        handle.write(MAGIC)
        handle.write(struct.pack("<I", count))
        handle.write(table)
        handle.write(pool)
    return count


def main() -> None:
    if len(sys.argv) != 5:
        print("usage: build_dict.py <input.txt> <output.snapdict> <max_words> <latin|cyrillic>", file=sys.stderr)
        sys.exit(2)
    count = compile_dict(pathlib.Path(sys.argv[1]), pathlib.Path(sys.argv[2]), int(sys.argv[3]), sys.argv[4])
    print(f"wrote {count} words -> {sys.argv[2]}")


if __name__ == "__main__":
    main()

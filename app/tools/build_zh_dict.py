#!/usr/bin/env python3
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
import math
import pathlib
import struct
import sys
import urllib.request

MAGIC = b"SNAPPY01"
PINYIN_SIMP_URL = "https://raw.githubusercontent.com/rime/rime-pinyin-simp/master/pinyin_simp.dict.yaml"
OPENCC_ST_CHARS_URL = "https://raw.githubusercontent.com/BYVoid/OpenCC/master/data/dictionary/STCharacters.txt"
OPENCC_ST_PHRASES_URL = "https://raw.githubusercontent.com/BYVoid/OpenCC/master/data/dictionary/STPhrases.txt"


def fetch(url: str, path: pathlib.Path) -> str:
    if not path.is_file():
        path.parent.mkdir(parents=True, exist_ok=True)
        req = urllib.request.Request(url, headers={"User-Agent": "kumi-dict-builder/1.0"})
        with urllib.request.urlopen(req, timeout=30) as resp:
            path.write_bytes(resp.read())
    return path.read_text(encoding="utf-8", errors="ignore")


def is_cjk(ch: str) -> bool:
    code = ord(ch)
    return (0x4E00 <= code <= 0x9FFF) or (0x3400 <= code <= 0x4DBF)


def parse_opencc_map(text: str) -> dict[str, str]:
    mapping: dict[str, str] = {}
    for line in text.splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "\t" not in line:
            continue
        src, dsts = line.split("\t", 1)
        first = dsts.split()[0].strip()
        if src and first:
            mapping[src] = first
    return mapping


def convert_to_traditional(text: str, phrases: dict[str, str], chars: dict[str, str]) -> str:
    if text in phrases:
        return phrases[text]
    out = []
    i = 0
    n = len(text)
    while i < n:
        matched = None
        for span in range(min(4, n - i), 1, -1):
            sub = text[i : i + span]
            if sub in phrases:
                matched = phrases[sub]
                i += span
                break
        if matched is not None:
            out.append(matched)
        else:
            ch = text[i]
            out.append(chars.get(ch, ch))
            i += 1
    return "".join(out)


def parse_pinyin_simp(yaml_text: str) -> list[tuple[str, str, float]]:
    in_body = "\n..." not in yaml_text
    entries: list[tuple[str, str, float]] = []
    for raw in yaml_text.splitlines():
        line = raw.strip()
        if not in_body:
            if line == "...":
                in_body = True
            continue
        if not line or line.startswith("#"):
            continue
        parts = line.split("\t")
        if len(parts) < 2:
            continue
        hanzi = parts[0].strip()
        if not hanzi or len(hanzi) > 8 or not all(is_cjk(c) for c in hanzi):
            continue
        syllables = [s for s in parts[1].strip().lower().split() if s]
        if not syllables or len(syllables) > 8:
            continue
        compact = "".join(s.replace("ü", "v") for s in syllables)
        if len(compact) > 36 or not all("a" <= c <= "z" for c in compact):
            continue
        weight = 1.0
        if len(parts) >= 3:
            w_raw = parts[2].strip()
            if w_raw.endswith("%"):
                try:
                    weight = float(w_raw[:-1]) * 10.0
                except ValueError:
                    weight = 1.0
            else:
                try:
                    weight = float(w_raw)
                except ValueError:
                    weight = 1.0
        if weight <= 0.0:
            weight = 0.5 if len(hanzi) == 1 else 0.1
        entries.append((compact, hanzi, weight))
    return entries


def write_snappy(entries: list[tuple[str, str, float]], dst: pathlib.Path, max_entries: int) -> int:
    best: dict[tuple[str, str], float] = {}
    for key, hanzi, weight in entries:
        pair = (key, hanzi)
        prev = best.get(pair)
        if prev is None or weight > prev:
            best[pair] = weight

    ranked = sorted(best.items(), key=lambda item: (-item[1], len(item[0][1]), item[0][0], item[0][1]))
    per_key: dict[str, int] = {}
    chosen: list[tuple[str, str, float]] = []
    for (key, hanzi), weight in ranked:
        cap = 10 if len(hanzi) == 1 else 6
        cnt = per_key.get(key, 0)
        if cnt >= cap:
            continue
        per_key[key] = cnt + 1
        chosen.append((key, hanzi, weight))
        if len(chosen) >= max_entries:
            break

    max_log = math.log(chosen[0][2] + 1.0)
    min_log = math.log(chosen[-1][2] + 1.0)
    span = max(0.001, max_log - min_log)

    rows = []
    for key, hanzi, weight in chosen:
        norm = (math.log(weight + 1.0) - min_log) / span
        rank = max(1, min(255, int(round(1 + norm * 254))))
        rows.append((key, -rank, hanzi, rank))
    rows.sort(key=lambda item: (item[0], item[1], item[2]))

    pool = bytearray()
    table = bytearray()
    kept = 0
    for key, _, hanzi, rank in rows:
        kb = key.encode("utf-8")
        hb = hanzi.encode("utf-8")
        if len(kb) > 255 or len(hb) > 255:
            continue
        ko = len(pool)
        pool.extend(kb)
        ho = len(pool)
        pool.extend(hb)
        table.extend(struct.pack("<IBIBB", ko, len(kb), ho, len(hb), rank))
        kept += 1

    dst.parent.mkdir(parents=True, exist_ok=True)
    with dst.open("wb") as handle:
        handle.write(MAGIC)
        handle.write(struct.pack("<I", kept))
        handle.write(table)
        handle.write(pool)
    return kept


def main() -> None:
    if len(sys.argv) != 5:
        print("usage: build_zh_dict.py <cache_dir> <zh_cn.snappy> <zh_tw.snappy> <max_entries>", file=sys.stderr)
        sys.exit(2)
    cache = pathlib.Path(sys.argv[1])
    out_cn = pathlib.Path(sys.argv[2])
    out_tw = pathlib.Path(sys.argv[3])
    max_entries = int(sys.argv[4])

    yaml_text = fetch(PINYIN_SIMP_URL, cache / "pinyin_simp.dict.yaml")
    st_chars = parse_opencc_map(fetch(OPENCC_ST_CHARS_URL, cache / "STCharacters.txt"))
    st_phrases = parse_opencc_map(fetch(OPENCC_ST_PHRASES_URL, cache / "STPhrases.txt"))

    simp = parse_pinyin_simp(yaml_text)
    trad = [(k, convert_to_traditional(h, st_phrases, st_chars), w) for (k, h, w) in simp]

    n_cn = write_snappy(simp, out_cn, max_entries)
    n_tw = write_snappy(trad, out_tw, max_entries)
    print(f"wrote {n_cn} entries -> {out_cn}")
    print(f"wrote {n_tw} entries -> {out_tw}")


if __name__ == "__main__":
    main()

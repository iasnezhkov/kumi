# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
import re
import sys
import xml.etree.ElementTree as ET
from pathlib import Path

USAGE = "usage: check_l10n.py [res-dir] [qualifier ...]  (default: all values-* with strings.xml)"
PLACEHOLDER = re.compile(r"%(?:\d+\$)?[-#+0,(]*\d*(?:\.\d+)?[sdfxXcSDFeEgGbBhH%]")


def load(path):
    root = ET.parse(path).getroot()
    out = {}
    for el in root.findall("string"):
        out[el.get("name")] = (el.get("translatable") != "false", "".join(el.itertext()))
    return out


def escape_errors(text):
    if len(text) > 1 and text[0] == '"' and text[-1] == '"':
        return []
    errs = []
    if re.search(r"(?<!\\)'", text):
        errs.append("unescaped apostrophe")
    if re.search(r'(?<!\\)"', text):
        errs.append("unescaped quote")
    if text[:1] in ("@", "?"):
        errs.append("leading @ or ?")
    return errs


def placeholders(text):
    return sorted(PLACEHOLDER.findall(text))


def main(argv):
    if argv and argv[0] in ("-h", "--help"):
        print(USAGE)
        return 0
    res = Path(argv[0]) if argv else Path(__file__).resolve().parent.parent / "app/src/main/res"
    wanted = argv[1:]
    base = load(res / "values/strings.xml")
    keys = {k: v[1] for k, v in base.items() if v[0]}
    dirs = sorted(
        d for d in res.glob("values-*")
        if (d / "strings.xml").is_file() and (not wanted or d.name.removeprefix("values-") in wanted)
    )
    failed = False
    for d in dirs:
        try:
            tr = load(d / "strings.xml")
        except ET.ParseError as err:
            print(f"{d.name}: XML ERROR {err}")
            failed = True
            continue
        missing = [k for k in keys if k not in tr]
        extra = [k for k in tr if k not in keys]
        bad = [k for k in keys if k in tr and placeholders(keys[k]) != placeholders(tr[k][1])]
        empty = [k for k in keys if k in tr and not tr[k][1].strip()]
        esc = [f"{k} ({', '.join(escape_errors(v[1]))})" for k, v in tr.items() if escape_errors(v[1])]
        status = "OK" if not (missing or extra or bad or empty or esc) else "FAIL"
        print(f"{d.name}: {status} keys={len(tr)}/{len(keys)} missing={len(missing)} extra={len(extra)} placeholder_mismatch={len(bad)} empty={len(empty)} escape={len(esc)}")
        for label, items in (("missing", missing), ("extra", extra), ("placeholder", bad), ("empty", empty), ("escape", esc)):
            for k in items[:20]:
                print(f"  {label}: {k}")
        failed = failed or status == "FAIL"
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import ast
from pathlib import Path
import re
import subprocess
import sys
import xml.etree.ElementTree as ET

ROOT = Path(__file__).resolve().parents[1]
BINARY_SUFFIXES = {".apk", ".so", ".ko", ".img", ".bin", ".jar", ".elf", ".zip", ".a", ".o", ".kumi", ".key", ".jks", ".keystore", ".p12", ".pem"}
BINARY_MAGIC = (b"\x7fELF", b"PK\x03\x04", b"!<arch>\n")
ALLOWED_BINARIES = {
    "app/gradle/wrapper/gradle-wrapper.jar",
    "app/app/src/main/assets/dict/en.snapdict",
    "app/app/src/main/assets/dict/ru.snapdict",
    "app/app/src/main/assets/dict/uk.snapdict",
    "app/app/src/main/assets/dict/ja.snapja",
    "app/app/src/main/assets/dict/ko.snapdict",
    "app/app/src/main/assets/dict/zh_cn.snappy",
    "app/app/src/main/assets/dict/zh_tw.snappy",
}
CYRILLIC_ALLOWED_PREFIXES = (
    "app/app/src/main/res/values-",
    "app/app/src/main/java/org/mindone/snapcase/ime/",
    "app/app/src/test/java/org/mindone/snapcase/ime/",
    "app/app/src/main/assets/dict/",
    "app/tools/build_dict.py",
)
CYRILLIC_ALLOWED_FILES = {
    "app/app/src/main/res/values/strings.xml",
}
ABSOLUTE_PATH = re.compile(r"/(?:Users|home|Volumes)/[^ /\"'`]+/")
FORBIDDEN_INTERNAL = re.compile(
    r"\b(?:Taiki|k6mods|mtkclient|orbstack|mind_one|FACTS\.md)\b|~/lima|Co-Authored-By:",
    re.IGNORECASE,
)
CYRILLIC = re.compile("[\u0400-\u04ff]")
LINK = re.compile(r"\]\(([^)#\s]+)(?:#[^)]*)?\)")
DOCS_REFERENCE = re.compile(r"docs/[A-Za-z0-9_-]+\.md")
TEXT_SUFFIXES = {".rs", ".toml", ".md", ".py", ".yml", ".yaml", ".x", ".txt", ".json", ".kt", ".kts", ".xml", ".sh", ".c", ".h", ".properties", ".pro", ".kcm", ".tsv"}


def tracked_files():
    output = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "--", "."],
        cwd=ROOT, check=True, capture_output=True, text=True,
    ).stdout
    return [ROOT / line for line in output.splitlines() if line and (ROOT / line).is_file()]


def cyrillic_allowed(rel: str) -> bool:
    return rel in CYRILLIC_ALLOWED_FILES or rel.startswith(CYRILLIC_ALLOWED_PREFIXES)


def main():
    problems = []
    for path in tracked_files():
        relative = path.relative_to(ROOT).as_posix()
        data = path.read_bytes()
        if relative in ALLOWED_BINARIES:
            continue
        if path.suffix.lower() in BINARY_SUFFIXES or data.startswith(BINARY_MAGIC):
            problems.append(f"binary file: {relative}")
            continue
        if path.suffix.lower() not in TEXT_SUFFIXES and path.name not in {"LICENSE", ".gitignore"}:
            continue
        try:
            text = data.decode("utf-8")
        except UnicodeDecodeError:
            problems.append(f"not UTF-8 text: {relative}")
            continue
        allow_cyr = cyrillic_allowed(relative)
        for number, line in enumerate(text.splitlines(), 1):
            if ABSOLUTE_PATH.search(line):
                problems.append(f"absolute developer path: {relative}:{number}")
            if relative != "tools/tree-lint.py" and FORBIDDEN_INTERNAL.search(line):
                problems.append(f"internal workspace reference: {relative}:{number}")
            if not allow_cyr and CYRILLIC.search(line):
                problems.append(f"non-English text: {relative}:{number}")
        if path.suffix == ".md":
            for target in LINK.findall(text):
                if "://" in target or target.startswith("mailto:"):
                    continue
                if target.startswith("app/") and not (ROOT / "app").exists():
                    continue
                if not (path.parent / target).exists():
                    problems.append(f"broken link: {relative} -> {target}")
        for reference in DOCS_REFERENCE.findall(text):
            if not (ROOT / reference).exists():
                problems.append(f"reference to a missing document: {relative} -> {reference}")
        if path.suffix == ".py":
            try:
                ast.parse(text, filename=relative)
            except SyntaxError as error:
                problems.append(f"python does not parse: {relative}: {error}")
        if path.suffix == ".xml":
            try:
                ET.fromstring(text)
            except ET.ParseError as error:
                problems.append(f"xml does not parse: {relative}: {error}")
    for problem in problems:
        print(problem)
    if problems:
        print(f"{len(problems)} problem(s)")
        return 1
    print("tree is clean")
    return 0


if __name__ == "__main__":
    sys.exit(main())

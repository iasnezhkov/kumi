# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import argparse
import concurrent.futures
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest

CACHE = "KUMI_EMULATOR_CACHE"


def test_classes(directory):
    suite = unittest.TestLoader().discover(str(directory))
    names = []

    def walk(item):
        if isinstance(item, unittest.TestSuite):
            for child in item:
                walk(child)
        else:
            if type(item).__name__ == "_FailedTest":
                names.append(item.id().rsplit(".", 1)[-1])
                return
            name = f"{type(item).__module__}.{type(item).__qualname__}"
            if name not in names:
                names.append(name)

    walk(suite)
    return names


def run_class(directory, name, environment):
    started = time.monotonic()
    result = subprocess.run([sys.executable, "-m", "unittest", name], cwd=directory, capture_output=True, text=True,
                            env=environment)
    return name, result.returncode, time.monotonic() - started, result.stderr


def summary(output):
    for line in reversed(output.strip().splitlines()):
        if line.startswith(("OK", "FAILED")):
            return line
    return "no result"


def counts(output):
    ran = skipped = 0
    for line in output.splitlines():
        if line.startswith("Ran "):
            ran = int(line.split()[1])
        if "skipped=" in line:
            skipped = int(line.split("skipped=")[1].split(")")[0].split(",")[0])
    return ran, skipped


def main(argv=None):
    parser = argparse.ArgumentParser(description="Run every unittest class under a directory in parallel processes")
    parser.add_argument("directory", type=Path)
    parser.add_argument("--jobs", type=int, default=min(8, os.cpu_count() or 1))
    args = parser.parse_args(argv)
    directory = args.directory.resolve()
    names = test_classes(directory)
    started = time.monotonic()
    failed, ran, skipped = [], 0, 0
    temporary = None if os.environ.get(CACHE) else tempfile.mkdtemp(prefix="kumi-emulator-cache-")
    environment = dict(os.environ, **({CACHE: temporary} if temporary else {}))
    try:
        failed, ran, skipped = run_all(directory, names, args.jobs, environment)
    finally:
        if temporary:
            shutil.rmtree(temporary, ignore_errors=True)
    for name, output in failed:
        print(f"\n==== {name}\n{output}")
    print(f"\n{len(names)} classes, {ran} tests, {skipped} skipped, {len(failed)} failed classes, "
          f"{time.monotonic() - started:.1f}s wall")
    return 1 if failed else 0


def run_all(directory, names, jobs, environment):
    failed, ran, skipped = [], 0, 0
    with concurrent.futures.ThreadPoolExecutor(max_workers=jobs) as pool:
        futures = [pool.submit(run_class, directory, name, environment) for name in names]
        for future in concurrent.futures.as_completed(futures):
            name, code, seconds, output = future.result()
            class_ran, class_skipped = counts(output)
            ran += class_ran
            skipped += class_skipped
            print(f"{seconds:7.1f}s  {summary(output):<28} {name}", flush=True)
            if code:
                failed.append((name, output))
    return failed, ran, skipped


if __name__ == "__main__":
    raise SystemExit(main())

# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0

import os
from pathlib import Path
import unittest

REQUIRE = os.environ.get("KUMI_REQUIRE_EMULATOR") == "1"


def missing(*names):
    return [name for name in names if not os.environ.get(name)]


def requires(*names):
    absent = missing(*names)
    if not absent:
        return lambda item: item
    reason = "set " + ", ".join(absent)
    if REQUIRE:
        def fail(item):
            if isinstance(item, type):
                def setUpClass(cls):
                    raise AssertionError(f"KUMI_REQUIRE_EMULATOR=1 but {reason}")
                item.setUpClass = classmethod(setUpClass)
                return item

            def failing(*args, **kwargs):
                raise AssertionError(f"KUMI_REQUIRE_EMULATOR=1 but {reason}")
            return failing
        return fail
    return unittest.skip(reason)


def read(name):
    return Path(os.environ[name]).read_bytes()

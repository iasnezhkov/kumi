#!/bin/bash
# SPDX-License-Identifier: PolyForm-Noncommercial-1.0.0
set -euo pipefail

usage() {
    local prog
    if [ -d "$(dirname "$0")/kumi" ]; then
        prog="case/check.sh"
        venv="case/kumi/.venv"
    else
        prog="./check.sh"
        venv=".venv"
    fi
    echo "usage: $prog fast|full|app"
    echo "  fast  formatting, clippy, host tests, RV32 builds, tree lint, emulator tests on the synthetic ROM"
    echo "  full  fast plus the private boot ROM and stock dumps: signed images with the RFC 8032 test key, every emulator"
    echo "        test with nothing skipped, the hardware G2 report, and the CLI scenarios with RAM kept and lost;"
    echo "        needs KUMI_BROM_DUMP and KUMI_STOCK_DUMP, and KUMI_G2_REPORT for the byte-for-byte stage0 check"
    echo "  app   Android app unit tests, debug and system APKs and lint, then the Gradle daemons are stopped"
    echo "KUMI_CHECK_OUT names the output directory (a new temporary one by default);"
    echo "KUMI_PYTHON names a Python with the emulator requirements ($venv by default)"
    exit 2
}

[ $# -eq 1 ] || usage
TIER=$1
CASE=$(cd "$(dirname "$0")" && pwd)
if [ -d "$CASE/kumi" ]; then
    K=$CASE/kumi
    HAS_SNAPKIT=1
else
    K=$CASE
    HAS_SNAPKIT=0
fi
C=(cargo)
if command -v rustup >/dev/null; then
    TC_BIN=$(cd "$K" && dirname "$(rustup which cargo 2>/dev/null)" 2>/dev/null || true)
    if [ -n "$TC_BIN" ] && [ -x "$TC_BIN/cargo" ]; then
        export PATH="$TC_BIN:$PATH"
        C=("$TC_BIN/cargo")
    else
        C=(rustup run stable cargo)
    fi
fi
PY=${KUMI_PYTHON:-python3}
[ -z "${KUMI_PYTHON:-}" ] && [ -x "$K/.venv/bin/python" ] && PY=$K/.venv/bin/python
"$PY" -c "import unicorn" 2>/dev/null || { echo "$PY has no unicorn: create $K/.venv (see docs) or set KUMI_PYTHON"; exit 2; }
OUT=${KUMI_CHECK_OUT:-$(mktemp -d "${TMPDIR:-/tmp}/kumi-check.XXXXXX")}
mkdir -p "$OUT"
RFC8032_TEST_SECRET=9d61b19deffd5a60ba844af492ec2cc44449c5697b326919703bac031cae7f60
RFC8032_TEST_PUBLIC=d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a
T=firmware/target/riscv32imc-unknown-none-elf/release
STARTED=$SECONDS

step() { echo "== $1 ($((SECONDS - STARTED)) s)"; }

fast() {
    cd "$K"
    step fmt
    "${C[@]}" fmt --check
    (cd firmware && "${C[@]}" fmt --check)
    step clippy
    "${C[@]}" clippy -q --workspace --all-targets -- -D warnings
    step host-tests
    "${C[@]}" test -q --workspace 2>&1 | awk '/^test result/ {p += $4; f += $6} END {print "passed", p, "failed", f; exit f > 0}'
    step target-build
    "${C[@]}" target-build -q
    step firmware
    (cd firmware && KUMI_PUBLIC_KEY_HEX=$RFC8032_TEST_PUBLIC "${C[@]}" build -q --release \
        && KUMI_PUBLIC_KEY_HEX=$RFC8032_TEST_PUBLIC "${C[@]}" clippy -q --release -- -D warnings)
    cp "$T/kumi-fw" "$OUT/kumi-fw.elf"
    cp "$T/kumi-stage0" "$OUT/kumi-stage0.elf"
    cp "$T/kumi-probe" "$OUT/kumi-probe.elf"
    step firmware-lab
    (cd firmware && "${C[@]}" build -q --release -p kumi-fw --features lab \
        && "${C[@]}" clippy -q --release -p kumi-fw --features lab -- -D warnings)
    cp "$T/kumi-fw" "$OUT/kumi-fw-lab.elf"
    "${C[@]}" run -q --release -p kumi-client --example emulator-vectors > "$OUT/rpc.json"
    step tree-lint
    "$PY" tools/tree-lint.py
    step emulator-synthetic-rom
    KUMI_RUNTIME_ELF="$OUT/kumi-fw.elf" KUMI_LAB_RUNTIME_ELF="$OUT/kumi-fw-lab.elf" KUMI_RPC_VECTORS="$OUT/rpc.json" \
        "$PY" tools/run_tests.py tools
    if [ "$HAS_SNAPKIT" = 1 ] && [ -d "$CASE/tests" ]; then
        step snapkit
        (cd "$CASE" && "$PY" -m unittest discover -q -s tests -t . 2>&1 | tail -1)
    fi
}

full() {
    : "${KUMI_BROM_DUMP:?set KUMI_BROM_DUMP to the 64 KiB boot ROM dump}"
    : "${KUMI_STOCK_DUMP:?set KUMI_STOCK_DUMP to the 4 MiB stock flash dump}"
    fast
    cd "$K"
    step pack
    printf '%s\n' "$RFC8032_TEST_SECRET" > "$OUT/test.key"
    rm -f "$OUT/stage0.bin" "$OUT/runtime.bin" "$OUT/update.bin" "$OUT/runtime-lab.bin"
    "${C[@]}" run -q --release -p kumi-install --bin kumi-pack -- stage0 "$OUT/kumi-stage0.elf" "$OUT/stage0.bin" "$RFC8032_TEST_PUBLIC"
    "${C[@]}" run -q --release -p kumi-install --bin kumi-pack -- image "$OUT/kumi-fw.elf" "$OUT/runtime.bin" "$OUT/test.key" 0.1.0 1
    "${C[@]}" run -q --release -p kumi-install --bin kumi-pack -- image "$OUT/kumi-fw.elf" "$OUT/update.bin" "$OUT/test.key" 0.2.0 2
    "${C[@]}" run -q --release -p kumi-install --bin kumi-pack -- image "$OUT/kumi-fw-lab.elf" "$OUT/runtime-lab.bin" "$OUT/test.key" 0.1.0 1
    KUMI_BOOTED_IMAGE_SIZE=$(wc -c < "$OUT/runtime.bin" | tr -d ' ') \
        "${C[@]}" run -q --release -p kumi-client --example emulator-vectors > "$OUT/rpc-native.json"
    step emulator-real-rom
    KUMI_REQUIRE_EMULATOR=1 KUMI_RUNTIME_ELF="$OUT/kumi-fw.elf" KUMI_RPC_VECTORS="$OUT/rpc.json" \
        KUMI_STAGE0_BIN="$OUT/stage0.bin" KUMI_RUNTIME_IMAGE="$OUT/runtime.bin" KUMI_NATIVE_RPC_VECTORS="$OUT/rpc-native.json" \
        KUMI_LAB_RUNTIME_ELF="$OUT/kumi-fw-lab.elf" KUMI_LAB_RUNTIME_IMAGE="$OUT/runtime-lab.bin" \
        KUMI_STAGE0_ELF="$OUT/kumi-stage0.elf" KUMI_PROBE_ELF="$OUT/kumi-probe.elf" "$PY" tools/run_tests.py tools
    step cli
    export KUMI_EMULATOR_CACHE="$OUT/emulator-cache"
    "${C[@]}" build -q --release -p kumi-cli --bin kumi
    for retention in kept lost; do
        "$PY" - "$OUT" "$PY" "$retention" <<'EOF'
import json
import os
import sys
out, python, retention = sys.argv[1:]
config = {"python": python, "server": "tools/emulator_server.py", "brom": os.environ["KUMI_BROM_DUMP"],
          "flash": os.environ["KUMI_STOCK_DUMP"], "elf": f"{out}/kumi-fw.elf", "stage0": f"{out}/stage0.bin",
          "image": f"{out}/runtime.bin", "retention": retention}
with open(f"{out}/emulator-{retention}.json", "w") as handle:
    json.dump(config, handle, indent=2)
EOF
    done
    target/release/kumi --emulator "$OUT/emulator-kept.json" exercise "$OUT/update.bin" > "$OUT/exercise.json"
    echo "exercise passed"
    for retention in kept lost; do
        target/release/kumi --emulator "$OUT/emulator-$retention.json" exercise-recovery > "$OUT/recovery-$retention.json"
        echo "exercise-recovery passed with RAM $retention"
    done
}

app() {
    cd "$CASE/app"
    if [ -z "${JAVA_HOME:-}" ] && [ -d "/Applications/Android Studio.app/Contents/jbr/Contents/Home" ]; then
        export JAVA_HOME="/Applications/Android Studio.app/Contents/jbr/Contents/Home"
    fi
    step gradle
    status=0
    ./gradlew --console=plain -q testDebugUnitTest assembleDebug lint assembleSystem || status=$?
    ./gradlew --stop >/dev/null
    return $status
}

case $TIER in
    fast) fast ;;
    full) full ;;
    app) app ;;
    *) usage ;;
esac
step "passed, output in $OUT"

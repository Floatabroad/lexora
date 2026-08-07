#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo ">> build (release, inkwell)"
cargo build --release --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" output.ll output.o output' EXIT

pass=0
fail=0
failed=""

for f in tests/equiv/*.lx; do
    name="$(basename "$f" .lx)"

    if ! ./target/release/lexora --backend string-ir "$f" >/dev/null 2>&1; then
        echo "FAIL  $name  (string_ir derlemedi)"
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    llc output.ll -o "$TMP/a.s" 2>/dev/null
    clang -fsanitize=address -no-pie "$TMP/a.s" -o "$TMP/a.sir" 2>/dev/null
    timeout 120 "$TMP/a.sir" >/dev/null 2>"$TMP/sir.err"

    if ! ./target/release/lexora --backend inkwell "$f" >/dev/null 2>&1; then
        echo "FAIL  $name  (inkwell derlemedi)"
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    cc -fsanitize=address output.o -o "$TMP/a.ink" 2>/dev/null
    timeout 120 "$TMP/a.ink" >/dev/null 2>"$TMP/ink.err"

    bad=""
    for e in "$TMP/sir.err" "$TMP/ink.err"; do
        if grep -q "ERROR: AddressSanitizer\|ERROR: LeakSanitizer" "$e"; then
            bad="$bad $(basename "$e" .err)"
            grep -m1 "ERROR: " "$e" | sed 's/^/        /'
        fi
    done
    if [ -n "$bad" ]; then
        echo "FAIL  $name ($bad)"
        fail=$((fail + 1)); failed="$failed $name"
    else
        pass=$((pass + 1))
    fi
done

echo "------------------------------------------"
echo "asan: PASS $pass   FAIL $fail   (her case iki backend, ASan + LSan)"
if [ $fail -ne 0 ]; then
    echo "basarisiz:$failed"
    exit 1
fi

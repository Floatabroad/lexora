#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo ">> build (debug + release, inkwell)"
cargo build --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }
cargo build --release --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }

trap 'rm -f output.ll output.o output' EXIT

pass=0
fail=0
failed=""

for f in tests/robust/*.lx; do
    name="$(basename "$f" .lx)"
    for prof in debug release; do
        for be in string-ir inkwell; do
            err="$(timeout 30 "./target/$prof/lexora" --backend "$be" "$f" 2>&1 >/dev/null)"
            rc=$?
            bad=""
            if [ $rc -ne 0 ] && [ $rc -ne 1 ]; then
                bad="cikis kodu $rc (0 veya 1 olmali)"
            elif printf '%s' "$err" | grep -q "panicked"; then
                bad="derleyici panic"
            elif printf '%s' "$err" | grep -qi "stack overflow"; then
                bad="stack overflow"
            fi
            if [ -n "$bad" ]; then
                echo "FAIL  $name [$prof/$be]  $bad"
                printf '%s\n' "$err" | grep -m1 "panicked at\|stack overflow" | sed 's/^/        /'
                fail=$((fail + 1))
                failed="$failed $name[$prof/$be]"
            else
                pass=$((pass + 1))
            fi
        done
    done
done

echo "------------------------------------------"
echo "robust: PASS $pass   FAIL $fail"
if [ $fail -ne 0 ]; then
    echo "basarisiz:$failed"
    exit 1
fi

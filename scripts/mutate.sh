#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

N="${1:-300}"
START="${SEED:-1}"

echo ">> build (debug + release, inkwell)"
cargo build --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }
cargo build --release --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" output.ll output.o output' EXIT

clean=0
panic=0
crash=0
hang=0
spurious=0
badlist=""

for i in $(seq "$START" $((START + N - 1))); do
    prog="$TMP/m.lx"
    python3 scripts/mutate_gen.py "$i" > "$prog" || { echo "uretici hatasi (seed $i)"; exit 1; }

    err="$(timeout 30 ./target/debug/lexora "$prog" 2>&1 >/dev/null)"
    rc=$?
    if [ $rc -eq 124 ]; then
        echo "FAIL seed $i  (debug takildi, 30sn)"
        hang=$((hang + 1)); badlist="$badlist $i"
    elif printf '%s' "$err" | grep -q "panicked"; then
        echo "FAIL seed $i  (DERLEYICI PANIC)"
        printf '%s\n' "$err" | grep -m1 "panicked at" | sed 's/^/    /'
        cp "$prog" "mutate-fail-$i.lx"
        panic=$((panic + 1)); badlist="$badlist $i"
    elif [ $rc -ne 0 ] && [ $rc -ne 1 ]; then
        echo "FAIL seed $i  (cikis kodu $rc, 0 veya 1 olmali)"
        cp "$prog" "mutate-fail-$i.lx"
        crash=$((crash + 1)); badlist="$badlist $i"
    else
        clean=$((clean + 1))
    fi

    rerr="$(timeout 30 ./target/release/lexora "$prog" 2>&1 >/dev/null)"
    if printf '%s' "$rerr" | grep -q "cok derin"; then
        echo "FAIL seed $i  (SAHTE derinlik hatasi: uretec derin ifade uretmez)"
        cp "$prog" "mutate-fail-$i.lx"
        spurious=$((spurious + 1)); badlist="$badlist $i"
    fi
done

echo "------------------------------------------"
echo "mutasyon: $N program (seed $START..$((START + N - 1)))"
echo "  temiz: $clean   PANIC: $panic   cokme: $crash   takilma: $hang   sahte-derinlik: $spurious"
if [ $((panic + crash + hang + spurious)) -ne 0 ]; then
    echo "basarisiz seed'ler:$badlist  (program mutate-fail-<seed>.lx olarak kaydedildi)"
    exit 1
fi
echo "mutasyon: PASS"

#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

N="${1:-200}"
START="${SEED:-1}"
BIN="target/release/lexora"

echo ">> build (release, inkwell)"
cargo build --release --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" output.ll output.o output' EXIT

compiled=0
rejected=0
bad=0
timeouts=0
badlist=""

RUN_TIMEOUT="${RUN_TIMEOUT:-10}"
RUN_VMEM="${RUN_VMEM:-4000000}"

run_prog() {
    RUN_OUT="$(ulimit -v "$RUN_VMEM"; timeout "$RUN_TIMEOUT" "$1" 2>/dev/null)"
    RUN_CODE=$?
}

for i in $(seq "$START" $((START + N - 1))); do
    prog="$TMP/p.lx"
    python3 scripts/fuzz_gen.py "$i" > "$prog" || { echo "uretici hatasi (seed $i)"; exit 1; }

    if ! "$BIN" --backend string-ir "$prog" >/dev/null 2>"$TMP/tc.err"; then
        rejected=$((rejected + 1)); continue
    fi
    if ! llc output.ll -o "$TMP/a.s" 2>"$TMP/llc.err"; then
        echo "FAIL seed $i  (llc string-ir IR'ini reddetti)"
        sed 's/^/    /' "$TMP/llc.err" | head -3
        cp "$prog" "fuzz-fail-$i.lx"; bad=$((bad + 1)); badlist="$badlist $i"; continue
    fi
    clang -no-pie "$TMP/a.s" -o "$TMP/a.sir" 2>/dev/null
    run_prog "$TMP/a.sir"; sir_out="$RUN_OUT"; sir_code=$RUN_CODE

    if ! "$BIN" --backend inkwell "$prog" >/dev/null 2>"$TMP/ink.err"; then
        echo "FAIL seed $i  (inkwell derleme)"
        sed 's/^/    /' "$TMP/ink.err" | head -3
        cp "$prog" "fuzz-fail-$i.lx"; bad=$((bad + 1)); badlist="$badlist $i"; continue
    fi
    run_prog "./output"; ink_out="$RUN_OUT"; ink_code=$RUN_CODE

    if [ "$sir_code" = 124 ] && [ "$ink_code" = 124 ]; then
        timeouts=$((timeouts + 1)); continue
    fi

    compiled=$((compiled + 1))
    if [ "$sir_out" != "$ink_out" ] || [ "$sir_code" != "$ink_code" ]; then
        echo "FAIL seed $i  (BACKEND SAPMASI)"
        echo "  string-ir (exit $sir_code):"; printf '%s\n' "$sir_out" | sed 's/^/    /' | head -10
        echo "  inkwell   (exit $ink_code):"; printf '%s\n' "$ink_out" | sed 's/^/    /' | head -10
        cp "$prog" "fuzz-fail-$i.lx"
        bad=$((bad + 1)); badlist="$badlist $i"
    fi
done

echo "------------------------------------------"
echo "fuzz: $N program (seed $START..$((START + N - 1)))"
echo "  derlenen: $compiled   TC-red: $rejected   sonlanmayan: $timeouts   SAPMA: $bad"
if [ "$bad" -ne 0 ]; then
    echo "basarisiz seed'ler:$badlist  (program fuzz-fail-<seed>.lx olarak kaydedildi)"
    exit 1
fi
echo "fuzz: PASS"

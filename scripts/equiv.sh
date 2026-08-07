#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CASES="tests/equiv"
BIN="target/release/lexora"
BLESS=0

for arg in "$@"; do
    case "$arg" in
        --bless) BLESS=1 ;;
        *) echo "bilinmeyen arg: $arg (yalnizca --bless)"; exit 2 ;;
    esac
done

echo ">> build (release, inkwell)"
cargo build --release --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" output.ll output.o output' EXIT

run_capture() {
    REPLY_OUT="$("$1" 2>/dev/null)"
    REPLY_CODE=$?
}

pass=0
fail=0
failed=""

for lx in "$CASES"/*.lx; do
    name="$(basename "$lx" .lx)"
    golden="$CASES/$name.out"

    if ! "$BIN" --backend string-ir "$lx" >/dev/null 2>"$TMP/sir.err"; then
        echo "FAIL  $name  (string-ir derleme)"
        sed 's/^/    /' "$TMP/sir.err"
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    if ! llc output.ll -o "$TMP/$name.s" 2>"$TMP/llc.err"; then
        echo "FAIL  $name  (llc)"; sed 's/^/    /' "$TMP/llc.err"
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    clang -no-pie "$TMP/$name.s" -o "$TMP/$name.sir" 2>/dev/null
    run_capture "$TMP/$name.sir"
    sir_out="$REPLY_OUT"; sir_code="$REPLY_CODE"

    if ! llc -O2 output.ll -o "$TMP/$name.o2.s" 2>"$TMP/llc.err"; then
        echo "FAIL  $name  (llc -O2)"; sed 's/^/    /' "$TMP/llc.err"
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    clang -no-pie "$TMP/$name.o2.s" -o "$TMP/$name.o2.sir" 2>/dev/null
    run_capture "$TMP/$name.o2.sir"
    sir2_out="$REPLY_OUT"; sir2_code="$REPLY_CODE"

    if ! "$BIN" --backend inkwell "$lx" >/dev/null 2>"$TMP/ink.err"; then
        echo "FAIL  $name  (inkwell derleme)"
        sed 's/^/    /' "$TMP/ink.err"
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    run_capture "./output"
    ink_out="$REPLY_OUT"; ink_code="$REPLY_CODE"

    if ! "$BIN" --backend inkwell -O2 "$lx" >/dev/null 2>"$TMP/ink.err"; then
        echo "FAIL  $name  (inkwell -O2 derleme)"
        sed 's/^/    /' "$TMP/ink.err"
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    run_capture "./output"
    ink2_out="$REPLY_OUT"; ink2_code="$REPLY_CODE"

    if [ "$BLESS" -eq 1 ]; then
        printf '%s\n' "$sir_out" > "$golden"
        echo "BLESS $name  (exit $sir_code)"
        continue
    fi

    ok=1
    [ "$sir_out" = "$ink_out" ] || ok=0
    [ "$sir_code" = "$ink_code" ] || ok=0
    [ "$sir_out" = "$sir2_out" ] || ok=0
    [ "$sir_code" = "$sir2_code" ] || ok=0
    [ "$sir_out" = "$ink2_out" ] || ok=0
    [ "$sir_code" = "$ink2_code" ] || ok=0
    if [ -f "$golden" ]; then
        [ "$sir_out" = "$(cat "$golden")" ] || ok=0
    else
        ok=0
    fi

    if [ "$ok" -eq 1 ]; then
        echo "PASS  $name  (exit $sir_code, O0+O2)"
        pass=$((pass + 1))
    else
        echo "FAIL  $name"
        echo "  string-ir (exit $sir_code):"; printf '%s\n' "$sir_out" | sed 's/^/    /'
        echo "  inkwell   (exit $ink_code):"; printf '%s\n' "$ink_out" | sed 's/^/    /'
        echo "  string-ir -O2 (exit $sir2_code):"; printf '%s\n' "$sir2_out" | sed 's/^/    /'
        echo "  inkwell -O2   (exit $ink2_code):"; printf '%s\n' "$ink2_out" | sed 's/^/    /'
        if [ -f "$golden" ]; then
            echo "  golden:"; sed 's/^/    /' "$golden"
        else
            echo "  golden yok: $golden (once \`make equiv-bless\`)"
        fi
        fail=$((fail + 1)); failed="$failed $name"
    fi
done

echo "------------------------------------------"
if [ "$BLESS" -eq 1 ]; then
    echo "golden dosyalari guncellendi: $CASES/"
    exit 0
fi
echo "PASS: $pass   FAIL: $fail"
if [ "$fail" -ne 0 ]; then
    echo "basarisiz:$failed"
    exit 1
fi

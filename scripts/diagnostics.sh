#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

CASES="tests/diagnostics"
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

pass=0
fail=0
failed=""

for lx in "$CASES"/*.lx; do
    name="$(basename "$lx" .lx)"
    golden="$CASES/$name.stderr"

    "$BIN" "$lx" >/dev/null 2>"$TMP/err"
    code=$?
    err="$(cat "$TMP/err")"

    if [ "$BLESS" -eq 1 ]; then
        printf '%s\n' "$err" > "$golden"
        echo "BLESS $name  (exit $code)"
        continue
    fi

    ok=1
    [ "$code" = "1" ] || ok=0
    if [ -f "$golden" ]; then
        [ "$err" = "$(cat "$golden")" ] || ok=0
    else
        ok=0
    fi

    if [ "$ok" -eq 1 ]; then
        echo "PASS  $name  (exit $code)"
        pass=$((pass + 1))
    else
        echo "FAIL  $name  (exit $code)"
        if [ -f "$golden" ]; then
            diff <(cat "$golden") <(printf '%s\n' "$err") | sed 's/^/    /'
        else
            echo "    golden yok: $golden"
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

#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

cargo build --features inkwell --quiet || exit 1
BIN=target/debug/lexora
REPEAT="tests/equiv/structs.lx tests/equiv/try.lx tests/equiv/string_basic.lx"

pass=0
fail=0
failed=""

for f in tests/equiv/*.lx; do
    name="$(basename "$f" .lx)"
    if ! out="$("$BIN" --backend inkwell -g "$f" 2>&1 >/dev/null)"; then
        echo "FAIL  $name (-g)"
        printf '%s\n' "$out" | head -4 | sed 's/^/    /'
        fail=$((fail + 1)); failed="$failed $name"; continue
    fi
    if ! out="$("$BIN" --backend inkwell -O2 -g "$f" 2>&1 >/dev/null)"; then
        echo "FAIL  $name (-O2 -g)"
        printf '%s\n' "$out" | head -4 | sed 's/^/    /'
        fail=$((fail + 1)); failed="$failed $name(O2)"; continue
    fi
    pass=$((pass + 1))
done

for f in $REPEAT; do
    name="$(basename "$f" .lx)"
    for i in 2 3; do
        if ! "$BIN" --backend inkwell -g "$f" > /dev/null 2>&1; then
            echo "FAIL  $name (-g, tekrar $i)"
            fail=$((fail + 1)); failed="$failed $name-r$i"
        fi
    done
done

rm -f output output.o output.ll output.s

echo "------------------------------------------"
if [ "$fail" -ne 0 ]; then
    echo "dbg-smoke: PASS $pass   FAIL $fail"
    echo "basarisiz:$failed"
    exit 1
fi
echo "dbg-smoke: PASS $pass (korpusun tamami, -g ve -O2 -g)"

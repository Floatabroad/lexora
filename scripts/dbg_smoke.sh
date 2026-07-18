#!/usr/bin/env bash
set -u
cd "$(dirname "$0")/.."

cargo build --features inkwell --quiet || exit 1
BIN=target/debug/lexora
CASES="tests/equiv/structs.lx tests/equiv/try.lx tests/equiv/string_basic.lx"

fail=0
for f in $CASES; do
  for i in 1 2 3; do
    if ! "$BIN" --backend inkwell -g "$f" > /dev/null 2>&1; then
      echo "FAIL  $f (-g, kosu $i)"
      fail=1
      continue 2
    fi
  done
  echo "PASS  $f (-g x3)"
done

if ! "$BIN" --backend inkwell -O2 -g tests/equiv/structs.lx > /dev/null 2>&1; then
  echo "FAIL  tests/equiv/structs.lx (-O2 -g)"
  fail=1
else
  echo "PASS  tests/equiv/structs.lx (-O2 -g)"
fi

rm -f output output.o output.ll output.s

if [ "$fail" -ne 0 ]; then
  echo "------------------------------------------"
  echo "dbg-smoke: FAIL"
  exit 1
fi
echo "------------------------------------------"
echo "dbg-smoke: PASS"

#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

N="${1:-200}"
START="${SEED:-1}"

echo ">> build (release, inkwell)"
cargo build --release --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" output.ll output.o output' EXIT

ok=0
wrong=0
rejected=0
badlist=""

RUN_TIMEOUT="${RUN_TIMEOUT:-10}"
RUN_VMEM="${RUN_VMEM:-4000000}"

run_prog() {
    ( ulimit -v "$RUN_VMEM"; timeout "$RUN_TIMEOUT" "$1" >"$2" 2>/dev/null )
    RUN_CODE=$?
}

for i in $(seq "$START" $((START + N - 1))); do
    prog="$TMP/o.lx"
    want="$TMP/o.expect"
    python3 scripts/oracle_gen.py "$i" "$prog" "$want" || { echo "uretici hatasi (seed $i)"; exit 1; }

    if ! ./target/release/lexora --backend string-ir "$prog" >/dev/null 2>"$TMP/tc.err"; then
        echo "FAIL seed $i  (ON-UC REDDETTI; uretec yalnizca gecerli program uretir)"
        grep -m1 '^error:' "$TMP/tc.err" | sed 's/^/    /'
        cp "$prog" "oracle-fail-$i.lx"
        rejected=$((rejected + 1)); badlist="$badlist $i"; continue
    fi
    llc output.ll -o "$TMP/a.s" 2>/dev/null
    clang -no-pie "$TMP/a.s" -o "$TMP/a.sir" 2>/dev/null
    run_prog "$TMP/a.sir" "$TMP/sir.out"; sir_code=$RUN_CODE

    ./target/release/lexora --backend inkwell "$prog" >/dev/null 2>&1
    run_prog "./output" "$TMP/ink.out"; ink_code=$RUN_CODE

    if [ "$sir_code" = 124 ] || [ "$ink_code" = 124 ]; then
        echo "FAIL seed $i  (SONLANMADI: string_ir=$sir_code inkwell=$ink_code; kehanet ureteci sonlanan program uretmeli)"
        cp "$prog" "oracle-fail-$i.lx"
        wrong=$((wrong + 1)); badlist="$badlist $i"; continue
    fi

    bad=""
    cmp -s "$TMP/sir.out" "$want" || bad="string_ir"
    cmp -s "$TMP/ink.out" "$want" || bad="$bad inkwell"
    if [ -n "$bad" ]; then
        echo "FAIL seed $i  (YANLIS CEVAP:$bad)"
        echo "  beklenen:"; head -6 "$want" | sed 's/^/    /'
        echo "  string_ir:"; head -6 "$TMP/sir.out" | sed 's/^/    /'
        echo "  inkwell:"; head -6 "$TMP/ink.out" | sed 's/^/    /'
        cp "$prog" "oracle-fail-$i.lx"
        wrong=$((wrong + 1)); badlist="$badlist $i"
    else
        ok=$((ok + 1))
    fi
done

echo "------------------------------------------"
echo "kehanet: $N program (seed $START..$((START + N - 1)))"
echo "  dogru: $ok   YANLIS: $wrong   on-uc reddi: $rejected"
if [ $((wrong + rejected)) -ne 0 ]; then
    echo "basarisiz seed'ler:$badlist  (program oracle-fail-<seed>.lx olarak kaydedildi)"
    exit 1
fi
echo "kehanet: PASS"

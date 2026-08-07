#!/usr/bin/env bash
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

echo ">> build (debug, inkwell)"
cargo build --features inkwell --quiet 2>/dev/null || { echo "build basarisiz"; exit 1; }

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP" output.ll output.o output' EXIT

python3 scripts/typesweep.py "$TMP"

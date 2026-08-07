FILE ?= test.lx

build:
	cargo build

run: build
	cargo run -- $(FILE)
	llc output.ll -o output.s
	clang -no-pie output.s -o output
	./output

clean:
	rm -f output output.ll output.s output.o

equiv:
	./scripts/equiv.sh

equiv-bless:
	./scripts/equiv.sh --bless

diag:
	./scripts/diagnostics.sh

diag-bless:
	./scripts/diagnostics.sh --bless

dbg-smoke:
	./scripts/dbg_smoke.sh

fuzz:
	./scripts/fuzz.sh $(N)

robust:
	./scripts/robust.sh

mutate:
	./scripts/mutate.sh $(N)

oracle:
	./scripts/oracle.sh $(N)

asan:
	./scripts/asan.sh

typesweep:
	./scripts/typesweep.sh

.PHONY: build run clean equiv equiv-bless diag diag-bless dbg-smoke fuzz robust mutate oracle asan typesweep




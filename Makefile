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

.PHONY: build run clean equiv equiv-bless




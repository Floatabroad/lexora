FILE ?= test.lx

build:
	cargo build

run: build
	cargo run -- $(FILE)
	llc output.ll -o output.s
	clang -no-pie output.s -o output
	./output

clean:
	rm -f output output.ll output.s

.PHONY: build run clean




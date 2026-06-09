<div align="center">

# Lexora

**A small statically-typed, compiled language I'm building from scratch in Rust — mostly to learn how compilers actually work.**

It takes `.lx` source, type-checks it, and lowers it down to LLVM IR and native binaries.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/Rust-2024-orange.svg)](https://www.rust-lang.org/)
[![LLVM](https://img.shields.io/badge/LLVM-IR-262d3a.svg)](https://llvm.org/)

</div>

---

## Overview

Lexora is a hobby project — a place for me to learn compiler internals by
actually building one, not a production tool. That said, it's a complete
end-to-end pipeline: a hand-written lexer, an arena-allocated AST with string
interning, a scope-aware type checker, and **two interchangeable
code-generation backends** that target LLVM. The frontend is shared; only the
final IR-emission stage differs.

```
source.lx → Lexer → Parser → TypeChecker → CodeGen → LLVM IR → native binary
```

## Example

```rust
fn sum_to(n: i32) -> i32 {
    let total: i32 = 0;
    let i: i32 = 1;
    while i <= n {
        total = total + i;
        i = i + 1;
    }
    return total;
}

fn main() -> i32 {
    let n: i32 = 10;
    print(sum_to(n));
    print("done");
    return 0;
}
```

## Features

- **Types** — `i32`, `i64`, `bool`, `void`, `str`, fixed-size arrays `[T; N]`, and user-defined `struct`s
- **Functions** — typed parameters and return values
- **Control flow** — `if` / `else if` / `else`, `while`, `for i in a..b`
- **Operators** — arithmetic, comparison, logical (`and` / `or` / `not`), unary negation, explicit `as` casts
- **Aggregates** — array literals, indexing, struct literals, field access and assignment
- **Built-ins** — `print(...)` for integers, booleans, and strings
- **Runtime safety** — checked division (divide-by-zero), array bounds checks, and overflow-checked `+` / `-` / `*`; a violation prints a diagnostic and exits non-zero instead of misbehaving

## Architecture

The frontend (lexer, parser, type checker, AST) is where I spent most of my
time, trying to do things the "proper" way rather than the quickest:

- **Zero-copy lexer** — tokens borrow directly from the source string
- **Arena-allocated AST** — nodes live in a single `bumpalo` arena; cache-friendly, `O(1)` to drop
- **String interning** — identifiers are `u32` symbols, so comparison is integer-fast
- **Scope-stack type checker** — lexically correct shadowing and resolution
- **Typed expressions** — the type checker records each expression's type once, into a side table keyed by a per-node id, so both backends read one authoritative source instead of separately re-deriving types
- **Span-based diagnostics** — every node carries its source location

### Two backends

| Backend | How it works | Notes |
| --- | --- | --- |
| `string_ir` *(default)* | Emits textual LLVM IR through a type-safe builder | No external dependencies; pure Rust; lowered to a binary via `llc` + `clang` |
| `inkwell` *(optional)* | Builds LLVM's in-memory IR via the official Rust bindings | Runs real optimization passes (`mem2reg` and friends, selectable with `-O0`…`-O3`) and emits a native object; requires LLVM |

The backend is selected **at runtime** with `--backend`, while the heavyweight
LLVM-linked `inkwell` backend is gated behind a **build-time feature** — so the
default build needs no LLVM development libraries at all.

Because both backends share the entire frontend, they must agree byte-for-byte
on every program. That invariant is enforced by an equivalence harness
(`make equiv`, or `cargo test --features inkwell`) that compiles each case in
`tests/equiv/` with both backends and diffs their output.

## Building

Requires a recent Rust toolchain.

```bash
# Default build — string_ir backend, no LLVM dependency
cargo build --release

# With the inkwell backend (requires a matching system LLVM)
cargo build --release --features inkwell
```

## Usage

```bash
# Compile with the default backend → emits output.ll
cargo run -- hello.lx

# Produce a native binary (string_ir backend)
llc output.ll -o output.s
clang -no-pie output.s -o hello
./hello

# Choose a backend explicitly
cargo run -- --backend string-ir program.lx

# inkwell backend → emits output.o and links a native binary via cc
cargo run --features inkwell -- --backend inkwell program.lx

# Pick an optimization level for the inkwell backend: -O0 (default) … -O3
cargo run --features inkwell -- --backend inkwell -O2 program.lx
```

## Project layout

```
src/
  span.rs          Byte-offset source spans
  symbol.rs        String interner and symbols
  error.rs         Unified error type with Display diagnostics
  lexer.rs         Zero-copy tokenizer
  ast.rs           Arena-allocated AST
  parser.rs        Recursive-descent parser with precedence climbing
  typechecker.rs   Scope-stack type checker
  backend/
    string_ir/     Textual LLVM IR backend (default)
    inkwell/       LLVM in-memory IR backend (--features inkwell)
std/
  math.lx          Standard library helpers
```

## Status

Both backends now compile the full language surface to working native binaries.
`string_ir` lowers textual IR via `llc` + `clang`; `inkwell` emits a native
object through LLVM's `TargetMachine` and links it with `cc`, and runs LLVM
optimization passes (`mem2reg` promotes stack slots into SSA registers) at a
chosen `-O` level. The two backends produce **byte-identical output** across the
`tests/equiv/` corpus, checked automatically by the equivalence harness.

Generated code is also runtime-safe: division-by-zero, out-of-bounds indexing,
and integer overflow are each guarded and routed through a shared panic path
that prints a diagnostic and exits — and both backends agree byte-for-byte on
that behaviour too.

It's still a learning project, so expect rough edges, missing features, and the
occasional `unimplemented!()` — I add things as I get to them.

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([`LICENSE-MIT`](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.

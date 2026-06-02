<div align="center">

# Lexora

**A statically-typed, compiled programming language written from scratch in Rust.**

Lexora compiles `.lx` source to LLVM IR and produces native binaries.

[![License: MIT OR Apache-2.0](https://img.shields.io/badge/License-MIT%20OR%20Apache--2.0-blue.svg)](#license)
[![Rust](https://img.shields.io/badge/Rust-2024-orange.svg)](https://www.rust-lang.org/)
[![LLVM](https://img.shields.io/badge/LLVM-IR-262d3a.svg)](https://llvm.org/)

</div>

---

## Overview

Lexora is a small but real compiler. It has a hand-written, zero-copy lexer, an
arena-allocated AST with string interning, a scope-aware type checker, and **two
interchangeable code-generation backends** that target LLVM. The frontend is
shared; only the final IR-emission stage differs.

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

## Architecture

The frontend (lexer, parser, type checker, AST) is built with performance and
correctness in mind:

- **Zero-copy lexer** — tokens borrow directly from the source string
- **Arena-allocated AST** — nodes live in a single `bumpalo` arena; cache-friendly, `O(1)` to drop
- **String interning** — identifiers are `u32` symbols, so comparison is integer-fast
- **Scope-stack type checker** — lexically correct shadowing and resolution
- **Span-based diagnostics** — every node carries its source location

### Two backends

| Backend | How it works | Notes |
| --- | --- | --- |
| `string_ir` *(default)* | Emits textual LLVM IR through a type-safe builder | No external dependencies; pure Rust; lowered to a binary via `llc` + `clang` |
| `inkwell` *(optional)* | Builds LLVM's in-memory IR via the official Rust bindings | Enables real optimization passes and native object emission; requires LLVM |

The backend is selected **at runtime** with `--backend`, while the heavyweight
LLVM-linked `inkwell` backend is gated behind a **build-time feature** — so the
default build needs no LLVM development libraries at all.

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
cargo run --features inkwell -- --backend inkwell program.lx
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

The `string_ir` backend is complete and produces working native binaries across
the full language surface. The `inkwell` backend supports scalars, control flow,
casts, arrays, function calls and `print`, with struct support and native object
emission in progress.

## License

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([`LICENSE-MIT`](LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.

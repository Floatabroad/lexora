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
    total
}

fn main() -> i32 {
    let n: i32 = 10;
    print(sum_to(n));
    print("done");
    return 0;
}
```

Enums, pattern matching, generics, and heap-owning recursive types — a generic
linked list that builds, traverses, and frees itself:

```rust
enum Shape {
    Circle(i32),
    Rect(i32, i32),
}

fn area(s: Shape) -> i32 {
    match s {
        Shape::Circle(r) => r * r * 3,
        Shape::Rect(w, h) => w * h,
    }
}

enum List<T> {
    Cons(T, Box<List<T>>),
    Nil,
}

fn sum(l: List<i32>) -> i32 {
    match l {
        List::Cons(v, rest) => v + sum(*rest),
        List::Nil => 0,
    }
}

fn main() -> i32 {
    print(area(Shape::Circle(10))); // 300
    print(area(Shape::Rect(4, 5))); // 20

    let l = List::Cons(1, box List::Cons(2, box List::Cons(3, box List::<i32>::Nil)));
    print(sum(l));                  // 6 — every node freed, valgrind-clean

    let max: i32 = if 3 > 2 { 3 } else { 2 };
    print(max);                     // if and match are expressions
    return 0;
}
```

Error handling with a real generic `Result` and the `?` operator, plus
heap-allocated owned strings:

```rust
enum Result<T, E> {
    Ok(T),
    Err(E),
}

fn half(x: i32) -> Result<i32, String> {
    if x / 2 * 2 == x {
        Result::<i32, String>::Ok(x / 2)
    } else {
        Result::<i32, String>::Err(string("odd ") + "number")
    }
}

fn quarter(x: i32) -> Result<i32, String> {
    let h = half(x)?;               // Err short-circuits out of the function
    half(h)
}

fn main() -> i32 {
    match quarter(8) {
        Result::Ok(v) => print(v),  // 2
        Result::Err(m) => print(m), // owned message, freed with the enum
    }
    let s: String = string("heap-allocated, owned, freed automatically");
    print(s);
    print(s);                       // printing borrows — no move, no copy
    print(len(s));                  // 42, read from the length header
    return 0;
}
```

## Features

- **Types** — `i32`, `i64`, `f32`, `f64`, `bool`, `void`, `str`, heap-owned `String`, fixed-size arrays `[T; N]`, and user-defined `struct`s. Floats follow IEEE-754: comparisons use *ordered* predicates, and the four-way cast matrix between integers and floats is explicit
- **Comments** — `//` line comments, treated as whitespace by the lexer
- **Type inference** — the annotation on `let` is optional (`let x = 42;`); the type checker infers the variable's type from its initializer. There's no implicit widening, so the inferred type is always unambiguous
- **Functions** — typed parameters and return values, plus **generic functions** (`fn first<T>(x: T) -> T`) monomorphized the same way generic enums are: one emitted body per concrete instantiation, with type arguments inferred from the arguments *or* from the expected return type, or given explicitly (`identity::<i32>(5)`). Following rustc rather than C++ templates, `T` is opaque — an operation needing a bound is rejected at the definition, not at the call site
- **Control flow** — `if` / `else if` / `else`, `while`, `for i in a..b`
- **Expression-oriented** — blocks produce values: the last expression of a block (no trailing `;`) is its value, so a function body can end in a plain expression instead of `return`. `if` and `match` are expressions too (`let max = if a > b { a } else { b };`), lowered through a shared result-slot pattern that `mem2reg` promotes into SSA phis. Statements are parsed expression-first, rustc-style — an assignment is just an expression followed by `=`, so any place expression works on the left-hand side
- **Operators** — arithmetic, comparison, logical (`and` / `or` / `not`), unary negation, explicit `as` casts
- **Aggregates** — array literals, indexing, struct literals, field access and assignment, nested to any depth: reads and writes both go through one recursive place-address core, so `o.i.x`, `s.a[i]`, `(*b).x`, `m[1][0] = 7` and `mk().x` all work, and aggregate literals are ordinary expressions (`f(P { x: 1 })`, `[[1, 2], [3, 4]]`)
- **Methods** — `impl` blocks on structs and enums, with `self` passed as a **borrow** rather than by value: no allocation of its own, so the existing place machinery handles `self.field` unchanged and mutation comes for free — `p.translate(1)` changes the caller's value, while a free function taking the same struct by value cannot. An owning receiver is never consumed, so `k.len()` twice is fine; conversely `self` may not escape by value from an owning receiver, which would be a double free
- **Heap & ownership** — `box e` allocates on the heap (`Box<T>`), `*b` reads/writes through it, and `let inner = *b;` *moves out* of a box — the contents transfer to the new owner and the heap block is freed on the spot. Values are *moved* rather than copied, and an ownership pass (modelled on rustc's borrow checker) tracks moves across branches and loops at compile time. Drops are elaborated automatically — each owning slot gets a drop flag, scope exit frees what's still live, and one recursive `drop_in_place` core walks whatever the type contains: a box, a string, an enum variant, an owning struct field, an array element. Ownership is tracked **per owning place**, not per variable, so a single field can be moved out while the rest of the struct stays usable (`let name = person.name;` leaves `person.age` readable), and assigning to an owning target frees the old value first — including re-initializing a field that was moved out. Using a partially moved value as a whole, or moving out of an array element, is a compile error, matching rustc. No leaks, no double-frees; verified leak-free under `valgrind` and ASan/LSan
- **Enums & pattern matching** — C-like enums (`enum Color { Red, Green, Blue }`), data-carrying variants (`enum Shape { Circle(i32), Rect(i32, i32) }`), and heap-owning variants (`Box<T>` fields), which makes recursive types like linked lists possible: `enum List { Cons(i32, Box<List>), Nil }`. Every owning enum gets a generated per-type drop function, so freeing a list recurses at runtime instead of unrolling at compile time. `match` works on any expression, arms are `=> expr` or `=> { ...; expr }` blocks with payload binding (`Shape::Rect(w, h) => w * h`), a `_` wildcard, and compile-time exhaustiveness checking
- **Generic enums (monomorphization)** — enums take type parameters (`enum Option<T> { Some(T), None }`, multi-parameter `enum Pair<A, B>` too), compiled the way rustc does it: the generic definition itself is never lowered — each concrete instantiation gets its own layout and, if it owns heap data, its own drop function, under a mangled name (`Option<i32>` and `Option<Box<i32>>` are two separate types in the emitted IR). Type arguments are inferred from constructor arguments by structural unification — `Option::Some(box 7)` is an `Option<Box<i32>>` with no annotation needed — or written explicitly with a turbofish (`Option::<i32>::None`) when there is nothing to infer from. Recursive generic types work end-to-end: `List<i32>` monomorphizes into a self-recursive drop function
- **Error handling — the `?` operator** — works on any two-variant `Ok`/`Err` enum named `Result` (a plain library definition, not a builtin): `Ok(v)` unwraps to the value, `Err(e)` returns early from the enclosing function. The error is *rebuilt* in the caller's own `Result` instantiation, so using a `Result<i32, str>?` inside a function returning `Result<bool, str>` is legal — those are two different monomorphized layouts, and the early-return path constructs the right one. The error types must match exactly (no implicit `From` conversion), and live heap values are freed before the early return
- **Heap strings** — `str` literals stay static and freely copyable; `String` (created with `string("...")`) is a heap-allocated, *owned* value carrying a hidden length header in front of NUL-terminated bytes. It moves and drops through the same ownership machinery as `Box<T>`, and can live inside a struct field, an array element, or an enum variant — which is what makes `Result<i32, String>` carry a real error message. `+` concatenates (consuming its operands), `==` / `!=` compare by content with an `O(1)` length short-circuit, `<` and friends order byte-wise, and `len` is a header read. Reads *borrow*: printing or comparing a `String` twice is fine; unnamed temporaries are freed right after use, so `print(a + b)` is legal, while handing `print` an owning temporary that nothing can free is a compile error
- **Built-ins** — `print(...)` for integers, floats, booleans, and strings; `string(...)` to build an owned `String` from a literal; `len(...)` for `str` and `String`
- **Runtime safety** — checked division (divide-by-zero), array bounds checks, and overflow-checked `+` / `-` / `*`; a violation prints a diagnostic and exits non-zero instead of misbehaving. The scope is deliberately **integers only** — in IEEE-754 overflow and division by zero produce defined results (`inf`, `nan`), so float arithmetic is left unguarded
- **Diagnostics** — compile errors are rendered rustc-style: the offending source line(s), a caret underline (which spans multiple lines when the error does), a short inline label, and `note` / `help` lines. Identifier typos get a Levenshtein-based "did you mean?" suggestion. The lexer, parser, and type checker all *recover* from errors — bad nodes are poisoned so a single statement can surface several independent errors without spurious cascades, and the compiler reports as many as it can in one run (capped) instead of stopping at the first. Locations resolve through a source map, so they stay correct even when the error lives in an imported file
- **Debug info** — the `inkwell` backend can emit DWARF (`-g`): line tables, function parameters, locals, and struct types (with named fields and real offsets), so compiled programs are debuggable in `gdb` / `lldb` — breakpoints, single-stepping, and `print` of variables and struct fields all work

## Architecture

The frontend (lexer, parser, type checker, AST) is where I spent most of my
time, trying to do things the "proper" way rather than the quickest:

- **Zero-copy lexer** — tokens borrow directly from the source string
- **Arena-allocated AST** — nodes live in a single `bumpalo` arena; cache-friendly, `O(1)` to drop
- **String interning** — identifiers are `u32` symbols, so comparison is integer-fast
- **Scope-stack type checker** — lexically correct shadowing and resolution
- **Typed expressions** — the type checker records each expression's type once, into a side table keyed by a per-node id, so both backends read one authoritative source instead of separately re-deriving types
- **Span-based diagnostics** — every node carries its source location, and a source map turns global byte offsets back into `file:line:column` (correct across imports), feeding a small presentation layer that renders rustc-style errors with caret underlines and labelled `note` / `help` lines

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
`tests/equiv/` with both backends and diffs their output. A second snapshot
harness (`make diag`) locks down the diagnostic output itself: each case in
`tests/diagnostics/` is compiled and its rendered errors are diffed against a
golden `.stderr`, so the rustc-style formatting stays pinned too. Around those
sit seven more `make` gates: `dbg-smoke` recompiles the whole corpus with `-g`,
`asan` reruns it under ASan/LSan, `robust` feeds it pathological input (deeply
nested types, unclosed blocks, absurd array sizes) to assert the compiler never
panics, `typesweep` walks a few hundred type shapes through every type position,
and three randomized gates: `fuzz` compares the two backends on generated
programs, `oracle` checks output against an expectation computed independently
in Python — catching bugs where *both* backends agree and are wrong — and
`mutate` corrupts valid programs to keep the "no panic" invariant honest.

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

# Emit DWARF debug info (inkwell backend) and debug in gdb/lldb
cargo run --features inkwell -- --backend inkwell -g program.lx
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
  move_check.rs    Ownership / move pass (Box<T> move tracking, drop elaboration)
  source_map.rs    Global byte offsets → file:line:column
  diagnostic.rs    Diagnostic presentation layer (rustc-style rendering)
  suggest.rs       Levenshtein "did you mean?" suggestions
  backend/
    string_ir/     Textual LLVM IR backend (default)
    inkwell/       LLVM in-memory IR backend (--features inkwell)
std/
  math.lx          Standard library helpers
tests/
  equiv/           Backend-equivalence corpus (.lx + golden .out)
  diagnostics/     Diagnostic snapshot corpus (.lx + golden .stderr)
  robust/          Pathological inputs — must not panic
scripts/           The make gates (equiv, diag, dbg-smoke, asan, robust,
                   typesweep, fuzz, oracle, mutate) and their generators
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

The `inkwell` backend can additionally emit DWARF debug info (`-g`), so compiled
programs can be stepped through in `gdb` / `lldb` with full line, parameter,
local-variable, and struct information.

Beyond the stack-only core, the language now has a heap: `Box<T>` with move
semantics, a compile-time ownership pass, deref-moves out of boxes, and
automatic RAII drops (including recursive drop glue for nested boxes) — both
backends are byte-identical here and the heap corpus is leak-free under
`valgrind`. On top of that sit `enum`s and `match`: C-like enums lower to an
`i32` tag, data-carrying variants use a tagged-union layout (`{ tag, payload }`
with a per-variant struct view), and variants can own heap data — recursive
types like `enum List { Cons(i32, Box<List>), Nil }` build, traverse
(`sum(*rest)` moves each node out of its box), and free themselves through
generated per-type drop functions.

The language has also completed a full expression-oriented transition: blocks
have values, function bodies can end in a trailing expression, `if` and `match`
are expressions, match scrutinees are arbitrary expressions, and statements are
parsed expression-first — one unified block grammar throughout.

Most recently, enums went generic — with real monomorphization rather than
type erasure. The type checker resolves each instantiation (inferring type
arguments from constructor arguments via structural unification, with a
turbofish escape hatch), records every concrete instantiation in a table, and
both backends then emit one layout and one drop function per instantiation
under mangled names — a generic definition by itself produces no code at all.
That makes `Option<T>` / `Result<T, E>` plain library-style definitions instead
of compiler builtins, and both backends stay byte-identical (and valgrind-clean)
across the generic corpus.

Since then the error-handling chain has closed: the `?` operator desugars to a
match-like early return, and because `Result` is monomorphized per
instantiation, the `Err` path rebuilds the error inside the *enclosing
function's* `Result` type — cross-`T` uses compile to two distinct layouts and
still work. The language also grew a real heap string: `String` values are
length-prefixed, NUL-terminated heap blocks addressed by a data pointer (so
`printf`-style printing needs no conversion), with concatenation, content
comparison, ordering, and `len`, owned and freed by the same move/drop machinery
as `Box<T>`. Owning values then moved *into* aggregates — a `String` can sit in
a struct field, an array element, or an enum variant, which is what turns
`Result<i32, String>` into something you'd actually use — and the three drop
paths collapsed into a single recursive `drop_in_place` core.

Then place lowering was unified. Reading and writing a location now
share one recursive address core, so nested field/index/deref chains work to any
depth in both directions (`o.i.x`, `m[1][0] = 7`, `(*b).x`), aggregate literals
became ordinary expressions, and the three separate assignment statements
collapsed into one place-targeted form. Along the way a measured crash came out
of hiding: the textual backend was emitting `alloca` wherever a temporary
happened to be created, so a hot loop grew the stack until it segfaulted —
both backends now follow the same discipline (every stack slot lives in the
entry block), and the corpus checks it mechanically.

Most recently the language grew in four steps. Floats (`f32` / `f64`) went in
end-to-end. Generic functions joined generic enums on the same monomorphization
machinery, inferring type arguments from arguments or from the expected return
type. `impl` blocks brought methods, with `self` passed as a borrow — which made
mutation through a method fall out of the existing place machinery for free.
And ownership became **per place** instead of per variable: a struct field can be
moved out on its own while its siblings stay live, and assigning to an owning
target frees the old value first, so a moved-out field can be re-initialized.
Each of those steps was measured against `rustc`'s actual behaviour rather than
guessed at, and each landed without changing a single byte of the IR emitted for
programs that don't use it.

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

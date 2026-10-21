# CoreX

A low-level, statically typed, ahead-of-time systems programming language and a
working compiler. CoreX targets Linux x86-64, compiles through LLVM 20, and
produces statically linked ELF executables with **no libc, no garbage collector,
and no runtime library**.

```corex
fn fact(n: u64) -> u64 {
    if n <= 1 {
        return 1;
    }
    return n * fact(n - 1);
}

fn main() -> i32 {
    let result: u64 = fact(10);
    return (result % 256) as i32;
}
```

Status: **MVP implemented and tested** — 133 automated tests pass, including
integration tests that compile programs to native binaries and assert their
output and exit codes at every optimization level.
Language surface is deliberately small; see [Limitations](#limitations).

---

## Quick start

Prerequisites:

- Rust 1.99+ with `cargo` (no external crates are used).
- LLVM 20 tools: `llvm-as`, `opt`, `llc` (project verified with 20.1.8).
- A C compiler driver for linking (`cc` or `COREX_CC`), capable of
  `-nostdlib -static -no-pie`.

```bash
# Point the compiler at LLVM if it is not on PATH:
export COREX_LLVM_BIN=/path/to/llvm-20/bin

cd compiler
cargo build --release

./target/release/corex run ../examples/hello.cx       # Hello, World!
./target/release/corex run ../examples/factorial.cx   # 3628800
```

Build a binary you can keep:

```bash
./target/release/corex build ../examples/factorial.cx -o factorial --opt 2
./factorial        # prints 3628800, exits 0
file factorial     # ELF 64-bit LSB executable, x86-64, statically linked (abridged)
```

---

## CLI

```
corex build <file.cx | -> [-o <output>] [--opt <0-3>]
corex run   <file.cx | -> [--opt <0-3>] [-- <program args>...]
corex exec  '<source>' [--opt <0-3>] [-- <program args>...]
corex repl  [--opt <0-3>]
corex emit-ir <file.cx | -> [--verify]
corex check <file.cx | ->
```

`-` reads source from stdin, `exec` runs source given as an argument, and `repl`
starts an interactive session where a blank line compiles and runs the block:

```bash
printf 'fn main() -> i32 { return 7; }' | corex run -        # exits 7
corex exec 'fn main() -> i32 { return 7; }'                 # exits 7
corex repl                                                  # type code, blank line runs it
```

Inside the REPL:

- a complete one-liner runs on **Enter** — `1 + 2` prints `3`, `print(42)` prints
  `42`;
- a trailing `;` runs it as a statement instead, so no value is printed
  (`1 + 2;`);
- function definitions accumulate and a redefinition replaces the old one;
- a blank line submits a multi-statement block, and pasted programs (with blank
  lines between functions) run once when their entry point is complete;
- arrows/Home/End/Delete/Backspace edit the line, Up/Down walk history (saved in
  `~/.corex_history`), Ctrl-C abandons the current block;
- bindings do **not** persist between submissions — each run is a new process.
  Wrap statements that share values in `{ ... }` and finish with a blank line.

`:help` lists the remaining commands (`:run`, `:ir`, `:show`, `:submit`,
`:history`, `:reset`, `:quit`).

| Exit code | Meaning |
|---|---|
| 0 | success |
| 1 | compile error (diagnostics printed) |
| 2 | usage or I/O error |
| 3 | toolchain failure (missing `llvm-as`/`opt`/`llc`, verifier rejected IR, link failed) |
| program's status | `corex run` forwards the program's exit status (128+signal on Unix) |

`--opt 0` emits unoptimized IR to `llc`; `--opt 1..3` runs
`opt -passes=default<O{n}>` before lowering. `emit-ir --verify` assembles the
module with `llvm-as` and reports the verifier result.

Environment: `COREX_LLVM_BIN` (LLVM tool directory), `COREX_CC` (linker driver),
`COREX_KEEP_TEMP` (keep intermediate `.ll`/`.bc`/`.o` for inspection).

---

## Language in one screen

- **Types:** `i8 i16 i32 i64`, `u8 u16 u32 u64`, `usize isize`, `bool`, `char`,
  `f32 f64`, `*T`, `*mut T`, `[T; N]`, slices `[T]`, `str`, `enum`, and
  `struct` values. Arrays cannot be passed or returned by value; structs,
  slices, and `str` can.
- **Bindings:** `let x: i32 = 1;` (immutable) or `let x = 1;` (inferred);
  `let mut` makes the binding mutable. The initializer is mandatory.
- **Items:** functions, `static`/`static mut` globals with constant
  initializers, enums with explicit `i32` discriminants, and structs.
- **Functions:** `fn name(a: T, b: U) -> R { ... }`; recursion allowed.
- **Control flow:** `if/else`, `while`, `loop`, `break`, `continue`, `return`,
  `unsafe { ... }`. Blocks are statements, not expressions.
- **Operators:** `+ - * / %`, comparisons, `&& || !`, `& | ^ << >>`, compound
  assignment (`+=`, `<<=`, …), unary `-`, `as`, indexing, field access,
  slicing, and `len(x)`.
- **Semantics that matter:**
  - integer overflow wraps (`(255 as u8) + 1 == 0`);
  - shift counts are masked (`(1 as u8) << 8 == 1`);
  - integer division by zero, `MIN / -1`, dynamic out-of-bounds indices, and
    slice-range violations abort with a message on stderr and exit 101;
  - literal out-of-bounds indices/ranges are compile errors;
  - floats are IEEE 754 and never abort on division by zero;
  - no implicit numeric conversions — use `as`.
- **Memory:** `alloc(bytes)` → `*mut u8` via `mmap`, `free(ptr, bytes)` via
  `munmap`; raw pointer dereference, pointer arithmetic, integer/pointer casts,
  and syscalls require `unsafe`. No borrow checker: the language does **not**
  claim memory safety for pointer code.
- **Syscalls:** `syscall(nr, a1, ..., a6)` with 1–7 arguments (missing arguments
  are passed as 0), returning `isize` in `rax`.
- **Printing:** `print(value)` writes an integer, `bool`, `char`, float (fixed
  six decimals), enum discriminant, or string/`[u8]` slice plus a newline to
  stdout; no `unsafe`, no heap allocation.
- **Entry:** `fn _start()` for full control, or `fn main() -> i32` (the compiler
  synthesizes `_start`, exits with the low 8 bits of the return value).

Full normative details: [`docs/language-spec.md`](docs/language-spec.md) and
[`docs/grammar.ebnf`](docs/grammar.ebnf).

---

## Examples

| File | Demonstrates | Expected |
|---|---|---|
| [`examples/hello.cx`](examples/hello.cx) | `_start`, `write`/`exit` syscalls, no libc | prints `Hello, World!`, exit 0 |
| [`examples/factorial.cx`](examples/factorial.cx) | recursion, `while`/`loop`/`break`, arrays, decimal formatting | prints `3628800`, exit 0 |
| [`examples/pointers.cx`](examples/pointers.cx) | `alloc`, `*mut u64`, casts, `free` | prints `123456789`, exit 0 |
| [`examples/loop_sum.cx`](examples/loop_sum.cx) | `while`, `continue`, `usize` arithmetic | exit status 51 |
| [`examples/features.cx`](examples/features.cx) | enums, structs, globals, floats, chars, inference, compound assignment, slices, strings, `len` | deterministic tour output, exit 0 |

---

## Testing

```bash
cd compiler
cargo test                 # 133 tests: 53 unit + 80 integration
cargo test --lib           # stage unit tests only
cargo test --test native   # compile → link → execute, asserts stdout/exit codes
cargo test --test cli      # stdin, exec, REPL, flags and exit codes
cargo test --test ir       # every example is assembled and verified by llvm-as
cargo test --test diagnostics  # invalid programs rejected with specific codes
cargo test --test features     # globals, enums, floats, structs, slices, strings
cargo test --test opt_levels   # identical semantics at --opt 0 through 3
                               # (wrapping, shifts, aborts, aliasing, print)
                               # plus 1,104 generated value assertions
cargo test --test robustness   # malformed input yields diagnostics, not panics
```

The integration tests invoke the real toolchain; they fail loudly if LLVM 20 is
missing rather than silently skipping.

Benchmarks (CoreX vs gcc, same source semantics):

```bash
cd compiler && cargo build --release && cd ..
./scripts/bench.sh 5
```

Results and caveats: [`docs/benchmarks.md`](docs/benchmarks.md).

---

## Repository layout

```
compiler/
  Cargo.toml            zero dependencies
  src/
    token.rs            token kinds
    lexer.rs            byte-oriented lexer
    ast.rs              AST, Type, IntTy
    parser.rs           recursive descent + precedence climbing
    semantic.rs         scopes, types, mutability, unsafe, return flow
    codegen.rs          LLVM 20 textual IR backend
    driver.rs           verify → optimize → object → link → run
    repl.rs             interactive session (definitions accumulate)
    lineedit.rs         TTY line editing + history (stty raw mode)
    main.rs             CLI (files, stdin, inline source, REPL)
  tests/
    diagnostics.rs      rejection cases by error code
    ir.rs               IR verification and regressions
    native.rs           executed binaries
    cli.rs              stdin, exec, and REPL behavior
    opt_levels.rs       same semantics at --opt 0..3 (wrapping, shifts, aborts)
    features.rs         globals/enums/floats/structs/slices/strings end to end
    robustness.rs       malformed/adversarial input never panics
examples/               hello, factorial, pointers, loop_sum, features, playground
bench/                  two kernels with C references
scripts/bench.sh        benchmark harness
scripts/try.sh          compile-and-run one file (check / ir / build modes)
docs/                   spec, grammar, memory model, architecture, PRD, NFR,
                        research, ADRs, task tracker, request log, benchmarks
ENGINEERING.md          roles, conventions, testing and documentation rules
master.md               current status, milestones, next actions
```

---

## Limitations

Explicitly not implemented: unions, tuples, generics, traits, closures, function
pointers, modules, imports, `extern` / C FFI, labeled break, `match`, macros,
threads, debug information, cross-compilation, and any CoreX-level optimizer.
Slices and strings have no concatenation, formatting, or comparison operators.

Safety: pointer validity, allocation lifetime, alignment, double free, and
use-after-free are the programmer's responsibility. The compiler guarantees types,
mutable-place writes, `unsafe` marking, return paths, constant bounds, and runtime
aborts for dynamic bounds and division by zero. See
[`docs/memory-model.md`](docs/memory-model.md) §6–§7.

---

## Documentation index

| Document | Contents |
|---|---|
| [`docs/language-spec.md`](docs/language-spec.md) | Normative syntax and semantics, diagnostics catalog |
| [`docs/grammar.ebnf`](docs/grammar.ebnf) | Formal grammar matching the parser |
| [`docs/memory-model.md`](docs/memory-model.md) | Allocation model, pointer invariants, guarantee matrix, UB |
| [`docs/architecture.md`](docs/architecture.md) | Compiler modules, pipeline, codegen strategy, testing architecture |
| [`docs/PRD.md`](docs/PRD.md) | Vision, users, FRs, MVP scope, non-goals, risks, DoD |
| [`docs/NFR.md`](docs/NFR.md) | Measurable non-functional requirements and their status |
| [`docs/research.md`](docs/research.md) | Evidence-labelled research on comparable languages |
| [`docs/competitor-analysis.md`](docs/competitor-analysis.md) | Side-by-side comparison and what CoreX borrows/rejects |
| [`docs/ADRs.md`](docs/ADRs.md) | Append-only architecture decisions |
| [`docs/tasks.md`](docs/tasks.md) | Task tracker with validation evidence |
| [`docs/request-log.md`](docs/request-log.md) | Append-only record of requests and outcomes |
| [`docs/benchmarks.md`](docs/benchmarks.md) | Measured performance with methodology |

## AI assistance

AI coding assistants were used to help research, implement, test, and document
this project. Every change is validated by the test suite and the checks recorded
in `docs/tasks.md`; the maintainer remains responsible for the content.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT license ([LICENSE-MIT](LICENSE-MIT))

at your option.

This is a permissive, OSI-approved dual license: you may use, modify, redistribute,
sublicense, and sell the compiler and its documentation, and you may fork the
repository. Apache-2.0 adds an express patent grant; MIT is the simplest
permissive option. Pick whichever you prefer.

### Contributing

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in this project by you is dual-licensed under MIT OR Apache-2.0,
without additional terms or conditions. There is no CLA and no copyright
assignment.

### Output and third parties

Programs you compile with CoreX are your own work; the compiler's license does
not apply to their output, and the produced binaries link no runtime library.
Build tools (Rust, LLVM, `cc`, `stty`) are not redistributed. See
[THIRD_PARTY.md](THIRD_PARTY.md).

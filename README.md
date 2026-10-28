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

Status: **CoreX 0.3.0 — C-class systems language implemented and tested** — 205 automated
tests pass (68 unit, 137 integration), including native compile/run tests at
every optimization level, C ABI tests in both directions, separate-compilation
and manifest builds, DWARF inspection, LSP sessions, and sanitizer aborts.

The language now has function pointers, C control flow, unions/bitfields/packed
layout, `sizeof`/`alignof`/`offsetof`, C FFI and variadic calls, file modules
with visibility and separate compilation, `corex.toml` projects, static
aggregate constants (`static` arrays/structs/function-pointer tables and `const`
array lengths), a freestanding standard library with a string map/set and a
growable builder, atomics/threads/futexes, inline assembly, macros, debug info,
a formatter, linter, docs generator, test runner, and a language server. It
still has no garbage collector, hidden allocation, runtime, or libc dependency.
See [Limitations](#limitations).

---

## Quick start

For a step-by-step toolchain setup, verification, and troubleshooting, see
[`docs/installation.md`](docs/installation.md). Short version:

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
corex build <file.cx | -> | directory> [-o <output>] [--opt <0-3>]
            [--link <obj|a>]... [-L <dir>]... [-l <lib>]... [--hosted]
            [--shared] [--emit obj|shared|staticlib] [-g] [--sanitize]
            [--cfg name=value]... [-I <dir>]...
corex run   <file.cx | -> [--opt <0-3>] [-- <program args>...]
corex exec  '<source>' [--opt <0-3>] [-- <program args>...]
corex repl  [--opt <0-3>]
corex emit-ir <file.cx | -> [--verify]
corex check <file.cx | ->
corex fmt   <file.cx | -> [-w | --check]
corex lint  <file.cx | -> [--deny-warnings]
corex doc   <file.cx | -> [-o <out.md>]
corex test  <file.cx | -> [--opt <0-3>] [--cfg ...] [-I <dir>]...
corex-lsp
```

`corex build <directory>` reads `corex.toml`; `--emit staticlib|shared` produces
archives/shared objects, `--emit obj` a single unit object. `-g` emits DWARF 5,
`--sanitize` checks signed overflow and hardens the allocator. `corex-lsp` is a
stdio language server (diagnostics, completion, definition, references, rename,
hover, formatting).

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
- top-level items (functions, structs, unions, typedefs, enums, consts, globals, macros) accumulate; redefinition replaces the old one;
- use `:reset` before pasting an unrelated program; existing `main`/`_start` definitions are re-run after each new definition;
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
  `f32 f64`, `*T`, `*mut T`, `[T; N]`, slices `[T]`, `str`, `cstr`, `vec<T>`,
  `enum`, `struct`, `union`, and function types `fn(T) -> R`. Arrays cannot be
  passed or returned by value; structs, slices, `str`, and vectors can.
- **Items:** functions (including `extern "C"` and `extern "corex"`),
  `typedef`, `const`, `static`/`static mut`/`static mut volatile` globals,
  enums, structs (`repr(C)`, packed, `align(N)`), unions, file modules
  (`module`/`mod`/`pub`), `#[cfg(...)]` attributes, declarative `macro`s, and
  legacy textual `import`s.
- **Bindings:** `let x: i32 = 1;` (immutable) or `let x = 1;` (inferred);
  `let mut` makes the binding mutable. The initializer is mandatory.
- **Functions:** `fn name(a: T, b: U) -> R { ... }`; recursion allowed.
- **Control flow:** `if/else`, `while`, `loop`, `for`, `do while`, `switch` with
  C fallthrough, labels and `goto`, `break`, `continue`, `return`,
  `unsafe { ... }`. Blocks are statements, not expressions.
- **Operators:** `+ - * / %`, comparisons, `&& || !`, `& | ^ << >>`, compound
  assignment (`+=`, `<<=`, …), unary `-`, `as`, indexing, field access
  (`p->x` is `(*p).x`), slicing, `len(x)`, `sizeof(T)`, `alignof(T)`, and
  `offsetof(T, field)`.
- **Compiler data and systems work:** `vec_new`/`vec_push`/`vec_slice`/
  `vec_free`; `read_file`/`write_file`; `panic`/`assert`; function-pointer
  dispatch tables; unions and tagged unions; bitfields; flexible array members;
  designated/partial initializers; C ABI calls and variadic C calls;
  `main(argc, argv, envp)`; atomics, `volatile`, threads, futex mutexes, TLS,
  inline assembly, and `dlopen` (hosted).
- **Semantics that matter:**
  - integer overflow wraps (`(255 as u8) + 1 == 0`);
  - shift counts are masked (`(1 as u8) << 8 == 1`);
  - integer division by zero, `MIN / -1`, dynamic out-of-bounds indices, and
    slice-range violations abort with a message on stderr and exit 101;
  - literal out-of-bounds indices/ranges are compile errors;
  - floats are IEEE 754 and never abort on division by zero;
  - no implicit numeric conversions — use `as`.
- **Memory:** `alloc(bytes)` → `*mut u8` via `mmap`, `free(ptr, bytes)` via
  `munmap`; the freestanding stdlib adds C-style `malloc`/`calloc`/`realloc`/
  `free`, an `Allocator` interface, and an arena. Raw pointer access, pointer
  arithmetic, integer/pointer casts, atomics, inline asm, and syscalls require
  `unsafe`. No borrow checker: the language does **not** claim memory safety for
  pointer code.
- **Syscalls:** `syscall(nr, a1, ..., a6)` with 1–7 arguments (missing arguments
  are passed as 0), returning `isize` in `rax`.
- **Printing:** `print(value)` writes an integer, `bool`, `char`, float (fixed
  six decimals), enum discriminant, or string/`[u8]` slice plus a newline to
  stdout; no `unsafe`, no heap allocation.
- **Files and errors:** `read_file(path)` returns an owned `vec<u8>`;
  `write_file(path, data)` returns `0`/`-1`; `panic(msg)` and
  `assert(cond, msg)` abort with exit 101. There are no exceptions or `Result`.
- **Entry:** `fn _start()` for full control, `fn main() -> i32` (the compiler
  synthesizes `_start`), or `fn main(argc: i32, argv: **u8, envp: **u8) -> i32`.
  With `--hosted`, the C runtime calls `main` instead.

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
| [`examples/compiler_data.cx`](examples/compiler_data.cx) | tokenizer using `enum`, `struct`, `const`, `vec<T>`, strings, slices, `assert` | token count and kind output, exit 0 |
| [`examples/dispatch.cx`](examples/dispatch.cx) | function-pointer dispatch table, unions of opcode data, `sizeof` | stack-machine result `30`, exit 0 |

---

## Testing

```bash
cd compiler
cargo test                 # 205 tests: 68 unit + 137 integration
cargo test --lib           # stage unit tests only
cargo test --test native   # compile → link → execute, asserts stdout/exit codes
cargo test --test cli      # stdin, exec, REPL, flags and exit codes
cargo test --test ir       # every example is assembled and verified by llvm-as
cargo test --test diagnostics  # invalid programs rejected with specific codes
cargo test --test aggregates   # static array/struct tables, const array lengths
cargo test --test collections  # std_map/std_builder: order, collisions, growth
cargo test --test driver_helpers  # envp lookup, pipe/dup2 capture, run_argv
cargo test --test features     # globals, enums, floats, structs, slices, strings
                               # nested vec<vec<T>> types, shift operators
cargo test --test compiler_caps # vec, const, file I/O, imports, panic/assert
cargo test --test opt_levels   # identical semantics at --opt 0 through 3
                               # (wrapping, shifts, aborts, aliasing, print)
                               # plus 1,104 generated value assertions
cargo test --test robustness   # malformed input yields diagnostics, not panics
cargo test --test extended     # function pointers, switch/for/do/goto, layout, cstr, macros
cargo test --test ffi          # C helper compiled with cc, linked and called both ways
cargo test --test modules      # file modules, cfg, manifest, static and shared libraries
cargo test --test stdlib       # allocator, mem/str, files, formatting, time, math
cargo test --test concurrency  # atomics, volatile, inline asm, threads, futex, TLS
cargo test --test tooling      # fmt, lint, docs, test runner, DWARF, sanitizers, LSP
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

## Self-hosting

A CoreX-written bootstrap compiler lives in `selfhost/`. It compiles its own
source (7,721 lines across nine files), emits LLVM 20 textual IR through
external `llvm-as`/`llc`/`cc`, and produces a freestanding static x86-64 ELF.
Stage 1 is built by the Rust compiler; Stage 2 builds itself and Stage 3
rebuilds it. **Stage 2, Stage 3, and Stage 4 are byte-identical**
(`sha256 644c22c5…`) under `--opt 0` with LLVM 20.1.8 and cc 15.2.0.

```sh
selfhost/bootstrap.sh          # Stage 1-4, hash check, behavioural corpus
tests/selfhosting/run.sh build/stage2/corexc
```

The bootstrap implements a documented subset (see
`docs/selfhosting/bootstrap-architecture.md`); floats, modules, function
pointers, and other v0.3 features are rejected by that compiler, not silently
miscompiled. `vec<T>` and slices `[T]` are supported, including `&array`
coercion, range slicing, runtime index bounds checks, and `str`/`[u8]`
compatibility. Rust `corex` remains the full-language Stage 0 reference.
Details: `docs/selfhosting/master.md`, `bootstrap-results.md`,
`validation-report.md`.

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
    codegen.rs          LLVM 20 textual IR backend, debug metadata, sanitizer mode
    driver.rs           units, interfaces, objects, archives, linking, manifest
    mangle.rs           module symbol/reference rewriting
    format.rs           whitespace formatter
    lint.rs             unused/unreachable/constant-condition warnings
    docgen.rs           Markdown from `///` comments
    lsp.rs              JSON-RPC language server
    repl.rs             interactive session (items accumulate)
    lineedit.rs         TTY line editing + history (stty raw mode)
    main.rs             CLI (files, stdin, inline source, REPL, tooling)
  lsp_main.rs           `corex-lsp` binary
  tests/
    diagnostics.rs      rejection cases by error code
    ir.rs               IR verification and regressions
    native.rs           executed binaries
    cli.rs              stdin, exec, and REPL behavior
    opt_levels.rs       same semantics at --opt 0..3 (wrapping, shifts, aborts)
    aggregates.rs       static array/struct/function-pointer tables, const lengths
    collections.rs      std_map/std_builder behavior, collisions, growth, order
    driver_helpers.rs   envp lookup, pipe/dup2 capture, spawn_argv/run_argv
    features.rs         globals/enums/floats/structs/slices/strings end to end
    compiler_caps.rs    vec, const, file I/O, imports, panic/assert
    robustness.rs       malformed/adversarial input never panics
    extended.rs         function pointers, C control flow, layout, cstr, macros
    ffi.rs              C helper compiled and linked both directions
    modules.rs          modules, cfg, manifest, archives, shared libraries/dlopen
    stdlib.rs           allocator, memory/strings, files, formatting, time, math
    concurrency.rs      atomics, volatile, asm, threads, futex, TLS
    tooling.rs          fmt/lint/doc/test, DWARF, sanitizers, LSP
selfhost/               CoreX-written bootstrap compiler (9 files, 7,721 lines)
tests/selfhosting/      bootstrap corpus and differential harness
lib/corex/              freestanding standard library modules
examples/               hello, factorial, pointers, loop_sum, features, compiler_data,
                        dispatch, playground
bench/                  five kernels with C references
scripts/bench.sh        benchmark harness
scripts/try.sh          compile-and-run one file (check / ir / build modes)
docs/                   spec, grammar, memory model, architecture, PRD, NFR,
                        research, ADRs, task tracker, request log, benchmarks
ENGINEERING.md          roles, conventions, testing and documentation rules
master.md               current status, milestones, next actions
```

---

## Limitations

Explicitly not implemented: tuples, general generics, traits, closures,
exceptions, `async`, labeled `break`, `match`, operator overloading, a package
registry, thread join, language-level `thread_local`, and targets other than
Linux x86-64. Function pointers, unions, namespaced file modules, `extern`/C FFI,
macros, threads, debug information, and tooling are implemented (see the spec).

Feature boundaries:

- **Globals.** Constant initializers support integers, `bool`, `char`, floats,
  enums, string literals, `&global`, and `&function`. Aggregate constants are
  supported for `static` declarations: arrays (including nested arrays and
  arrays of structs), structs with a plain C layout, `str` elements, and
  function-pointer tables are emitted as real LLVM globals with no runtime
  initializer. Static struct literals outside the plain C layout are rejected
  with `E0226`: unions, bitfields, packed/`align(N)` structs, and flexible array
  members. Constant initializers may only read previously declared immutable
  globals (forward references are rejected with `E0226`). Array lengths may be
  integer literals, previously
  declared integer `const` names, or constant arithmetic over them; a length
  that is not constant is `E0100`.
- **Slices and strings.** `[T]` and `str` are shallow two-word copies and have no
  equality, concatenation, or formatting operators. A slice has no implicit
  conversion to an array; create one with a string literal, `&array` in a slice
  context, or `s[a..b]`. Writing through a slice that points at read-only data
  (for example a string literal) is undefined behavior even when the slice
  binding is mutable.
- **Structs.** Field access through a pointer is explicit: use `(*p).x`, not
  `p.x`. Structs cannot be empty and have no built-in equality or printing;
  compare fields or write a function.
- **Dynamic arrays.** `vec<T>` is a 24-byte `{data,len,cap}` header. Copying a
  vector copies the header and shares the backing block; two owning copies can
  double-free. Treat a vector as owned by one binding and access it through
  `&`/`&mut`.
- **Modules.** `module name;` + `mod name;` compile each file independently and
  expose only `pub` items through generated interfaces. Legacy
  `import "file.cx";` still expands textually into one flat namespace. Struct
  fields have no per-field visibility. Public signatures may not expose private
  local types.
- **Threads/TLS.** `std_thread::spawn` exists but there is no join; threads exit
  via `exit(2)`. TLS is the explicit `std_tls` table keyed by thread id, not a
  compiler `thread_local`. Stacks are caller-allocated and must be freed only
  after the thread exits.
- **Function pointer ABI.** Function-pointer calls use the CoreX internal
  aggregate convention; a C function with aggregate parameters must be called
  directly through its `extern "C"` declaration.
- **Error handling.** `panic` and `assert` abort with a message and exit 101.
  There are no exceptions, `Result`, `Option`, or `?`; recoverable failures use
  ordinary values.
- **File I/O.** The `read_file` builtin returns an empty `vec<u8>` on failure (it
  does not distinguish empty files from errors). `std_fs::read_file(path, buf,
  cap)` returns the byte count, `0` for an empty file, and `-1` on failure for
  callers that need the distinction (such as a driver). `write_file` returns `0`
  or `-1`. Paths are byte strings and are not UTF-8 validated.
- **Floats.** `print` uses six fractional digits, truncated toward zero (not
  rounded). Non-finite values print as
  `nan`/`inf`/`-inf`; magnitudes ≥ 1e18 abort with exit 101. There is no decimal
  type and no implicit integer/float conversion.
- **Strings.** A string literal is `str` by default; assigning it to `[u8; N]`
  requires exactly `N` bytes. There is no UTF-8 validation, NUL terminator, or
  string interpolation.

Safety: pointer validity, allocation lifetime, alignment, double free, and
use-after-free are the programmer's responsibility. The compiler guarantees types,
mutable-place writes, `unsafe` marking, return paths, constant index/range bounds,
and runtime aborts for dynamic bounds and division by zero. See
[`docs/memory-model.md`](docs/memory-model.md) §6–§7.

---

## Documentation index

| Document | Contents |
|---|---|
| [`docs/installation.md`](docs/installation.md) | Toolchain setup, build, verification, environment variables, troubleshooting |
| [`CHANGELOG.md`](CHANGELOG.md) | Release notes and version history |
| [`docs/resources.md`](docs/resources.md) | Curated project docs, examples, tests, source map, and external references |
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
| [`docs/self-hosting.md`](docs/self-hosting.md) | Self-hosting assessment: verdict, component mapping, blockers, bootstrap plan, effort and risk (pre-implementation baseline) |
| [`docs/selfhosting/master.md`](docs/selfhosting/master.md) | Self-hosting status: stages, hashes, validation, next priorities |
| [`docs/selfhosting/bootstrap-architecture.md`](docs/selfhosting/bootstrap-architecture.md) | Stage model, subset, toolchain, determinism, ABI |
| [`docs/selfhosting/bootstrap-results.md`](docs/selfhosting/bootstrap-results.md) | Measured Stage 1–4 results, hashes, corpus |
| [`docs/selfhosting/validation-report.md`](docs/selfhosting/validation-report.md) | Layered validation evidence and limits |
| [`lib/corex`](lib/corex) | Freestanding standard library source (modules listed in spec §12.7) |

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

# CoreX Self-Hosting Assessment

> **Implementation status (2026-10-28):** this document is the pre-implementation
> assessment. A CoreX-written bootstrap compiler now exists under `selfhost/`;
> Stage 1–4 are built by `selfhost/bootstrap.sh`, and Stage 2 == Stage 3 ==
> Stage 4 byte-for-byte. See `docs/selfhosting/bootstrap-results.md` and
> `validation-report.md`. The analysis below remains the rationale and risk
> record, not the current status.


**Question:** Is CoreX stable and complete enough to reimplement the CoreX
compiler in CoreX today?

**Verdict: conditionally yes — the language and library prerequisites are now
closed.** CoreX is expressive enough to write a compiler: every Rust construct
in `compiler/src` has a CoreX encoding, and a compiler-shaped microcosm (arena
AST, tagged unions, string-keyed hash table, function-pointer dispatch,
IR-style text emission) compiled and ran correctly at `--opt 0,1,2,3`. The five
language/library blockers listed in §1.1 (P0-1 … P0-5) are implemented and
tested on branch `room/corex-p0-blocker-fixes-by6h`: static aggregate
constants, `const` array lengths, `std_map`/`std_builder`, driver process and
environment helpers, nested `vec<vec<T>>` syntax, and `switch` divergence.
What remains before starting the port is process work, not language design: a
CoreX-native differential test harness (P0-6) and a performance baseline (P0-7).

| Question | Answer |
|---|---|
| Can CoreX express a compiler frontend/backend? | Yes — see §4.2 probes and §5.5 microcosm |
| Are the P0 language/library blockers closed? | **Yes** — P0-1 … P0-5 implemented and tested (§1.1, §8.1) |
| Is it ready to start the port **today**? | Almost — remaining gates are P0-6 (differential harness) and P0-7 (performance baseline) |
| Is a full self-host realistic after prerequisites? | Yes, at 6–9 engineer-months for one experienced compiler engineer; 3–5 months for a small team |
| Recommended next step | Build P0-6/P0-7, then run the 4–6 week spike in §8 with a go/no-go gate |

Assessment baseline: `main` at commit `2b10973` ("docs: sync merged C-class
test counts and limitations"), 2026-10-22. Worktree branch:
`room/corex-self-hosting-assessment-xvuu`.

**Update (2026-10-22, branch `room/corex-p0-blocker-fixes-by6h`):** P0-1 … P0-5
are implemented, tested, and committed; `cargo test` reports **205 passed, 0
failed** (68 unit, 137 integration), and every example prints identical output
and exit status at `--opt 0,1,2,3`. The rows below record the new evidence. The
remaining gates are P0-6 and P0-7 in §8.1, plus B6 if parallel builds are ever
required.

Toolchain used for every check below: Rust 1.99.1, LLVM 20.1.8, gcc 15.2.0,
Linux x86-64.

---

## 1. Executive verdict

### 1.1 Exact blockers

These were the blockers at baseline. Five are now closed; each closed row names
the test that proves it.

| # | Blocker | Status and evidence | Remaining impact |
|---|---|---|---|
| B1 | **Compile-time aggregate constants.** Baseline: `static`/`const` initializers accepted only scalars, `str`, `&global`, and `[u8; N]` from a string literal; array/struct/function-pointer tables and `const` array lengths were rejected (`E0226`, `E0100`). | **Closed.** `static` arrays, nested arrays, structs (plain C layout), `str` elements, `&global`, and `&fn` elements emit real LLVM global initializers; integer `const` names and constant expressions are valid array lengths. `tests/aggregates.rs` runs the tables at `--opt 0..3` and verifies the IR with `llvm-as`; spec §4.2/§4.3. | None for data layout; static structs still require a plain C layout (unions/bitfields/packed are `E0226`). |
| B2 | **No map or set in `lib/corex`.** | **Closed.** `lib/corex/std_map.cx` adds an insertion-ordered string-keyed `Map` (u64 values, open-addressed index over a dense entry array, `map_get` with a found flag) and `Set`; keys are borrowed `str`. `tests/collections.rs` covers growth, collisions, missing keys, and iteration order. | No generics: one value type (`u64`) keeps the API small, as intended. |
| B3 | **No growable string builder or owned string.** | **Closed.** `lib/corex/std_builder.cx` adds `Builder` over `std_alloc::realloc` with reserve, byte/str/char/i64/u64/hex/f64 appends, `len`, `data`, `finish`, and a latched `failed` flag; every push returns `bool`. Number formatting is delegated to `std_fmt`, so there is one implementation. `tests/collections.rs` covers growth and 64-bit/hex/float formatting. | `finish` returns a borrowed `str`; there is still no owned string type, by design. |
| B4 | **Driver primitives were missing.** | **Closed.** `lib/corex/std_env.cx` scans `envp` (`env_get` with found flag, `env_has`, `env_get_or`, no allocation); `std_io` adds `pipe`, `dup2`, `read_all`; `std_process` adds `spawn_argv`/`run_argv`; `std_fs::read_file` returns `-1` on failure and `0` for an empty file (documented in the module). `tests/driver_helpers.rs` forks `/bin/echo`, captures stdout through a pipe, and checks the exit status. | None for invoking LLVM tools; `exec` still needs a caller-built NULL-terminated vector. |
| B5 | **Three small language/semantic gaps.** | **Closed.** `vec<vec<u8>>` and `vec<vec<vec<i32>>>` parse without whitespace by splitting `>>` only in type-argument position; `>>`/`>>=` remain shift operators in expressions. A `switch` with `default` diverges when every arm diverges without a `break`; fallthrough semantics are unchanged. Parser and semantic unit tests, `tests/features.rs::nested_generic_types_and_shift_operators`, `tests/extended.rs::switch_with_default_satisfies_return_analysis`, and the E0206 diagnostics test cover it. | None. |
| B6 | **No thread join and no language-level TLS.** | **Open.** `lib/corex/std_thread.cx` has spawn/yield/exit/free_stack; `docs/tasks.md` known gaps; `docs/language-spec.md` §13. | A compiler port must be single-threaded or use `fork`/`wait` per unit. Parallel compilation with processes works; threads do not. |

### 1.2 Why the answer is not simply "CoreX is Turing-complete"

Turing completeness is irrelevant to maintainability. The real question is
whether a compiler can be written with the data structures and I/O a compiler
needs, without a second runtime or a hidden allocation model. The answer is
yes in principle, with C-in-C ergonomics: tagged unions instead of payload
enums, `switch` instead of `match`, explicit context pointers instead of
closures, hand-written containers instead of generics, and arena allocation
instead of `Box`/`Rc`.

The port is therefore **feasible but expensive**. The language and library
prerequisites are now in place; what remains is the differential harness and a
performance baseline, plus the ordinary engineering risk of a 20k+ line port.

---

## 2. Current capability summary

### 2.1 Language

Implemented, documented in `docs/language-spec.md` and exercised by the suite:

- scalar types `i8..i64`, `u8..u64`, `usize`/`isize`, `bool`, `char`, `f32`/`f64`;
- pointers `*T`/`*mut T`, arrays `[T; N]`, slices `[T]`, `str`, `cstr`,
  `vec<T>`, structs, unions, bitfields, flexible array members, and
  function types `fn(T) -> R`;
- C control flow: `if`, `while`, `loop`, `for`, `do while`, `switch` with
  fallthrough, labels/`goto`, `break`, `continue`, `return`, `unsafe`;
- functions, recursion, `extern "C"` declarations/exports, variadic C calls,
  `main(argc, argv, envp)`, `_start`;
- modules (`module`/`mod`/`pub`), generated interfaces, `corex.toml`,
  `#[cfg]`, separate compilation, static archives, shared objects, `dlopen`;
- `const`, statics (including `static mut`), enums (i32 discriminants), typedefs,
  `repr(C)`, packed/`align(N)` structs, `sizeof`/`alignof`/`offsetof`;
- atomics, volatile globals, inline asm, threads/futex/TLS, expression macros,
  DWARF 5 debug info, sanitizers;
- freestanding stdlib in `lib/corex` (16 modules).

Not implemented (spec §13, README "Limitations"): tuples; generics beyond
`vec<T>`; traits; closures; exceptions; `async`; `match`; payload enums;
operator overloading; destructors; labeled `break`; language `thread_local`;
thread join; targets other than Linux x86-64.

### 2.2 Compiler

`compiler/src` is 18,020 lines including inline tests (17,289 non-test lines),
zero external crates, Rust edition 2021, LLVM 20 textual IR.

| Area | File | Lines | Role |
|---|---|---:|---|
| Tokens | `token.rs` | 267 | 103 `TokenKind` variants, descriptions |
| Diagnostics | `diagnostics.rs` | 321 | spans, source map, rendering |
| Lexer | `lexer.rs` | 787 | byte-oriented, structured errors |
| AST | `ast.rs` | 847 | `Program`, `Stmt` (13 variants), `ExprKind` (11), `Type` (15), layouts |
| Parser | `parser.rs` | 2,301 | recursive descent + precedence climbing, recovery, macros, cfg |
| Semantic | `semantic.rs` | 4,330 | scopes, types, layouts, const eval, mutability, unsafe, return flow |
| Codegen | `codegen.rs` | 4,001 | textual LLVM IR, ABI, debug metadata, sanitizer mode |
| Driver | `driver.rs` | 1,357 | units, interfaces, objects, archives, linking, manifest |
| Mangle | `mangle.rs` | 404 | deterministic `module$item` rewriting |
| Formatter | `format.rs` | 270 | whitespace-only, idempotent |
| Linter | `lint.rs` | 356 | `W0001`–`W0004` |
| Docgen | `docgen.rs` | 217 | Markdown from `///` |
| LSP | `lsp.rs` | 908 | dependency-free JSON-RPC over stdio |
| REPL | `repl.rs` | 508 | accumulating definitions, `:` commands |
| Line editor | `lineedit.rs` | 273 | raw mode via `stty` |
| CLI | `main.rs` | 844 | commands, stdin, exit codes |

Pipeline: lex → parse → semantic side tables → textual IR → `llvm-as` →
`opt -passes=default<O{n}>` → `llc` → `cc -nostdlib -static -no-pie`.

### 2.3 Standard library

`lib/corex`: `std_alloc` (malloc/calloc/realloc/free, `Allocator`, arena),
`std_mem`, `std_str`, `std_io` (including `pipe`, `dup2`, `read_all`),
`std_fs` (stat, directories, read/write, failure-distinguishing `read_file`),
`std_process` (fork/exec/wait, `spawn_argv`/`run_argv`), `std_env` (`envp`
lookup), `std_time`, `std_math`, `std_fmt`, `std_map` (insertion-ordered
string-keyed map/set), `std_builder` (growable byte builder),
`std_sync` (futex mutex/condvar/spinlock), `std_thread`, `std_tls`, `std_dl`.

The collection and builder gap is closed: `std_map` stores borrowed `str` keys
with `u64` values in insertion order and `std_builder` grows with
`std_alloc::realloc` and reports failure through a return value.

### 2.4 Tooling, tests, benchmarks, build

- CLI: `build run exec repl emit-ir check fmt lint doc test`; `corex-lsp`.
- Tests: **205 passed, 0 failed** (68 unit + 137 integration). Per binary:
  `aggregates` 6, `cli` 17, `collections` 2, `compiler_caps` 8, `concurrency` 3,
  `diagnostics` 29, `driver_helpers` 3, `extended` 9, `features` 8, `ffi` 2,
  `ir` 6, `modules` 7, `native` 18, `opt_levels` 5, `robustness` 4,
  `stdlib` 3, `tooling` 7. Unit tests: `semantic` 29, `parser` 18, `lexer` 9,
  `diagnostics` 4, `format` 4, `lsp` 3, `docgen` 1.
- Native coverage: compile → link → execute and assert stdout/exit status;
  `tests/native.rs::repeated_builds_are_byte_identical`; every example is
  assembled and verified in `tests/ir.rs`; static aggregate tables are executed
  at `--opt 0..3` and their IR is verified in `tests/aggregates.rs`.
- Optimization-level parity: `tests/opt_levels.rs` runs fixed cases, all
  examples, and 1,104 generated value assertions at `--opt 0..3`.
- Benchmark suite: `bench/*.cx` + `bench/*.c`, `scripts/bench.sh`.
- Build: `cargo build --release` in `compiler/` (3.86 s on the assessment
  machine), requires `rustc`/`cargo`, LLVM 20 tools, and `cc`.
- README claims are consistent with this run except for known stale prose in
  `docs/architecture.md` (line counts and module descriptions are from v0.1) and
  some task-tracker counts; treat code and tests as authoritative.

---

## 3. Compiler component mapping

The table maps each Rust area to the CoreX facility that would replace it. "Cost"
is relative effort in CoreX, not a blocker by itself.

| Rust area | What it does | CoreX encoding | Cost / risk |
|---|---|---|---|
| `lexer.rs` + `token.rs` | Byte scanner, 103 token kinds, spans | `enum TokenKind` (no payload), `struct Token { kind, span }`, `u8` indexing, `switch`; `examples/compiler_data.cx` is a working tokenizer | Low. `switch` fallthrough and no `match` make the dispatch table verbose. |
| `ast.rs` | Untyped AST, `NodeId`, `Type`, layouts | Tagged structs: `struct Expr { kind: ExprKind, span: Span, ... }` plus a `union` payload; recursive links via `*mut`; `NodeId` as `u32`/`usize` | Medium–high. 11 `ExprKind` and 13 `Stmt` variants become `switch` trees; every node must be initialized field-by-field. |
| `parser.rs` | Recursive descent, precedence climbing, recovery, macros | Free functions + explicit parser state; `Result<T, ()>` becomes an out-parameter or sentinel return plus a diagnostics vector; `goto` is available for recovery | High. 2,106 lines of Rust with 28 `match` sites; CoreX version is plausibly 2,500–3,500 lines. |
| `semantic.rs` | Scopes, name resolution, type checking, layout, const eval, flow | `Vec` of scopes with string-keyed maps; `NodeId` side tables as `vec<T>` indexed by id; layouts as structs; diagnostics vector | High. 4,216 lines and 76 `match` sites; `Rc<StructInfo>` becomes an arena pointer or manual refcount. |
| `codegen.rs` | Textual LLVM IR, ABI, debug metadata, helpers | `vec<u8>`/custom buffer + `std_fmt`; explicit `push_str`/`push_i64`; label maps as custom maps; `String` return values as `(ptr,len)` pairs | High. 673 `format!` calls become manual formatting; this is the largest mechanical rewrite. |
| `driver.rs` | Units, interfaces, tool invocation, linking, manifest | `read_file`/`write_file`, `std_fs` dirs/stat, `fork`/`exec`/`wait`, temp dirs from `getpid` + `monotonic_ns`, `cc`/`llvm-*` argv arrays | High. `Command::output()` capture, `PATH`/`COREX_LLVM_BIN`, and arbitrary argv are not in the stdlib (B4). |
| `mangle.rs` | Symbol/qualified-name rewriting | Custom string map/set; symbol table as `vec` + hash index | Medium. Straightforward once B2 exists. |
| `format.rs` | Line-oriented whitespace normalizer | Byte/char loop over `str`, `vec<u8>` output; same state machine | Low–medium. Pure string processing. |
| `lint.rs` | Unused/unreachable/constant-condition warnings | AST walk, scope stack, sets | Medium. Same AST traversal cost as semantic. |
| `docgen.rs` | `///` comments → Markdown | Line scanner + buffer | Low. |
| `lsp.rs` | JSON-RPC, diagnostics, completion, definition, references, rename, hover, format | Tagged-union JSON (`Json` enum becomes tag + union), `vec` of key/value pairs, sorted arrays where order matters; stdio read/write | Medium–high. 908 lines; no `BTreeMap`, so deterministic object order needs a sorted array or explicit order. |
| `repl.rs` | Accumulating definitions, candidate selection | Source buffers, parser driver; `stty` via `std_process` or syscalls | Medium. |
| `lineedit.rs` | Raw mode, history, key handling | `read` on fd 0, `stty` child process, byte classification | Low–medium. |
| `main.rs` | CLI, stdin, exit codes | `main(argc, argv, envp)`, `read_file`, `write_file`, `exit` | Low. |

### 3.1 Lexer, parser, AST representation

CoreX has exactly the pieces a hand-written frontend needs: a byte-indexable
`str`, `u8`/`char` literals, integer bases, `switch`, structs, enums, pointers,
and `vec<T>`. `examples/compiler_data.cx` already tokenizes source with
`enum TokenKind`, `struct Token`, `const`, `vec<Token>`, slices, and `assert`,
and `tests/compiler_caps.rs::compiler_data_example_tokenizes` runs it.

The missing construct is a sum type with payloads. CoreX's union + tag pattern
works, and the payload is accessed through `unsafe`:

```corex
enum Kind { Num, Add, Mul }
struct Node { kind: Kind, value: i64, lhs: *mut Node, rhs: *mut Node }
union Payload { num: i64, pair: Pair }
struct Expr { kind: Kind, payload: Payload }
```

This is the C pattern; it is expressive, but every visitor is an explicit
`switch` with `break` and a payload cast. The Rust AST is 840 lines; a CoreX
equivalent is likely 1,200–1,800 lines.

### 3.2 Symbol tables and name resolution

Rust uses **88 `HashMap`** and **37 `HashSet`** occurrences across
`compiler/src`. The important ones:

- `HashMap<NodeId, Type>` / `HashMap<NodeId, LocalRef>` / `HashMap<NodeId, Callee>`
  — these become `vec<T>` indexed by `NodeId`, which is faster and simpler.
- `HashMap<String, FnSig>` / `HashMap<String, LocalRef>` / `HashMap<String, ...>`
  — these need a string-keyed map. CoreX has none.
- `HashMap<(u32,u32,u32), u32>` (debug scopes) and `HashMap<Type, u32>` —
  key encoding must be manual.
- `HashSet<String>` in `mangle.rs` and `lint.rs` — needs a set.

A string-keyed open-addressing table with `u64` FNV-1a is about 40 lines in
CoreX and was exercised in the microcosm (§5.5). It must be added to
`lib/corex` with a deterministic iteration order for reproducible builds.

### 3.3 Semantic analysis and layout

`semantic.rs` is the largest module (4,216 lines, 76 `match` sites). It produces
side tables only; the AST is never mutated. That design ports well:

- `expr_types`, `idents`, `calls`, `let_slots` → `vec` indexed by `NodeId`;
- `fn_sigs`, `globals`, `consts`, `enum_defs`, `struct_defs` → string maps;
- scopes → `vec` of `Scope { names: map }`;
- `Rc<StructInfo>` → arena pointer, since structs live to the end of compilation;
- `Cell`/`RefCell` → ordinary fields, because the port can pass `*mut Analyzer`
  explicitly;
- constant evaluation, layout, and diagnostics are ordinary arithmetic and
  struct code.

The hard part is volume and type dispatch, not missing semantics. CoreX already
implements the checks the compiler relies on: no implicit conversions, mutable
places, `unsafe` gating, return flow, and bounds diagnostics.

### 3.4 LLVM IR emission and string building

`codegen.rs` is a textual emitter: it builds `String`s with `format!` (673
occurrences), stores function bodies in `Vec<String>`, and deduplicates string
literals with a `Vec` scan. `std_builder::Builder` now provides the growable
byte buffer (`reserve`, `push_byte`, `push_str`, `push_i64`, `push_hex`,
`push_f64`), and `std_fmt` remains the typed number formatter it calls into.

The port should emit IR and diagnostics through `Builder`; do not add a second
formatting path. The risk is silent formatting drift in 673 call sites, which
differential IR tests catch.

### 3.5 Driver, subprocesses, and file I/O

The Rust driver's four jobs now have CoreX counterparts:

1. **Tool discovery** reads `COREX_LLVM_BIN` and `PATH`, then probes
   `/usr/lib/llvm-N/bin` (`driver.rs:158-190`). CoreX scans `envp` from `main`
   with `std_env::env_get`/`env_has` and checks paths with `std_fs::exists`.
2. **Capturing tool output** uses `Command::output()` for `llvm-as`, `opt`,
   `llc`, and `cc`. CoreX uses `std_io::pipe` (syscall 22),
   `std_io::dup2` (syscall 33), and `std_io::read_all`, then
   `std_process::wait` for the status.
3. **Arbitrary argv** is needed for `llc -filetype=obj ...` and `cc -nostdlib
   ...`. `std_process::spawn_argv(path, argv, envp)` and `run_argv` take a
   caller-built NULL-terminated `**u8` array; `exec(path, argv, envp)` still
   accepts the full array directly.
4. **Temp directories** need `getpid`, a monotonic timestamp, `mkdir`, and
   cleanup. `std_process::getpid` and `std_time::monotonic_ns` exist;
   `std_fs::mkdir`/`rmdir` exist.

`std_fs::read_file(path, buf, cap)` returns the byte count, `0` for an empty
file, and `-1` on failure, so the driver does not need to `stat` first. The
`read_file(path) -> vec<u8>` builtin keeps its empty-vector-on-failure contract.

### 3.6 CLI, REPL, and tooling

- `main.rs` maps directly to `main(argc, argv, envp)` and `exit`; stdin source
  uses `read(0, ...)`; `exec` uses in-memory source.
- `repl.rs` is source accumulation and re-parsing; no special runtime.
- `lineedit.rs` shells out to `stty` in Rust; CoreX can do the same with
  `std_process` (needs a `spawn_argv` helper) or raw `ioctl` via syscall.
- `format.rs`, `lint.rs`, and `docgen.rs` are string/AST passes.
- `lsp.rs` is the only module needing a JSON value tree and ordered objects;
  a tagged union plus `vec<(str, Json)>` works, with explicit key sorting where
  the protocol requires stable order. `BTreeMap` is not needed semantically.

---

## 4. Hard blockers vs friction

### 4.1 Classification

| Area | Status in CoreX | Classification | Workaround |
|---|---|---|---|
| Payload enums / `match` | Not supported (`enum E { A(i32) }` → `E0100`; `match` → `E0100`) | **Friction, high cost** | Tagged struct + union + `switch`; proved in §5.5 |
| Generics | Only `vec<T>` (`docs/language-spec.md` §3) | **Friction** | Duplicate containers per type; store `u64` indices in one map |
| Traits | Not supported | **Friction** | Function-pointer vtables (`fn(*mut u8) -> ...` + context) |
| Closures | Not supported (closure literal → `E0101`) | **Friction** | Explicit context struct + function pointer; proved in §4.2 |
| Maps / hash tables | `std_map`: insertion-ordered string-keyed `Map` (u64 values) and `Set` | **Library present** | None; borrowed keys mean key storage must outlive the entry |
| String builders / formatting | `std_builder` grows with `std_alloc::realloc`; `std_fmt` remains a typed fixed-buffer writer | **Library present** | Use `builder_*` for output, `std_fmt` when a caller-owned buffer is enough; `std_str::str_eq` for comparison |
| Error propagation | No `Result`, `?`, or exceptions; `panic` aborts 101 | **Friction** | Return codes + out-params + diagnostics `vec` |
| Static aggregate constants | Supported for `static` arrays/structs/`str`/`&global`/`&fn`; integer `const` names and constant expressions are array lengths | **Good** | Struct literals require a plain C layout (`E0226` otherwise) |
| Macros / hygiene | Expression macros only, non-hygienic, unit-local (spec §12.10) | **Friction** | Ordinary functions; avoid macros in the port |
| Thread join / TLS | No join; explicit `std_tls` table (spec §13) | **Hard limit for concurrency** | Single-threaded compiler; `fork`/`wait` for parallel units |
| ABI / FFI | `extern "C"`, `cstr`, varargs, `main(argc,argv,envp)` all work | **Good** | None needed for the compiler itself |
| Build / bootstrap | Rust `cargo` builds stage0; CoreX invokes LLVM through `spawn_argv`/`run_argv`, `pipe`/`dup2`, and `read_all` | **Process gap, not language** | Keep Rust as stage0; build the P0-6 differential harness |
| Performance | Kernels at C parity except fib (~1.4–1.6×); frontend 22 ms / 4,001 lines | **Unvalidated for a CoreX-written compiler** | Measure a spike before committing; budget 2–4× Rust frontend |

### 4.2 Probes run against the current compiler

All probes were compiled with the release `corex` binary built from this
checkout. Source files live outside the repository (`/tmp`) and were not added
to the project.

| Probe | Result | Code |
|---|---|---|
| Payload enum | error | `enum E { A(i32), B }` → `E0100: expected '}', found '('` |
| `match` | error | `match x { ... }` → `E0100: expected ';', found identifier` |
| Generic function | error | `fn id<T>(x: T) -> T` → `E0100: expected '(', found '<'` |
| Closure | error | closure literal (`let f = ...`) → `E0101: expected expression` |
| Static aggregate table | **works** | `static TOKENS: [str; 2] = ["a","b"];` emits `@TOKENS = constant [2 x { ptr, i64 }]`; `tests/aggregates.rs` |
| Static function-pointer table | **works** | `static TABLE: [fn(i64)->i64; 1] = [&a];` emits `[1 x ptr] [ptr @a]` |
| `const` as array length | **works** | `const N: usize = 4; static A: [u8; N] = ...` and `[u8; N * 2]` |
| `str` equality | error | `"a" == "b"` → `E0210`; use `std_str::str_eq` |
| Nested `vec` without spaces | **works** | `vec<vec<u8>>` and `vec<vec<vec<i32>>>` parse and run; `>>`/`>>=` stay shifts in expressions |
| `switch` + all cases return | **works** | A `switch` with `default` and diverging arms satisfies return analysis; no trailing `return` needed |
| `switch` fallthrough | behavior | `case 1: print(1); case 2: print(2);` prints `1 2`; use `break` |
| `envp` lookup / pipe capture | **works** | `std_env::env_get`, `std_io::pipe`/`dup2`/`read_all`; `tests/driver_helpers.rs` forks `/bin/echo` and checks its status |
| Tagged union + `switch` | **works** | `struct Node { tag, payload: union }`; prints `42` |
| Function-pointer context | **works** | `fn apply(f: fn(*mut u8)->i64, ctx: *mut u8)` prints `5` |
| C varargs (`--hosted`) | **works** | `extern "C" fn snprintf(...);` with `c"%d-%s"` links and runs |
| `main(argc, argv, envp)` | **works** | `tests/ffi.rs::main_receives_argc_argv_and_envp` |
| Local aggregate tables | **works** | `let table: [fn(i64)->i64; 2] = [&f, &g];` prints `40` |

### 4.3 What remains, and what no longer blocks

Static aggregate data used to be the one construct whose absence changed the
design of the compiler rather than just its verbosity. It is now supported:
tables such as keyword names, token descriptions, diagnostic catalogs, LLVM type
spellings, and function-pointer dispatch maps can be declared directly.

```corex
const N: usize = 2;
static NAMES: [str; 2] = ["fn", "let"];
static OPS: [fn(i64, i64) -> i64; 1] = [&add];
struct Entry { name: str, code: i32 }
static TABLE: [Entry; 2] = [Entry { name: "a", code: 1 },
                           Entry { name: "b", code: 2 }];
```

These emit real LLVM global initializers; no `init_tables()` function, no
startup allocation, and no global initialization order. `tests/aggregates.rs`
compiles and runs the tables at `--opt 0..3` and verifies the IR with `llvm-as`.

The remaining hard limit is **B6** (no thread join or language-level TLS), which
affects only parallel compilation. Everything else is C-in-C ergonomics with a
proven encoding: tagged unions instead of payload enums, `switch` instead of
`match`, explicit context pointers instead of closures, hand-written containers
instead of generics. Performance for a CoreX-written compiler is still
unvalidated (P0-7), and the differential harness (P0-6) does not exist yet.

---

## 5. Evidence checks

### 5.1 Full test suite

```console
$ cd compiler
$ PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo test
...
test result: ok. 68 passed; 0 failed   (lib)
test result: ok. 6 passed;  0 failed   (aggregates)
test result: ok. 17 passed; 0 failed   (cli)
test result: ok. 2 passed;  0 failed   (collections)
test result: ok. 8 passed;  0 failed   (compiler_caps)
test result: ok. 3 passed;  0 failed   (concurrency)
test result: ok. 29 passed; 0 failed   (diagnostics)
test result: ok. 3 passed;  0 failed   (driver_helpers)
test result: ok. 9 passed;  0 failed   (extended)
test result: ok. 8 passed;  0 failed   (features)
test result: ok. 2 passed;  0 failed   (ffi)
test result: ok. 6 passed;  0 failed   (ir)
test result: ok. 7 passed;  0 failed   (modules)
test result: ok. 18 passed; 0 failed   (native)
test result: ok. 5 passed;  0 failed   (opt_levels)
test result: ok. 4 passed;  0 failed   (robustness)
test result: ok. 3 passed;  0 failed   (stdlib)
test result: ok. 7 passed;  0 failed   (tooling)
```

Total: **205 passed, 0 failed**. This matches the README and `docs/tasks.md`.
The suite includes native compile/run tests, IR verification, diagnostics,
modules/archives/shared objects, C ABI in both directions, DWARF inspection,
sanitizers, an LSP session, optimization-level equivalence, static aggregate
tables, collections/builder behavior, and driver process capture.

### 5.2 Examples at every optimization level

`compiler/target/release/corex run <example> --opt N` for `N = 0,1,2,3`:

| Example | stdout | exit |
|---|---|---|
| `hello.cx` | `Hello, World!` | 0 |
| `factorial.cx` | `3628800` | 0 |
| `pointers.cx` | `123456789` | 0 |
| `loop_sum.cx` | (empty) | 51 |
| `features.cx` | 13 lines, first `13`, `4`, `4`, `6`, `true` | 0 |
| `compiler_data.cx` | `9`, `0`, `fn`, `4`, `1` | 0 |
| `dispatch.cx` | `30`, `true` | 0 |
| `playground.cx` | `A` | 147 |

All outputs and exit statuses are identical at `--opt 0,1,2,3`.

### 5.3 Benchmarks (independent run)

`./scripts/bench.sh 3` on this machine (AMD Ryzen 7 6800H, gcc 15.2.0):

| Kernel | CoreX / C median |
|---|---:|
| `mix` | 1.001 |
| `fib` | 1.467 |
| `array` | 0.955 |
| `struct` | 1.000 |
| `f64` | 1.000 |

This is consistent with `docs/benchmarks.md` (which reports `fib` 1.636 on a
7-run sample). Generated code is C-class for arithmetic, pointers, structs, and
floating point; recursive call-heavy code is 1.4–1.6× slower. For a compiler,
the relevant workload is pointer-heavy and branch-heavy, so expect C-class
performance, not Rust-class.

### 5.4 Known gaps from project docs (verified against source)

- `handoff.md`: no thread join; explicit TLS table; expression macros only;
  `volatile` globals only; no package manager; no non-x86-64 targets.
- README "Limitations": no tuples/generics beyond `vec<T>`/traits/closures/
  exceptions/async/labeled break/`match`/operator overloading; vec copies are
  shallow; arrays cannot be passed/returned by value; the `read_file` builtin
  cannot distinguish an empty file from an error (use `std_fs::read_file`).
- `docs/tasks.md`: T-001…T-028, T-041…T-046, T-049…T-055, and T-056…T-078
  COMPLETED; no IN PROGRESS or BLOCKED items. All roadmap claims are tied to
  named tests.
- `docs/PRD.md` §7 still lists "Self-hosting: the compiler stays in Rust;
  CoreX cannot compile itself" as a v0.1 non-goal. That statement describes
  intent at v0.1, not a language impossibility; this assessment supersedes it
  as a planning question.
- `docs/architecture.md` is stale: its module line counts and pipeline
  description date from v0.1. Current line counts are in §2.2.

### 5.5 What can be written in CoreX today

A compiler-shaped microcosm was implemented and run at all optimization levels:

- bump arena over `std_alloc::malloc`;
- `Node { kind: enum, value, lhs: *mut Node, rhs: *mut Node }`;
- recursive evaluator using `switch` on the tag;
- recursive IR-style emitter using `std_fmt::Fmt`;
- string-keyed hash table with FNV-1a and linear probing;
- `std_str::str_eq` for keys;
- a function-pointer dispatch call with a `*mut u8` context.

Output at `--opt 0,1,2,3`, identical at every level and exit 0:

```
42
-1
50
8
((6 * 7) + 8)
160
```

This is direct evidence that the core data structures and control flow of a
compiler are expressible. The gaps are library ergonomics, not semantics.

---

## 6. Bootstrap plan

### 6.1 Principles

1. **The Rust compiler remains the reference implementation and stage0.** It is
   the oracle for every differential check.
2. **Port in dependency order, one gate at a time.** Do not start semantic
   analysis before the lexer/parser can round-trip the corpus.
3. **Single compilation unit first.** Use `import`/one unit to avoid module
   interface bugs during the early port; switch to file modules and
   `corex.toml` only after the compiler can compile itself as one unit.
4. **Determinism is a feature.** Stable maps, insertion-order iteration, and
   counter-based temporary names are required for the fixed-point test.
5. **No feature creep.** Do not add `match`, traits, or closures for the port;
   use the C-style encodings. The language should not change merely to make the
   bootstrap easier.

### 6.2 Phases and gates

| Phase | Work | Exit gate |
|---|---|---|
| **0. Prerequisites** | Implement P0 items in §8; add the differential harness and a CoreX-native test runner | `cargo test` remains green (205); map/builder/driver helpers and aggregate tests pass; probes B1/B2/B3/B4 have documented workarounds |
| **1. Infrastructure** | Arena, growable builder, string map/set, diagnostics/source map, path helpers, temp dirs | Unit tests for map/builder; can read a file and emit a diagnostic with a span |
| **2. Lexer** | Port `token.rs` + `lexer.rs`; token dump mode | Token stream for every `examples/*.cx` and corpus file matches the Rust lexer; negative cases produce the same codes |
| **3. AST + parser** | Tagged AST, recursive descent, precedence climbing, recovery, `#[cfg]`, macros (last) | Parses all corpus files; AST dump normalized and compared; malformed corpus yields the same diagnostic codes and counts |
| **4. Semantic** | Scopes, types, layouts, const eval, mutability, unsafe, return flow | `check` accepts/rejects the same corpus; `semantic.rs` unit-test equivalents pass |
| **5. Codegen** | IR emitter, helpers, ABI, debug metadata, sanitizer mode | `llvm-as` verifies every module; produced binaries match Rust-built binaries on stdout/exit at `--opt 0..3` |
| **6. Driver** | Tool discovery, temp files, assemble/opt/link, units/interfaces, manifest, archives/shared | `build`, `run`, `check`, `emit-ir --verify` work on the corpus; module and FFI tests pass |
| **7. Tooling** | `fmt`, `lint`, `doc`, `test`, `repl`, `lineedit`; LSP last | Existing tooling tests pass against the CoreX compiler; LSP session matches |
| **8. Self-compile** | Build the CoreX compiler source with itself; fixed-point test | Stage2 and Stage3 agree; full acceptance in §6.5 |

### 6.3 Port order detail

```
infrastructure
  → lexer (tokens, spans, errors)
  → AST (tagged nodes, arenas)
  → parser (items, statements, expressions, recovery)
  → semantic (collect, resolve, type, layout, const, flow)
  → codegen (functions, scalars, control flow, aggregates, ABI, debug)
  → driver (check/build/run, LLVM tools, link)
  → modules/interfaces/manifest
  → tooling (fmt/lint/doc/test/repl, LSP optional)
  → self-compile
```

### 6.4 Keeping Rust as the bootstrap and comparing outputs

- Build both compilers: `corex` (Rust) and `corexc` (CoreX source compiled by
  `corex`).
- **Differential corpus:** every `examples/*.cx`, every source string embedded
  in `compiler/tests/*.rs`, `bench/*.cx`, and `lib/corex/*.cx`.
- **Behavioral comparison:** compile each program with both compilers at the
  same optimization level; compare stdout, stderr, and exit status. This is the
  strongest check and does not depend on IR formatting.
- **Diagnostic comparison:** for negative inputs, compare the ordered list of
  `code` values and the start line/column. Do not compare prose.
- **IR comparison:** normalize temporary names, paths, and module names, then
  diff; or run `llvm-as` on both and compare verified modules. Behavior is the
  release gate; IR equality is a debugging aid.
- **Twin-build determinism:** compile the same file twice and compare IR/ELF
  SHA-256; the Rust compiler already has
  `tests/native.rs::repeated_builds_are_byte_identical`.
- **Bootstrap delta:** when the CoreX compiler cannot compile a construct that
  the Rust compiler can, record it as a bootstrap-subset limitation until the
  corresponding phase lands. Never weaken the Rust compiler to match a
  bootstrap bug.

### 6.5 Self-compile milestone and fixed point

1. **Stage 0 (bootstrap):** Rust `corex` compiles the CoreX compiler source →
   `corexc-stage1`.
2. **Stage 1:** `corexc-stage1` compiles the same source → `corexc-stage2`.
3. **Stage 2:** `corexc-stage2` compiles the same source → `corexc-stage3`.
4. **Fixed point:** `corexc-stage2` and `corexc-stage3` must produce identical
   IR (and ideally identical ELF) for the compiler source, and both must pass
   the corpus. If they differ, the difference is a determinism bug in the CoreX
   compiler, not an acceptable variance.
5. **Independent check:** build the compiler source with `corexc-stage1` from a
   clean checkout and compare against the bootstrap artifact.

A reasonable first milestone is **not** "compiler compiles itself"; it is
"CoreX compiler passes the lexer/parser corpus and emits a working hello-world
binary". Self-compilation comes only after codegen and driver parity.

### 6.6 Acceptance criteria

| ID | Criterion | Evidence |
|---|---|---|
| AC-1 | Corpus parity: every corpus program produces the same stdout/stderr/exit with both compilers at `--opt 0..3` | differential runner report |
| AC-2 | Negative parity: every negative input produces the same ordered diagnostic codes and start positions | differential runner report |
| AC-3 | IR validity: every emitted module passes `llvm-as` | `emit-ir --verify` |
| AC-4 | Existing behavior tests: the 205-test suite's behavioral cases pass when run against `corexc` | ported/parameterized harness |
| AC-5 | Modules/FFI: separate compilation, archives, shared objects, `dlopen`, C calls both directions work | `tests/modules.rs`, `tests/ffi.rs` equivalents |
| AC-6 | Determinism: twin builds produce identical artifacts | SHA-256 comparison |
| AC-7 | Fixed point: stage2 and stage3 agree on the compiler source | normalized IR/ELF diff |
| AC-8 | Performance: self-compile at `--opt 2` under an agreed budget (suggest ≤ 10 s on the reference machine), frontend within 2× of Rust `corex check` on the same input | timing harness |
| AC-9 | No regression: Rust `corex` remains the default and all 205 tests pass | CI |

### 6.7 Rollback

- Ship the CoreX compiler as `corexc` behind a flag or separate binary; keep
  `corex` (Rust) as the default until AC-1…AC-9 pass.
- Tag each bootstrap stage (`bootstrap-stage1`, `bootstrap-stage2`) so a failed
  fixed point can be bisected.
- If the port stalls, delete only `bootstrap/`; `compiler/` and the language
  tests are untouched.
- Never make the Rust compiler depend on the CoreX compiler for normal builds.

---

## 7. Honest effort and risk

### 7.1 Scale

| Item | Estimate | Basis |
|---|---:|---|
| Rust non-test source | 16,978 lines | `wc -l compiler/src/*.rs` minus inline tests |
| CoreX port | 22,000–30,000 lines | No generics/match/closures; manual formatting and maps add 30–60% |
| Lexer + parser + AST | 5,000–7,000 lines | 3,733 Rust lines, plus tagged-union boilerplate |
| Semantic | 5,500–8,000 lines | 4,216 Rust lines, 76 `match` sites |
| Codegen | 5,000–7,000 lines | 3,953 Rust lines, 673 `format!` sites |
| Driver + modules | 3,000–4,500 lines | 1,756 Rust lines plus process helpers |
| Tooling | 3,000–4,500 lines | fmt/lint/docgen/LSP/repl |
| Calendar, 1 engineer | 6–9 months | Includes differential debugging; assumes P0 prerequisites done |
| Calendar, 2–3 engineers | 3–5 months | Parallel by phase, one integration owner |

These are estimates from line counts and construct density, not measured
velocity. A 4–6 week spike should replace them with real numbers before a full
commitment.

### 7.2 Highest-risk areas

| Risk | Why | Mitigation |
|---|---|---|
| **AST/parser volume** | 281 `match` sites become `switch` trees with no exhaustiveness checking; fallthrough and missing `break` are silent bugs | Mechanical port with AST-dump differential tests; one node kind at a time; `switch` lint |
| **IR string emission** | 673 `format!` sites; manual width/hex/float formatting; O(n²) concatenation if naive | Growable builder with `reserve`; snapshot tests; `llvm-as` on every module |
| **Driver process handling** | The wrappers exist, but `fork`/`exec`/pipe cleanup, argument vectors, and tool timeouts still need careful integration; `llvm-as`/`opt`/`llc`/`cc` failures must produce `E0300`, not hangs | Test missing tools, bad IR, and linker failures; close every pipe end; enforce timeouts |
| **Memory bugs** | Manual arena, shallow `vec` copies, no destructors; a double free or UAF in the compiler is a crash | One arena per compilation; never copy owning `vec` headers; `--sanitize` on the CoreX compiler build |
| **Determinism** | Hash iteration order and map collisions can change output and break the fixed point | Insertion-order maps; counter-only temp names; twin-build SHA-256 gate |
| **Performance** | CoreX has no LLVM optimization of its own; frontend is allocation-light but bounds-checked; single-threaded | Benchmark a 10k-line input early; budget 2–4× Rust; consider `fork` per unit |
| **Bootstrap circularity** | A bug in the CoreX compiler that only appears when compiling itself is hard to localize | Keep Rust as oracle; stage tags; minimize bootstrap-only constructs; test the compiler source as a corpus file from phase 2 |

### 7.3 What would make it fail

- The CoreX compiler source grows past ~50k lines and no one can maintain two
  implementations; the Rust compiler keeps moving and the port never catches up.
- The project adds language features for the port (payload enums, traits,
  closures, GC) and loses the C-class identity instead of writing C-style code.
- A map/set implementation with nondeterministic iteration makes the fixed
  point impossible to reach.
- IR emission is written as repeated string concatenation, making self-compile
  minutes instead of seconds.
- The driver uses the new pipe/`spawn_argv` wrappers without closing every pipe
  end or enforcing timeouts, so a failed tool hangs or the differential harness
  cannot classify the failure.
- The team treats "Turing-complete" as sufficient and starts the semantic port
  before the lexer/parser corpus is stable.

---

## 8. Recommendation

### 8.1 Prerequisites before the full port

These P0 prerequisites de-risk the port. P0-1 … P0-5 are implemented and tested
(branch `room/corex-p0-blocker-fixes-by6h`); P0-6 and P0-7 remain.

| # | Prerequisite | Status | Evidence |
|---|---|---|---|
| P0-1 | Static aggregate initializers (arrays/structs/function pointers) and `const` array lengths | **COMPLETED** | `tests/aggregates.rs`: keyword/dispatch/struct tables run at `--opt 0..3`; `llvm-as` verifies the IR |
| P0-2 | `lib/corex` collections: string-keyed map/set with deterministic iteration, and a growable byte builder with integer/float/hex append | **COMPLETED** | `lib/corex/std_map.cx`, `lib/corex/std_builder.cx`; `tests/collections.rs` covers insert/lookup/grow/collisions/iteration order and 64-bit formatting |
| P0-3 | Driver helpers: `envp` scan, `spawn_argv`/`run_argv`, `pipe`/`dup2`, and a `read_file` that distinguishes failure | **COMPLETED** | `lib/corex/std_env.cx`, `std_io` `pipe`/`dup2`/`read_all`, `std_process` `spawn_argv`/`run_argv`, documented `std_fs::read_file`; `tests/driver_helpers.rs` forks `/bin/echo`, captures stdout, and reports the status |
| P0-4 | Nested generic syntax without whitespace | **COMPLETED** | Parser splits `>>` only in type position; parser unit tests, `tests/features.rs::nested_generic_types_and_shift_operators`; `>>`/`>>=` stay shifts |
| P0-5 | `switch` divergence-aware when a `default` is present | **COMPLETED** | Semantic unit tests, `tests/extended.rs::switch_with_default_satisfies_return_analysis`, E0206 diagnostics test |
| P0-6 | A CoreX-native test runner for the port and a differential runner that compares stdout/exit/diagnostic codes against Rust `corex` | **NOT STARTED** | `make check-bootstrap` runs the corpus and fails on divergence |
| P0-7 | A performance baseline: 10k-line input, `check` and `build --opt 2`, memory ceiling | **NOT STARTED** | Numbers recorded in `docs/benchmarks.md` or a bootstrap note |

P0-4 and P0-5 were small compiler changes included because they remove daily
friction from the port; they do not change the language's philosophy. The same
holds for P0-1 … P0-3: C-like data and explicit allocation, no new runtime.

### 8.2 Recommended before the semantic/codegen phases

- **P1:** path normalization (`dirname`/`basename`/`.`/`..`) and directory
  iteration tests for the driver; `envp` lookup and `read_file` failure
  distinction are already in `std_env`/`std_fs`.
- **P1:** use `std_builder` for the port's IR and diagnostic output rather than
  adding a second formatting path; `std_fmt` remains the number formatter.
- **P1:** a decision on modules: one unit for the first bootstrap, then
  `corex.toml` once interfaces are proven.
- **P2:** thread join and parallel unit builds; not needed for a first fixed
  point, but needed if compile time becomes a bottleneck.
- **Optional, not required:** payload enums/`match`. They would cut AST code
  size substantially, but they are a language design change and contradict the
  C-class goal. The tagged-union approach is proven; do not add `match` just for
  the bootstrap.

### 8.3 What is *not* needed

Do not add these for self-hosting. They are P2 or Rust-specific and would
violate CoreX's stated constraints:

- ownership/borrow checking, lifetimes, or a borrow checker;
- traits or type classes;
- closures or lambdas (context pointers suffice);
- garbage collection or hidden allocation;
- exceptions, `Result`, or `?` (error codes and diagnostics suffice);
- `async`/await;
- operator overloading;
- a package registry or dependency manager;
- targets other than Linux x86-64;
- incremental compilation or a query system for the first bootstrap;
- an LSP or REPL rewrite before the compiler itself is self-hosted (tooling can
  follow);
- thread join or language TLS unless parallel builds are explicitly required
  (`fork`/`wait` is enough for batch compilation).

### 8.4 Suggested decision

Run a **4–6 week spike** after P0-1…P0-7:

1. lexer + parser for the CoreX subset used by the compiler source;
2. AST dump differential against the Rust parser on the full corpus;
3. a minimal codegen that emits `hello.cx` and `factorial.cx`;
4. a driver that assembles, links, and runs those two programs;
5. measured line count and throughput.

Go/no-go: proceed to semantic/codegen only if the parser corpus is clean, the
two binaries match Rust-built output at `--opt 0..3`, and the projected
self-compile time is within the budget. If not, stop at the spike and keep the
Rust compiler; the language is still usable, and the missing pieces are known.

---

## Appendix A — Probe commands

Baseline probes were re-run after the P0 fixes. Still-unsupported constructs are
language-design choices; the rest are now covered by repository tests.

```sh
CX=compiler/target/release/corex
$CX check /tmp/cxprobe/payload.cx          # E0100 (payload enum, unsupported by design)
$CX check /tmp/cxprobe/generic.cx          # E0100 (generic function, unsupported by design)
$CX check /tmp/cxprobe/aggconst.cx         # ok: static aggregate table
$CX check /tmp/cxprobe/constlen.cx         # ok: `const` array length
$CX run   /tmp/cxprobe/nested.cx           # ok: `vec<vec<u8>>` without whitespace
$CX run   /tmp/cxprobe/tagged.cx           # 42
$CX run   /tmp/cxprobe/fnctx.cx            # 5
$CX run   /tmp/cxprobe/fall.cx             # 1 2 3
$CX run   /tmp/cxprobe/cstrvar.cx --hosted # 7
```

The fixed cases are also exercised in-repo: `tests/aggregates.rs`,
`tests/features.rs::nested_generic_types_and_shift_operators`, and
`tests/extended.rs::switch_with_default_satisfies_return_analysis`.

## Appendix B — Minimal CoreX encoding used by the microcosm

```corex
enum Kind { Num, Add, Mul }

struct Node {
    kind: Kind,
    value: i64,
    lhs: *mut Node,
    rhs: *mut Node,
}

fn node(a: *mut Arena, kind: Kind, value: i64,
        lhs: *mut Node, rhs: *mut Node) -> *mut Node {
    unsafe {
        let p: *mut u8 = (*a).base + (*a).used;
        (*a).used += sizeof(Node);
        let n: *mut Node = p as *mut Node;
        (*n).kind = kind; (*n).value = value;
        (*n).lhs = lhs;   (*n).rhs = rhs;
        return n;
    }
}

fn eval(n: *mut Node) -> i64 {
    unsafe {
        switch (*n).kind {
        case Kind::Num: { return (*n).value; }
        case Kind::Add: { return eval((*n).lhs) + eval((*n).rhs); }
        default:        { return eval((*n).lhs) * eval((*n).rhs); }
        }
    }
}
```

This pattern is the port's building block: arena allocation, pointer-linked
nodes, tagged dispatch, and explicit returns. The `switch` has a `default` and
every arm returns, so it satisfies return analysis without a trailing `return`.
It compiled and ran at `--opt 0,1,2,3` with output `((6 * 7) + 8)` / `160`.

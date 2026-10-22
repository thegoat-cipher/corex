# CoreX Self-Hosting Assessment

**Question:** Is CoreX stable and complete enough to reimplement the CoreX
compiler in CoreX today?

**Verdict: conditionally yes — not today.** CoreX is expressive enough to write
a compiler: every Rust construct in `compiler/src` has a CoreX encoding, and a
compiler-shaped microcosm (arena AST, tagged unions, string-keyed hash table,
function-pointer dispatch, IR-style text emission) compiled and ran correctly at
`--opt 0,1,2,3`. It is **not** complete enough to start a full bootstrap now:
the standard library lacks a map/set and a growable formatter, the language
cannot express compile-time aggregate tables, and the driver-facing process
primitives required to invoke LLVM are missing. Those are prerequisites, not
language design changes.

| Question | Answer |
|---|---|
| Can CoreX express a compiler frontend/backend? | Yes — see §4.2 probes and §5.5 microcosm |
| Is it ready to start the port **today**? | **No** — six blockers in §1.1 |
| Is a full self-host realistic after prerequisites? | Yes, at 6–9 engineer-months for one experienced compiler engineer; 3–5 months for a small team |
| Recommended next step | A 4–6 week spike after the P0 prerequisites in §8, with a go/no-go gate |

Assessment baseline: `main` at commit `2b10973` ("docs: sync merged C-class
test counts and limitations"), 2026-10-22. Worktree branch:
`room/corex-self-hosting-assessment-xvuu`.

Toolchain used for every check below: Rust 1.99.1, LLVM 20.1.8, gcc 15.2.0,
Linux x86-64.

---

## 1. Executive verdict

### 1.1 Exact blockers

These are things that make a bootstrap start today a bad bet. Each was verified
against the compiler, not inferred from prose.

| # | Blocker | Evidence | Impact on a compiler port |
|---|---|---|---|
| B1 | **No compile-time aggregate constants.** `static`/`const` initializers accept only scalars, `str`, `&global`, and `[u8; N]` from a string literal. Array/struct/function-pointer tables and `const` array lengths are rejected. | `static TOKENS: [str; 2] = ["a", "b"];` → `E0226`; `static TABLE: [fn(i64) -> i64; 1] = [&a];` → `E0226`; `const N: usize = 4; static A: [u8; N] = ...` → `E0100`; spec §4.2/§4.3 | Keyword tables, token descriptions, diagnostic catalogs, and dispatch tables must be heap-built in `main`, or encoded as functions/ifs. This is the only gap that changes how compiler data is laid out. |
| B2 | **No map or set in `lib/corex`.** | `lib/corex/*.cx` has no hash table, tree map, or set. | Symbol tables, string interning, module/interface maps, and label maps must be written and duplicated per value type (no generics). A 20-line open-addressing map is feasible (§5.5), but it is not in the library. |
| B3 | **No growable string builder or owned string.** `str` is a 2-word borrowed slice; `std_fmt::Fmt` takes a fixed caller buffer and reports overflow with a `bool`. `str` has no `==`, concatenation, or formatting. | `lib/corex/std_fmt.cx:7-31`; `str` equality → `E0210`; `std_str::str_eq` is the workaround | IR emission (673 `format!` sites in Rust), diagnostics rendering, and LSP JSON need a growable byte buffer plus manual formatting. Feasible, but it is a new library layer. |
| B4 | **Driver primitives are missing.** No `getenv` helper (the `envp` vector is reachable only through `main`), no `pipe`/`dup2`, `std_process::spawn3` accepts only two arguments, and `read_file` cannot distinguish an empty file from an error. | `lib/corex/std_process.cx:55-67`; no `getenv`/`pipe`/`dup2` in `lib/corex`; README "File I/O" limitation | Tool discovery (`COREX_LLVM_BIN`, `PATH`), capturing `llvm-as`/`opt`/`llc` stderr, and passing full argument vectors require raw syscalls or new stdlib wrappers. |
| B5 | **Three small language/semantic gaps** that hit parser and codegen code: nested generic syntax without whitespace (`vec<vec<u8>>`) is rejected; `switch` never satisfies return analysis even with `default`; `switch` falls through by default. | `vec<vec<u8>>` → `E0100` (needs `vec< vec<u8> >`); `switch` with all cases returning → `E0206`; probe prints `1 2 3` | The AST dispatcher needs `break` in every arm and a trailing `return`; nested containers need spaced syntax. Cost and bug surface, not a logical blocker. |
| B6 | **No thread join and no language-level TLS.** | `lib/corex/std_thread.cx` has spawn/yield/exit/free_stack; `docs/tasks.md` known gaps; `docs/language-spec.md` §13 | The compiler must be single-threaded, or use `fork`/`wait` per unit. Parallel module compilation is possible with processes, not threads. |

### 1.2 Why the answer is not simply "CoreX is Turing-complete"

Turing completeness is irrelevant to maintainability. The real question is
whether a compiler can be written with the data structures and I/O a compiler
needs, without a second runtime or a hidden allocation model. The answer is
yes in principle, with C-in-C ergonomics: tagged unions instead of payload
enums, `switch` instead of `match`, explicit context pointers instead of
closures, hand-written containers instead of generics, and arena allocation
instead of `Box`/`Rc`.

The port is therefore **feasible but expensive**, and the missing library
surface — not the language core — is what makes "today" the wrong start date.

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
- freestanding stdlib in `lib/corex` (13 modules).

Not implemented (spec §13, README "Limitations"): tuples; generics beyond
`vec<T>`; traits; closures; exceptions; `async`; `match`; payload enums;
operator overloading; destructors; labeled `break`; language `thread_local`;
thread join; targets other than Linux x86-64.

### 2.2 Compiler

`compiler/src` is 17,615 lines including inline tests (16,978 non-test lines),
zero external crates, Rust edition 2021, LLVM 20 textual IR.

| Area | File | Lines | Role |
|---|---|---:|---|
| Tokens | `token.rs` | 267 | 103 `TokenKind` variants, descriptions |
| Diagnostics | `diagnostics.rs` | 321 | spans, source map, rendering |
| Lexer | `lexer.rs` | 787 | byte-oriented, structured errors |
| AST | `ast.rs` | 840 | `Program`, `Stmt` (13 variants), `ExprKind` (11), `Type` (15), layouts |
| Parser | `parser.rs` | 2,106 | recursive descent + precedence climbing, recovery, macros, cfg |
| Semantic | `semantic.rs` | 4,216 | scopes, types, layouts, const eval, mutability, unsafe, return flow |
| Codegen | `codegen.rs` | 3,953 | textual LLVM IR, ABI, debug metadata, sanitizer mode |
| Driver | `driver.rs` | 1,352 | units, interfaces, objects, archives, linking, manifest |
| Mangle | `mangle.rs` | 404 | deterministic `module$item` rewriting |
| Formatter | `format.rs` | 270 | whitespace-only, idempotent |
| Linter | `lint.rs` | 356 | `W0001`–`W0004` |
| Docgen | `docgen.rs` | 217 | Markdown from `///` |
| LSP | `lsp.rs` | 908 | dependency-free JSON-RPC over stdio |
| REPL | `repl.rs` | 472 | accumulating definitions, `:` commands |
| Line editor | `lineedit.rs` | 273 | raw mode via `stty` |
| CLI | `main.rs` | 844 | commands, stdin, exit codes |

Pipeline: lex → parse → semantic side tables → textual IR → `llvm-as` →
`opt -passes=default<O{n}>` → `llc` → `cc -nostdlib -static -no-pie`.

### 2.3 Standard library

`lib/corex`: `std_alloc` (malloc/calloc/realloc/free, `Allocator`, arena),
`std_mem`, `std_str`, `std_io`, `std_fs` (stat, directories, read/write),
`std_process` (fork/exec/wait), `std_time`, `std_math`, `std_fmt`,
`std_sync` (futex mutex/condvar/spinlock), `std_thread`, `std_tls`, `std_dl`.

There is no collection library. This is the largest practical gap for a
compiler port.

### 2.4 Tooling, tests, benchmarks, build

- CLI: `build run exec repl emit-ir check fmt lint doc test`; `corex-lsp`.
- Tests: **175 passed, 0 failed** (61 unit + 114 integration). Per binary:
  `cli` 15, `compiler_caps` 8, `concurrency` 3, `diagnostics` 29, `extended` 6,
  `features` 7, `ffi` 2, `ir` 6, `modules` 4, `native` 18, `opt_levels` 5,
  `robustness` 2, `stdlib` 2, `tooling` 7. Unit tests: `semantic` 26,
  `parser` 14, `lexer` 9, `diagnostics` 4, `format` 4, `lsp` 3, `docgen` 1.
- Native coverage: compile → link → execute and assert stdout/exit status;
  `tests/native.rs::repeated_builds_are_byte_identical`; every example is
  assembled and verified in `tests/ir.rs`.
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
literals with a `Vec` scan. `std_fmt::Fmt` provides typed pushes
(`push_str`, `push_i64`, `push_hex`, `push_f64`) into a fixed buffer, and
`vec<u8>` provides growth.

What is missing is a **growable formatter**. The port needs one of:

1. a `Builder { data: *mut u8, len, cap }` with `reserve`/`push_byte`/
   `push_str`/`push_i64` in `lib/corex`; or
2. a `vec<u8>` with a small formatting helper that appends digits.

Either is straightforward and was prototyped (§5.5). The risk is silent
formatting drift in 673 call sites, which differential IR tests catch.

### 3.5 Driver, subprocesses, and file I/O

The Rust driver does four things CoreX's stdlib does not cover end to end:

1. **Tool discovery** reads `COREX_LLVM_BIN` and `PATH`, then probes
   `/usr/lib/llvm-N/bin` (`driver.rs:158-190`). CoreX must scan `envp` from
   `main` and implement path joining/existence checks (`std_fs::exists`).
2. **Capturing tool output** uses `Command::output()` for `llvm-as`, `opt`,
   `llc`, and `cc`. `std_process` only forks/execs with inherited stdio. Add
   `pipe`/`dup2` wrappers (raw syscalls 22/33 are available via `syscall`) or
   redirect stderr to a temp file and read it back.
3. **Arbitrary argv** is needed for `llc -filetype=obj ...` and `cc -nostdlib
   ...`. `std_process::spawn3` takes two arguments; `exec(path, argv, envp)`
   already accepts a full `**u8` array, so a small `spawn_argv` helper is
   enough.
4. **Temp directories** need `getpid`, a monotonic timestamp, `mkdir`, and
   cleanup. `std_process::getpid` and `std_time::monotonic_ns` exist;
   `std_fs::mkdir`/`rmdir` exist.

`read_file` returns an empty `vec<u8>` on failure, so the driver must `stat`
first or use `std_fs::read_file` with an explicit result. This is a documented
limitation, not a surprise.

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
| Maps / hash tables | Not in stdlib | **Missing library, not language** | Implement open addressing; proved in §5.5 |
| String builders / formatting | `std_fmt::Fmt` fixed-capacity; no format strings; `str` no `==`/concat | **Missing library** | `vec<u8>` + `Fmt`; `std_str::str_eq` |
| Error propagation | No `Result`, `?`, or exceptions; `panic` aborts 101 | **Friction** | Return codes + out-params + diagnostics `vec` |
| Static aggregate constants | Rejected (`E0226`/`E0231`; `const` length → `E0100`) | **Hard language limit** | Heap-init tables in `main`; encode tables as functions |
| Macros / hygiene | Expression macros only, non-hygienic, unit-local (spec §12.10) | **Friction** | Ordinary functions; avoid macros in the port |
| Thread join / TLS | No join; explicit `std_tls` table (spec §13) | **Hard limit for concurrency** | Single-threaded compiler; `fork`/`wait` for parallel units |
| ABI / FFI | `extern "C"`, `cstr`, varargs, `main(argc,argv,envp)` all work | **Good** | None needed for the compiler itself |
| Build / bootstrap | CoreX compiler is built by Rust `cargo`; CoreX can invoke LLVM tools | **Process gap, not language** | Keep Rust as stage0; add `spawn_argv`/pipe helpers |
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
| Static aggregate table | error | `static TOKENS: [str; 2] = ["a","b"];` → `E0226` |
| Static function-pointer table | error | `static TABLE: [fn(i64)->i64; 1] = [&a];` → `E0226` |
| `const` as array length | error | `const N: usize = 4; static A: [u8; N] = ...` → `E0100: expected array length` |
| `str` equality | error | `"a" == "b"` → `E0210`; use `std_str::str_eq` |
| Nested `vec` without spaces | error | `vec<vec<u8>>` → `E0100: expected '>', found '>>'`; `vec< vec<u8> >` works |
| `switch` + all cases return | error | still `E0206` without a trailing `return`; the analysis does not treat `switch` as diverging |
| `switch` fallthrough | behavior | `case 1: print(1); case 2: print(2);` prints `1 2`; use `break` |
| Tagged union + `switch` | **works** | `struct Node { tag, payload: union }`; prints `42` |
| Function-pointer context | **works** | `fn apply(f: fn(*mut u8)->i64, ctx: *mut u8)` prints `5` |
| C varargs (`--hosted`) | **works** | `extern "C" fn snprintf(...);` with `c"%d-%s"` links and runs |
| `main(argc, argv, envp)` | **works** | `tests/ffi.rs::main_receives_argc_argv_and_envp` |
| Local aggregate tables | **works** | `let table: [fn(i64)->i64; 2] = [&f, &g];` prints `40` |

### 4.3 The one genuine expressiveness limit

Static aggregate data is the only construct whose absence changes the design of
the compiler rather than just its verbosity. In Rust, tables such as keyword
names, token descriptions, diagnostic catalogs, LLVM type spellings, and
function-pointer dispatch maps are compile-time constants. In CoreX they must be
built at process start:

```corex
static mut KEYWORDS: *mut vec<str> = null;   // initialized in init_tables()

fn init_tables() {
    unsafe {
        KEYWORDS = std_alloc::malloc(sizeof(vec<str>)) as *mut vec<str>;
        *KEYWORDS = vec_new(64);
        vec_push(KEYWORDS, "fn");
        ...
    }
}
```

This is exactly how a C compiler would do it, and it is compatible with CoreX's
no-hidden-allocation philosophy. It is not a fatal blocker, but it means the
self-hosted compiler has startup work and a global init order to maintain.

---

## 5. Evidence checks

### 5.1 Full test suite

```console
$ cd compiler
$ PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo test
...
test result: ok. 61 passed; 0 failed   (lib)
test result: ok. 14 passed; 0 failed   (cli)
test result: ok. 8 passed;  0 failed   (compiler_caps)
test result: ok. 3 passed;  0 failed   (concurrency)
test result: ok. 29 passed; 0 failed   (diagnostics)
test result: ok. 6 passed;  0 failed   (extended)
test result: ok. 7 passed;  0 failed   (features)
test result: ok. 2 passed;  0 failed   (ffi)
test result: ok. 6 passed;  0 failed   (ir)
test result: ok. 4 passed;  0 failed   (modules)
test result: ok. 18 passed; 0 failed   (native)
test result: ok. 5 passed;  0 failed   (opt_levels)
test result: ok. 2 passed;  0 failed   (robustness)
test result: ok. 2 passed;  0 failed   (stdlib)
test result: ok. 7 passed;  0 failed   (tooling)
```

Total: **175 passed, 0 failed**. This matches the README and `docs/tasks.md`.
The suite includes native compile/run tests, IR verification, diagnostics,
modules/archives/shared objects, C ABI in both directions, DWARF inspection,
sanitizers, an LSP session, and optimization-level equivalence.

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
  shallow; arrays cannot be passed/returned by value; `read_file` ambiguity.
- `docs/tasks.md`: T-001…T-028, T-041…T-046, T-049…T-055, and T-056…T-077
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
| **0. Prerequisites** | Implement P0 items in §8; add the differential harness and a CoreX-native test runner | `cargo test` still 175; new tests cover map/builder/driver helpers; probes B1/B2/B3/B4 have documented workarounds |
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
| AC-4 | Existing behavior tests: the 175-test suite's behavioral cases pass when run against `corexc` | ported/parameterized harness |
| AC-5 | Modules/FFI: separate compilation, archives, shared objects, `dlopen`, C calls both directions work | `tests/modules.rs`, `tests/ffi.rs` equivalents |
| AC-6 | Determinism: twin builds produce identical artifacts | SHA-256 comparison |
| AC-7 | Fixed point: stage2 and stage3 agree on the compiler source | normalized IR/ELF diff |
| AC-8 | Performance: self-compile at `--opt 2` under an agreed budget (suggest ≤ 10 s on the reference machine), frontend within 2× of Rust `corex check` on the same input | timing harness |
| AC-9 | No regression: Rust `corex` remains the default and all 175 tests pass | CI |

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
| **Driver process handling** | No pipes/env/argv stdlib; `llvm-as`/`opt`/`llc`/`cc` failures must produce `E0300`, not hangs | Add wrappers first; test missing tools, bad IR, and linker failures; timeouts |
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
- The driver remains unable to capture tool stderr, so failures are silent and
  the differential harness cannot classify them.
- The team treats "Turing-complete" as sufficient and starts the semantic port
  before the lexer/parser corpus is stable.

---

## 8. Recommendation

### 8.1 Do not start the full port today

Start only after these P0 prerequisites are implemented and tested. They are
small relative to the port and de-risk it substantially.

| # | Prerequisite | Acceptance |
|---|---|---|
| P0-1 | Decide and document the static-table story: either support `static` aggregate initializers (arrays/structs/function pointers) or bless the `init_tables()` pattern with an example and a test | Keyword table and dispatch table examples compile and run |
| P0-2 | Add `lib/corex` collections: string-keyed map/set with deterministic iteration, and a growable byte builder with integer/float/hex append | Unit tests for insert/lookup/grow/iteration order; used by a sample program |
| P0-3 | Add driver helpers: `getenv`/`envp` scan, `spawn_argv`, `pipe`/`dup2` (or documented temp-file capture), and a `read_file` variant that distinguishes failure | A CoreX program runs `llvm-as` with arguments, captures stderr, and reports failure |
| P0-4 | Fix nested generic lexing so `vec<vec<u8>>` parses without whitespace | Parser test plus a diagnostic regression |
| P0-5 | Make `switch` divergence-aware when a `default` is present, or document the trailing-`return` convention in the spec and examples | Semantic test: a `switch` with all cases returning compiles |
| P0-6 | Add a CoreX-native test runner for the port and a differential runner that compares stdout/exit/diagnostic codes against Rust `corex` | `make check-bootstrap` runs the corpus and fails on divergence |
| P0-7 | Establish a performance baseline: 10k-line input, `check` and `build --opt 2`, memory ceiling | Numbers recorded in `docs/benchmarks.md` or a bootstrap note |

P0-4 and P0-5 are small compiler changes. They are included because they remove
daily friction from the port; they do not change the language's philosophy.

### 8.2 Recommended before the semantic/codegen phases

- **P1:** `read_file` error distinction, `getenv`/`PATH` parsing helpers,
  `dirname`/`basename`/path normalization, and directory iteration tests.
- **P1:** a stable `Builder` API used by both `std_fmt` and the compiler, so
  formatting code is not duplicated.
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

```sh
CX=compiler/target/release/corex
$CX check /tmp/cxprobe/payload.cx          # E0100
$CX check /tmp/cxprobe/generic.cx          # E0100
$CX check /tmp/cxprobe/aggconst.cx         # E0226
$CX check /tmp/cxprobe/constlen.cx         # E0100
$CX run   /tmp/cxprobe/tagged.cx           # 42
$CX run   /tmp/cxprobe/fnctx.cx            # 5
$CX run   /tmp/cxprobe/fall.cx             # 1 2 3
$CX run   /tmp/cxprobe/cstrvar.cx --hosted # 7
```

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
        return 0;   // switch does not satisfy return analysis
    }
}
```

This pattern is the port's building block: arena allocation, pointer-linked
nodes, tagged dispatch, and explicit returns. It compiled and ran at `--opt
0,1,2,3` with output `((6 * 7) + 8)` / `160`.

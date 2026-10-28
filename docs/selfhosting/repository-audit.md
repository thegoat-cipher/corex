# Repository Audit — CoreX

> **Post-audit status (2026-10-28):** this file records the pre-implementation
> baseline. A CoreX-written compiler now exists under `selfhost/`; Stage 1/2/3
> bootstrap and the Stage 2 == Stage 3 == Stage 4 fixed point are verified. See
> `bootstrap-results.md`, `validation-report.md`, and `master.md`. Statements
> below about "no CoreX-written compiler" describe the audit baseline only.


Audited checkout: thread worktree `.../worktrees/thr_n5ffyqycgb-1/first-app`,
branch `room/corex-selfhost-audit-ycgb`, HEAD `fccff01`
(`docs: add complete syntax tutorial`). The main checkout
`/home/vib/project/mobile/first-app` is at `5f0b4bc` (`main`); this worktree
adds `8de5adf` (CI LLVM install) and `fccff01` (syntax tutorial).

Every claim below was read from this checkout or produced by the commands in
§10. Items not exercised are marked **Unknown**. Line counts are `wc -l` on the
checkout; "non-test" subtracts the inline `#[cfg(test)]` block that starts at
the cited marker.

## 1. Repository layout

| Path | Contents |
|---|---|
| `compiler/` | Rust compiler: `Cargo.toml` (23 lines), `Cargo.lock` (only `corex`), `src/` 18 files / 18,166 lines, `tests/` 17 files / 4,800 lines |
| `lib/corex/` | 16 freestanding stdlib modules, 2,419 lines total |
| `examples/` | 8 `.cx` programs |
| `bench/` | 5 `.cx` kernels with 5 C references (`array`, `f64`, `fib`, `mix`, `struct`) |
| `docs/` | 17 files: spec, grammar, architecture, memory model, PRD, NFR, tasks, self-hosting, ADRs, benchmarks, research, installation, tutorial, etc. |
| `scripts/` | `bench.sh`, `example-matrix.sh`, `publish-github.sh`, `try.sh` |
| `.github/workflows/ci.yml` | Linux x86-64 CI: Rust 1.99.1, LLVM 20, `cargo test`, examples, install smoke |
| Top level | `README.md` (436 lines), `master.md` (139), `handoff.md` (67), `ENGINEERING.md`, `CHANGELOG.md`, `THIRD_PARTY.md`, MIT/Apache-2.0 licenses |

There is no `bootstrap/`, `corexc`, `Makefile`, `rust-toolchain.toml`, or
CoreX-written compiler source in the checkout (`find` over the tree; see
`docs/self-hosting.md:659-660`).

## 2. Compiler implementation

Rust 1.99.1, edition 2021, **zero external crates**. `compiler/Cargo.toml:1-23`
declares the `corex` library, the `corex` binary (`src/main.rs`), and
`corex-lsp` (`src/lsp_main.rs`); `cargo metadata` reports zero dependencies and
`Cargo.lock` contains only `corex 0.3.0`.

| Area | File | Total lines | Non-test | Entry point / key definitions |
|---|---|---:|---:|---|
| Tokens | `compiler/src/token.rs` | 267 | 267 | `TokenKind` at `token.rs:6`; 103 variants |
| Diagnostics | `compiler/src/diagnostics.rs` | 321 | 274 | `Span:11`, `Diagnostic:37`, `SourceFile:146` |
| Lexer | `compiler/src/lexer.rs` | 787 | 668 | `pub fn lex(src: &str)` at `lexer.rs:9` |
| AST | `compiler/src/ast.rs` | 847 | 847 | `Program:435`, `Type:166` (15 variants), `Stmt:588` (18 variants), `ExprKind:702` (24 variants) |
| Parser | `compiler/src/parser.rs` | 2,301 | 2,069 | `pub fn parse` at `parser.rs:15`; `parse_with_cfg:21`; macro expansion `:1408` |
| Semantic | `compiler/src/semantic.rs` | 4,388 | 4,141 | `pub fn analyze` at `semantic.rs:178`; side tables `Analysis:142` |
| Codegen | `compiler/src/codegen.rs` | 4,023 | 4,023 | `pub fn generate` at `codegen.rs:164`; `generate_debug:170`, `generate_full:180`, `generate_tests:192` |
| Driver | `compiler/src/driver.rs` | 1,418 | 1,418 | `check:33`, `load_source:44`, `emit_ir:150`, `build:310`, `compile_units:917`, `run:1392` |
| Mangling | `compiler/src/mangle.rs` | 407 | 407 | module symbol/reference rewriting |
| Formatter | `compiler/src/format.rs` | 270 | 236 | `format_source:22` |
| Linter | `compiler/src/lint.rs` | 356 | 356 | `lint:20` |
| Docgen | `compiler/src/docgen.rs` | 217 | 182 | `generate:12` |
| LSP | `compiler/src/lsp.rs` | 908 | 880 | `run:289`; JSON-RPC over stdio |
| REPL | `compiler/src/repl.rs` | 506 | 506 | `run:29` |
| Line editor | `compiler/src/lineedit.rs` | 273 | 273 | raw-mode TTY line editing/history |
| CLI | `compiler/src/main.rs` | 848 | 848 | `main:57`; command dispatch `:74-83`; `cmd_*` at `:339-821` |
| Library root | `compiler/src/lib.rs` | 24 | 24 | module declarations `:10-16` |
| LSP binary | `compiler/src/lsp_main.rs` | 5 | 5 | `corex::lsp::run()` |
| **Total** | | **18,166** | **17,424** | |

Pipeline (single file; module mode is implemented by `compile_units` and
described in `docs/architecture.md` §12.1):

```text
driver::load_source (driver.rs:44)
  → lexer::lex (lexer.rs:9)
  → parser::parse (parser.rs:15)
  → semantic::analyze (semantic.rs:178)
  → codegen::generate (codegen.rs:164)
  → llvm-as (driver.rs:752)
  → opt -passes=default<O{n>> when opt > 0 (driver.rs:773-794)
  → llc -filetype=obj -O{n} -relocation-model=static (driver.rs:797-819)
  → cc -nostdlib -static -no-pie (driver.rs:294-299, :859)
```

`driver.rs` is documented as the only module that talks to external processes
(`driver.rs:1-6`). Module mode discovers `mod name;` units, compiles each to an
object with generated interfaces, mangles `module$item`, and links/archives
them (`driver.rs:561`, `:917`, `:1102`, `:1124`; `mangle.rs`).

Documentation staleness, not missing code: `docs/architecture.md:42-60` still
prints the v0.1 module table (token 216, parser 1,187, semantic 2,953, codegen
2,457, driver 389, main 437 lines) and `docs/self-hosting.md:103` says
18,020 total / 17,289 non-test; the measured values are 18,166 / 17,424.
`docs/self-hosting.md:601` also repeats 16,978 non-test lines. These are stale
numbers; the files and entry points above exist.

## 3. Backend and toolchain

| Tool | Verified version | Location |
|---|---|---|
| `rustc` | 1.99.1 (58cf8f9c3 2026-10-13), host `x86_64-unknown-linux-gnu` | `$HOME/.cargo/bin` |
| `cargo` | 1.99.1 (5f94df478 2026-08-27) | `$HOME/.cargo/bin` |
| `llvm-as` | Ubuntu LLVM 20.1.8 | `$HOME/opt/llvm20/bin` |
| `llc` | Ubuntu LLVM 20.1.8 | `$HOME/opt/llvm20/bin` |
| `opt` | Ubuntu LLVM 20.1.8 | `$HOME/opt/llvm20/bin` |
| `cc` | gcc 15.2.0 (Ubuntu 15.2.0-4ubuntu4) | `/usr/bin/cc` |
| `llvm-ar` | Ubuntu LLVM 20.1.8 | `$HOME/opt/llvm20/bin` |
| `ar` / `readelf` | GNU binutils 2.45 | `/usr/bin` |

`clang` and `lld` are **not installed**; they are not required by the build
path. Rust's own `rustc` reports internal LLVM 23.1.1, but the compiler does not
link LLVM: codegen emits textual LLVM 20 IR and invokes the tools above
(`docs/architecture.md` §4-5; `driver.rs:734-869`).

Backend shape (verified in `codegen.rs` and `driver.rs`):

- Textual IR, opaque `ptr`, no `target datalayout`; `llvm-as` both assembles and
  runs the verifier, then `opt` (for `--opt 1..3`) and `llc` produce an object.
- `cc -nostdlib -static -no-pie` links a static ELF; `--hosted` opts into libc
  and `--emit obj|shared|staticlib` select other artifacts (`driver.rs:282-303`,
  `:826-914`).
- Runtime helpers are emitted only when used: `__corex_syscall` (`codegen.rs:527`),
  `__corex_alloc` mmap (`:539`), `__corex_free` munmap (`:819`),
  `__corex_read_file` (`:759`), `__corex_panic` (`:721`).

Release build result (`cd compiler && cargo build --release`):

```text
Finished `release` profile [optimized] target(s) in 4.31s
target/release/corex      1,758,768 bytes
target/release/corex-lsp  1,189,512 bytes
$ ./target/release/corex --version
corex 0.3.0 (LLVM 20 backend)
```

`file target/release/corex` reports `ELF 64-bit LSB pie executable ... dynamically
linked`; that is the compiler binary. Programs it emits are static:
`file /tmp/cx_fact` → `ELF 64-bit LSB executable, x86-64 ... statically linked`.

## 4. Tests and exact totals

Command (path required because Rust/LLVM are user-local):

```bash
cd compiler
PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo test
```

Exact result: **205 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out**
across 21 test-result lines. That is 68 unit tests + 137 integration tests;
the doc-test suite is 0.

| Test binary | Passed |
|---|---:|
| `unittests src/lib.rs` | 68 |
| `unittests src/main.rs` | 0 |
| `unittests src/lsp_main.rs` | 0 |
| `tests/aggregates.rs` | 6 |
| `tests/cli.rs` | 17 |
| `tests/collections.rs` | 2 |
| `tests/compiler_caps.rs` | 8 |
| `tests/concurrency.rs` | 3 |
| `tests/diagnostics.rs` | 29 |
| `tests/driver_helpers.rs` | 3 |
| `tests/extended.rs` | 9 |
| `tests/features.rs` | 8 |
| `tests/ffi.rs` | 2 |
| `tests/ir.rs` | 6 |
| `tests/modules.rs` | 7 |
| `tests/native.rs` | 18 |
| `tests/opt_levels.rs` | 5 |
| `tests/robustness.rs` | 4 |
| `tests/stdlib.rs` | 3 |
| `tests/tooling.rs` | 7 |
| `Doc-tests corex` | 0 |
| **Total** | **205** |

Unit-test breakdown from `#[test]` markers: `semantic` 29, `parser` 18,
`lexer` 9, `diagnostics` 4, `format` 4, `lsp` 3, `docgen` 1 = 68. Integration
`#[test]` markers total 137. CI enforces `passed -ne 205` as a hard failure
(`.github/workflows/ci.yml:73-82`) and also runs `--test ffi --test modules`
(`:87`).

## 5. Build and test commands

```bash
# full build + tests (verified)
cd compiler
PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo test
cargo build --release

# compiler smoke checks (verified)
./target/release/corex --version
./target/release/corex run ../examples/hello.cx
./target/release/corex run ../examples/factorial.cx
./target/release/corex build ../examples/factorial.cx -o /tmp/cx_fact --opt 2

# all examples at --opt 0..3 (verified with release binary)
cd ..
COREX=compiler/target/release/corex ./scripts/example-matrix.sh

# CI variant
cd compiler && cargo test --locked
```

`cargo test` invokes the real LLVM/`cc` toolchain; missing LLVM fails rather
than silently skipping (`README.md:242-243`). `scripts/example-matrix.sh`
defaults to `compiler/target/debug/corex` unless `COREX` is set
(`scripts/example-matrix.sh:13`).

## 6. Examples (observed output and exit status)

Run with the release binary, no flags:

| File | Observed stdout | Exit |
|---|---|---:|
| `examples/hello.cx` | `Hello, World!` | 0 |
| `examples/factorial.cx` | `3628800` | 0 |
| `examples/pointers.cx` | `123456789` | 0 |
| `examples/loop_sum.cx` | (empty) | 51 |
| `examples/features.cx` | 13 lines: `13`, `4`, `4`, `6`, `true`, `3.000000`, `66`, `4`, `10`, `po`, `ore`, `5`, `7` | 0 |
| `examples/compiler_data.cx` | `9`, `0`, `fn`, `4`, `1` (5 lines) | 0 |
| `examples/dispatch.cx` | `30`, `true` (2 lines) | 0 |
| `examples/playground.cx` | `A` | 147 |

`scripts/example-matrix.sh` with `COREX=compiler/target/release/corex` reports
`all 8 examples match at --opt 0,1,2,3` — identical stdout and exit status at
every optimization level.

## 7. Standard library (`lib/corex`)

16 modules, 2,419 lines. "Pub items" counts top-level `pub fn`/`pub struct`/
`pub enum`/`pub type` declarations, not methods.

| Module | Lines | Pub items | Key API / notes |
|---|---:|---:|---|
| `std_alloc` | 288 | 13 | `malloc`, `calloc`, `realloc`, `free`, `aligned_alloc`, `Allocator`, `c_allocator`, `Arena` (`arena_new/alloc/reset/free`) |
| `std_mem` | 78 | 6 | `memcpy`, `memmove`, `memset`, `memzero`, `memcmp`, `memchr` |
| `std_str` | 173 | 10 | `strlen`, `strcmp`, `strncmp`, `strcpy`, `strncpy`, `strcat`, `strchr`, `str_eq`, `str_find`, `str_copy_cstr` |
| `std_io` | 149 | 15 | fd flags, `open`, `read`, `write`, `seek`, `close`, `pipe`, `dup2`, `read_all`, `write_all`, `puts`, `eputs`, `getbyte`, `putbyte` |
| `std_fs` | 246 | 19 | `Stat`, `stat_path`, `lstat_path`, `file_size`, `exists`, `is_dir`, `is_file`, `mkdir`, `rmdir`, `unlink`, `rename`, `getcwd`, `chdir`, `read_file`, `write_file`, `Dir`/`dir_*` |
| `std_process` | 112 | 11 | `exit`, `getpid`, `fork`, `exec`, `wait`, `spawn`, `spawn3`, `spawn_argv`, `run`, `run_argv`, `env_count` |
| `std_env` | 72 | 3 | allocation-free `envp` scan: `env_get`, `env_has`, `env_get_or` |
| `std_time` | 57 | 7 | `Timespec`, `clock_gettime`, `now_ns`, `monotonic_ns`, `sleep_ns`, `sleep_ms`, `unix_seconds` |
| `std_math` | 168 | 18 | integer abs/min/max, `clz64`, `ctz64`, `popcount64`, `is_pow2`, `next_pow2`, `fabs`, `sqrt`, `trunc`, `floor`, `ceil`, `round`, `powi` |
| `std_fmt` | 210 | 18 | typed `Fmt` writer: `push_str`, `push_cstr`, `push_i64`, `push_u64`, `push_hex`, `push_bin`, `push_bool`, `push_f64`, `finish`, `finish_cstr`; fixed buffer |
| `std_map` | 270 | 17 | insertion-ordered string-keyed `Map` with `u64` values; `map_*`; `Set` with `set_*`; borrowed keys |
| `std_builder` | 214 | 16 | growable byte `Builder`: reserve/push byte/str/char/i64/u64/hex/f64, len/data/failed/finish |
| `std_sync` | 121 | 14 | futex `Mutex`, `Condvar`, `SpinLock` |
| `std_thread` | 94 | 9 | `Thread`, `spawn` (clone + asm trampoline), `gettid`, `yield_now`, `thread_exit`, `tid`, `stack`, `stack_size`, `free_stack`; no join |
| `std_tls` | 131 | 4 | explicit 64-slot table: `tls_alloc`, `tls_get`, `tls_release`, `tls_thread_id` |
| `std_dl` | 36 | 5 | hosted `dlopen`/`dlsym`/`dlclose`/`dlerror`: `open`, `open_now`, `sym`, `close`, `error` |

Tests: `stdlib.rs` (allocator/mem/str/files/format/time/dirs), `collections.rs`
(map/set/builder), `concurrency.rs` (atomics/threads/futex/TLS),
`driver_helpers.rs` (envp, fork/pipe/argv), `modules.rs` (stdlib modules via
`-I ../lib/corex`).

## 8. What exists vs what is documentation-only

**Implemented and verified (source + passing tests).**

- Rust compiler 0.3.0: lexer, parser, typed AST, semantic analysis, textual
  LLVM 20 codegen, driver, CLI, REPL, LSP, formatter, linter, doc generator,
  test runner, DWARF 5, sanitizers.
- Language surface in `docs/language-spec.md` §3-§12: scalars/pointers/arrays/
  slices/`str`/`cstr`/`vec<T>`, structs/unions/bitfields/flexible arrays,
  function pointers, C control flow (`switch`/`for`/`do`/`goto`), modules and
  `corex.toml`, C FFI/varargs, atomics/volatile/threads/futex/explicit TLS,
  inline asm, expression macros.
- 16 stdlib modules, 8 examples, 17 integration test files, CI workflow.

**Documentation-only, stale, or contradictory.**

- `docs/self-hosting.md` is an assessment/plan, not a compiler. It explicitly
  marks P0-6 (differential harness) and P0-7 (performance baseline) **NOT
  STARTED** (`docs/self-hosting.md:659-660`); `make check-bootstrap` is named
  but no `Makefile` exists. There is no `bootstrap/`, `corexc`, or CoreX
  compiler source anywhere in the tree.
- The §5.5 microcosm is described as implemented and run, but its source is not
  checked in; only the snippet in Appendix B exists (`docs/self-hosting.md:458`,
  `:741`). It is not reproducible from the repository; treat the claim as
  unverified.
- `docs/architecture.md` is v0.1 through §8 (`architecture.md:1-3`); its module
  line table is stale (see §2). §12 describes the v0.3 multi-unit/tooling
  architecture and matches the source.
- `docs/self-hosting.md:103` / `:601` line counts are stale (18,020/17,289 and
  16,978 vs measured 18,166/17,424).
- `docs/language-spec.md:646` says "no TLS" in §9, while §12.8
  (`language-spec.md:967`) documents `std_tls` and thread-local tables; the code
  has `lib/corex/std_tls.cx`. The §9 sentence is stale, not a missing feature.
- `docs/PRD.md:225` says "Self-hosting. The compiler stays in Rust; CoreX cannot
  compile itself", contradicting the later self-hosting assessment. The PRD is
  v0.1-era; do not treat it as current capability evidence.
- Benchmark numbers (`docs/benchmarks.md`, `master.md` §6) were **not re-run**
  in this audit. Treat them as **Unknown**, not verified.

**Unknown / not exercised here.** External URL liveness and prose claims in
`docs/research.md`/`competitor-analysis.md`; NFR statuses in `docs/NFR.md`;
interactive TTY line editing (tests drive the REPL through pipes); performance
ratios.

## 9. Capability matrix

"Self-Hosting Ready" = enough verified language/library capability to write that
component in CoreX today, not whether the bootstrap process exists. "Tested"
names the suite or test that covers the capability (counts overlap).

| Capability | Implemented | Tested | Self-Hosting Ready | Blockers |
|---|---|---|---|---|
| **Lexer** | Yes — `compiler/src/lexer.rs:9`, 787 lines; `token.rs:6` defines 103 `TokenKind` variants | 9 unit tests (`lexer.rs:669`); `tests/diagnostics.rs` (29); `tests/robustness.rs` (4) | **Yes** | None verified. Byte-oriented; comments/literals handled. |
| **Parser** | Yes — `parser.rs:15`, 2,301 lines; recursive descent + precedence climbing; recovery; `cfg` and macros expand during parse (`:1408`, `:1698`) | 18 unit tests (`parser.rs:2070`); `tests/diagnostics.rs`; `features`/`extended`/`modules` | **Yes** | No `match`/payload enums; 128-level nesting cap (`E0111`); tagged-union AST must replace Rust enums. |
| **AST** | Yes — `ast.rs:435` `Program`, `:166` `Type` (15 variants), `:588` `Stmt` (18), `:702` `ExprKind` (24) | Exercised by parser/semantic/codegen tests; no standalone AST test file | **Yes** | No payload enums; 24 expression + 18 statement variants become tag+union+`switch`; arena allocation instead of `Box`/`Rc`. |
| **Type Checker** | Yes — `semantic.rs:178`, 4,388 lines; scopes, types, layouts, const eval, mutability, `unsafe`, return flow | 29 unit tests (`semantic.rs:4142`); `tests/diagnostics.rs` 29 | **Yes** | `HashMap<Type,_>`/`Rc<StructInfo>` need manual encoding/arena; 91 textual `HashMap` and 37 `HashSet` uses total across `src`. |
| **Memory Model** | Yes — `alloc`/`free` builtins (`semantic.rs:28`), mmap/munmap helpers (`codegen.rs:539`, `:819`); `std_alloc` malloc/arena | `tests/native.rs` (18) heap/pointer tests; `tests/stdlib.rs` (3) allocator; guarantee matrix `docs/memory-model.md:211-233` | **Yes** | No destructors/RAII; `vec<T>` copies are shallow (double-free risk); manual free. |
| **LLVM Backend** | Yes — textual IR `codegen.rs:164` (4,023 lines); `llvm-as` verify + `opt` + `llc` (`driver.rs:734-841`) | `tests/ir.rs` (6); `tests/native.rs` (18); `tests/opt_levels.rs` (5) | **Yes** | 694 `format!` sites in `src` (379 in `codegen.rs`) must become `std_builder`/`std_fmt`; float constants need bit reinterpretation (union/pointer cast probe works). |
| **Native Compilation** | Yes — `driver.rs:310` build; `llvm-as`/`opt`/`llc`/`cc -nostdlib -static -no-pie` (`:294-299`, `:752-869`) | `tests/native.rs` (18); release build emitted a static ELF (`/tmp/cx_fact`) | **Yes** | External LLVM 20/`cc` required; no per-tool timeout. |
| **File I/O** | Yes — `read_file`/`write_file` builtins (`semantic.rs:3187`, `:3205`), runtime helper (`codegen.rs:759`), `std_fs` (19 pub fns) | `tests/compiler_caps.rs::file_io_round_trip_and_missing_file`; `tests/stdlib.rs::file_io_formatting_time_and_directories` | **Yes** | Builtin `read_file` returns empty `vec<u8>` on failure; use `std_fs::read_file` (`-1` on failure) to distinguish. |
| **Compiler CLI** | Yes — `main.rs:57` dispatch; `cmd_build` `:339` through `cmd_test` `:821`; `corex-lsp` (`lsp_main.rs:4`) | `tests/cli.rs` (17) | **Yes** | None verified. |
| **Modules** | Yes — `driver.rs:917` `compile_units`, interfaces `:561`, `mangle.rs`, manifest `:1209`; `module`/`mod` syntax | `tests/modules.rs` (7): visibility, `cfg`, manifest exe/archive/shared, `dlopen` | **Yes** | Stdlib has no `realpath`/`dirname`/`basename`; path canonicalization/dedup must be written. First bootstrap can use one unit. |
| **FFI** | Yes — `extern "C"` declarations/exports, varargs, `cstr`, `main(argc,argv,envp)` (`semantic.rs:95`, `:1160`; spec §12.5) | `tests/ffi.rs` (2) compiles a C helper with `cc` both directions; `tests/extended.rs::c_strings_convert_in_both_directions` | **Yes** | Not required for the compiler itself; aggregate function-pointer calls use the internal ABI, not C ABI. |
| **Threads/TLS** | **Partial** — `std_thread.cx:38` `spawn` (clone + asm trampoline), `std_tls.cx:57` `tls_alloc` (64-slot explicit table) | `tests/concurrency.rs` (3): threads/futex/TLS/atomics | **Partial** — single-threaded compiler only | No thread join (`std_thread.cx:1-6`), no language `thread_local`; use `fork`/`wait` for parallel units. |
| **Generics/vec** | **Partial** — only `vec<T>` (`ast.rs:177`; `semantic.rs:3242`); `std_map` is string→`u64`, `std_builder` | `tests/compiler_caps.rs::dynamic_arrays_grow_index_and_free`; `tests/features.rs::nested_generic_types_and_shift_operators`; `tests/collections.rs` (2) | **Partial** | No general generics/traits; map values are `u64`; duplicate containers or encode pointers; no `sort` in stdlib. |
| **Slices** | Yes — `Type::Slice`/`Str` (`ast.rs:175-176`), ranges/bounds (`semantic.rs:2129+`), `std_str` | `tests/features.rs::slices_strings_len_and_ranges`, `::runtime_slice_bounds_abort` | **Yes** | No equality/concat; use `std_str::str_eq`; shallow copies. |
| **Macros** | **Partial** — expression macros `macro name(...) = expr;`, expansion `parser.rs:1408` | `tests/extended.rs::declarative_macros_expand_at_compile_time` | **Partial** — avoid in port | Expression-only, non-hygienic, unit-local; recursion cap 64 (`parser.rs:1426`). |
| **LSP/REPL** | Yes — `lsp.rs:289` `run` (908 lines); `repl.rs:29` `run` (506 lines); `lineedit.rs` | `tests/tooling.rs::language_server_reports_diagnostics_and_edits`; 10 REPL tests in `tests/cli.rs` | **Yes** | LSP JSON/object order must be hand-built; TTY editing is not covered by automated tests (pipes only). |

## 10. Commands actually run for this audit

| Command | Result |
|---|---|
| `cd compiler && PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo test` | 205 passed, 0 failed; 21 result lines |
| `cargo build --release` | success, 4.31 s; binary 1,758,768 bytes |
| `./target/release/corex --version` | `corex 0.3.0 (LLVM 20 backend)` |
| `corex run` on all 8 examples | outputs and exits in §6 |
| `corex build examples/factorial.cx -o /tmp/cx_fact --opt 2` | success; `file` reports static ELF; runs `3628800` |
| `corex check examples/hello.cx` | `ok: 1 function(s), entry '_start'` |
| `corex emit-ir examples/hello.cx --verify` | `IR verified by llvm-as` |
| `COREX=... ./scripts/example-matrix.sh` | `all 8 examples match at --opt 0,1,2,3` |
| `cargo tree --depth 1` / `cargo metadata` | only `corex v0.3.0`, 0 dependencies |
| `wc -l compiler/src/*.rs compiler/tests/*.rs` | 18,166 / 4,800 lines |

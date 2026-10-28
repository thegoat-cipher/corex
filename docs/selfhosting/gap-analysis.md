# Gap Analysis — Rust CoreX → CoreX Self-Hosted Compiler

> **Post-audit status (2026-10-28):** this file records the pre-implementation
> baseline. A CoreX-written compiler now exists under `selfhost/`; Stage 1/2/3
> bootstrap and the Stage 2 == Stage 3 == Stage 4 fixed point are verified. See
> `bootstrap-results.md`, `validation-report.md`, and `master.md`. Statements
> below about "no CoreX-written compiler" describe the audit baseline only.


Baseline: `fccff01` (`room/corex-selfhost-audit-ycgb`); evidence and exact
commands are in `repository-audit.md`. Tags: **[V]** verified by source, test,
or command in this audit; **[I]** inference/estimate, not measured here.

## 1. Bottom line

**[V]** The current Rust compiler is complete for its documented v0.3 scope:
205/205 tests pass, `cargo build --release` succeeds, `corex 0.3.0` runs, and it
emits static ELF binaries (`repository-audit.md` §3-§4).

**[V]** No CoreX-written compiler exists in the repository. There is no
`bootstrap/`, `corexc`, `Makefile`, or differential harness; `find` returns
nothing for `*bootstrap*`/`*corexc*`. `docs/self-hosting.md:659-660` marks P0-6
(differential runner) and P0-7 (performance baseline) **NOT STARTED**.

**[V]** The language/library prerequisites that the project called P0-1…P0-5
are present and tested: static aggregate constants, `const` array lengths,
`std_map`/`std_builder`, driver process/env helpers, nested `vec<vec<T>>`, and
`switch` divergence (`docs/self-hosting.md:649-660`; tests named in
`repository-audit.md` §4).

**[V]** No single missing *language* feature was found that blocks a compiler
port. The remaining gaps are (a) a few utility-library primitives, (b) the
bootstrap/differential process, and (c) the raw volume of the port.

**[I]** The port is feasible but expensive. The project's own estimate is
22,000–30,000 CoreX lines and 6–9 engineer-months for one experienced compiler
engineer (`docs/self-hosting.md:597-604`). The dominant cost is codegen/string
emission, not semantic analysis.

## 2. Existing language/library capabilities (verified)

These are the capabilities a self-hosted compiler can rely on today.

| Capability | Evidence | Compiler use |
|---|---|---|
| Byte strings, `str`, `cstr`, slices, arrays | `ast.rs:166-184`; `docs/language-spec.md` §3, §6.8; `tests/features.rs` | Source text, tokens, diagnostics, IR text |
| `vec<T>`, including nested `vec<vec<T>>` | `semantic.rs:3242`; `tests/compiler_caps.rs`; `tests/features.rs::nested_generic_types_and_shift_operators` | Token/AST/diagnostic lists |
| Structs, unions, bitfields, flexible arrays, packed/aligned layouts | `ast.rs:99-164`; `tests/extended.rs::layout_introspection_unions_bitfields_and_compound_literals` | AST nodes/tagged unions, layout tables |
| Function pointers and dispatch tables | `tests/extended.rs::function_pointers_are_values_and_dispatch_indirectly`; `examples/dispatch.cx` | Visitor/dispatch tables |
| `switch` with fallthrough, `for`, `do while`, labels/`goto` | `tests/extended.rs::switch_fallthrough_for_do_while_and_goto` | Parser/analyzer state machines and recovery |
| `const`, `static`, aggregate constants, function-pointer tables | `tests/aggregates.rs` (6) | Keyword/diagnostic/type tables with no startup init |
| File I/O (`read_file`/`write_file`, `std_fs`) | `tests/compiler_caps.rs`; `tests/stdlib.rs` | Reading source/interfaces; writing IR/objects |
| Process/env helpers (`fork`, `exec`, `wait`, `pipe`, `dup2`, `spawn_argv`, `run_argv`, `envp`) | `tests/driver_helpers.rs` (3) | Invoking `llvm-as`/`opt`/`llc`/`cc`, capturing output |
| Modules (`module`/`mod`/`pub`), interfaces, `corex.toml`, archives/shared objects | `tests/modules.rs` (7) | Multi-file compiler and separate compilation |
| `extern "C"`, varargs, `cstr`, `main(argc,argv,envp)` | `tests/ffi.rs` (2) | Optional C interop; CLI args |
| Atomics, volatile, inline asm, threads, futexes, explicit TLS | `tests/concurrency.rs` (3) | Single-threaded compiler; optional parallelism |
| Expression macros, `#[cfg]` | `tests/extended.rs`; `tests/modules.rs` | Optional; avoid in the port |
| Formatter/linter/docgen/test runner/LSP/REPL | `tests/tooling.rs` (7); `tests/cli.rs` (17, 10 REPL) | Tooling after the compiler core |

## 3. Missing capabilities

Every "missing" claim below was checked against `compiler/src`, `lib/corex`,
`docs/language-spec.md`, and the release binary; none is inferred from the docs
alone.

| Gap | Evidence | Impact on a CoreX compiler | Workaround already supported |
|---|---|---|---|
| **No general generics, traits, closures, `match`, payload enums, destructors** | `ast.rs:166-184`: the only generic type is `Type::Vec`; `docs/language-spec.md:1056-1062` lists them out of scope; probes: `enum E { A(i32) }` → `E0100`, `match` → `E0100`, `fn id<T>` → `E0100` | AST/visitor/analyzer code is more verbose; no exhaustive `match` | Tagged struct + `union` + `switch`; context pointer + function pointer; arena instead of `Box`/`Rc`; explicit cleanup |
| **No `parse_i64`/`parse_f64` in `lib/corex`** | `grep` over `lib/corex` finds no `parse*`/`strtol`/`atof`; Rust lexer uses `text.parse::<f64>()` at `lexer.rs:446` | Lexer needs decimal-to-binary64 conversion for float literals | Integer parsing is manual `checked_mul` in Rust (`lexer.rs:332-344`); floats can store literal text and let LLVM parse a decimal constant, or implement a parser |
| **No `realpath`/`dirname`/`basename`/path normalization** | `grep` for `realpath`, `dirname`, or `basename` over `lib/corex` → none; Rust driver uses `std::fs::canonicalize` at `driver.rs:58,83` | Module/import resolution, dedup, cycle detection, output paths | Implement lexical normalization plus `readlink`/`getcwd`; first bootstrap can use one unit and relative paths |
| **No `sort` in `lib/corex`** | `grep -rn 'sort' lib/corex` → none | Deterministic ordering of symbol tables, diagnostics, generated names | Insertion order from `std_map`; hand-written insertion/merge sort over `vec<T>` |
| **`std_map` is string→`u64` only** | `lib/corex/std_map.cx:1-20`; `docs/language-spec.md:946` | Type/key tables (`HashMap<Type, u32>`) need encoding or linear search | Store pointers/indices as `u64`; keep a parallel `vec<Type>`; linear scan for small tables |
| **No thread join; no language `thread_local`** | `lib/corex/std_thread.cx:1-6` says "no JOIN"; `grep join lib/corex` → none; `std_tls` is a 64-slot explicit table (`std_tls.cx:13,57`) | In-process parallel compilation is unavailable | Single-threaded compiler; `fork`/`wait` per unit for parallelism |
| **No `f64::to_bits` language builtin** | No `to_bits` in `lib/corex` or `docs/language-spec.md`; the Rust implementation uses `value.to_bits()` at `codegen.rs:3980` | Emitting exact LLVM hex FP constants | Probed and working: `union Bits { f: f64, u: u64 }` or write `f64` and read through `*u64` in `unsafe` |
| **`std_fmt::push_f64` is fixed six decimals, not round-trip** | `lib/corex/std_fmt.cx:146-175` rounds to 1e-6 and prints `nan`/`inf` | Cannot be used to emit arbitrary `f64` IR constants | Use the union/pointer bit path, or emit the original decimal literal and let LLVM parse it |
| **No `Rc`/`Box`/destructors/RAII** | Language has raw pointers and explicit `alloc`/`free` (`docs/memory-model.md:3-10`); no ownership | Compiler object graphs must be arena-allocated and freed explicitly | Arena per compilation; one owner per `vec`; free at phase boundaries |
| **No `Option`/`Result`/`?`/exceptions** | `docs/language-spec.md:1056-1062`; errors are `panic`/`assert` (exit 101) | Fallible parse/analysis/driver steps need sentinels/out-params | Return `bool`/`i32` plus diagnostic `vec`; `panic` only for unrecoverable bugs |
| **No `str` equality/concat/formatting operators** | `docs/language-spec.md` §6.8; `str_eq` is `std_str` | Symbol comparison and diagnostic construction | `std_str::str_eq`, `std_builder`, `std_fmt` |
| **No process timeout** | `lib/corex/std_process.cx:30` `wait(pid)`; no timeout parameter | A hung `llvm-as`/`llc`/`cc` can hang the compiler | Parent-side alarm thread or `waitpid(WNOHANG)` loop; not present in stdlib |
| **No package manager/registry; stdlib is source via `-I`** | `tests/*.rs` use `include_paths: ../lib/corex`; README "Limitations" | Compiler build must pass `-I`/manifest `include_paths`; no versioned stdlib resolution | `corex.toml` `include_paths` (`docs/language-spec.md` §12.6); vendor `lib/corex` |
| **No LLVM library binding** | `driver.rs` invokes `llvm-as`/`opt`/`llc` as processes; no `llvm-sys`/FFI | Self-hosted compiler must keep emitting textual IR and shelling out | Already the design (ADR-0002); no change needed |
| **Linux x86-64 only** | `docs/language-spec.md:1056-1062`; `driver.rs` target triple | No cross-compilation or other hosts | Out of scope for the first bootstrap |

## 4. Process and toolchain gaps

| Gap | Evidence | Consequence |
|---|---|---|
| **No differential harness (P0-6)** | `docs/self-hosting.md:659` says NOT STARTED; `make check-bootstrap` is named but there is no `Makefile`; no `bootstrap/` or corpus runner in the tree | No automated proof that a CoreX compiler matches Rust `corex` on stdout/exit/diagnostics |
| **No performance baseline (P0-7)** | `docs/self-hosting.md:660` says NOT STARTED; benchmark suite measures CoreX-vs-C kernels, not a CoreX-written compiler (`scripts/bench.sh`; `docs/benchmarks.md`) | Self-compile time and memory ceiling are unmeasured; the go/no-go gate is undefined |
| **No CoreX-native test runner** | All 205 tests are Rust `cargo test`; the Rust test harness compiles/runs CoreX programs (`tests/native.rs`, `tests/ir.rs`) | Porting tests requires a CoreX driver/runner first |
| **No bootstrap stages or tags** | No `bootstrap/`, `corexc`, stage tags, or `make` targets | Cannot reproduce stage1/stage2/stage3 or bisect a fixed-point failure |
| **Rust stage0 is mandatory** | `compiler/Cargo.toml`; CI installs Rust 1.99.1 (`.github/workflows/ci.yml:17,29`) | Bootstrapping needs a working Rust toolchain before any CoreX compiler exists |
| **LLVM 20 + `cc` are external** | `driver.rs:160` tool discovery; `:734-869` invokes tools; `cc` default from `COREX_CC` | The CoreX compiler still depends on LLVM 20.1.8 and a C linker driver |
| **CI pins the test count at 205** | `.github/workflows/ci.yml:73-82` fails if `passed != 205` | Any added/removed test requires a CI edit; a ported suite cannot reuse this job unchanged |
| **CI is Ubuntu 24.04 x86-64 only** | `.github/workflows/ci.yml:23` | No macOS/Windows/ARM coverage; self-host CI must be added |
| **No `rust-toolchain.toml`** | `ls rust-toolchain.toml` → missing; CI sets `RUST_VERSION` in env | Local Rust version drift is possible; reproducibility depends on docs/CI |
| **Stdlib is not installed/package-resolved** | Tests point `include_paths` at `../lib/corex`; README documents no package manager | Every build must configure `-I`; no stable stdlib ABI/version |
| **No installable CoreX compiler binary** | No `corexc` target; only Rust `cargo build`/`cargo install` (CI smoke installs `corex`) | The self-hosted compiler has no distribution path yet |

## 5. Shortest bootstrap path

This is a dependency-ordered path; it keeps Rust `corex` as the oracle at every
gate. Phase lengths are **[I]** estimates, not measured.

### Phase 0 — Close the process and utility gaps (1–3 weeks [I])

1. Build P0-6: a CoreX-native differential runner. Freeze a corpus of
   `examples/*.cx`, `lib/corex/*.cx`, `bench/*.cx`, and the source strings
   embedded in `compiler/tests/*.rs`. Compare stdout/stderr/exit and ordered
   diagnostic codes/positions against Rust `corex`.
2. Build P0-7: a 10k-line input, `check` and `build --opt 2` timing/memory
   baseline on the reference machine.
3. Add the missing stdlib primitives with tests:
   - path normalization/canonicalization (`realpath`, `dirname`, `basename`,
     `.`/`..`), or a documented single-unit restriction;
   - `parse_i64`/`parse_f64` (or a literal-text float strategy);
   - a deterministic `sort` for symbol/diagnostic tables;
   - optional `waitpid(WNOHANG)`/timeout helper for tool processes.
4. Freeze the Rust compiler as stage0 and record token/AST/diagnostic dumps as
   golden artifacts.

### Phase 1 — CoreX infrastructure

Arena allocator, growable `Builder`, string-keyed map/set (already in
`std_map`), source map/spans/diagnostics, path helpers, temp directories, and
process/pipe wrappers. Gate: write a file, read it back, emit a diagnostic with
a span, spawn a tool and capture stdout.

### Phase 2 — Lexer

Port `token.rs` + `lexer.rs`. Gate: token stream, spans, and error codes match
Rust on the full corpus, including malformed inputs. This is the smallest
end-to-end proof that the CoreX toolchain works.

### Phase 3 — AST + parser

Port `ast.rs` + `parser.rs` using tagged structs + `union` payloads + `switch`.
Gate: normalized AST dumps and diagnostic codes/positions match; parse every
corpus file; macros and `#[cfg]` last. The Rust parser's 128-level recursion cap
must be preserved or deliberately changed.

### Phase 4 — Semantic analysis

Port scopes, type checking, layouts, const evaluation, mutability/`unsafe`, and
return-flow analysis. Gate: `check` accepts/rejects the same corpus with the
same codes; port the `semantic` unit-test equivalents.

### Phase 5 — LLVM codegen

Port `codegen.rs`. This is the largest mechanical rewrite: 379 `format!` calls
in `codegen.rs` (694 across `src`) become `std_builder`/`std_fmt` pushes; 24
`ExprKind` and 18 `Stmt` variants become explicit `switch` arms. Gate:
`llvm-as` verifies every module; emitted binaries match Rust output/exit at
`--opt 0..3`; twin builds are byte-identical; no `nsw`/`nuw`; all abort paths
still exit 101.

### Phase 6 — Driver, modules, FFI

Port tool discovery, temp files, `llvm-as`/`opt`/`llc`/`cc`, modules/interfaces/
mangling/`corex.toml`, archives/shared objects, and C ABI. Gate: `build`,
`run`, `check`, `emit-ir --verify` work on the corpus; `modules.rs` and
`ffi.rs` equivalents pass.

### Phase 7 — Tooling (optional for the first fixed point)

Formatter, linter, docgen, test runner, REPL/line editor; LSP last. These are
not needed for self-compilation and can remain Rust binaries longer.

### Phase 8 — Self-compile and fixed point

1. Stage 0: Rust `corex` builds the CoreX compiler source → `corexc-stage1`.
2. Stage 1: `corexc-stage1` builds the same source → `corexc-stage2`.
3. Stage 2: `corexc-stage2` builds it again → `corexc-stage3`.
4. `stage2` and `stage3` must produce identical IR and preferably identical
   ELF; both must pass the corpus.

Acceptance gates from `docs/self-hosting.md` §6.6: corpus parity (AC-1),
negative/diagnostic parity (AC-2), `llvm-as` validity (AC-3), behavioral tests
(AC-4), modules/FFI (AC-5), twin-build determinism (AC-6), stage2/stage3 fixed
point (AC-7), performance budget (AC-8), and Rust `corex` remaining the default
with all 205 tests green (AC-9).

## 6. What is *not* needed

Based on the source and the project's own constraints, do **not** add these for
the bootstrap: payload enums/`match` (tagged unions + `switch` are proven),
traits, closures, GC, exceptions/`Result`, `async`, operator overloading,
borrow checking/lifetimes, a package registry, non-x86-64 targets, incremental
compilation, or thread join/language TLS. They either contradict the C-class
design or are unnecessary for a first fixed point.

## 7. Verified vs inferred

| Statement | Status |
|---|---|
| Rust compiler 0.3.0 passes 205/205 tests; release binary runs; output ELF is static | Verified (`repository-audit.md` §3-§4) |
| No CoreX compiler, bootstrap stage, Makefile, or differential harness exists | Verified by tree search; `docs/self-hosting.md:659-660` |
| Language has tagged unions, `switch`, function pointers, `vec<T>`, modules, FFI, explicit TLS | Verified by source + passing tests |
| `lib/corex` has no `parse_*`, `realpath`/`dirname`/`basename`, or `sort` | Verified by `grep` over `lib/corex` |
| `std_thread` has no join; TLS is an explicit 64-slot table | Verified by `std_thread.cx:1-6`, `std_tls.cx:13,57` |
| Float bit reinterpretation is possible via union/pointer cast | Verified by probe (`union Bits { f: f64, u: u64 }`, exit 0) |
| Codegen float constants use `f64::to_bits`; `std_fmt::push_f64` is fixed-six-decimal | Verified (`codegen.rs:3980`, `std_fmt.cx:146-175`) |
| Port size 22k–30k lines; 6–9 engineer-months one engineer; 3–5 months small team | **Inference** from the project's estimate (`docs/self-hosting.md:597-604`), not measured |
| Performance of a CoreX-written compiler will be within 2–4× Rust | **Inference**; P0-7 is not started |
| A mechanical port will fit the schedule | **Inference**; no spike was run in this audit |
| Benchmark ratios in `docs/benchmarks.md` | **Unknown**; not re-run here |
| NFR statuses in `docs/NFR.md`, external research claims | **Unknown**; not re-verified here |

## 8. Conclusion

There is no verified language-level showstopper. The compiler's architecture is
already compatible with CoreX's C-class feature set: textual IR instead of an
LLVM API, explicit data structures instead of generics, tagged unions instead
of payload enums, and process-based tool invocation instead of library calls.
The gating work is:

1. **P0-6/P0-7** — differential harness and performance baseline (not started).
2. **Utility library** — path normalization, numeric parsing, sorting, and
   timeout support.
3. **The port itself** — lexer → AST/parser → semantic → codegen → driver, with
   codegen the largest and riskiest phase.

The highest-risk items are codegen string emission (379 `format!` sites in
`codegen.rs`), determinism (insertion-order maps, counter-only temp names), and
memory bugs in a no-destructor compiler. The shortest credible path is the
phase sequence in §5 with Rust `corex` as the permanent oracle; do not start
semantic/codegen before the lexer/parser corpus is clean.

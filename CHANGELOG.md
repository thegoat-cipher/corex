# Changelog

All notable changes to CoreX are documented here. The compiler package and both
Rust binaries (`corex`, `corex-lsp`) share one version; the self-hosted
`corexc` reports the same release.

## [0.4.0] - 2026-10-30

Self-hosted compiler release. The Rust `corex` compiler remains the full-language
Stage 0 reference; `selfhost/` now builds itself reproducibly. Still zero Rust
dependencies, LLVM 20 textual-IR backend, Linux x86-64 only.

### Self-hosted bootstrap

- CoreX-written bootstrap compiler: 545 functions / 17,747 lines across 15
  `selfhost/*.cx` files, built by `selfhost/bootstrap.sh`.
- Stage 1 is built by Rust `corex`; Stage 2 builds itself, Stage 3 rebuilds it,
  and Stage 4 repeats the fixed point. Stage 2/3/4 are byte-identical
  (`sha256 e66cad85…`, 637,200 bytes).
- Optimized self-builds are deterministic and self-reproducing at `--opt 1..3`
  (`f463fd3b…`, `0d213b2b…`, `2c1cfb85…`); `selfhost/repro-opt.sh --corpus`
  gates this.
- Gates: 270-check corpus on each of Stages 1-3, CLI 80, LSP 35, differential
  dumps 81, fuzz/ASan 330, concurrency 33, and `lib/corex` 16/16.

### Language, runtime, and tooling

- Self-hosted concurrency surface: `volatile` globals and builtins, all atomic
  builtins, inline `asm`, and the `std_thread`/`std_tls` thread and TLS runtime.
- DWARF `-g`/`--debug`: compile unit, per-function `DW_TAG_subprogram`, and
  line locations.
- Static function-pointer and `str` tables, compound literals, and designated
  array/struct initializers.
- LSP server (`corexc lsp`): diagnostics, hover, definition, references,
  rename, completion, and formatting; 35 protocol checks in bootstrap and CI.
- Externalized behavior suite (`tests/selfhosting/behavior.py`): all 205 Rust
  test rows parameterized, with 137 passed / 0 failed / 68 blocked (`--lib`
  internals) / 0 timeout.

### Cleanup and audit fixes

- SH-044 bootstrap cleanup: 558 → 534 functions before the tooling merge
  (545 / 17,747 in the final integrated tree), with redundant unsafe wrappers,
  dead helpers, and duplicate wrappers removed.
- P0–P3 master-audit fixes: tool failures and signal-killed tools fail builds
  with exit 3; built-in `#[cfg]` keys; frontend `--cfg`/`-I` on `check` and
  `emit-ir`; `E0226` for non-plain aggregate statics; float compound
  assignment; labels in unreachable blocks; linear-time `check_binary`.
- Version strings for `corex`, `corexc`, and `corexc-lsp`/`corex-lsp` report
  `0.4.0`; CI asserts `corex 0.4.0`.

### Readiness

- Second readiness audit verdict: **READY WITH CAVEATS**. AC-2 diagnostic
  positions or counts differ from the Rust oracle for 13 of 65 negative inputs;
  AC-4 is externalized for 137 CLI-expressible rows while 68 Rust-internal unit
  tests are blocked; AC-8 is met with a documented metric. The bootstrap itself
  is reproducible and passes every executable gate.

### Release engineering

- Package version is now `0.4.0`; `cargo test --locked` reports 205 passed
  (68 unit, 137 integration).
- CI runs the Rust suite, examples at every optimization level, the
  self-hosting bootstrap with ASan, `repro-opt --corpus`, and an install smoke
  test that asserts the `0.4.0` version.

## [0.3.0] - 2026-10-22

C-class systems-language release. Still zero Rust dependencies, LLVM 20
textual-IR backend, Linux x86-64 only.

### Language and backend

- Function types, function pointers, indirect calls, and dispatch tables.
- `switch`/`case`/`default` with C fallthrough, `for`, `do while`, labels, and
  `goto`.
- Unions, bitfields, flexible array members, designated and partial
  initializers, and compound literals.
- `typedef`, `repr(C)`, packed and `align(N)` structs, and
  `sizeof`/`alignof`/`offsetof` with C layout.
- `extern "C"` declarations and exports, C strings, variadic calls, and
  `main(argc, argv, envp)`.
- File modules with public/private interfaces, deterministic `module$item`
  mangling, separate compilation, `corex.toml`, and `#[cfg]`.
- Static aggregate constants, `const` array lengths, nested `vec<vec<T>>`, and
  `switch` divergence analysis.
- Volatile and atomic operations, inline assembly, declarative expression
  macros, and explicit thread-local storage.
- DWARF 5 debug info with `-g`; `--sanitize` checked signed arithmetic and
  allocator hardening.

### Runtime and standard library

- Freestanding `lib/corex`: allocator (`malloc`/`calloc`/`realloc`/`free`,
  `Allocator`, arena), memory and string, I/O, files, directories, process,
  time, math, and formatting.
- Insertion-ordered `std_map`/`std_set`, growable `std_builder`, and driver
  helpers (`std_env`, pipes, `spawn_argv`/`run_argv`).
- Threads via `clone`, futex mutex/condvar/spinlock, and `dlopen` support.

### Tooling

- `corex fmt`, `corex lint`, `corex doc`, `corex test`, and the `corex-lsp`
  language server.
- `--emit obj|shared|staticlib`, hosted builds, and extra-object linking.
- Benchmark harness in `scripts/bench.sh`.

### Audit fixes (2026-10-21 review)

- Linear-time `+`/`-` type checking; deep expressions report `E0111` instead of
  aborting.
- Signed bitfields sign-extend; indirect calls use the lowered ABI for
  struct/slice/`str` parameters.
- Labeled `break`/`return` analysis fixed; a module-level `main` is mangled.
- PIC relocation for shared and static outputs; temporary build directories are
  removed on every error path.
- `std_dl` is lazy by default with `open_now` for eager loading; `std_alloc`
  rejects overflowing size arithmetic.
- Specification, README, and test totals synchronized: `cargo test` reports
  **205 passed** (68 unit, 137 integration).

### Release engineering

- GitHub Actions CI on Linux x86-64 with Rust 1.99.1 and LLVM 20.1.8: full test
  suite, C FFI and separate-compilation tests, every example at
  `--opt 0,1,2,3`, and a `cargo install --locked` smoke test.
- Package version is now `0.3.0`; `corex --version` and `corex-lsp` report it.

[0.4.0]: https://github.com/thegoat-cipher/corex/releases/tag/v0.4.0
[0.3.0]: https://github.com/thegoat-cipher/corex/releases/tag/v0.3.0

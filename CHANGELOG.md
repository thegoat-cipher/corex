# Changelog

All notable changes to CoreX are documented here. The compiler package and both
binaries (`corex`, `corex-lsp`) share one version.

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

[0.3.0]: https://github.com/thegoat-cipher/corex/releases/tag/v0.3.0

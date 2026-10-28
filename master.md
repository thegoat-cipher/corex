# CoreX — Master Status

Last updated: 2026-10-22. This file is a living summary. `docs/tasks.md` holds
the authoritative per-task status and evidence; `docs/ADRs.md` holds decisions.

---

## 1. Vision

CoreX is a low-level, statically typed, ahead-of-time systems language with
predictable execution, direct hardware access, explicit memory control, and a
minimal runtime. It compiles to native Linux x86-64 through LLVM 20 and produces
statically linked ELF executables that do not depend on libc.

Non-negotiable constraints, all still satisfied:

- no garbage collector, no borrow checker, no lifetimes, no exceptions;
- no hidden allocation: every byte is a stack slot or an explicit allocation;
- raw pointers and `unsafe` as a narrow capability, not a managed safety layer;
- freestanding by default; hosted/libc linking is opt-in (`--hosted`);
- defined integer semantics (wrapping unsigned arithmetic, masked shifts,
  aborting division/signed overflow when checked);
- direct Linux syscalls in the runtime helpers, no libc dependency by default;
- zero Rust dependencies in the compiler.

---

## 2. Current phase

**v0.3: the C-class roadmap is implemented.** P0 and P1 features are complete
and tested; P2 remains optional and not started.

| Workstream | Status | Evidence |
|---|---|---|
| Function pointers + ABI groundwork | COMPLETED | `tests/extended.rs`, `examples/dispatch.cx`, `tests/ffi.rs` |
| C control flow + aggregates | COMPLETED | `tests/extended.rs` |
| Layout + introspection | COMPLETED | `tests/extended.rs`; spec §12.3–12.4 |
| C ABI / FFI / cstr / varargs / main args | COMPLETED | `tests/ffi.rs` |
| Modules + separate compilation + manifest + cfg | COMPLETED | `tests/modules.rs` |
| Allocator API + C memory/string library | COMPLETED | `lib/corex`, `tests/stdlib.rs` |
| Stdlib (I/O, files, process, time, math, formatting) | COMPLETED | `tests/stdlib.rs` |
| DWARF / formatter / linter / LSP / docs / test runner / sanitizers | COMPLETED | `tests/tooling.rs` |
| P1: volatile, atomics, threads, mutex/condvar, TLS | COMPLETED | `tests/concurrency.rs` |
| P1: inline asm, macros, cfg, shared libraries, dlopen | COMPLETED | `tests/concurrency.rs`, `tests/modules.rs` |
| P1: benchmarks | COMPLETED | `scripts/bench.sh`, `docs/benchmarks.md` |
| Self-hosting P0 blockers (static aggregates, collections/builder, driver helpers, nested `vec`, `switch` divergence) | COMPLETED | `tests/aggregates.rs`, `tests/collections.rs`, `tests/driver_helpers.rs`; `docs/self-hosting.md` §1.1 |

Test totals: `cargo test` → **205 passed, 0 failed** (68 unit, 137 integration).

---

## 3. Architecture summary

```
entry.cx
  → module discovery (`mod name;`, include paths, cycle check)
  → per unit (dependency order):
      dependency interfaces (.cxi CoreX source)
      + unit source → lexer → parser (cfg-aware, macros expand here)
      → `module$item` mangling → semantic analysis
      → interface generation for `pub` items
      → LLVM 20 textual IR → llvm-as → opt -passes=default<O<n>> → llc
  → link objects (+ archives/shared objects) with cc
```

- Rust 1.99.1, zero external crates.
- Backend: textual LLVM IR, no FFI to LLVM (ADR-0002).
- Layout: natural C layout for plain structs; opaque `[size x i8]` LLVM types for
  unions/packed/bitfield/flexible-array types, field access by byte offsets
  (ADR-0026).
- Two aggregate conventions: CoreX internal `ptr`/`sret`, C ABI by value for
  plain aggregates (ADR-0027).
- Modules: file units, generated source interfaces, deterministic
  `module$item` mangling, C symbols unmangled (ADR-0025).
- Memory: explicit allocation, raw pointers, lexical `unsafe`; C-style
  allocator + arena in the freestanding library (ADR-0030).
- Threads/TLS: `clone` + inline-asm trampoline, futex sync, explicit TLS table
  (ADR-0031).
- Debug info: opt-in DWARF 5 line tables, subprograms, locals (ADR-0032).

---

## 4. Active tasks

No task is IN PROGRESS or BLOCKED. All T-001…T-028, T-041…T-046, T-049…T-055,
and T-056…T-078 are COMPLETED. See `docs/tasks.md`.

---

## 5. Blockers and known gaps

None blocking. Explicit limitations (documented, not hidden):

1. No thread join; `std_thread` provides spawn/yield/exit and stack cleanup.
2. TLS is an explicit table keyed by thread id, not compiler `thread_local`.
3. Macros are expression macros, unit-local, non-hygienic.
4. `volatile` applies to global declarations; pointers use
   `volatile_load`/`volatile_store`.
5. `fib(35)` remains ~1.6× slower than GCC; the arithmetic/pointer/struct/FP
   kernels are at parity (0.95–1.06×). See `docs/benchmarks.md`.
6. Package management and non-x86-64 targets are out of scope (P2).

Environment notes:

- LLVM 20.1.8 is installed user-locally at `/home/vib/opt/llvm20`; the compiler
  finds it through `COREX_LLVM_BIN`, `PATH`, or `/usr/lib/llvm-N/bin`.
- Rust is installed under `$HOME/.cargo`; add it to `PATH` if needed.

---

## 6. Verification status

| Component | Verified by |
|---|---|
| Lexer/parser/AST | unit tests, diagnostics suite, robustness suite |
| Semantic analysis | unit tests + `tests/diagnostics.rs` |
| Codegen | `tests/ir.rs`, native tests at `--opt 0..3` |
| C ABI | `tests/ffi.rs` (C helper compiled with `cc`, both directions) |
| Modules/projects | `tests/modules.rs` (manifest, archive, shared + dlopen) |
| Stdlib | `tests/stdlib.rs` |
| Concurrency | `tests/concurrency.rs` |
| Tooling | `tests/tooling.rs` (DWARF via `readelf`, LSP frames, sanitizers) |
| Performance | `scripts/bench.sh`, `docs/benchmarks.md` |

Measured (median, CoreX/C): `mix` 1.017, `array` 1.056, `struct` 0.964,
`f64` 0.947, `fib` 1.636. Compiler throughput on a 4,001-line synthetic unit:
22 ms frontend, 798 ms full `--opt 2` build.

---

## 7. Next actions

1. Close the `fib` codegen gap: compare LLVM/GCC frame layouts and inliner
   decisions for leaf-recursive functions; benchmark after each change.
2. Add thread join (`CLONE_CHILD_CLEARTID` + futex) and automatic stack
   reclamation.
3. Add compiler-managed `thread_local` on top of the explicit TLS block.
4. Grow the macro system (statement macros, hygiene) if demand exists.
5. P2 only: monomorphized generics, cross-compilation targets, package manager.


---

## 8. Self-hosting (2026-10-28)

A CoreX-written compiler now lives under `selfhost/` and builds itself:

| Stage | Built by | Hash |
|---|---|---|
| Stage 1 | Rust `corex` | `7b2f8837…` |
| Stage 2 | Stage 1 | `644c22c5…` |
| Stage 3 | Stage 2 | `644c22c5…` |
| Stage 4 | Stage 3 | `644c22c5…` |

Stage 2/3/4 are byte-identical at `--opt 0` (LLVM 20.1.8, cc 15.2.0). The
79-case corpus (64 positive runs at `--opt 0..3` + 15 negative) passes on Stage 1,
Stage 2, and Stage 3. Stage 1 differs from Stage 2 by design: it is compiled by
the Rust backend (different ABI), while Stage 2 onward use the self-hosted
backend. The Rust compiler is unchanged and remains the full-language reference
(205/205 tests). See `docs/selfhosting/` — start with `master.md`,
`bootstrap-results.md`, and `validation-report.md`.

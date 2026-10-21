# CoreX — Master Status

Last updated: 2026-10-21. This file is a living summary. `docs/tasks.md` holds
the authoritative per-task status and evidence; `docs/ADRs.md` holds decisions.

---

## 1. Vision

CoreX is a low-level, statically typed, ahead-of-time systems language with
predictable execution, direct hardware access, explicit memory control, and a
minimal runtime. It compiles to native Linux x86-64 through LLVM 20 and produces
statically linked ELF executables that do not depend on libc.

Non-negotiable constraints, all satisfied by the current implementation:

- no garbage collector, no background reclamation;
- no hidden allocation (every byte is a stack slot or an explicit `alloc`);
- no mandatory runtime, no exceptions, no reflection, no TLS;
- explicit memory management with raw pointers, gated by `unsafe`;
- defined integer semantics (wrapping arithmetic, masked shifts, aborting
  division and bounds violations);
- platform-specific syscall access separated from language semantics.

---

## 2. Current phase

**Milestones 1–6 are complete. Milestone 7 (hardening) is in progress and the
MVP definition-of-done is met for the implemented language subset.**

| Milestone | Status | Notes |
|---|---|---|
| 1. Research and specification | COMPLETED | Research, competitor analysis, PRD, NFR, spec, grammar, memory model, architecture, ADRs |
| 2. Compiler frontend | COMPLETED | Lexer, parser, AST, diagnostics, recovery; 23 unit tests (lexer 9, parser 14) |
| 3. Semantic validation | COMPLETED | Scopes, types, mutability, unsafe gating, return flow; 26 unit tests + 24 rejection tests |
| 4. LLVM backend | COMPLETED | LLVM 20 textual IR; verifier in the build loop; 6 IR tests |
| 5. Executable compiler | COMPLETED | `corex build`/`run` produce and execute static ELF; factorial prints 3628800 |
| 6. Low-level capabilities | COMPLETED | Pointers, pointer arithmetic, arrays, `alloc`/`free` via mmap/munmap, direct syscalls |
| 7. Hardening | IN PROGRESS | 122 tests, benchmark harness, documented defect fixes; fuzzing, golden IR, and debug info remain |

Definition-of-done status (all satisfied):

1. Compiler builds — `cargo build` succeeds.
2. Lexer token set — 8 lexer tests.
3. Parser ASTs — 14 parser tests.
4. Invalid syntax diagnostics — parser and integration tests.
5. Semantic rejection of type errors — 22 rejection tests.
6. LLVM IR verification — every example assembled by `llvm-as` in tests and in
   `corex build`.
7. Factorial compiles to native — `corex run examples/factorial.cx`.
8. Factorial returns expected results — prints `3628800`, exit 0.
9. Hello World via direct syscall — prints `Hello, World!`, exit 0.
10. Automated tests pass — 122/122.
11. Documentation matches behavior — spec §11 codes verified against
    `semantic.rs`; grammar matches `parser.rs`.
12. Task tracker has completion evidence — `docs/tasks.md`.

---

## 3. Architecture summary

```
.cx source
  → lexer (token.rs, lexer.rs)
  → parser (ast.rs, parser.rs)
  → semantic analysis (semantic.rs)  ← analysis side tables
  → LLVM 20 IR text (codegen.rs)
  → llvm-as (assemble + verify)
  → opt -passes=default<O{n}>        (when --opt > 0)
  → llc -filetype=obj -O{n}
  → cc -nostdlib -static -no-pie
  → static x86-64 ELF
```

- Rust 1.99.1, zero external crates: 6398 lines of compiler source and 1739
  lines of integration tests (8137 total).
- Backend writes IR text; no FFI to LLVM (ADR-0002).
- Memory model: explicit allocation with raw pointers and lexical `unsafe`
  capability gating (ADR-0003); no borrow checker and no false safety claims.
- Defined integer semantics (ADR-0004) and runtime bounds checks (ADR-0008).
- Freestanding target with direct syscalls (ADR-0005); entry-point stack
  realignment and IR optimization pipeline (ADR-0012).

---

## 4. Active tasks

No task is IN PROGRESS or BLOCKED.

Backlog (NOT STARTED, prioritized in `docs/tasks.md`): structs, floats, globals,
modules, C FFI, DWARF debug info, checked-arithmetic mode, opt-in owned heap
handles, ARM64/freestanding targets, fuzzing, golden IR snapshots.

---

## 5. Blockers

None. Environment notes:

- LLVM is installed user-locally at `/home/vib/opt/llvm20`; the compiler finds it
  through `COREX_LLVM_BIN` or `PATH`. A machine without LLVM 20 tools will fail
  the integration tests loudly with `E0303` (by design, not skipped).
- Rust is installed under `$HOME/.cargo`; `cargo` is not on the default `PATH`
  unless the environment file is sourced.

No architectural conflict is unresolved. Open design questions are listed at the
end of `docs/ADRs.md` and are explicitly out of scope for v0.1.

---

## 6. Integration status

| Component | Status | Verified by |
|---|---|---|
| `token.rs` / `lexer.rs` | Integrated | 9 unit tests |
| `ast.rs` / `parser.rs` | Integrated | 14 unit tests |
| `semantic.rs` | Integrated | 16 unit + 22 rejection tests |
| `codegen.rs` | Integrated | 6 IR tests, 18 native tests |
| `driver.rs` / `main.rs` | Integrated | native tests exercise the same API; manual CLI runs |
| `examples/` (5 programs) | Integrated | each compiled and executed in tests |
| `scripts/bench.sh` | Integrated | two kernels vs gcc, results in `docs/benchmarks.md` |
| Documentation set | Integrated | cross-checked against implementation |

Measured results (single machine, 5 runs, median): integer-loop workload at
parity with GCC (`CoreX/C = 0.997`); recursive `fib(35)` at `1.467×` GCC time.
See `docs/benchmarks.md` for method and caveats.

---

## 7. Next actions

1. Add `opt`-stage CI check and a golden-IR corpus once the LLVM version policy is
   settled (ADR open question 5 / task T-040).
2. Address the `fib` gap: review inlining and frame layout for leaf-recursive
   functions; measure again (no optimization at the CoreX level yet).
3. Extend fuzzing to the lexer/parser and to generated IR (task T-039).
4. Design structs and globals together, since both require a data-layout policy
   (tasks T-030, T-032).
5. Add DWARF line tables (task T-035) — the largest usability gap for debugging.

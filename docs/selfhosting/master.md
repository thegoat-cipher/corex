# CoreX Self-Hosting — Master Status

Last updated: 2026-10-28.

## 1. Objective

A CoreX compiler written in CoreX that compiles its own source and produces a
functional native Linux x86-64 executable, with a reproducible multi-stage
bootstrap.

## 2. Current stage

**Stage 3 reproducibility verified (fixed point: Stage 2 == Stage 3 == Stage 4).**
The integrated compiler is **469 functions / 15,065 lines** across 14
`selfhost/*.cx` files.

| Stage | Compiler | Output | Status |
|---|---|---|---|
| 0 | Rust `corex 0.3.0` | `build/stage1/corexc` | COMPLETED |
| 1 | Stage 0 | `build/stage2/corexc` | COMPLETED |
| 2 | Stage 1 | `build/stage3/corexc` | COMPLETED |
| 3 | Stage 2 | `build/stage4/corexc` | COMPLETED |

Hashes (final integrated run, `--opt 0`):

```text
stage1 ccffc9721da61cd5cf880a1616e01f7c762fba53939e7618c29b299321447032
stage2 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
stage3 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
stage4 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
```

Stage 2/3/4 are byte-identical. Stage 1 is Rust-compiled and differs by design
(backend/ABI), but passes the same 241-check behavioural corpus.

## 3. Completed milestones

- Baseline audit: Rust compiler 205/205 tests, release binary, toolchain.
- CoreX-native runtime: arena, growable buffer, strings/paths, files, env,
  fork/exec/wait.
- Lexer, AST, parser, type model, semantic analysis, LLVM IR backend, driver.
- Stage 1, Stage 2, Stage 3, Stage 4 builds.
- Reproducibility gate: Stage 2 == Stage 3 == Stage 4 at `--opt 0`.
- Behavioural corpus: 241 checks per stage on Stage 1/2/3.
- Slices and `vec<T>`: bounds checks, range slicing, `str`/`[u8]` compatibility,
  shallow headers, nested/slice/str elements.
- Floats: `f32`/`f64` literals, arithmetic, comparisons, casts, ABI, six-digit
  `print`, special values.
- Checked division/remainder (zero and `MIN/-1` abort 101).
- Static aggregate/global initializers, `str_to_cstr`, `read_file`,
  `write_file`.
- Diagnostics with stable codes, parser recovery, malformed-input robustness.
- Function types/pointers, indirect calls, struct fields and dispatch tables.
- `for`/`do while`/labels/`goto`, union/bitfield/FAM/packed/`align(N)` layout.
- File modules, mangling, `corex.toml`, `#[cfg]`, visibility, macros.
- C `extern`, variadics, `--hosted`, shared/static libraries, `--emit obj`.
- CLI parity: `run`, `exec`, `fmt`, `lint`, `doc`, `test`, `repl`, stdin,
  `emit-ir --verify`, emit/link flags (`-g` rejected: no DWARF).
- Differential token/AST/IR dumps with a Rust oracle; known IR ABI divergences
  recorded in `tests/selfhosting/dump-ir-expected.txt`.
- Fuzzing/ASan: 326 checks, no signals or sanitizer reports.
- Performance baseline: 496 ms self-compile, 10.08 MB/s frontend.
- Optimized reproducibility gate: `repro-opt.sh --corpus` passes at `--opt 1..3`.
- CI updated for the integrated gates.

## 4. Active tasks

See `tasks.md`. Outstanding: **SH-033** (threads/atomics/volatile/TLS/asm),
**SH-034** (`lib/corex` blocked), **SH-037** (LSP/editor), **SH-039** (full Rust
suite against `corexc`), **SH-043** (remote push/CI), **SH-044** (P3 cleanup not
merged), **SH-046** (full acceptance).

## 5. Blockers

- SH-033: no `volatile`, atomic intrinsics, thread/TLS grammar, or inline asm in
  the bootstrap; this blocks `std_sync`, `std_tls`, `std_thread`, and the
  concurrency portion of the 205-test port.
- SH-034: 10/16 `lib/corex` modules built at P1 time; floats are now integrated,
  but SH-033 and remaining attribute/tooling gaps remain.
- SH-037: LSP/line-editor parity is not ported.
- SH-039: full behavioural-suite harvest requires SH-033 and remaining tooling.
- SH-043: commits are local; remote CI has not run.
- SH-044: P3 cleanup commits conflict with the much larger integrated branches;
  they are non-functional and were not merged.
- SH-046: AC-1…AC-9 cannot complete until the above are resolved.

## 6. Architectural decisions

See `ADRs.md` (ADR-SH-001…). Key choices: single textual translation unit via
`import`, subset compiler instead of language changes, process-lifetime arena,
textual LLVM IR, first-class aggregate ABI, explicit else blocks, masked
shifts, no `Expr.index` reuse, str-compatible slices with runtime bounds checks,
24-byte `vec<T>` with spec growth and checked indexing, Stage2/3/4 hash
equality, Rust Stage 0 unchanged. The integrated compiler also uses separate
AST fields for struct-literal names (`Expr.sfield`) versus member access
(`Expr.field`) and a whole-program module build (visibility enforced in
semantic analysis rather than by generated interface files).

## 7. Validation

```sh
selfhost/bootstrap.sh                  # Stage 1-4, corpus, CLI, dumps, fuzz, perf
selfhost/repro-opt.sh --corpus         # --opt 1..3 determinism/fixed point
(cd compiler && cargo test)            # Rust reference suite 205/205
```

Final observed gates:

```text
check: ok: 469 function(s), entry `main`
stage2 == stage3 == stage4 (d270cd67…)
selfhosting tests: 241 passed, 0 failed (Stage 1/2/3)
cli tests: 62 passed, 0 failed
dump tests: 73 passed, 0 failed
fuzz tests: 326 passed, 0 failed
cargo test: 205 passed, 0 failed
repro-opt: opt1 a5a09716…, opt2 e076f652…, opt3 76445b56…
```

`file build/stage2/corexc` → static ELF x86-64; `ldd` → not a dynamic
executable.

## 8. Next implementation priorities

1. **SH-033:** parser -> semantic builtins -> codegen for volatile, atomics,
   TLS, threads, inline asm; port `tests/concurrency.rs`.
2. **SH-034:** rebuild all 16 `lib/corex` modules and close the remaining gaps.
3. **SH-039:** parameterize/harvest the 205 Rust tests against `corexc`.
4. **SH-037:** LSP/editor and remaining tooling.
5. **SH-043/046:** push, run remote CI, record final AC-1…AC-9 evidence.
6. **SH-044:** redo the cleanup on the integrated tree if desired; it is
   non-functional.

## 9. Known limits

Still absent from the bootstrap: threads, atomics, `volatile`, TLS, inline asm,
LSP/line-editor parity, DWARF (`-g`), and full Rust-suite parity. Module builds
are whole-program (no separate interface files). `examples/features.cx` still
fails because float compound assignment (`ratio *= 2.0`) is rejected by
semantic analysis; the other seven examples match the oracle. Aggregate/bool calling
conventions differ from the Rust compiler, so objects from the two compilers are
not link-compatible for aggregate parameters. Vec constants/globals are
rejected; vec headers copy shallowly. REPL prints no prompts. See
`bootstrap-architecture.md` §4`, `memory-audit.md` §5, and the worklogs for
details.

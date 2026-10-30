# CoreX Self-Hosting — Master Status

Last updated: 2026-10-29.

## 1. Objective

A CoreX compiler written in CoreX that compiles its own source and produces a
functional native Linux x86-64 executable, with a reproducible multi-stage
bootstrap.

## 2. Current stage

**Stage 3 reproducibility verified (fixed point: Stage 2 == Stage 3 == Stage 4).**
The integrated compiler is **545 functions / 17,747 lines** across 15
`selfhost/*.cx` files (including the LSP).

| Stage | Compiler | Output | Status |
|---|---|---|---|
| 0 | Rust `corex 0.3.0` | `build/stage1/corexc` | COMPLETED |
| 1 | Stage 0 | `build/stage2/corexc` | COMPLETED |
| 2 | Stage 1 | `build/stage3/corexc` | COMPLETED |
| 3 | Stage 2 | `build/stage4/corexc` | COMPLETED |

Hashes (final integrated run, `--opt 0`):

```text
stage1 57075baed1bb96564739e1274f16716fcd5fa693ee22c5a916eef7e5433796c0
stage2 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage3 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage4 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
```

Stage 2/3/4 are byte-identical. Stage 1 is Rust-compiled and differs by design
(backend/ABI), but passes the same 270-check behavioural corpus.

## 3. Completed milestones

- Baseline audit: Rust compiler 205/205 tests, release binary, toolchain.
- CoreX-native runtime: arena, growable buffer, strings/paths, files, env,
  fork/exec/wait.
- Lexer, AST, parser, type model, semantic analysis, LLVM IR backend, driver.
- Stage 1, Stage 2, Stage 3, Stage 4 builds.
- Reproducibility gate: Stage 2 == Stage 3 == Stage 4 at `--opt 0`.
- Behavioural corpus: 270 checks per stage on Stage 1/2/3.
- Slices and `vec<T>`: bounds checks, range slicing, `str`/`[u8]` compatibility,
  shallow headers, nested/slice/str elements.
- Floats: `f32`/`f64` literals, arithmetic, comparisons, casts, ABI, six-digit
  `print`, special values, and compound assignment (`+= -= *= /=`, plus
  `%=` via `frem` when libm is linked).
- Checked division/remainder (zero and `MIN/-1` abort 101).
- Static aggregate/global initializers for plain layouts, including
  function-pointer tables (`[fn(...); N] = [&f]`), `[str; N]`, nested arrays,
  and struct arrays; non-plain (packed/union/bitfield/`align`) statics are
  rejected with E0226, matching the oracle. Plain structs use inline
  structural IR types like the oracle.
- Compound literals: partial struct literals (`Point { x: 9, ... }`) and typed
  designated array literals (`[i32; 4] { 1: 10, 3: 30 }`).
- DWARF: `-g`/`--debug` emits a compile unit, per-function
  `DW_TAG_subprogram`, and line locations (no variable DIEs); `readelf
  --debug-dump=decodedline/info` sees the source file, lines, and functions.
- `str_to_cstr`, `read_file`, `write_file`.
- Diagnostics: all 65 negative cases assert a code and match the oracle.
- Function types/pointers, indirect calls, struct fields and dispatch tables.
- `for`/`do while`/labels/`goto`, union/bitfield/FAM/packed/`align(N)` layout;
  labels in unreachable regions are emitted so `goto` targets stay defined.
- File modules, mangling, `corex.toml`, `#[cfg]` with built-in
  `target_os`/`target_arch`/`sanitize` keys, visibility, macros.
- C `extern`, variadics, `--hosted`, shared/static libraries, `--emit obj`.
- CLI parity: `run`, `exec`, `fmt`, `lint`, `doc`, `test`, `repl`, stdin,
  `emit-ir --verify`, emit/link flags, frontend `--cfg`/`-I` everywhere;
  failed builds and signal-killed tools exit nonzero.
- Differential token/AST/IR dumps with a Rust oracle; 81/81, known IR ABI
  divergences recorded in `tests/selfhosting/dump-ir-expected.txt`.
- Language server: `corexc lsp` (stdio JSON-RPC; diagnostics, hover,
  definition, references, rename, completion, formatting); 35/35 protocol
  checks in `tests/selfhosting/lsp.py`, gated by bootstrap and CI.
- Parameterized Rust behaviour suite: `tests/selfhosting/behavior.py` runs all
  205 rows against any `corexc` with a 60 s per-process timeout and reports
  `passed/skipped/failed/blocked/timeout`: 137 passed, 0 failed, 68 blocked
  (the Rust-internal `--lib` unit tests), 0 timeout (see
  `worklogs/semantic-hang.md`).
- Semantic checker is linear in left-deep operator chains (the pointer
  pre-check and literal adoption no longer re-walk the left operand); the
  64-term regression runs in 0.10 s under a 30 s case timeout.
- Fuzzing/ASan: 330 checks, no signals or sanitizer reports; CI requires ASan.
- Concurrency surface: volatile globals and builtins, all atomic builtins,
  inline `asm`, and the `std_thread`/`std_tls` runtime, with the three
  `tests/concurrency.rs` behaviours ported to `tests/selfhosting/concurrency.sh`.
- `lib/corex`: all 16 modules build with both compilers.
- Performance baseline: see the single integrated table in
  `bootstrap-results.md` §3.
- Optimized reproducibility gate: `repro-opt.sh --corpus` passes at `--opt 1..3`.
- CI updated for the integrated gates.
- Remote CI: workflow_dispatch run
  [41095491312](https://github.com/thegoat-cipher/corex/actions/runs/41095491312)
  green on `47b60b5` (cargo 205, examples, full bootstrap with ASan,
  `repro-opt --corpus`, install smoke). Main was pushed as `96c3790`; its
  push-triggered run
  [41097021388](https://github.com/thegoat-cipher/corex/actions/runs/41097021388)
  completed green.

## 4. Active tasks

No tracked task is outstanding. `tasks.md` marks SH-043 (remote CI) and SH-046
(AC-1…AC-9) complete; the independent master audit is the remaining external
verification.

## 5. Blockers

None. Branch `room/corex-tooling-hang-fix-g3gb` is pushed and remote CI run
[41095491312](https://github.com/thegoat-cipher/corex/actions/runs/41095491312)
is green on `47b60b5`.

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

Final observed gates (see `bootstrap-results.md` for hashes and the integrated
performance table):

```text
check: ok: 545 function(s), entry `main`
stage2 == stage3 == stage4
selfhosting tests: 270 passed, 0 failed (Stage 1/2/3)
cli tests: 80 passed, 0 failed
lsp tests: 35 passed, 0 failed
dump tests: 81 passed, 0 failed
fuzz tests: 330 passed, 0 failed
concurrency tests: 33 passed, 0 failed
lib tests: 16 passed, 0 failed
cargo test: 205 passed, 0 failed
behavior suite: 137 passed, 0 failed, 68 blocked, 0 timeout (205 rows)
remote CI: run 41095491312 green (13/13 steps)
repro-opt: deterministic and self-reproducing at --opt 1..3
```

`file build/stage2/corexc` → static ELF x86-64; `ldd` → not a dynamic
executable.

## 8. Next implementation priorities

1. **SH-043/046:** push, run remote CI, record final AC-1…AC-9 evidence.
2. **SH-044:** redo the cleanup on the integrated tree if desired; it is
   non-functional.

## 9. Known limits

DWARF is minimal: line tables and `DW_TAG_subprogram` DIEs, but no local
variable/type DIEs. The LSP is implemented; the line editor remains the plain
`repl` without prompt output. The 68 `--lib` unit tests cannot run against an
external compiler (`behavior.py` reports them as blocked). Module builds are whole-program (no separate interface
files). All 8 examples match the oracle; `lib/corex` builds 16/16. Static
initializers for non-plain aggregates are rejected (E0226). Aggregate/bool
calling conventions differ from the Rust compiler, so objects from the two
compilers are not link-compatible for aggregate parameters. Vec
constants/globals are rejected; vec headers copy shallowly. Threads/TLS are
library-level (`std_thread`/`std_tls`), TLS is an explicit table, and there is
no thread join. REPL prints no prompts. See `bootstrap-architecture.md` §4,
`memory-audit.md` §5, and the worklogs for details.

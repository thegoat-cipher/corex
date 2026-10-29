# Tooling hang fix, LSP gate, and behaviour-suite accounting (SH-037/SH-039)

Date: 2026-10-29
Branch: `room/corex-tooling-hang-fix-g3gb` (base `room/corex-tooling-blockers-ieia` @ `9cdbece`,
which is based on `main` @ `ee5a555`).

## 1. Exponential `check_binary` hang (critical)

Repro:

```sh
printf 'fn main() -> i32 { return 1 + 1 + ... (64 terms); }' > /tmp/linear.cx
build/stage2/corexc check /tmp/linear.cx
```

Before the fix, with the pre-fix Stage 1 binary:

| terms | wall time |
|---|---|
| 16 | 0.10 s |
| 24 | 1.60 s |
| 32 | did not finish in 20 s (`timeout` killed it) |

After the fix the same 64-term input takes 0.10 s, and 1024 terms also take
0.10 s. `timeout 10 build/stage2/corexc check /tmp/linear64.cx` exits 0.

### Root cause

Two independent re-walks of the same left subtree in
`selfhost/semantic.cx:check_binary`:

1. The `Plus`/`Minus` pointer-arithmetic pre-check called
   `ck(s, (*e).lhs, null)` and then the normal path called
   `ck(s, (*e).lhs, expected)` again. Every level of a left-deep chain walked
   its left subtree twice: `T(n) = 2*T(n-1) + O(1)`.
2. The untyped-integer-literal adoption step re-checked the left operand when
   `is_untyped(lhs)` was true. `is_untyped` is recursive over literal-only
   `+ - * / % & | ^ << >>` trees, so a pure-literal chain was re-walked once
   per enclosing operator — the second exponential path.

### Fix

`selfhost/semantic.cx`:

- The left operand is type-checked **once**. Pointer detection runs *after*
  the normal check (`if (op == Plus || op == Minus) && type_is_ptr(t1)`), so
  pointer arithmetic keeps the same type result and diagnostics
  (`pointer arithmetic requires a usize offset`, `... requires unsafe`,
  now coded `E0209`).
- The untyped-literal re-check is skipped when the literal already has the
  operand type (`!type_eq(t1, t2)`); it exists only for leaf-literal
  adoption, and skipping it also removes duplicated diagnostics inside the
  subtree.
- A semantic recursion bound (`check_expr` wrapper, `MAX = 64`, one `E0111`
  per compilation) mirrors the oracle's `MAX_EXPR_DEPTH`. Flat left-deep
  chains are parsed iteratively, so the parser's own depth guard never sees
  them; the oracle rejects a 4500-term chain with
  `error[E0111]: expression nesting depth exceeds the limit of 64`.

### Regression

- `tests/selfhosting/cases/positive/pointer_chain.cx`: a 64-term `+` chain and
  a 32-term `-` chain plus `p + 1` / `p - 1` inside `unsafe`; prints `132`,
  exit 0, at `--opt 0..3`.
- `tests/selfhosting/run.sh` wraps positive `check`/`build` (and negative
  `check`) in `timeout ${COREX_CASE_TIMEOUT:-30}` and reports rc 124 as
  `timed out` instead of hanging the gate.
- `robustness::flat_deep_expression_chains_are_diagnosed` now passes against
  `corexc` (E0111, one diagnostic, no stack overflow).

## 2. Harness timeout (Defect 2)

`tests/selfhosting/behavior.py` previously ran `cargo test --test <suite>` with
no bound; one hung compiler stopped the whole run for >30 minutes.

Now:

- every `cargo test` invocation runs with `start_new_session=True` and a
  `--timeout` (default 60 s); on expiry the whole process group is SIGKILLed,
  the test is recorded as `timeout`, and the run continues;
- tests are run one at a time (`--exact NAME --test-threads=1`) after a
  `--list`, so attribution is exact and a hang can never consume the suite;
- the summary is `passed/skipped/failed/blocked/timeout` (JSON includes the
  timeout, suite errors, blocked reasons, and failure names);
- exit status is nonzero when any test fails **or** times out.

The 68 `cargo test --lib` unit tests call Rust-internal modules directly and
cannot run against an external binary; they are enumerated and reported as
`blocked` with that reason, so the total stays 205 instead of silently
shrinking to 137.

## 3. LSP (SH-037)

`selfhost/lsp.cx` (1,499 lines) implements the same surface as the Rust
`corex-lsp`: `initialize`, `initialized`, `shutdown`, `exit`,
`textDocument/didOpen|didChange|didClose` with `publishDiagnostics`,
`completion`, `definition`, `references`, `rename`, `hover`, and whole-document
`formatting`, over Content-Length framed JSON-RPC 2.0 on stdio. `corexc lsp`
is wired in `selfhost/driver.cx`; `bootstrap.sh` and CI run the gate.

`tests/selfhosting/lsp.py` runs a fixed protocol sequence against Stage 2:
**35 passed, 0 failed**. It covers capabilities, two diagnostics (W0001 lint,
E0201 error) with ranges, hover (signature and null on punctuation),
definition, references (2), rename (2 edits) plus invalid-identifier rejection,
formatting (one edit, exact text), completion (keywords, function and local
entries), full-document `didChange` (last change wins), unknown method → null,
`didClose` clearing diagnostics, and exit status 0.

Not implemented/out of scope: oracle-only `--oracle` differential mode is
available (`lsp.py --oracle compiler/target/release/corex-lsp`) but not part of
the gate; positions are byte columns (identical for ASCII).

## 4. Compiler parity fixes made while validating the suite

These were found by the newly bounded suite and are included because they were
prerequisite to trusting it (each has a passing test):

- `if`/`else` where both arms diverge no longer emits an unreferenced merge
  label (`error: expected instruction opcode`); `done: if c { return 1; }
  else { return 2; }` now verifies and runs.
- `panic(...)` satisfies return analysis and rejects non-`str`/`[u8]`
  arguments (`E0203`).
- `_start` falling through emits `exit(0)` instead of `ret` (SIGSEGV).
- `corex.toml` keys may be indented (the oracle accepts standard TOML).
- `module::CONST` is accepted in array lengths; non-constant array lengths are
  rejected with `E0100`.
- C default argument promotions for `...`: `f32` → `double`, `bool`/`char`/
  narrow integers → `i32` (fixes the C vararg float result 2.5 → 4.0).
- Diagnostic codes added to the paths exercised by `tests/diagnostics.rs`
  (E0202/E0207/E0208/E0209/E0212/E0213/E0217/E0219/E0222/E0227/E0229/E0232),
  including duplicate locals, `_start` signature, zero-length local arrays,
  slice bounds, and unknown array lengths.
- Float constant folding for `+ - * /` in `static`/`const` initializers.
- `cstr` can be printed and `c"..." as str` is lowered via `_cx_cstr_str`.
- `const P: *T = &G` inlines the global address.

The oracle's `KNOWN_CODES` adapter table was extended to every code the oracle
emits; previously unknown codes were silently parsed as `""`.

## 5. Final gates

```text
compiler/target/release/corex check selfhost/corexc.cx
  ok: 558 function(s), entry `main`; 17,653 lines across 15 selfhost/*.cx files
stage1 c779aa6b2bd8f0fea6101cc617b486ebaca24ec67550061547c31ae5920d46d6
stage2 4739e03ef87d810d69672cfc6d81697e93d864d0fec607bd2402c160c3bb4204
stage3 4739e03ef87d810d69672cfc6d81697e93d864d0fec607bd2402c160c3bb4204
stage4 4739e03ef87d810d69672cfc6d81697e93d864d0fec607bd2402c160c3bb4204
reproducible: stage2 == stage3 == stage4
selfhosting tests: 270 passed, 0 failed (Stage 1/2/3)
cli tests: 80 passed, 0 failed (Stage 1/2)
lsp tests: 35 passed, 0 failed
dump tests: 81 passed, 0 failed
fuzz tests: 330 passed, 0 failed; ASan: enabled
concurrency tests: 33 passed, 0 failed
lib tests: 16 passed, 0 failed
64-term regression: exit 0 in 0.10 s under `timeout 10`
```

Behaviour suite (`tests/selfhosting/behavior.py --compiler build/stage2/corexc --json`):

```text
counts: passed 126, skipped 0, failed 9, blocked 70, timeout 0 (205 total)
blocked: 68 `cargo test --lib` unit tests (Rust-internal APIs only) +
         tooling::debug_info_has_line_tables_and_subprograms (DWARF rejected) +
         tooling::test_runner_reports_pass_and_failure (`BuildConfig::test_mode`)
```

Remaining divergences (reported as `failed`, not reclassified):

| Test | Evidence | Class |
|---|---|---|
| `aggregates::static_aggregate_tables_run_at_every_optimization_level` | `error[E0226]: global initializers must be constant expressions` for `static OPS: [fn(i64,i64)->i64;1] = [&add]` (function-pointer statics are a documented SH-023 limit) | unsupported feature |
| `aggregates::static_aggregates_are_real_llvm_globals` | same fixture; also asserts the oracle's exact `@OPS = constant [1 x ptr] [ptr @add]` text | unsupported feature + IR text |
| `cli::temp_directories_are_private_and_unpredictable` | `COREX_KEEP_TEMP=1` leaves 0 dirs named `corex-build-*`; the CLI adapter owns no builder temp dir | oracle driver internals |
| `compiler_caps::imports_resolve_relative_files_and_diagnose_paths` | uses `driver::load_source` expansion and expects `bad.cx:2:12`; the adapter cannot reproduce per-file spans after expansion | oracle-only API |
| `extended::layout_introspection_unions_bitfields_and_compound_literals` | `error[E0100]: unexpected token` on `(Point){...}`, `Point { x: 9, ... }`, `[i32;4]{1:10,3:30}` | unsupported syntax |
| `ir::factorial_ir_contains_expected_structure` | asserts `define i64 @fact(i64 %p0)`; the bootstrap names parameters `%a0` (documented ABI difference) | oracle IR text |
| `ir::ir_targets_linux_x86_64_without_libc_calls` | `examples/compiler_data.cx: IR contains undef`; the bootstrap builds `str` values with `insertvalue ... undef` (valid IR, different shape) | oracle IR text |
| `ir::start_emits_exactly_one_exit_path` | counts `__corex_syscall(i64 60`; the bootstrap runtime symbol is `_cx_syscall` | oracle IR text |
| `modules::file_modules_export_public_items_and_hide_private_ones` | bootstrap reports `item is private to its module`; the test asserts the oracle prose `undefined item ` + `math::secret` | diagnostic prose |

No behaviour test times out; the harness terminates in ~24 s.

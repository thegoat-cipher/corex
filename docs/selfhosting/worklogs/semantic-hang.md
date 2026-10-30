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

## 4. Compiler parity fixes (all verified by the suite)

The bounded suite exposed real divergences; these are now fixed and covered by
the passing Rust tests, not reclassified:

- `if`/`else` where both arms diverge no longer emits an unreferenced merge
  label (`error: expected instruction opcode`); `done: if c { return 1; }
  else { return 2; }` verifies and runs.
- A trailing `syscall(60|231, ...)` (including `60 as usize`) terminates the
  block with `unreachable`, so `_start` does not get a second implicit exit.
  `_start` falling through emits `exit(0)` instead of `ret` (SIGSEGV).
- `panic(...)` satisfies return analysis and rejects non-`str`/`[u8]`
  arguments (`E0203`).
- `corex.toml` keys may be indented; `module::CONST` is valid in array
  lengths; non-constant array lengths are rejected with `E0100`.
- C default argument promotions for `...`: `f32` → `double`, `bool`/`char`/
  narrow integers → `i32` (the C vararg float result is now 4.0, not 2.5).
- Float constant folding for `+ - * /` in `static`/`const` initializers;
  `const P: *T = &G` inlines the global address.
- `cstr` is printable and `c"..." as str` lowers via `_cx_cstr_str`.
- Static aggregate tables: function pointers (`static OPS: [fn(...); 1] =
  [&add]`), `[str; N]`, nested arrays, struct arrays, and globals initialized
  from earlier globals are accepted and emitted as real LLVM constants.
- Plain structs use inline structural IR types (`{ { ptr, i64 }, i32 }`),
  matching the oracle; the known-IR-divergence file was regenerated for the
  intended signature change (`dump.sh --update-known`).
- IR text parity for the checked contracts: user parameters are `%pN`, the
  runtime helper is `@__corex_syscall`, and `str` values use
  `insertvalue ... zeroinitializer` instead of `undef`.
- Compound literals: `Point { x: 9, ... }` (omitted fields zeroed) and
  `[i32; 4] { 1: 10, 3: 30 }` (designated indices), with bounds checks
  (`E0222`) and type checks.
- Visibility: a private module item is reported as an undefined item with its
  `a::b` name, matching the oracle's name resolution.
- `COREX_KEEP_TEMP=1` keeps the build's IR/object in a private
  `$TMPDIR/corex-build-<hex>` directory (mode 0700, no pid).
- `corexc build --test` links the generated test runner (used by
  `BuildConfig::test_mode`), and `-g`/`--debug` emits DWARF: one compile unit,
  per-function `DW_TAG_subprogram`, and line locations on calls and returns
  (no local-variable DIEs). The `-g` contract test now asserts the sections.
- Diagnostic codes were added to every path exercised by
  `compiler/tests/diagnostics.rs` (E0202/E0207/E0208/E0209/E0212/E0213/E0217/
  E0219/E0222/E0227/E0229/E0232), including duplicate locals, `_start`
  signature, local zero-length arrays, slice bounds, and array lengths.
- The external-compiler adapter materializes an expanded `load_source` buffer
  from its root unit (`SourceFile::root_text`) and maps `file:line:col`
  diagnostics back through the segment table (`SourceFile::offset_in`).

## 5. Behaviour suite (SH-039)

`tests/selfhosting/behavior.py --compiler build/stage2/corexc --json`:

```text
counts: passed 137, skipped 0, failed 0, blocked 68, timeout 0 (205 rows)
```

All 137 integration tests pass (including the previously failing static
aggregate tables, compound literals, IR text, temp-dir, `load_source`, and
private-item tests). The 68 `cargo test --lib` unit tests call Rust-internal
modules directly and cannot be routed through a CLI; they are enumerated and
reported as `blocked` with that reason, so the total stays 205. No test needs a
special-case blocklist entry, and no test times out.

## 6. Final gates

```text
compiler/target/release/corex check selfhost/corexc.cx
  ok: 545 function(s), entry `main`; 17,747 lines across 15 selfhost/*.cx files
stage1 57075baed1bb96564739e1274f16716fcd5fa693ee22c5a916eef7e5433796c0
stage2 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage3 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage4 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
reproducible: stage2 == stage3 == stage4
selfhosting tests: 270 passed, 0 failed (Stage 1/2/3)
cli tests: 80 passed, 0 failed (Stage 1/2)
lsp tests: 35 passed, 0 failed
dump tests: 81 passed, 0 failed
fuzz tests: 330 passed, 0 failed; ASan: enabled
concurrency tests: 33 passed, 0 failed
lib tests: 16 passed, 0 failed
cargo test --locked: 205 passed, 0 failed
behavior suite: 137 passed, 0 skipped, 0 failed, 68 blocked, 0 timeout
repro-opt: opt1 0745f798dc417c21868f697161e1a7b923f1b0284a6304076f0ee8fad8b1da42 (524,584 B)
           opt2 2cd22d2900ac4c9c2f3b1fc46473fe495841655a90cc3e394d928d665a1214f9 (588,360 B)
           opt3 37d52093a0ab10ac304aecc5fa58e6a0299f5bc8a5322e1a607f5ec908752bf5 (955,224 B)
           deterministic and self-reproducing; 270 corpus checks at each level
perf: 1304 ms self-compile (best of 3), 113 ms frontend check, 42 ms corpus
      (0.63 MB/s), 4.72 MB/s closure throughput, 99,456/24,720 KiB peak RSS
64-term regression: exit 0 in 0.10 s under `timeout 10`
```

## 7. Remaining blockers

SH-039/AC-4, SH-043, and SH-046 are complete. Branch
`room/corex-tooling-hang-fix-g3gb` is pushed and remote CI run
[41095491312](https://github.com/thegoat-cipher/corex/actions/runs/41095491312)
is green on `47b60b5` (13/13 steps). The independent master audit is the
remaining external verification. DWARF is minimal (line tables and
subprograms; no variable DIEs, which the oracle test does not require).

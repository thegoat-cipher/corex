# Second Self-Host Readiness Audit — Final Integrated Tree

Date: 2026-10-30
Audited commit: `cb11cc6` (`main` / `origin/main`, "docs: record green main CI
and final acceptance")
Baseline for comparison: `0c55d59` (pre-self-hosting baseline)
Host: Linux 6.17.0-41-generic, x86-64, 16 cores
Toolchain: rustc/cargo 1.99.1, LLVM 20 (`$HOME/opt/llvm20/bin`), cc 15.2.0
Scope: read-only re-audit of the final integrated tree. No source, test, or
existing doc was changed; this report is the only commit.

All commands were run from the worktree root with
`PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH"` unless noted.

## 1. Verdict

**READY WITH CAVEATS.** The compiler at `cb11cc6` is genuinely self-hosted and
reproducible: Stage 1 is built by Rust, Stage 2 builds itself, Stage 2/3/4 are
byte-identical (`bcf17bc8…`), and every executable gate I ran passed with the
documented counts (270 corpus checks on Stages 1/2/3, CLI 80, LSP 35, dumps 81,
fuzz/ASan 330, concurrency 33, `lib/corex` 16, `cargo test` 205, `behavior.py`
137/0/68/0, `repro-opt --corpus` at `--opt 1..3`). Remote CI is green at the
final main commit. The caveats are about claims, not the bootstrap: AC-2
("same ordered diagnostic codes and start positions") fails as written because
13/65 negative inputs differ from the Rust oracle in diagnostic count or start
column; AC-4 is externalized only for 137 behavioral rows, of which two still
exercise Rust internals and none are gated by CI; AC-8's PASS cites an
`--opt 0` number although the criterion is `--opt 2` (independently measured
here at 6.75 s, so the criterion holds). Several current-state docs carry stale
hashes, sizes, and line counts. None of these blocks the self-hosting property;
all of them should be corrected before the acceptance claim is treated as
literal.

## 2. Gate results (run, not quoted)

### 2.1 `selfhost/bootstrap.sh` — PASS (exit 0)

```text
stage1 57075baed1bb96564739e1274f16716fcd5fa693ee22c5a916eef7e5433796c0  (569,096 B)
stage2 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64  (637,200 B)
stage3 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64  (637,200 B)
stage4 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64  (637,200 B)
reproducible: stage2 == stage3 == stage4
```

| Gate | Exact observed result |
|---|---|
| corpus, Stage 1 | `270 passed, 0 failed` |
| corpus, Stage 2 | `270 passed, 0 failed` |
| corpus, Stage 3 | `270 passed, 0 failed` |
| CLI, Stage 1 | `80 passed, 0 failed` |
| CLI, Stage 2 | `80 passed, 0 failed` |
| LSP (`lsp.py`, Stage 2) | `35 passed, 0 failed` |
| differential dumps (Stage 2) | `81 passed, 0 failed` |
| fuzz/ASan (Stage 2) | `330 passed, 0 failed`; `ASan: enabled (60 mutated inputs + corpus)` |
| concurrency (Stage 2) | `33 passed, 0 failed` |
| `lib/corex` (Stage 2) | `16 passed, 0 failed` |
| performance baseline | self-compile `--opt 0` 805 ms; frontend check 74 ms; corpus 33 ms (0.80 MB/s); closure 7.21 MB/s; peak RSS 99,412 KiB / 24,908 KiB |
| `check selfhost/corexc.cx` | `ok: 545 function(s), entry main` |

The source closure is 17,747 lines across 15 files; `wc -l` matches the
documented total.

### 2.2 `selfhost/repro-opt.sh --corpus` — PASS (exit 0)

| Level | SHA-256 | Size |
|---|---|---:|
| `--opt 1` | `0745f798dc417c21868f697161e1a7b923f1b0284a6304076f0ee8fad8b1da42` | 518,920 B |
| `--opt 2` | `2cd22d2900ac4c9c2f3b1fc46473fe495841655a90cc3e394d928d665a1214f9` | 582,384 B |
| `--opt 3` | `37d52093a0ab10ac304aecc5fa58e6a0299f5bc8a5322e1a607f5ec908752bf5` | 949,520 B |

At each level `repeat == self == fixed == stage1 oracle`, and the 270-check
corpus passed with each optimized compiler. The hashes and sizes match
`bootstrap-results.md` §5 exactly.

### 2.3 Other gates

| Check | Result |
|---|---|
| `(cd compiler && cargo test --locked)` | `205 passed, 0 failed` (68 unit + 137 integration; exit 0) |
| `python3 tests/selfhosting/behavior.py --compiler build/stage2/corexc --json` | `passed 137, skipped 0, failed 0, blocked 68, timeout 0` (205 rows, exit 0) |
| blocked rows | exactly the 68 `corex::lib` unit tests; reason is the single string "unit tests call Rust-internal modules directly; no CLI equivalent exists" |
| `python3 tests/selfhosting/lsp.py build/stage2/corexc` | `35 passed, 0 failed` |
| `tests/selfhosting/lib.sh build/stage2/corexc` | `16 passed, 0 failed` |
| `scripts/example-matrix.sh` | `all 8 examples match at --opt 0,1,2,3` (self-consistency only; see F-11) |
| independent example differential (Stage 0 vs Stage 2, build rc + stdout + run rc, `--opt 0..3`) | 8/8 identical |
| independent corpus provenance (Stage 0 vs checked-in `.out`/`.rc`, 40 cases × 4 opt levels) | 160/160 identical |
| negative diagnostics, first code only | 65/65 match the oracle |
| negative diagnostics, ordered codes + start positions | 13/65 differ (5 count differences, 8 column differences) |
| 64-term `1+1+…` `check` | 2 ms, exit 0 |
| depth-limit boundary (`62..66`, `100` terms) | oracle and `corexc` agree at every size (65 terms is the first E0111) |
| `COREX_KEEP_TEMP=1` build | private `corex-build-<hex>` dir, mode 0700, contains `program.ll` and `program.o` |
| `-g` build | `readelf --debug-dump=decodedline` shows `hello.cx` lines at 0x401000; one `DW_TAG_subprogram` for the one-function example |

### 2.4 Remote CI (checked via GitHub API and `git ls-remote`)

| Run | Commit | Event | Conclusion |
|---|---|---|---|
| [41095491312](https://github.com/thegoat-cipher/corex/actions/runs/41095491312) | `47b60b5` | workflow_dispatch, branch `room/corex-tooling-hang-fix-g3gb` | success |
| [41097021388](https://github.com/thegoat-cipher/corex/actions/runs/41097021388) | `96c3790` | push, `main` | success |
| [41098039491](https://github.com/thegoat-cipher/corex/actions/runs/41098039491) | `cb11cc6` | push, `main` | success |

`git ls-remote origin` reports `refs/heads/main` at
`cb11cc6d56020c3d656df1b14ae80ada4ed48ce2`; the local HEAD is the same commit.
The CI job has 16 steps (12 substantive plus set-up/post/complete), not the
"13 steps" quoted in the docs (F-09). The workflow greps the exact corpus,
CLI, LSP, dump, fuzz, concurrency, lib, and `ASan: enabled` lines and asserts
205 cargo tests.

### 2.5 Documented-vs-observed discrepancies

- `bootstrap-results.md` §2 quotes Stage 1 `926ecdec…` / Stage 2/3/4
  `dcccb0e0…` at 510,960 / 575,128 bytes. The final tree builds
  `57075bae…` / `bcf17bc8…` at 569,096 / 637,200 bytes. The §2 table is a
  different, older run presented as current (F-04).
- `semantic-hang.md` quotes optimized sizes 524,584 / 588,360 / 955,224 bytes
  for the same three hashes; `repro-opt` observes 518,920 / 582,384 / 949,520.
  `bootstrap-results.md` §5 has the correct sizes; the worklog does not (F-05).
- `validation-report.md:59-60` says `ok: 534 function(s)`; the final tree is
  545 functions (F-06).
- `bootstrap-architecture.md:51-64` per-file line counts sum to 15,228 lines
  and omit `lsp.cx`; the tree is 17,747 lines including 1,581 LSP lines (F-06).
- Task rows SH-010…SH-013 and SH-044 still cite `96f1ef30…` / `f3e770a3…`;
  SH-037 says `selfhost/lsp.cx` is 1,499 lines (actual 1,581). The final gate
  block below those rows has the correct hashes (F-06).
- Performance numbers differ from the docs: 805 ms / 74 ms / 7.21 MB/s
  observed versus 1304 ms / 113 ms / 4.72 MB/s documented. This is host-load
  variation, not a hash/count inconsistency, but the table is presented as a
  single baseline while varying by ~1.6×.
- Root `master.md` (dated 2026-10-22) still says "249 checks; CLI 80; dumps 75;
  fuzz 327; gaps: SH-033/034/037/039" and "P2 remains optional and not
  started"; every one of those statements is superseded (F-06).

## 3. Task and acceptance-criteria audit

### 3.1 AC-1…AC-9

| ID | Reported | Audit result | Evidence / correction |
|---|---|---|---|
| AC-1 corpus parity | PASS (scope) | **PASS (scope: stdout + exit)** | 270/270 × 3 stages; 160/160 checked-in expectations reproduced by the Rust oracle at `--opt 0..3`; 8/8 examples identical. The runner does not compare stderr, so the criterion's stderr clause is unverified (F-12). |
| AC-2 negative parity | PASS | **FAIL as written** | All 65 first diagnostic codes match, but 13/65 inputs differ in ordered diagnostic count or start column; 5 report a different number of diagnostics and 8 report a different column (F-01). |
| AC-3 IR validity | PASS | **PASS** | Every corpus build runs `llvm-as`; 270 checks × 3 stages; CLI covers `emit-ir --verify`; dumps 81/81. |
| AC-4 205-test behavior suite | PASS (scope) | **PASS (scope), overstated wording** | 137 passed / 0 failed / 68 blocked / 0 timeout; blocked set is exactly `cargo test --lib`. No behavior-failure blocklist. But 2 of the 137 rows never invoke `corexc` (`docs_are_generated_from_doc_comments`, `language_server_reports_diagnostics_and_edits`), and `formatter_is_idempotent_and_semantics_preserving` tests the Rust formatter before compiling; "Every CLI-expressible integration test passes" is false. `behavior.py` is not run by bootstrap or CI (F-02). |
| AC-5 modules/FFI | PASS (scope) | **PASS (scope)** | 4 module projects (3 normal × 4 + 1 `must_fail`) and 1 FFI project × 4 in the corpus; behavior module/FFI tests pass externally; `lib/corex` 16/16. Whole-program modules remain a documented limit. |
| AC-6 determinism | PASS | **PASS** | Stage 2/3/4 byte-identical; `repro-opt` `repeat == self` at `--opt 1..3`. |
| AC-7 fixed point | PASS | **PASS** | `stage2 == stage3 == stage4`; `repro-opt` `fixed == self` at every level. |
| AC-8 performance | PASS | **PASS (measured), evidence is the wrong metric** | Criterion: self-compile at `--opt 2` ≤ 10 s and frontend within 2× of Rust. Measured here: 6,754 ms and 74 ms vs Rust 73 ms. The docs cite only the `--opt 0` 1304 ms figure and no Rust baseline; `perf.sh` only measures `--opt 0` (F-03). |
| AC-9 no regression | PASS | **PASS** | `compiler/src` changes since `0c55d59` are only additive `SourceFile::root_text`/`offset_in` test-support methods; `cargo test` 205/205; the Rust `corex` remains the default compiler. |

### 3.2 SH-001…SH-046

Rows **verified as stated** (code/tests/docs support the claim): SH-001–SH-009,
SH-014, SH-016–SH-018, SH-020–SH-022, SH-024–SH-027, SH-029–SH-036,
SH-038, SH-040–SH-043. Corrections and stale evidence:

| Row | Reported | Audit result |
|---|---|---|
| SH-010 | COMPLETED, Stage 1 `96f1ef30…` | COMPLETED; hash stale (final `57075bae…`). |
| SH-011 | COMPLETED, Stage 2 `f3e770a3…` | COMPLETED; hash stale (final `bcf17bc8…`). |
| SH-012 | COMPLETED, Stage 3 `f3e770a3…` | COMPLETED; hash stale. |
| SH-013 | COMPLETED, `stage2 == stage3 == stage4` | COMPLETED; equality true, cited hashes stale. |
| SH-015 | COMPLETED | COMPLETED for CI; the documentation set is not internally consistent (F-04–F-06). |
| SH-019 | COMPLETED, `floats.cx`, `float_compound.cx` | COMPLETED; both are corpus case names and pass. |
| SH-023 | COMPLETED, plain aggregates; non-plain rejected E0226 | COMPLETED; corpus + behavior suite confirm. |
| SH-025 | COMPLETED, 65/65 coded negatives | COMPLETED for the first code; not for positions/counts (F-01). |
| SH-026 | COMPLETED, 8/8 examples byte-identical to the oracle | COMPLETED; independently reproduced at `--opt 0..3`. |
| SH-037 | COMPLETED, `lsp.cx` 1,499 lines | COMPLETED; line count stale (1,581). |
| SH-039 | COMPLETED, "Every CLI-expressible integration test passes" | COMPLETED with overstated externalization; 2 rows are oracle-only, 1 partial; `behavior.py` is not in bootstrap/CI (F-02). |
| SH-044 | COMPLETED, `f3e770a3…`, 534 functions / 17,236 lines | COMPLETED; cleanup numbers are historical, final tree is 545 / 17,747 and the cited final hash is stale. |
| SH-045 | COMPLETED | **PARTIAL**: current-state docs carry stale hashes, sizes, line counts, and a stale root `master.md` (F-04–F-06). |
| SH-046 | COMPLETED, "All AC rows pass (AC-4 in scope)" | **COMPLETED WITH CAVEATS**: AC-2 fails as written; AC-1 and AC-5 are also scoped, not just AC-4; AC-8 evidence is missing. |

## 4. Findings

Severity is impact on the literal SH-046 acceptance claim, not on whether the
compiler can build itself.

| ID | Severity | Area | Evidence | Recommendation |
|---|---|---|---|---|
| F-01 | **HIGH** | AC-2 diagnostic parity | `docs/selfhosting/validation-report.md:110` claims PASS; `tests/selfhosting/run.sh:97` only greps for the first expected `error[CODE]`. Independent comparison of the oracle's `--> line:col` and `corexc`'s `path:line:col` for all 65 negatives: 13 differ. Count differences: `atomic_aggregate` (1→2), `static_const_array` (2→1), `syntax` (1→2), `vec_slice_ref` (1→2), `vec_untyped` (2→1). Column-only differences: `align_power` (8→7), `bitfield_type` (12→8), `bitfield_width` (12→8), `do_condition` (28→5), `duplicate_field` (20→8), `for_condition` (30→5), `macro_arity` (28→31), `macro_pub` (1→5). | Either compare the full ordered `(code, line, col)` sequence in the differential runner, or mark AC-2 "PASS (scope: first code)" and stop claiming position parity. |
| F-02 | **MEDIUM** | AC-4 / SH-039 externalization | `docs/selfhosting/tasks.md:57`, `validation-report.md:112`. With `COREX_TEST_COMPILER=/bin/false`, `tooling::docs_are_generated_from_doc_comments` and `tooling::language_server_reports_diagnostics_and_edits` still pass (they call `corex::docgen` and spawn `corex-lsp`); `formatter_…` fails only because it also calls `build`. `tests/selfhosting/behavior.py:48` has `BLOCKED_TESTS = {}`; the prior blocklist at `b6d7415` named exactly these tests. Neither `selfhost/bootstrap.sh` nor `.github/workflows/ci.yml` runs `behavior.py`. | Re-add explicit blocked entries for the oracle-only rows (with the CLI gate that covers them), or route them through `corexc fmt`/`doc`/`lsp`; add `behavior.py` to CI if it is called a gate. |
| F-03 | **MEDIUM** | AC-8 evidence | `validation-report.md:116` and `bootstrap-results.md:61-62` cite `--opt 0` 1304 ms and 113 ms; `perf.sh:37` measures only `--opt 0` and never invokes Rust `corex`. Criterion requires `--opt 2` and a 2× frontend comparison. Independent run: 6,754 ms `--opt 2`; 74 ms corexc vs 73 ms Rust. | Record the `--opt 2` and Rust-baseline numbers in `bootstrap-results.md` and make `perf.sh` measure them. The criterion itself is met on this host. |
| F-04 | **MEDIUM** | stale current-state hashes | `bootstrap-results.md:23-26` quotes Stage 1 `926ecdec…`, Stage 2/3/4 `dcccb0e0…` at 510,960/575,128 B. The final run is `57075bae…`/`bcf17bc8…` at 569,096/637,200 B. | Update §2 to the final run (or label it as an earlier run and point to §5/master). |
| F-05 | **LOW** | inconsistent optimized sizes | `bootstrap-results.md:128-130` and `semantic-hang.md:195-197` give different sizes for the same three hashes. `repro-opt` confirms 518,920/582,384/949,520. | Correct the worklog sizes. |
| F-06 | **LOW** | stale counts/hashes in current docs | `tasks.md:28-30,55,62`, `validation-report.md:59-60` (534 functions), `master.md:156-159` ("next priorities" still SH-043/046), root `master.md:31,47,157` (249 checks, gaps SH-033/034/037/039, "P2 not started"), `bootstrap-architecture.md:51-64` (sums to 15,228, omits `lsp.cx`). | Update current-state docs; keep the dated worklogs historical. |
| F-07 | **MEDIUM** | unbounded harnesses | `cli.sh`, `dump.sh:96-97`, `lib.sh`, `perf.sh:37`, `selfhost/repro-opt.sh:60-63`, and `scripts/example-matrix.sh` invoke the compiler without `timeout`; `bootstrap.sh` calls them. Only `run.sh`, `fuzz.sh`, `concurrency.sh`, `lsp.py`, and `behavior.py` are bounded. A wedged `corexc` stalls bootstrap until the CI 90-minute job timeout. | Wrap every compiler invocation in the harnesses with `timeout` (e.g., `COREX_CASE_TIMEOUT` or 120 s per build). |
| F-08 | **LOW** | dump gate counts skips as passes | `dump.sh:82-84` increments `pass` for the two AST-skipped cases; `dump.sh:96-97` pipes `emit-ir` into `norm_ir` without checking exit status or `pipefail`, so two failing `emit-ir` runs compare empty-to-empty and pass. | Report skips separately; check the exit status of both `emit-ir` commands before diffing. |
| F-09 | **LOW** | CI step-count claim | `tasks.md:61`, `master.md:149`, `validation-report.md:22` say "13/13 steps". The GitHub API shows 16 steps (12 substantive) on all three runs. | Correct to "12 substantive steps" or drop the count. |
| F-10 | **LOW** | bootstrap reproducibility hygiene | `selfhost/bootstrap.sh:39` builds Stage 0 with `cargo build --release` (no `--locked`) when the binary is missing; `bootstrap.sh`/`repro-opt.sh` never clear `build/stage*`/`build/opt*`, so a stale artifact can survive a failed overwrite. | Use `cargo build --release --locked`; clear or timestamp the stage/opt output directories. |
| F-11 | **LOW** | example-matrix provenance | `bootstrap-results.md:115` says 8/8 against the oracle, but `scripts/example-matrix.sh` runs one compiler (Rust debug by default) across opt levels. The claim is true by independent differential, not by this script. | Add an oracle-vs-corexc mode to the script, or reword the claim. |
| F-12 | **LOW** | abort-message claim | `validation-report.md:73` says abort cases exit 101 "with the expected message"; `run.sh` compares only stdout and exit status (the `.err` file is captured but never diffed). | Add expected stderr files/checks or narrow the claim. |
| F-13 | **LOW** | SH-046 scope wording | `tasks.md:64` says "All AC rows pass (AC-4 in scope)"; `validation-report.md` marks AC-1 and AC-5 as PASS (scope) as well. | List every scoped AC, or make the scopes explicit in the row. |
| F-14 | **LOW** | stale harness comment | `compiler/tests/common/mod.rs:20` says "the harness blocklist names the tests that depend on oracle-only behaviour"; `BLOCKED_TESTS` is empty. | Update the comment after F-02 is resolved. |

No correctness defect was found in the final compiler changes: the atomic
volatile/atomic/inline-asm paths emit the expected LLVM constructs and pass the
33-check concurrency gate; the LSP answers the 35 protocol checks; the static
tables, compound literals, IR text, private-item diagnostics, `COREX_KEEP_TEMP`,
`build --test`, DWARF `-g`, and `load_source` adapter behave as documented. The
`compiler/src/diagnostics.rs` additions are purely additive and are referenced
only from `compiler/tests/common/mod.rs`.

## 5. Residual risks / untested paths

1. **68 Rust-internal unit tests** (`cargo test --lib`) are never run against
   `corexc`. Lexer/parser/semantic/codegen internals are only exercised
   indirectly through the corpus, CLI, and behavioral integration tests.
2. **Two behavioral rows remain oracle-only** (docgen, LSP) and one is partial
   (formatter). Equivalent coverage exists in `cli.sh` and `lsp.py`, but it is
   not the same assertions.
3. **Diagnostic positions and counts** differ from the oracle for 13/65
   negatives (F-01). Tools/tests that consume line/column or expect one
   diagnostic per input can diverge.
4. **Diagnostic message text** differs broadly (e.g., `too many function
   arguments` vs `f takes 1 argument(s), found 2`); AC-2 does not require
   text, but consumers may.
5. **DWARF is minimal**: compile unit, per-function `DW_TAG_subprogram`, and
   line locations; no local-variable or type DIEs.
6. **Whole-program modules**: no separate interface files; visibility is
   enforced during whole-program semantic analysis.
7. **Aggregate/`bool` calling conventions** differ from the Rust compiler, so
   objects from the two compilers are not link-compatible for aggregate
   parameters.
8. **Threads/TLS are library-level**: `std_thread`/`std_tls`, explicit TLS
   table, no thread join.
9. **LSP positions are byte columns** (ASCII-identical); protocol coverage is
   35 checks, not a full client matrix; the `--oracle` differential mode is not
   in the gate.
10. **ASan covers `check` only**; `fmt`, `lint`, `doc`, `dump`, and `repl` are
    not sanitized. `detect_leaks=0`; the process-lifetime arena is not leak
    checked.
11. **Harness hang surface** (F-07): cli/dump/lib/perf/repro-opt/example-matrix
    have no per-command timeout.
12. **Stale build directories** (F-10) can make a local rerun look reproducible
    after a failed overwrite.
13. **Acceptance-test adapter double-runs the oracle**: `common::driver::check`
    reruns `corex::driver::check` on a corexc success to obtain a `Frontend`.
    A corexc-only semantic mistake that still accepts (and prints no
    diagnostic) is invisible to check-only tests; most tests also build/run,
    which partially covers this.
14. **`load_source` expansion remains a Rust helper**; the external run
    exercises `corexc`'s own import resolution during build, but the expanded
    buffer used for diagnostic mapping is produced by the oracle.
15. **Platform scope**: Linux x86-64 only; no cross-compilation or non-Linux
    targets; the compiler shells out to `llvm-as`, `opt`, `llc`, and `cc`.

## 6. Final self-hosting statement

**Can be claimed.** CoreX has a compiler written in CoreX (17,747 lines, 545
functions, 15 `selfhost/*.cx` files) that compiles its own source to a
freestanding static Linux x86-64 ELF. The bootstrap is reproducible: Stage 1
(compiled by Rust `corex 0.3.0`) is `57075bae…`; Stage 2/3/4 are byte-identical
at `bcf17bc8…`; optimized self-builds at `--opt 1..3` are deterministic and
reach their own fixed points (`0745f798…`, `2cd22d29…`, `37d52093…`). The
integrated gates pass with the exact counts above, and the final `main` commit
`cb11cc6` is pushed with green CI. The 137 CLI-expressible behavioral rows
pass, the 68 blocked rows are the Rust-internal unit tests, and there is no
behavior-failure blocklist.

**Cannot be claimed.** That the full 205-test suite runs against `corexc` (68
unit tests cannot, and 2 of the 137 "passed" rows still exercise Rust
internals). That diagnostic parity includes positions or per-input counts
(13/65 negatives differ). That AC-8 was evidenced as written (`--opt 2` and the
Rust frontend comparison are missing from the docs, though both pass when
measured). That `behavior.py` is enforced by CI or bootstrap. That the compiler
is ABI-compatible with Rust `corex` for aggregate parameters, that DWARF
describes variables/types, that module builds produce interfaces, or that
threads/TLS are runtime-level with join.

The central self-hosting claim is supported. The acceptance paperwork needs the
corrections in §4 before "all AC rows pass" is literally true.

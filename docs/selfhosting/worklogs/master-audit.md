# P0–P3 Master Audit — Integrated Self-Hosting Tree

> **Post-fix status (2026-10-28):** every finding below was fixed on
> `room/corex-audit-fixes-6cmw` and independently re-verified on merged `main`
> (`d6983fd`). This file is the pre-fix snapshot; see §7 for the follow-up
> evidence and residual blockers.
>
> **Update (2026-10-29):** SH-033 and SH-034 are now complete (volatile,
> atomics, inline asm, thread/TLS runtime, `lib/corex` 16/16); see
> `worklogs/sh033-034.md`. The blockers listed below for those two rows are
> historical.

Date: 2026-10-28
Audited commit: `084f1f3` (`main`), baseline `0c55d59`
Worktree: `room/corex-p0-p3-master-audit-auz3`
Scope: read-only audit of the integrated P0–P3 workstreams; no source, test, or
doc fix was made except this report.

## 1. Verdict

**The integrated gates are green; the integration is not ready for SH-046.**

The reproducible bootstrap itself is strong: Stage 1–4, the `--opt 0` fixed
point, the 241-check corpus on Stages 1–3, CLI 62, dumps 73, fuzz/ASan 326,
`--opt 1..3` determinism/fixpoint, and 205 Rust tests all pass with the recorded
hashes. However, the audit found five correctness defects that the corpus does
not exercise and several claims that the integrated tree does not support.

- **P0: FAIL (scope claim).** Floats/division/IO/diagnostics gates pass, but
  static initializers for non-plain aggregates emit invalid IR, and float
  compound assignment is not implemented. SH-023 is not complete for the
  packed/union/bitfield/`align(N)` structures delivered by P1.
- **P1: FAIL (integration).** The new language surface passes its corpus, but
  built-in `#[cfg]` keys are missing (silent wrong-branch selection), the
  frontend-only `--cfg`/`-I` options are rejected by `check`/`emit-ir`, and a
  `goto` into an unreachable nested block produces invalid IR.
- **P2: FAIL (tooling behavior).** CLI/dump/fuzz gates pass, but `build`
  always exits 0 and prints `compiled` when `llvm-as`/`llc`/`cc` fails; a tool
  killed by a signal is also reported as success. This invalidates the claimed
  CLI exit-code contract and can mask the P0/P1 IR bugs above.
- **P3: PARTIAL.** `repro-opt.sh --corpus` passes exactly. The cleanup commits
  were correctly not merged (SH-044 remains BLOCKED); the architecture and
  memory-audit documents are materially stale.

## 2. Gate results (run, not quoted from docs)

All commands were run from the worktree root with
`PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH"`.

| Command | Result | Observed evidence |
|---|---|---|
| `selfhost/bootstrap.sh` | **PASS** | Stage 1 `ccffc9721da61cd5cf880a1616e01f7c762fba53939e7618c29b299321447032`; Stage 2/3/4 `d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20`; sizes 484,184 / 542,128 bytes; `reproducible: stage2 == stage3 == stage4`. |
| corpus in bootstrap | **PASS** | `241 passed, 0 failed` on Stage 1, Stage 2, and Stage 3. Composition: 36 positive × 4 opt levels = 144; 52 negative; 28 malformed; 4 module projects (3 normal × 4 + 1 `must_fail` = 13); 1 FFI project × 4 = 4. Total 241. |
| CLI gate | **PASS** | `62 passed, 0 failed` on Stage 1 and Stage 2. |
| differential dumps | **PASS** | `73 passed, 0 failed` on Stage 2: 36 token + 36 AST (2 documented skips) + 1 IR baseline. |
| fuzz / ASan | **PASS** | `326 passed, 0 failed`; `ASan: enabled (60 mutated inputs + corpus)`. Base 215 checks (13 checked-in fuzz inputs + 2 generated + 200 mutants) plus 111 ASan checks (2 self-host + 36 positive + 13 fuzz + 60 mutants). |
| performance | **PASS (smoke only)** | `675 ms` self-compile `--opt 0` (best of 3), `61 ms` frontend check, corpus `28 ms` (0.83 MB/s), closure `7.30 MB/s`, peak RSS `93,984 KiB` self-compile / `20,736 KiB` frontend. `perf.sh` only gates the 60 s smoke bound. |
| `selfhost/repro-opt.sh --corpus` | **PASS** | opt1 `a5a097167d9f1e83f217384ef134792cc2bc7eff569eb9fd0a6e6b868808519b` (422,968 B); opt2 `e076f652dbbd03ec7357bb18e20b71d26e194c9dd14507673ee6e9813a7e1aac` (470,720 B); opt3 `76445b562f9bf982b410af38b41992722ae112df4999c4bd1d4549564b992b36` (874,760 B); `repeat == self == fixed == stage1 oracle` at each level; 241 checks with each optimized compiler. |
| `(cd compiler && cargo test --locked)` | **PASS** | `205 passed; 0 failed` (68 unit + 137 integration across 18 test binaries); no compiler warnings. |
| extra positive differential (36 cases, `--opt 0`) | **PASS** | Stage 1 vs Rust oracle: 36/36 identical stdout/exit. |
| example matrix (`scripts/example-matrix.sh`) | **PASS (self-consistency)** | All 8 examples agree across `--opt 0..3`. |
| bootstrap vs oracle examples | **7/8** | `features.cx` fails (see F-04); the other seven match stdout/exit. |
| `lib/corex` matrix | **12/16** | Builds: `std_alloc`, `std_builder`, `std_dl`, `std_env`, `std_fmt`, `std_fs`, `std_io`, `std_map`, `std_mem`, `std_process`, `std_str`, `std_time`. Blocked: `std_math` (float compound assignment), `std_sync`, `std_thread`, `std_tls` (SH-033). |
| artifact checks | **PASS** | Stage 2 is a static x86-64 ELF; `ldd` reports `not a dynamic executable`. |
| repo hygiene | **PASS** | `git status` clean; `build/` and `compiler/target/` ignored; no deleted files in `0c55d59..HEAD`; `compiler/` unchanged; `quick_tour.cx` absent; no tracked `.env`/secret files. `git diff --check` reports only intentional trailing whitespace in two truncated malformed fixtures. |

**Documented-vs-observed discrepancies**

- Hashes, sizes, 241/62/73/326 counts, and `cargo test 205` match the docs.
- Performance does not: docs quote both `496 ms`/`10.08 MB/s` and
  `695 ms`/`7.07 MB/s`; this integrated tree measures `675 ms`/`7.30 MB/s`
  (and `7.30 MB/s` closure throughput, not `10.08`).
- The docs say the 52 negative cases have stable diagnostic codes; only 30 have
  `.code` files and 22 are only checked for a non-empty diagnostic. One of the
  22 (`builtin_value`) even disagrees with the oracle code.
- The docs say SH-034 is `10/16`-blocked; the current integrated tree is
  `12/16`, and `std_math` is blocked by a P0 float gap, not only SH-033.

## 3. Findings

Severity is impact on SH-046 acceptance, not rarity. File:line references are to
the audited commit.

| ID | Severity | Area | file:line | Evidence | Recommendation |
|---|---|---|---|---|---|
| F-01 | **HIGH** | build/CLI status | `selfhost/driver.cx:1175-1188` | `dr_cmd_build` discards the return value of `dr_build` and unconditionally prints `compiled` and returns 0. Repro: `build/stage1/corexc build /tmp/audit_packed_static.cx -o /tmp/out` prints an `llvm-as` constant-type error, then `compiled ...` and exits 0; no output file. `-o /nonexistent-dir/out` similarly exits 0 silently. | Propagate `rc` (`if rc != 0 { return rc; }`) and print `compiled` only on success. Add CLI tests for invalid IR and unwritable output. This is a prerequisite for trusting any build gate. |
| F-02 | **HIGH** | external-tool status | `selfhost/runtime.cx:511-513` | `rt_run` returns `(status >> 8) & 255`, so a tool killed by a signal returns 0. Repro: a fake `COREX_CC` that runs `kill -SEGV $$` makes `build` print `compiled` and exit 0 with no binary. `rt_run_status` also ignores `wait4` failure. | Return `128 + signal` (or any nonzero) for signal death and treat `wait4 < 0` as failure; make `dr_exec` reject it. |
| F-03 | **HIGH** | P0/P1 static aggregates | `selfhost/codegen.cx:2860-2891`; non-plain lowering `codegen.cx:349-364` | `const_value` always emits an LLVM struct literal `{ ... }` for `K::Struct`, but `llty` lowers packed/union/`align`/bitfield/nested-non-plain structs to `[N x i8]`. Repro: `packed struct P { a: u8, b: u32 } static G: P = P { a: 1, b: 2 };` passes `check`, then `emit-ir --verify` fails with `constant expression type mismatch` on `@G = constant [5 x i8] { i8 1, i32 2 }`. Same for a union static (`[4 x i8] { i32 7, float ... }`). `build` then hides the failure per F-01. | Either lower non-plain structural constants to a `[size x i8]` byte initializer, or reject them in semantic analysis with a coded diagnostic (like E0226). Add packed/union/bitfield/`align` static regression cases. |
| F-04 | **HIGH** | P0 floats / examples / lib | `selfhost/semantic.cx:2389-2390`; `examples/features.cx:39`; `lib/corex/std_math.cx:162,164` | Compound assignment rejects every non-integer type (`compound assignment requires an integer`), although `gen_simple_bin` already handles float `+ - * / %`. `ratio *= 2.0` fails in the bootstrap but succeeds in the Rust oracle. This is why `features.cx` is still the one failing example and why `std_math` still does not build. | Allow `f32`/`f64` in the compound-assignment check and add float `+= -= *= /=` coverage. Then update SH-026/SH-034 claims and the example/library matrices. |
| F-05 | **HIGH** | P1 `#[cfg]` parity | `selfhost/driver.cx:731-746` (only `--cfg`/features); `selfhost/parser.cx:2004-2017` (`cfg_has`) | Rust `corex` always seeds `target_os=linux`, `target_arch=x86_64` (and `sanitize=true` with `--sanitize`); the bootstrap seeds none. Repro: `#[cfg(target_os = "linux")] fn main() -> i32 { return 1; }` with a `not(...)` fallback builds under the bootstrap and returns 2, while the oracle returns 1. This is a silent wrong-branch selection, not a diagnostic. | Seed the built-in cfg keys in `dr_opts_cfg`/manifest handling and add a cfg-parity test. |
| F-06 | **MEDIUM** | P1/P2 frontend options | `selfhost/driver.cx:244-295`, `:1458`, `:1471`, `:726-750` | `--cfg` and `-I` are gated behind `allow_link`, so `check` and `emit-ir` reject them: `corexc check file --cfg feature=fast` and `check file -I dir` both exit 2 `unknown option`; the oracle accepts both. `build - --cfg feature=fast` is accepted but `dr_front_stdin` never passes cfg/include lists, so the flag is ignored (program selected the no-cfg branch, rc 2 instead of 1). | Make `--cfg`/`-I` frontend options accepted by `check`/`emit-ir`/`build`/`run`/`exec`, and pass them through `parse_program_text` for stdin. Add CLI tests. |
| F-07 | **MEDIUM** | P1 labels/goto | `selfhost/codegen.cx:2728-2733` | `gen_block` skips all dead statements except a top-level `S::Label`; a label nested inside a dead block or dead `if` is never emitted. `goto done; { done: return 7; }` passes `check`, then `emit-ir --verify` fails with `use of undefined value '%L1'`. The Rust oracle has the same IR bug, so this is not a regression, but the bootstrap also reports success because of F-01. | Recursively collect/emit labels in unreachable regions, or reject a label target that is not reachable. Add an IR-verify regression case. |
| F-08 | **MEDIUM** | negative diagnostics | `tests/selfhosting/run.sh:75`; `docs/selfhosting/validation-report.md:56-57,84-85` | `run.sh` asserts a code only when `negative/<name>.code` exists. There are 52 negative cases but only 30 `.code` files; the other 22 are accepted as "rejected with any diagnostic". `builtin_value` is `error[E0201]` in the bootstrap and `error[E0218]` in the oracle. The validation report claims "52 negative cases are rejected with stable diagnostic codes". | Add `.code` files for the remaining 22 (or narrow the claim to 30 coded cases) and reconcile `builtin_value` with E0218. Make the runner assert codes for every negative case. |
| F-09 | **MEDIUM** | gate coverage | `tests/selfhosting/fuzz.sh:122-163`; `.github/workflows/ci.yml:113` | If the hosted ASan build or link fails, `fuzz.sh` prints `ASan: unavailable ... skipped` and still exits 0. CI only greps `fuzz tests: [0-9]+ passed, 0 failed`, not 326 or "ASan: enabled". A regression can silently halve the fuzz gate. | Make ASan mandatory for the bootstrap gate (or fail if `cc -fsanitize=address` is unavailable), and assert the exact 326/ASan-enabled line in CI. |
| F-10 | **LOW** | bootstrap CLI/docs | `selfhost/bootstrap.sh:10` | Usage advertises `[--jobs N]`, but the script never parses arguments. `bootstrap.sh --jobs 4` is silently ignored. | Parse/validate `--jobs`, or remove the advertised option. |
| F-11 | **HIGH** | docs: architecture | `docs/selfhosting/bootstrap-architecture.md:51-59,63-64,103-109,161-166` | The document is pre-P0/P1: it lists function pointers, unions, bitfields, FAM, packed/`align`, `for`, `do`, labels/goto, macros, cfg, modules, floats, variadics, and C `extern` as unsupported; says there are no module units and no symbol mangling; gives stale line counts (`driver.cx` 264 vs actual 1,609; `codegen.cx` 2,373 vs 3,513; `runtime.cx` 573 vs 1,104; etc.); and its CLI section lists only `check`/`emit-ir`/`build`. | Rewrite §3/§4/§8 against the integrated tree; keep historical ADRs separate. |
| F-12 | **HIGH** | docs: memory audit | `docs/selfhosting/memory-audit.md:86-90,138-139` | States that generated programs have no dynamic array/str bounds checks and that no sanitizer validation exists. Both are false in the integrated tree: `bounds_array/slice/str/vec` exit 101, and P2 ran 326 ASan/fuzz checks. | Update the limits to the implemented bounds checks and to the actual sanitizer policy (hosted ASan, `detect_leaks=0`, process-lifetime arena). |
| F-13 | **MEDIUM** | docs: performance | `docs/selfhosting/master.md:62`, `docs/selfhosting/tasks.md:54`, `docs/selfhosting/bootstrap-results.md:53-56`, `docs/selfhosting/validation-report.md:91` | Docs quote `496 ms`/`10.08 MB/s` and `695 ms`/`7.07 MB/s`; the actual integrated run is `675 ms`/`7.30 MB/s`, with `93,984 KiB` self-compile RSS. `496 ms` was the smaller P2 source; the integrated closure is 15,065 lines. | Record one integrated performance table and refer to it everywhere; date and machine-tag the numbers. |
| F-14 | **MEDIUM** | docs: SH-034 matrix | `docs/selfhosting/tasks.md:47`, `docs/selfhosting/master.md:78` | Says `10/16` at P1 time, "floats now integrated", and attributes remaining modules to SH-033. Current matrix is `12/16`; `std_builder` and `std_fmt` now build, while `std_math` is blocked by F-04 (float compound assignment), not by SH-033. | Update to 12/16 and name `std_math`'s actual blocker; keep SH-033 for `std_sync`/`std_tls`/`std_thread`. |
| F-15 | **LOW** | docs: `-g` status | `docs/selfhosting/tasks.md:49`; `docs/selfhosting/worklogs/p2.md:139-145` | SH-036 is marked COMPLETED with "`-g` is rejected"; the P2 worklog says "`-g` therefore remains BLOCKED". The behavior is correct (rejected, rc 2), but the task status is internally inconsistent. | Mark SH-036 COMPLETED for the rejected-flag contract, or record `-g` as an explicit sub-blocker; do not leave both statuses. |

**Additional observations**

- `Expr.sfield`/`Expr.field` are used consistently: struct-literal names use
  `sfield` in `parser.cx:1142`, `semantic.cx:1601/1623`, and
  `codegen.cx:1671/2881`; member access uses `field`. No lost-name bug found.
- `Diag` code handling is coherent for the coded set: `diag_addc` stores the
  code, `diag_warn` attaches `W0001..W0004`, and `dr_diags` renders
  `error[CODE]`/`warning[CODE]`. The remaining 109 uncoded `sdiag` sites are
  the documented P0 limitation, not a renderer defect.
- `sem_run_ex`/`cg_run_ex` call sites are correct (`build`/`run` pass
  `sanitize`/`hosted`/`make_entry`; `emit-ir` uses the default wrapper). No
  duplicated fields or duplicate function definitions were found.
- Generated `const_value` for scalar, array, `str`, and plain struct globals is
  correct; the defect is specifically the non-plain structural case (F-03).
- The 589 source lines containing `unsafe {` include the P1/P2 additions; the P3 nested-
  `unsafe` cleanup is not merged (`7a00a3f` is only on
  `room/corex-backlog-p3-4uge`).

## 4. Task-status audit (SH-001…SH-046)

All rows were checked against code, corpus, and worklogs. Rows not listed below
were supported by the gates and evidence; the listed rows overstate status,
have stale evidence, or rely on a claim that fails a direct probe.

| Row | Reported | Audit result | Corrected statement |
|---|---|---|---|
| SH-019 | COMPLETED | Binary float ops/literals/print pass; compound assignment is rejected. | COMPLETED for binary ops; add compound-assignment scope or fix F-04. |
| SH-020 | COMPLETED | Same as SH-019; `features.cx` and `std_math` still fail. | Partial for "float arithmetic" as commonly understood; fix F-04. |
| SH-023 | COMPLETED | Static scalar/array/plain-struct works; packed/union/`align`/bitfield static emits invalid IR and `check` accepts it. | COMPLETED only for plain aggregates; fix F-03 or mark the limitation. |
| SH-025 | COMPLETED | Malformed robustness and 30 coded negatives pass; 22 negatives have no code and `builtin_value` differs from the oracle. | COMPLETED for robustness; negative-code parity is partial (F-08). |
| SH-026 | COMPLETED | 7/8 examples is still the literal count, but the documented reason ("floats") is stale: `features.cx` now fails on float compound assignment. | Count supported; update the reason and re-run after F-04. |
| SH-030 | COMPLETED | Manifest modules/mangling/visibility pass; built-in cfg keys are missing and `check`/`emit-ir` reject `--cfg`/`-I`. | Partial for cfg parity; fix F-05/F-06. |
| SH-034 | BLOCKED | Correctly blocked, but current matrix is 12/16 and `std_math` is a P0 float-compound blocker. | BLOCKED; update evidence to 12/16 and name F-04. |
| SH-035 | COMPLETED | `build` returns 0 after tool/IR failure; `--cfg`/`-I` are rejected by `check`/`emit-ir` and ignored on stdin. | NOT COMPLETE as "CLI parity"; fix F-01/F-06 first. |
| SH-036 | COMPLETED | `-g` is rejected as documented, but the P2 worklog calls it BLOCKED and build/link failures are not propagated. | Partial; resolve F-15 and F-01/F-02. |
| SH-041 | COMPLETED | Baseline measured, but the quoted 496 ms/10.08 MB/s is from the smaller P2 source and does not match the integrated tree. | COMPLETED with corrected integrated numbers (F-13). |
| SH-045 | COMPLETED | Worklogs exist, but `bootstrap-architecture.md` and `memory-audit.md` are materially stale. | Partial; documentation maintenance is not complete (F-11/F-12). |
| SH-015 | COMPLETED | CI exists and its count greps match, but the documentation set is inconsistent. | Partial until F-11/F-12/F-13 are fixed. |
| SH-003 | COMPLETED | `bootstrap-architecture.md` exists but describes the baseline subset. | Partial; same documentation fix. |

Rows explicitly verified as correctly **BLOCKED/NOT STARTED**: SH-033 (no
volatile/atomic/asm/TLS surface), SH-037 (no LSP/line editor), SH-039 (no
205-test port harness), SH-043 (24 local commits ahead of `origin/main`; no push
or remote run), SH-044 (cleanup commits `7a00a3f`/`5d420d3`/`fca2d21` are only
on the P3 branch; `Program.decls` and the dead helpers remain), SH-046
(AC-1/AC-2 partial, AC-4 blocked). SH-042 is COMPLETED and reproduced exactly.
SH-001–SH-018, SH-021/022, SH-024, SH-027–SH-029, SH-031–SH-033, SH-037–SH-040
were otherwise supported by the corpus and code; no separate status correction
is required.

## 5. Residual risks / untested paths

1. **Non-plain static aggregates** (F-03): packed, union, `align(N)`, bitfield,
   and structs containing any of those. The corpus has no such static case;
   `check` accepts them and `build` hides the invalid IR.
2. **Function-pointer statics** are still rejected (`static T: fn(...) = &f`
   gives E0226). This is documented, but it is an aggregate/global ABI gap for
   SH-046.
3. **Float compound assignment** (F-04): `features.cx` and `std_math` remain
   outside the integrated matrix; this also means SH-026's "7/8" is not a
   float-complete result.
4. **Built-in cfg defaults** (F-05) and **frontend flag propagation** (F-06):
   `target_os`/`target_arch`/`sanitize` branches and `check`/`emit-ir` builds
   can select different code than the oracle.
5. **Unreachable labels** (F-07): a `goto` into a dead nested block/`if` passes
   `check`; both compilers emit undefined IR. The bootstrap's `build` reports
   success because of F-01.
6. **C aggregate ABI**: only the tested `{i64, i64}` / integer shapes are
   covered. Float aggregates, large aggregates, and mixed scalar/float
   variadics are untested; the P1 worklog explicitly limits the matrix.
7. **Whole-program modules**: no separate interface files; two modules with the
   same name are collapsed by `mod_seen`; a manifest source that is also a
   `mod` dependency is loaded twice; `corex.toml` parsing matches keys only at
   column 0 (indented `sources`/`features` are ignored). These are documented
   subset limits, not silent miscompiles, but they are outside SH-046.
8. **ASan/fuzz scope**: `fuzz.sh` fuzzes `check` only. `fmt`, `lint`, `doc`,
   `dump`, and `repl` were exercised manually against the malformed corpus with
   no signals, but they are not part of the sanitizer gate. ASan runs with
   `detect_leaks=0`; the 512 MiB process-lifetime arena is not leak-checked.
9. **Gate robustness**: ASan can be skipped (F-09); two AST cases are skipped in
   `dump.sh` by `dump-ast-skip.txt`; CI does not assert the exact fuzz count.
10. **Tool status**: `rt_run` masks signal death (F-02); `bootstrap.sh` and
    `repro-opt.sh` do not clear `build/stage*`/`build/opt*`, so a silent success
    with a stale binary can make a rerun look reproducible. CI's fresh
    checkout avoids this, local reruns do not.
11. **Known non-goals**: SH-033 concurrency (`std_sync`, `std_tls`,
    `std_thread`), SH-037 LSP/line editor, SH-039 full 205-test port, SH-043
    remote CI, `-g`/DWARF, and REPL prompts remain absent by design. SH-046
    cannot pass until the first three are resolved.
12. **Minor**: `source_filename` is hardcoded to `"corexc"` for every input
    (`codegen.cx`), so `emit-ir` on user source is misleading; temp paths are
    PID-keyed (`/tmp/corexc.<pid>...`) and are not unique per build within one
    process, though the driver does not currently run concurrent builds.

## 6. Recommended next actions (only what SH-046 needs)

1. **F-01/F-02:** propagate `dr_build`'s exit code, reject signal/wait failures
   from `rt_run`/`dr_exec`, and add a CLI test that a failed build exits
   nonzero and creates no file. Re-run `bootstrap.sh` and `repro-opt.sh` after.
2. **F-03:** implement byte-array constants for non-plain structs (or reject
   them with a coded diagnostic) and add packed/union/bitfield/`align` static
   cases to `statics.cx`/negative tests.
3. **F-04:** permit float compound assignment in `semantic.cx:2389`, add a
   float `+= -= *= /=` corpus case, then re-run `features.cx` and the `lib/corex`
   matrix; update SH-026/SH-034.
4. **F-05/F-06:** seed `target_os=linux`, `target_arch=x86_64`, and
   `sanitize=true` (when sanitizing); accept/pass `--cfg`/`-I` for
   `check`/`emit-ir`/stdin `build`/`exec`.
5. **F-07:** emit labels from unreachable regions or reject unreachable label
   targets; add an `emit-ir --verify` case for a dead nested label.
6. **F-08/F-09:** add `.code` files for the 22 uncoded negatives, reconcile
   `builtin_value` with E0218, make ASan mandatory, and assert 326/ASan-enabled
   in CI.
7. **Docs:** update `bootstrap-architecture.md` (subset, line counts, CLI),
   `memory-audit.md` (bounds/ASan), the perf table, and SH-034's 12/16 matrix.
   Keep SH-044 cleanup out of the acceptance path; it is non-functional.
8. **Re-run:** `selfhost/bootstrap.sh`, `selfhost/repro-opt.sh --corpus`,
   `(cd compiler && cargo test)`, `tests/selfhosting/{run,cli,dump,fuzz}.sh`,
   and the example/lib matrices. SH-046 still waits on SH-033, SH-037, SH-039,
   and SH-043 after these correctness fixes.

## 7. Follow-up verification (post-fix)

Fix branch: `room/corex-audit-fixes-6cmw` (`11ae908`, `6dce8c1`, `f942403`),
merged into `main` at `d6983fd`. Fixer commits were reviewed by the master
auditor; all behavior changes were re-run independently, not trusted from the
fixer report.

### Finding status

| Finding | Status | Independent evidence |
|---|---|---|
| F-01 build exit | FIXED | `build -o /nonexistent-dir/out` → rc 3, no `compiled`, no output |
| F-02 signal status | FIXED | fake `COREX_CC` (`kill -SEGV $$`) → rc 3, no `compiled`, no output |
| F-03 non-plain statics | FIXED (coded rejection) | packed static → `error[E0226]`; `emit-ir --verify` rc 1; plain `statics.cx` passes |
| F-04 float compound | FIXED | `features.cx` builds and matches the oracle; float `+= -= *= /= %=` corpus case |
| F-05 cfg defaults | FIXED | `target_os=linux` branch selected by default; `--sanitize` seeds `sanitize=true` |
| F-06 frontend options | FIXED | `check`/`emit-ir` accept `--cfg`/`-I`; stdin `build`/`run` honor them |
| F-07 dead labels | FIXED | block/if/while/switch labels after a non-label statement all pass `emit-ir --verify`; goto executes |
| F-08 negative codes | FIXED | 56 `.code` files; runner rejects uncoded negatives; 56/56 match |
| F-09 ASan gate | FIXED | `COREX_REQUIRE_ASAN=1` passes; CI asserts 327 + `ASan: enabled` |
| F-10 bootstrap usage | FIXED | `[--jobs N]` removed |
| F-11 architecture docs | FIXED | integrated subset, counts, CLI |
| F-12 memory docs | FIXED | bounds/ASan policy and residual risks |
| F-13 performance docs | FIXED | one integrated table (708 ms, 64 ms, 7.04 MB/s, RSS) |
| F-14 SH-034 matrix | FIXED | 13/16; `std_math` unblocked; SH-033 blocks sync/TLS/thread |
| F-15 SH-036 `-g` | FIXED | rejected by contract, exit 2 |

### Final merged gates

```text
check: ok: 474 function(s), entry `main`; 15,228 lines
stage1 f4192f40a4571bdfec5f4514bf035a1246486f80ad94fa54e2fedf79a17af884
stage2 c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78
stage3 c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78
stage4 c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78
reproducible: stage2 == stage3 == stage4
selfhosting tests: 249 passed, 0 failed (Stage 1/2/3)
cli tests: 80 passed, 0 failed
dump tests: 75 passed, 0 failed
fuzz tests: 327 passed, 0 failed; ASan: enabled
cargo test: 205 passed, 0 failed
examples: 8/8 match the oracle at --opt 0..3
lib/corex: 13/16 build
repro-opt: opt1 0f6bd235…, opt2 eda405ad…, opt3 58067778…
```

### Remaining blockers

Pre-blocked-work snapshot: SH-033/SH-034/SH-037/SH-044 were subsequently
completed; SH-039, SH-043, and SH-046 remain blocked per §8.

## 8. Blocked-work follow-up (2026-10-29)

> Superseded for SH-039 by §9; the rest of this section remains the 2026-10-29 snapshot.

All remaining blockers were assigned to dedicated workstreams and independently
re-verified.

### Completed

- **SH-033:** volatile globals/places, `volatile_load`/`volatile_store`, 12
  atomic builtins, inline `asm`, TLS/thread library runtime. `concurrency.sh`
  33/33 across `--opt 0..3` and the Rust oracle.
- **SH-034:** all 16 `lib/corex` modules build with both compilers
  (`lib.sh` 16/16).
- **SH-037:** `selfhost/lsp.cx` implements initialize/shutdown, document sync +
  publishDiagnostics, hover, definition, references, rename, completion, and
  formatting; `lsp.py` 35/35, gated by bootstrap/CI.
- **SH-044:** P3 cleanup re-applied on the integrated tree: 71 redundant nested
  `unsafe` wrappers, 20 dead helpers, `Program.decls`/`decls_tail`, orphaned
  parser wrappers. 534 functions / 17,236 lines; behavior unchanged.

### New defect found and fixed

The SH-039 harness exposed an exponential `check_binary` hang (32+ term
`1+1+…` expressions; `type_ok <- ck <- check_binary <- check_expr`). The left
subtree was type-checked twice: once in the pointer-arithmetic precheck and
again in the normal path. `check_binary` now checks each operand once and
`behavior.py` enforces a per-process timeout. A 64-term expression now checks
in 0.10 s and a regression is in the corpus.

### Still blocked

- **SH-039:** 126 passed / 9 failed / 70 blocked / 0 timeout over 205 rows.
  The 68 `--lib` tests are Rust-internal; the 9 divergences are static
  function-pointer/`str` tables, compound/designated literals, oracle IR text,
  builder temp-dir internals, `load_source` spans, and private-item prose.
  Evidence in `semantic-hang.md`. AC-4 remains blocked.
- **SH-043:** push attempted from integrated `main`; HTTPS has no askpass and
  SSH has no key, so remote CI has not run.
- **SH-046:** depends on SH-039 and SH-043.

### Final verified gates

```text
check: ok: 534 function(s), entry `main`; 17,236 lines
stage1 96f1ef30e68a682ba38fadbc31f15bb7befadcfb884d338a00176bf3f3f87b3d
stage2 f3e770a3c88e66a05494bea7e3a9dcaf405acec63c30e2bf9f4e0ad9c8ad3c71
stage3 f3e770a3c88e66a05494bea7e3a9dcaf405acec63c30e2bf9f4e0ad9c8ad3c71
stage4 f3e770a3c88e66a05494bea7e3a9dcaf405acec63c30e2bf9f4e0ad9c8ad3c71
selfhosting tests: 270 passed, 0 failed (Stage 1/2/3)
cli tests: 80 passed, 0 failed
lsp tests: 35 passed, 0 failed
dump tests: 81 passed, 0 failed
fuzz tests: 330 passed, 0 failed; ASan: enabled
concurrency tests: 33 passed, 0 failed
lib tests: 16 passed, 0 failed
cargo test: 205 passed, 0 failed
behavior: 126 passed, 9 failed, 70 blocked, 0 timeout (205 rows)
repro-opt: e3e6335e.../9af3b41a.../51c025a0... (full hashes in bootstrap-results)
```


## 9. SH-039 completion (2026-10-30)

The tooling workstream completed the remaining CLI-expressible behaviour
suite; no individual blocklist entries were added.

- `behavior.py` over all 205 rows: **137 passed, 0 failed, 68 blocked,
  0 timeout** (exit 0). The 68 blocked rows are exactly the Rust `--lib` unit
  tests that call Rust-internal modules and cannot be routed through a CLI.
- Fixed the 9 divergences: function-pointer/`str` static tables, compound and
  designated literals, IR text (`%pN`, `@__corex_syscall`, str construction),
  private module-item diagnostics, `COREX_KEEP_TEMP`, `build --test`, DWARF
  (`-g`), and the `load_source` external adapter.
- Regression: 64-term `1+1+…` checks in 0.10 s; per-test 60 s timeout remains.
- Gates: 270 corpus × Stage 1/2/3, CLI 80, LSP 35, dumps 81, fuzz/ASan 330,
  concurrency 33, `lib/corex` 16, `cargo test` 205.
- Final hashes: Stage 1 `57075bae…`; Stage 2/3/4 `bcf17bc8…`; optimized
  `0745f798…`, `2cd22d29…`, `37d52093…`.

Remaining: SH-043 (push blocked by missing GitHub credentials) and SH-046
(acceptance depends on SH-043). SH-033/034/037/039/044 are complete.

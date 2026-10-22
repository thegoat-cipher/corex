# Request Log

APPEND ONLY. Each entry records what was asked, what was done, and the outcome.
Never rewrite or delete an entry; add a new one to correct the record.

---

## R-0001 — 2026-10-21 — CoreX master directive

**Request.** Build CoreX: a low-level, statically typed, AOT-compiled systems
language with a real, tested compiler (not documents only). Required process:
repository inspection, research, PRD, architecture, language spec, formal
grammar, memory model, canonical examples, compiler frontend and backend,
tests, and synchronized documentation. Non-negotiable language constraints: no
GC, no hidden allocation, no mandatory runtime, explicit memory management,
definable arithmetic, bare-metal orientation, Linux x86-64 target, LLVM backend,
Rust implementation. Required deliverables: `docs/research.md`,
`docs/competitor-analysis.md`, `docs/PRD.md`, `docs/NFR.md`,
`docs/architecture.md`, `docs/tasks.md`, `docs/ADRs.md`, `docs/request-log.md`,
`docs/language-spec.md`, `docs/grammar.ebnf`, `docs/memory-model.md`, `README.md`,
`ENGINEERING.md`, `master.md`, `examples/hello.cx`, `examples/factorial.cx`, and the
compiler under `compiler/src/`.

**Constraint on execution.** One dedicated workstream per major task (ten named
workstreams), run in parallel where practical and sequentially otherwise, with the
limitation reported. Never fabricate research, tests, or tool output.

**Outcome.** Accepted. Repository was empty (no git, no files) and the
environment had no Rust or LLVM; toolchains were installed user-locally, then
the pipeline was built. See R-0002 for the workstream decision and R-0007 for
verification.

---

## R-0002 — 2026-10-21 — Workstream decomposition

**Request.** The initial directive asked for a dedicated workstream per major
task (ten named workstreams), with parallel execution where practical and an
explicit report where it is not.

**Action.** Research and Product Requirements ran as two independent workstreams
on separate checkouts; their files were reviewed and merged into the main tree.
The remaining workstreams (architecture, language specification, memory model,
lexer, parser, semantic analysis, backend, driver, testing, documentation) ran
sequentially in one checkout. Reason: those stages share the AST and analysis
types in a single crate, so parallel edits would overlap in the same files. The
limitation is stated in `ENGINEERING.md` and `master.md`.

**Outcome.** All ten workstreams produced their deliverables.
`docs/research.md`, `docs/competitor-analysis.md`, `docs/PRD.md`, and
`docs/NFR.md` were reviewed and merged; research citations were spot-checked
(HTTP 200).

---

## R-0003 — 2026-10-21 — Environment and toolchain decisions

**Request.** Implement the compiler without asking the user to install
dependencies; report what was actually available.

**Findings.** No `rustc`, `cargo`, `clang`, `llvm-config`, or `llc`; no sudo
rights; gcc 15.2, binutils, and 6.5 GB free disk.

**Action.**

1. Installed Rust 1.99.1 with `rustup` into `$HOME/.cargo` / `$HOME/.rustup`
   (no root).
2. Downloaded Ubuntu's LLVM 20.1.8 packages (`llvm-20`, `clang-20`,
   `libclang-cpp20`, `lld-20`) and extracted them with `dpkg -x` into
   `/home/vib/opt/llvm20`, giving `llvm-as`, `llc`, `opt`, `llvm-config`,
   `clang`, and `ld.lld` without root.
3. Verified the freestanding path by hand: a raw `syscall` assembly program
   assembled with `as`, linked with `gcc -nostdlib -static -no-pie`, and exited
   with status 42.

**Outcome.** Toolchain available via `COREX_LLVM_BIN` (documented in README).
No changes were made outside the home directory and the project.

---

## R-0004 — 2026-10-21 — Research deliverable and corrections

**Outcome.** `docs/research.md` (868 lines, 46 cited URLs, evidence labels
DOCUMENTED/COMMUNITY/HYPOTHESIS, confidence per section) and
`docs/competitor-analysis.md` (140 lines). The research workstream reported:

- Correction to the initial brief: Hare uses **qbe**, not LLVM; Odin is the only
  documented LLVM backend in the surveyed set.
- No official cross-language benchmark could be sourced; only Hare quantifies a
  backend gap (qbe at "25% to 75%" of LLVM performance).
- Jai, ISO C/C++ standard text, and Chromium's memory-safety page date could not
  be sourced and are marked as such.

**Disposition.** Accepted. The qbe correction is recorded in
`docs/competitor-analysis.md`; no research claim was promoted to a CoreX design
requirement without independent verification.

---

## R-0005 — 2026-10-21 — PRD/NFR deliverable and open-question dispositions

**Outcome.** `docs/PRD.md` (23 FRs, MVP scope, 14 non-goals, 7 risks, 12-point
definition of done) and `docs/NFR.md` (55 NFRs, each labelled Enforced,
Unvalidated, or Aspirational). The workstream raised 12 open questions
(OQ-1 … OQ-12). Dispositions:

| OQ | Question | Decision | Recorded in |
|---|---|---|---|
| OQ-1 | Bounds behavior: trap or undefined? | Dynamic indices trap (exit 101); constant indices are compile errors | ADR-0008, spec §7.4 |
| OQ-2 | `MIN / -1` behavior | Defined abort, exit 101, with division by zero | ADR-0004, spec §7.3 |
| OQ-3 | Entry convention and exit status | `_start()` or `main() -> i32`; `_start` wins; exit status is `main`'s low 8 bits; `_start` fallthrough exits 0 | ADR-0005, spec §4.3 |
| OQ-4 | LLVM integration model | External `llvm-as`, `opt`, `llc` processes; no FFI | ADR-0002, ADR-0012 |
| OQ-5 | `free`/null and zero-length arrays | No null literal; `0 as *mut u8` requires `unsafe` and `munmap(0, n)` fails silently; `[T; 0]` is rejected (`E0110`) | spec §8.1, grammar |
| OQ-6 | `break` values, loop expressions, shadowing, syscall return type | Statements only; no loop values; shadowing allowed in nested blocks; `syscall` returns `isize` | spec §5, §6, §8.1 |
| OQ-7 | Adoption of the 12-point definition of done | Adopted as the MVP gate; each item mapped to a completed task | PRD, `docs/tasks.md` |

The remaining OQ items were documentation/roadmap items (benchmarks absent,
debug info, modules, FFI, cross-compilation) and are recorded in the ADR open
questions section and the task backlog.

**Note.** The PRD and NFR were authored before the compiler existed. They were
not rewritten; measured facts were added separately in `docs/benchmarks.md` and
an addendum in `docs/NFR.md`.

---

## R-0006 — 2026-10-21 — Compiler implementation and defects found by verification

**Implemented.** Lexer, parser with recovery, semantic analysis, LLVM 20 textual
IR backend, driver, CLI, examples, and 79 tests. Defects caught by the
verification stages (not by inspection) and fixed:

1. **Invalid constant expression.** `ptrtoint (ptr %local to i64)` inside a call
   is not valid LLVM (constant expressions cannot reference function-local
   values). Found by `llvm-as` during the factorial build; fixed by materializing
   the conversion as an instruction in `__corex_fatal` and `__corex_free`; a
   regression test now forbids the pattern.
2. **Helper emitted after globals were finalized.** The `alloc` helper
   registered a message global after the module had already been assembled, so
   `@.str.N` was undefined. Found by `tests/native.rs` (heap round-trip); fixed by
   materializing entry point and helpers before module assembly.
3. **Missing fatal helper dependency.** `alloc` aborts through `__corex_fatal`,
   but the helper was emitted only when a fatal path was already present. Fixed
   by deriving `need_fatal` from `used_alloc` as well.
4. **Array literal parsing after adding repeat syntax.** Elements after the first
   were parsed at a comma. Fixed by consuming commas explicitly.
5. **Optimized builds segfaulted.** Root cause: the kernel enters `_start` with
   `%rsp` 16-byte aligned, while LLVM assumes the SysV callee state; enabling the
   `opt` pipeline exposed it as misaligned SSE accesses. Fixed with
   `alignstack=16` on `_start` and covered by
   `optimized_entry_point_preserves_stack_alignment`.

**Outcome.** 79 tests pass (38 unit, 41 integration). `corex run` produces
`Hello, World!` and `3628800` from the canonical examples.

---

## R-0007 — 2026-10-21 — Performance measurement decision

**Request.** "Performance claims must be validated using benchmarks."

**Finding.** The first pipeline (`llvm-as` → `llc`) left the IR unoptimized: a
simple loop ran at ~2.5× the equivalent C time. Adding
`opt -passes=default<O2>` before `llc` eliminated that gap (ADR-0012). Then the
first benchmark (`sum(i*i)` over a compile-time constant bound) was
closed-formed by LLVM and measured 3 ms against C's 232 ms — the loop no longer
existed, so the number was meaningless. The benchmark was replaced with two
kernels that cannot be folded: a data-dependent integer recurrence with a
runtime iteration count, and recursive `fib(35)`.

**Outcome.** Measured on one machine (Ryzen 7 6800H, 5 runs, median):
integer loop parity (CoreX/C = 0.997); recursive call workload 1.467× slower than
GCC. Recorded with methodology caveats in `docs/benchmarks.md`. No claim of
general performance superiority is made.

---

## R-0008 — 2026-10-21 — Documentation synchronization

**Action.** Wrote the language specification, normative grammar, memory model,
architecture, ADRs (0001–0013), task tracker, this log, `ENGINEERING.md`, `master.md`,
and README. Updated the architecture pipeline description for the `opt` stage and
entry-point alignment, and appended a measured-results addendum to NFR.md without
altering the original aspirational labels.

**Outcome.** Documentation describes only implemented behavior; every diagnostic
code in the spec exists in the compiler; every completed task cites a command or
test as evidence.

---

## R-0009 — 2026-10-21 — Hardening round after NFR review

**Trigger.** NFR-L2 requires deep nesting to produce a diagnostic rather than a
crash. Testing a generated file with 10,000 nested parentheses reproduced a
compiler stack overflow (`fatal runtime error: stack overflow`, exit 134),
because the recursive-descent parser had no depth bound.

**Action.**

1. Added `MAX_NESTING = 256` to the parser, enforced at `parse_expr`,
   `parse_unary`, `parse_type`, and `parse_block`. Violations report `E0111`.
   The limit also bounds downstream AST recursion in semantic analysis and codegen.
2. Cropped diagnostic source lines to 160 characters around the caret, so the new
   error is readable instead of printing a 20,000-character line. Added a
   renderer test for the cropping behavior.
3. Documented `E0111` and the nesting limit in `docs/language-spec.md` §5 and
   §11, and recorded measured results in a new NFR addendum.

**Outcome.** `corex check` on the 10,000-parens file now prints
`error[E0111]: nesting depth exceeds the limit of 256` and exits 1. Test totals
increased from 79 to 82 (41 unit, 41 integration), all passing. Test counts in
README, `master.md`, `ENGINEERING.md`, `docs/architecture.md`, and `docs/tasks.md`
were updated; earlier log entries that state the 79-test total are left
unchanged by design.

---

## R-0010 — 2026-10-21 — Direct terminal execution (stdin, exec, REPL)

**Request.** "Write something that can directly run the code into the terminal,"
and a request for a summary of the syntax plus a Hello World example.

**Action.**

1. All commands accept `-` as a source path and read stdin.
2. Added `corex exec '<source>'`, which compiles and runs source passed on the
   command line and forwards the program's exit status.
3. Added `corex repl` (ADR-0014): submissions are parsed, function definitions
   accumulate by name with redefinition replacing the old one, a blank line
   compiles and runs the combined program, statement-only input is wrapped in a
   temporary `main`, and `:` commands cover run/ir/show/reset/quit/help.
4. Added `compiler/tests/cli.rs` (6 tests) driving the real binary: stdin source,
   inline `exec`, exit-status forwarding, REPL accumulation, redefinition
   precedence, and statement wrapping.
5. Added `scripts/try.sh` and `examples/playground.cx` (R-0009 follow-up) so a
   single file can be checked, run, or dumped as IR with one command.

**Outcome.** `corex exec 'fn main() -> i32 { return 42; }'` exits 42;
`printf ... | corex run -` works; the REPL runs a submitted block and prints
`[exit N]`. Test totals increased from 82 to 88 (41 unit, 47 integration), all
passing. Docs updated: README CLI/layout, architecture module table and command
table, task tracker (T-023), ADR-0014.

---

## R-0011 — 2026-10-21 — User-reported E0206 and a broken REPL paste

**Report.** The user pasted a REPL transcript: typing the `hello.cx` program into
the REPL produced `E0100: expected '}'`, then a follow-on `E0201: undefined name
'message'` and `E0206: function main can reach the end ... without returning a
'i32' value`.

**Root causes.**

1. **REPL submitted on any blank line.** `examples/hello.cx` contains a blank
   line inside the function body, so the buffer was submitted mid-function.
   Everything after it was then wrapped into a temporary `main`, producing the
   confusing follow-on errors.
2. **Divergence analysis was too narrow.** A `main` whose last statement was
   `syscall(60, ...)` (and `while true { }`) was rejected with `E0206` even though
   it never returns. The diagnostic also read "a `i32` value".
3. The verification run for the first fix used a stale binary: only the library
   had been rebuilt, so the old message and old behavior were observed. Rebuilt
   before re-testing; the reported behavior was real and is now fixed.

**Fixes.**

- `repl.rs::needs_more_input` — submit on a blank line only when brackets are
  balanced, no string/block comment is open, and the buffer does not end in a
  dangling token. Blank lines inside a block are preserved. `:submit` / `:go`
  forces submission (ADR-0016).
- `semantic.rs` — `while true { ... }` without `break`, and a tail
  `syscall(60/231, ...)`, now count as divergence; `E0206` reworded and its note
  lists every accepted form (ADR-0015).
- Regression tests: 4 semantic unit tests and 2 CLI tests
  (`repl_accepts_a_pasted_program_with_internal_blank_lines`,
  `exec_accepts_a_program_terminating_with_the_exit_syscall`).

**Outcome.** Pasting `examples/hello.cx` into the REPL prints `Hello, World!` and
`[exit 0]`. `fn main() -> i32 { ... syscall(60, 3, ...); }` exits 3 without a
dummy `return`. Test totals: 88 → 94 (45 unit, 49 integration), all passing.

---

## R-0012 — 2026-10-21 — Python-like terminal experience

**Request.** "Anything else to make it so that I can write code inside a terminal
based app like python's terminal app?"

**Gap analysis.** The REPL could compile and run blocks, but (a) displayed
nothing for an expression, so a value could only be seen via hand-written
syscalls, and (b) had no line editing or history, so typing was painful.

**Action.**

1. **`print` intrinsic** (ADR-0017). `print(value)` accepts any integer type or
   `bool`, is safe (no `unsafe`), allocates nothing on the heap (20-byte stack
   buffer), and writes the value plus a newline to fd 1. Lowered to
   `__corex_print_i64` / `__corex_print_u64` / `__corex_print_bool`.
2. **Expression evaluation in the REPL** (ADR-0018). A chunk without a trailing
   `;` is tried as `print(<chunk>)`, then as statements; the first candidate that
   analyzes cleanly wins, checked together with the session's definitions, so
   `fact(10)` prints `3628800`. A trailing semicolon suppresses the value.
3. **Enter-to-run.** A complete one-liner runs on Enter; a multi-line block runs
   when complete and defining `main`/`_start`; otherwise a blank line or
   `:submit` runs it. Blank lines between pasted functions no longer submit
   early, and comment-only lines never run an empty program.
4. **Line editing and history** (`lineedit.rs`). Raw mode through the external
   `stty` utility (no crates, no FFI): arrows, Home/End, Delete, Backspace,
   Ctrl-A/E/B/F/U, Ctrl-C to abandon the block, Ctrl-D to quit, Up/Down history,
   persisted in `~/.corex_history`. Terminal state is restored by `Drop`.
   Non-TTY stdin keeps the plain read path, so pipes and tests are unaffected.
5. **Honest limitation.** Bindings do not persist between submissions (each run
   is a new process). The REPL now says this when an `E0201` appears and suggests
   wrapping related statements in `{ ... }`.

**Defects found while implementing.**

- Integer constants were not masked to the target width, so `-7` for `i32`
  emitted `i32 18446744069414584313`. `llvm-as` silently truncates out-of-range
  constants instead of rejecting them, so this produced wrong values. Fixed in
  `int_const` (mask before signed conversion).
- `__corex_print_i64` computed the magnitude but never wrote the `-` sign.
  Found by the print matrix test; fixed, now covered by
  `print_formats_integers_and_bools`.
- Candidates were first analyzed in isolation, so `fact(10)` could not resolve
  `fact`; fixed by analyzing against the accumulated session definitions.

**Outcome.** `1 + 2` prints `3` on Enter; `print(-7)` prints `-7`;
`fact(10)` prints `3628800`; a pasted `hello.cx` prints `Hello, World!` and exits
0. Test totals: 94 → 100 (47 unit, 53 integration), all passing.

---

## R-0013 — 2026-10-21 — Optimization-level correctness prioritized

**Request.** Before expanding the language, guarantee that defined semantics
survive LLVM `-O0` through `-O3`. The report's example: `INT_MAX + 1` compared
against `INT_MIN` in an `if`, which must print 42 at every level. The emitter
must not attach `nsw`. Signed division overflow, masked shifts, and dynamic
bounds checks need the same treatment. Preference stated: 1,000 semantic tests
across optimization levels over ten new features with uncertain behavior.

**Finding.** The reported program already printed `42` at `--opt 0..3`, and the
emitted IR contained zero `nsw`/`nuw` flags. That was true but unverified: nothing
in the suite would have noticed a regression.

**Action.**

1. `tests/opt_levels.rs::emitted_ir_never_claims_non_wrapping_arithmetic` —
   asserts the emitter attaches no `nsw`/`nuw` anywhere, over all examples and
   all semantic cases.
2. `semantics_are_identical_at_every_optimization_level` — 14 fixed programs ×
   4 levels: wrapping add/mul/negate at `i8`/`u32`/`i32`/`i64` boundaries,
   masked shifts (left and arithmetic right, counts above the width and negative),
   division/remainder by zero, `MIN / -1`, dynamic out-of-bounds abort,
   in-bounds dynamic index, pointer aliasing, and `print` boundary formatting.
3. `examples_produce_the_same_output_at_every_optimization_level` — the four
   examples × 4 levels.
4. `generated_arithmetic_matches_the_spec_at_every_optimization_level` — one
   generated program per integer type with 60 chained operations and comparisons;
   expected values computed in Rust from the spec; 1,104 value assertions total.
5. `wrapping_add_regression_from_report` — asserts the reported program emits a
   plain `add i32` and prints `42` at all four levels.

**Harness verification (mutation testing).**

- Masking shift counts by the width instead of `width-1` made the generated test
  fail immediately with five differing value lines.
- Emitting `add nsw` made the IR test fail with
  "`signed_add_wrap`: emitted IR contains ` nsw`".
- Both mutations were reverted; the restored compiler passes all 106 tests.

**Outcome.** 106 tests, 47 unit / 59 integration, ~4.4 s wall clock. This is not
the 1,000 hand-written programs requested; it is 1,104 generated semantic
assertions executed at four optimization levels, plus 72 fixed program runs, and
the harness is proven able to fail.

---

## R-0014 — 2026-10-21 — Full fact-check pass

**Request.** "Fact check everything and make sure no errors are found," using an
independent review.

**Action.** An independent review was carried out in a separate checkout to check
documentation against code and fresh command output. In parallel, the maintainer
re-derived every objective claim: line counts, test
counts per binary, diagnostic-code coverage, CLI behavior and exit codes, ELF
properties, twin-build determinism, benchmark ratios, and research URLs.

**Corrections (code, so documented behavior became true):**

1. Parameters were mutable; the spec says immutable. They are now immutable
   (`E0205` on assignment, `E0220` on `&mut`).
2. `_start` bodies ending in `exit` still received a synthesized exit; ADR-0015
   claimed otherwise. Codegen now omits it (IR test asserts exactly one exit).
3. Array types whose size overflows 64-bit multiplication (`[u64; 2^61]`) wrapped
   past the 1 MiB check; now rejected with `E0223`.
4. `else` accepted any statement; `grammar.ebnf` allows only an `if` chain or a
   block. Parser restricted to match.
5. The lexer accepted `0x_FF`, which the grammar forbids; now `E0004`.
6. The REPL's `:ir` claimed to verify IR but only printed it; it now runs
   `llvm-as` and says so.

**Corrections (documentation):** module line counts and test totals (now 116),
repository layout (5 examples, `lineedit.rs`, `robustness.rs`), master line
totals, PRD/NFR assumptions superseded by implementation (`CXnnnn` → `Exxxx`,
array parameters, optional return type, entry point choice, six-command CLI,
default `--opt 2`, exit codes, dynamic bounds checks, `usize` syscall arguments,
no `bool`/array comparisons), spec details (literal vs computed indices, cast
literal context, comments may be non-ASCII, `E0223` scope), memory-model wording
(`print` issues two writes), benchmark variance, and the stale
`should_blank_submit` reference (ADR-0020).

**Independent verification highlights:** 116 tests pass (51 unit, 65
integration); emitted IR contains no `nsw`/`nuw`; twin builds produce byte-identical
IR and ELF; the artifact is a static ELF with no dynamic section, no
`init_array`, no TLS; `alloc(0)` aborts 101; `cargo build --release --offline`
succeeds; all 34 diagnostic codes are documented and used; no broken relative
links; six sampled research URLs resolve (two required `-L`).

**Outcome.** All local findings fixed. The independent auditor's report is still
pending; its findings will be triaged in a follow-up commit rather than assumed
correct.

---

## R-0015 — 2026-10-21 — Independent audit triage

**Input.** An independent review delivered `AUDIT.md` (70 findings: 1 blocker,
36 wrong, 14 stale, 17 minor, 2 unverifiable; plus 9 verified claim groups).
The report was written against commit `1bca1c8`; a parallel fact-check pass
(`13b022c`…`126dd4e`) had already fixed part of it. The full report is preserved
verbatim as `docs/audit-2026-10-21.md`.

**Triage.** Every finding was re-checked against current HEAD.

- **Fixed before the report arrived (parallel pass):** F-05, F-10, F-12, F-17,
  F-24, F-27, F-38, F-41, F-42, F-43, F-45, F-46, F-47 (CLI/exit parts), F-48,
  F-49, F-52, F-64.
- **Fixed in this triage:**
  - F-19 (BLOCKER): builtin names (`syscall`, `alloc`, `free`, `print`) are now
    rejected as bindings and parameters with `E0202`; spec and grammar already
    said so. Tests added.
  - F-23: divergence recognizes constant exit numbers through `as` casts
    (`syscall(60 as usize, …)`), via `const_int`; covered in semantic and IR tests.
  - F-01/F-02/F-03/F-04/F-06/F-07/F-08/F-09: counts, abridged `file` output, and
    two invalid README snippets (`u8::MAX`, `1u8`) corrected; `cargo test --test
    cli` added to the documented commands.
  - F-13/F-14/F-15/F-16/F-18: `<<=` lexing description, `Callee::Print`, the
    false claim that `tests/ir.rs` executes binaries, the `--opt`/`llc`
    description, and the limitation-list ordering.
  - F-20/F-21/F-22/F-25/F-26: nesting-limit description now states the real
    construct-dependent limits; entry-point wording (“at least one … `_start`
    wins”); suffix-free examples; grammar reserved list includes `print`; `else`
    already restricted to `if`/block in the parser.
  - F-28…F-37: task-tracker counts (62 NFRs, 13 non-goals, semantic 26, native
    18, CLI 14, 9 REPL tests, ADR range, 27 completed tasks).
  - F-39: ADR-0021 records that ADR-0014 decisions 3–4 are superseded.
  - F-44/F-50/F-51: PRD FR-12 now specifies abort-101 instead of a hardware
    trap; OQ-1/2/6/8 are marked resolved with a pointer to PRD §12.
  - F-53…F-69: NFR rows reconciled with evidence (diagnostic span test added;
    600-input generated malformed corpus; multi-line render format; no global
    diagnostic cap; toolchain not pinned by file; no runtime LLVM version check;
    no target module; no CI; determinism test exists; 34 codes; gcc not clang).
- **Accepted as unverifiable:** F-40 (a read-only review cannot run mutations;
  the maintainer did, and both mutations fail the suite) and F-71 (research prose;
  33 URLs sampled, all HTTP 200, text not independently re-read).

**Result.** 122 tests pass (53 unit, 69 integration). No finding was dismissed
without a stated reason. The trap in this triage was the opposite direction:
trusting the report blindly would have “fixed” items already correct at HEAD
(for example F-38 and F-41…F-48), so each was re-run.

---

## R-0016 — 2026-10-21 — License the project for open-source use

**Request.** "Make it full open source, anyone can fork it and such."

**Finding.** The repository was public but had no license file, so default
copyright law applied: all rights reserved, and forking or reuse was not
permitted. A public repository without a license is not open source.

**Action.**

1. Added `LICENSE-MIT` (MIT, copyright "CoreX contributors") and
   `LICENSE-APACHE` (canonical Apache-2.0 text).
2. Set `license = "MIT OR Apache-2.0"` and `repository` in
   `compiler/Cargo.toml`.
3. Replaced the README's "no license" line with a License section covering
   fork/redistribute/sell rights, an inbound=outbound contribution statement
   (no CLA), and the fact that compiled programs are not derivative works.
4. Added `THIRD_PARTY.md`: no vendored code; Rust, LLVM, `cc`, and `stty` are
   build tools that are not redistributed; research docs cite and paraphrase
   third-party sources; benchmark C files are original.
5. Recorded the choice in ADR-0022 and task T-028.

**Outcome.** The repo is OSI open source under a permissive dual license; forks
and commercial use are permitted. GitHub license detection was checked after
pushing.

---

## R-0017 — C-class roadmap implementation (2026-10-22)

**Request:** Implement the complete CoreX C-class roadmap: function pointers;
C control flow; unions/bitfields/flexible arrays/designated and compound
literals; layout introspection and C layout; C ABI/FFI/`cstr`/varargs/main
args; file modules, interfaces, separate compilation and `corex.toml`;
allocator API and a minimal standard library; tooling (DWARF, formatter, linter,
LSP, docs generator, test runner, sanitizers); P1 (volatile/atomics/threads/TLS,
inline asm, macros, cfg, shared libraries/`dlopen`, varargs conformance,
benchmarks). Update all documentation and keep the suite green.

**Outcome:** Implemented end to end on branch
`room/corex-c-class-roadmap-deepseek-hjce`. 174 tests pass (61 unit, 113
integration); examples run at `--opt 0..3`; C interop, separate compilation,
static archives, shared libraries + `dlopen`, DWARF inspection, LSP sessions, and
sanitizer aborts are covered. Measured benchmark ratios are recorded in
`docs/benchmarks.md` (`mix` 1.017, `array` 1.056, `struct` 0.964, `f64` 0.947,
`fib` 1.636). Known gaps are listed in `docs/tasks.md` (no thread join, explicit
TLS table, expression-only macros, volatile globals + pointer builtins, `fib`
codegen gap).

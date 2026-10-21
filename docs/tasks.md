# CoreX Task Tracker

Status values: NOT STARTED, IN PROGRESS, BLOCKED, COMPLETED. Validated status
changed only after the acceptance criteria passed. Evidence names the command or
test that proves completion; counts are from the run on 2026-10-21.

Test totals: `cargo test` → **133 passed, 0 failed** (53 unit, 80 integration).

| ID | Description | Workstream | Priority | Depends on | Status | Acceptance criteria | Validation evidence |
|---|---|---|---|---|---|---|---|
| T-001 | Initialize repository skeleton, `.gitignore`, initial commit | Maintainer | P0 | — | COMPLETED | Repo has a git branch and commit; build artifacts ignored | `git log --oneline -1`; `.gitignore` excludes `compiler/target/`, `build/`, `*.o`, `*.ll` |
| T-002 | Install user-local Rust toolchain | Maintainer (Environment) | P0 | — | COMPLETED | `rustc` and `cargo` resolve without root | `rustc 1.99.1`; `cargo 1.99.1`; installed under `$HOME/.cargo` / `$HOME/.rustup` |
| T-003 | Install user-local LLVM 20 toolchain | Maintainer (Environment) | P0 | — | COMPLETED | `llvm-as`, `llc`, `opt`, `llvm-config` run; freestanding static ELF links and executes | `llvm-as --version` → 20.1.8; `llc --version` → 20.1.8; hand-written asm binary exits 42 |
| T-004 | Research existing systems languages and compiler patterns | Research (parallel workstream) | P0 | T-001 | COMPLETED | Evidence-labelled findings with source URLs and dates | `docs/research.md` (868 lines, 46 cited URLs; spot-checked HTTP 200); `docs/competitor-analysis.md` (140 lines) |
| T-005 | Product requirements and non-functional requirements | Product Requirements (parallel workstream) | P0 | T-004 | COMPLETED | Vision, users, use cases, numbered FRs, MVP, non-goals, risks, DoD, measurable NFRs | `docs/PRD.md` (23 FRs, 13 non-goals, 12-point DoD); `docs/NFR.md` (62 NFRs, each Enforced/Unvalidated/Aspirational) |
| T-006 | Language specification and normative grammar | Language Specification (isolated workstream) | P0 | T-005 | COMPLETED | Every implemented construct documented; grammar matches parser | `docs/language-spec.md`; `docs/grammar.ebnf`; parser tests parse the documented forms |
| T-007 | Memory model manifesto | Memory Management (isolated workstream) | P0 | T-006 | COMPLETED | Syntax/semantics for stack, heap, pointers, unsafe; safety invariants; honest guarantee matrix; no false safety claims | `docs/memory-model.md` §3–§7; enforced by `semantic.rs` (`E0209`, `E0205`, `E0220`, `E0222`) and tested |
| T-008 | Lexer with spans, comments, literals, error codes | Lexer/Parser (isolated workstream) | P0 | T-006 | COMPLETED | Toolchain-free tokenization; keywords, numbers in 4 bases, escapes, comments; structured errors | `cargo test --lib lexer::` → 8 passed |
| T-009 | Parser producing typed AST with recovery | Lexer/Parser (isolated workstream) | P0 | T-008 | COMPLETED | Documented grammar parsed; precedence; statement/function recovery; spans on errors | `cargo test --lib parser::` → 13 passed; `tests/diagnostics.rs` syntax cases |
| T-010 | Semantic analysis: scopes, types, mutability, unsafe, return flow | Semantic Analysis (isolated workstream) | P0 | T-009 | COMPLETED | Every rule in spec §4–§8 enforced; analysis tables produced for codegen | `cargo test --lib semantic::` → 26 passed; `tests/diagnostics.rs` → 24 passed |
| T-011 | LLVM 20 textual IR backend | LLVM Backend (isolated workstream) | P0 | T-010 | COMPLETED | Valid IR for arithmetic, control flow, calls, pointers, arrays, syscalls, entry synthesis | `tests/ir.rs` → 5 passed (every example assembles under `llvm-as`) |
| T-012 | Driver: verify → optimize → object → link → run; CLI | Maintainer (Backend integration) | P0 | T-011 | COMPLETED | `build`, `run`, `emit-ir`, `check` work; verifier in the loop; documented exit codes | `tests/native.rs` → 18 passed; `corex --version`; manual `emit-ir --verify` prints `IR verified by llvm-as` |
| T-013 | Hello World via direct `write`/`exit` syscalls | Examples (isolated workstream) | P0 | T-012 | COMPLETED | No libc, no printf, no heap; prints expected text; exits 0 | `corex run examples/hello.cx` → `Hello, World!`, exit 0; native test asserts exact stdout |
| T-014 | Recursive factorial with defined overflow | Examples (isolated workstream) | P0 | T-012 | COMPLETED | Explicit types, recursion, base case, deterministic wrap; prints expected value | `corex run examples/factorial.cx` → `3628800`, exit 0; wrap of `fact(21)` asserted in native tests |
| T-015 | Pointer/heap example | Examples (isolated workstream) | P1 | T-012 | COMPLETED | `alloc`, write/read through `*mut u64`, casts, `free`, prints value | `tests/native.rs::pointers_example_...` → `123456789` |
| T-016 | Invalid-program rejection suite | Testing and Quality (isolated workstream) | P0 | T-010 | COMPLETED | Every documented diagnostic code has a failing program | `tests/diagnostics.rs` → 22 passed |
| T-017 | IR verification and regression tests | Testing and Quality (isolated workstream) | P0 | T-011 | COMPLETED | All examples verifiable; no libc symbols; no `undef`; regression for invalid constant expressions | `tests/ir.rs` → 5 passed |
| T-018 | Executable behavior tests | Testing and Quality (isolated workstream) | P0 | T-012 | COMPLETED | Compile → link → execute; assert stdout and exit status; abort paths | `tests/native.rs` → 18 passed |
| T-019 | Process documentation (ADRs, request log, engineering/master, architecture) | Documentation (isolated workstream) | P0 | T-005, T-011 | COMPLETED | Append-only decision history; synchronized architecture and status docs | `docs/ADRs.md` (ADR-0001…0021); `docs/request-log.md`; `docs/architecture.md`; `ENGINEERING.md`; `master.md`; `README.md` |
| T-020 | Performance measurement harness and first results | Testing and Quality (isolated workstream) | P1 | T-012 | COMPLETED | Reproducible benchmark; equal results verified; numbers published with caveats | `scripts/bench.sh`; `docs/benchmarks.md` (mix ratio 0.997, fib ratio 1.467, 5 runs) |
| T-021 | Enforce `opt` stage and entry-point stack realignment | Maintainer (Backend integration) | P0 | T-012 | COMPLETED | Optimized builds are correct and faster; entry point obeys the SysV stack invariant | `tests/native.rs::optimized_entry_point_preserves_stack_alignment` (opt 3); all native tests run at `--opt 2`; ADR-0012 |
| T-022 | Synchronize docs with implemented behavior | Documentation (isolated workstream) | P0 | T-013…T-021 | COMPLETED | Spec/grammar/memory/architecture/tracker match the code; measured numbers replace claims | This tracker; `docs/language-spec.md` §11 codes match `semantic.rs`; NFR addendum points to measured results |
| T-023 | Terminal run paths: stdin source, inline `exec`, interactive REPL | Maintainer (CLI) | P1 | T-012 | COMPLETED | Source can be run without creating a file; REPL accumulates definitions, redefinition replaces, blank line runs; exit codes propagate | `tests/cli.rs` → 14 passed; manual `exec`/`run -`/`repl` runs recorded in `docs/request-log.md` R-0010 |
| T-024 | Return-analysis fixes: exit syscall and `while true` count as divergence; REPL blank-line submission | Maintainer (Semantic + CLI) | P0 | T-023 | COMPLETED | Exit-style `main` compiles; `while true` without break accepted; REPL accepts pasted programs with internal blank lines; wording fixed | 4 semantic unit tests (`accepts_exit_syscall_as_a_diverging_tail`, `accepts_while_true_without_break`, `rejects_while_true_with_break`, `rejects_non_exit_syscall_fallthrough`); 2 CLI tests (`repl_accepts_a_pasted_program_with_internal_blank_lines`, `exec_accepts_a_program_terminating_with_the_exit_syscall`); ADR-0015, ADR-0016 |
| T-025 | `print` intrinsic and Python-like REPL (printed expressions, Enter-to-run, line editing, history) | Maintainer (Language + CLI) | P0 | T-023, T-024 | COMPLETED | Values printable from programs and the REPL; one-liners run on Enter; arrows/history edit input on a TTY; no external crates | 2 semantic tests, 1 native test (`print_formats_integers_and_bools`), 9 CLI REPL tests; manual pty run via `script`; ADR-0017, ADR-0018 |
| T-026 | Optimization-level semantic regression suite (`--opt 0..3`) | Testing and Quality (isolated workstream) | P0 | T-017, T-018 | COMPLETED | Wrapping, shifts, division/bounds aborts, aliasing and print behave identically at O0–O3; emitter emits no `nsw`/`nuw`; harness proven by mutation | `tests/opt_levels.rs` → 5 passed; 14 fixed cases × 4 levels, 4 examples × 4, 4 generated programs × 69 values × 4 = 1,104 assertions; shift-mask and `nsw` mutations both fail; ADR-0019 |
| T-027 | Fact-check pass: reconcile docs with behavior, fix spec gaps | Documentation (isolated workstream) | P0 | T-019…T-026 | COMPLETED | Every quoted count, line number, diagnostic code and behavior claim verified against fresh output; contradictions fixed in code or docs | Local pass plus independent audit triage: 122 tests pass; all 71 audit findings dispositioned (fixed, already fixed, or documented unverifiable); module table matches `wc -l`; ADR-0021; R-0015; `docs/audit-2026-10-21.md` |
| T-028 | Open-source licensing: MIT OR Apache-2.0, third-party notices, contribution terms | Maintainer (Legal/docs) | P0 | T-019 | COMPLETED | Permissive, OSI-approved license; forks and commercial use allowed; build tools not redistributed; compiled output unencumbered | `LICENSE-MIT`, `LICENSE-APACHE`, `compiler/Cargo.toml` license field, README License section, `THIRD_PARTY.md`, ADR-0022, R-0016; GitHub license detection verified |
| T-041 | Compound assignment, type inference, characters | Maintainer (Language) | P0 | T-028 | COMPLETED | `+=` family through mutable places; `let` inference; `char` literals/escapes, comparisons, casts, printing | `tests/features.rs::compound_assignment_inference_and_characters`; `tests/diagnostics.rs` char/type cases; `tests/lexer`/parser unit tests |
| T-042 | Globals, enums, and constant evaluation | Maintainer (Language) | P0 | T-041 | COMPLETED | `static`/`static mut` with const initializers and address-of; enums with i32 discriminants, comparisons, casts, printing | `tests/features.rs::globals_enums_and_constant_addresses`; `tests/diagnostics.rs::rejects_non_constant_global_initializers`, `rejects_enum_and_character_type_errors` |
| T-043 | Floats and float printing | Maintainer (Language + Backend) | P1 | T-042 | COMPLETED | `f32`/`f64` literals, arithmetic, ordered comparisons, casts, compound assignment, globals, fixed-six print with nan/inf | `tests/features.rs::floats_arithmetic_casts_and_printing`; IR verifies via `llvm-as` |
| T-044 | Structs: layout, literals, fields, by-value ABI | Maintainer (Language + Backend) | P0 | T-042 | COMPLETED | Natural layout, nested/array structs, copy semantics, by-value params/returns, field assignment | `tests/features.rs::struct_values_fields_params_returns_and_arrays`; `tests/diagnostics.rs::rejects_recursive_structs_and_bad_fields` |
| T-045 | Slices, `str`, `len`, and ranges | Maintainer (Language + Backend) | P0 | T-044 | COMPLETED | `[T]`/`str` two-word values, array-to-slice coercion, indexing, `len`, `a..b` sub-slices, checked bounds | `tests/features.rs::slices_strings_len_and_ranges`, `runtime_slice_bounds_abort`; `tests/diagnostics.rs::rejects_invalid_slice_ranges_and_printables` |
| T-046 | Documentation, grammar, and example for extended features | Documentation (isolated workstream) | P0 | T-041…T-045 | COMPLETED | Spec/grammar/README/memory/architecture/PRD/task tracker match implemented behavior; checked-in example | `examples/features.cx`; `tests/features.rs::features_example_runs`; docs updated 2026-10-21 |

## Backlog (NOT STARTED)

| ID | Description | Priority | Blocked by |
|---|---|---|---|
| T-033 | Module system and multi-object linking | P2 | name mangling policy |
| T-034 | `extern` declarations and C FFI | P1 | ABI policy, header story |
| T-035 | Debug information (DWARF) | P1 | source map through AST → IR metadata |
| T-036 | Checked-arithmetic mode (`--checked-arithmetic`) | P2 | cost/benefit benchmark |
| T-037 | Opt-in owned heap handles (memory model C) | P3 | type system design |
| T-038 | ARM64 and freestanding targets | P2 | target abstraction in codegen |
| T-039 | Property/fuzz testing for the lexer, parser, and IR emitter | P1 | — |
| T-040 | Golden IR snapshots per construct | P2 | LLVM version policy |

## Status summary

- COMPLETED: T-001 … T-028 and T-041 … T-046 (34 tasks).
- IN PROGRESS: none.
- BLOCKED: none.

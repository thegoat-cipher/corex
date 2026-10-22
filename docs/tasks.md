# CoreX Task Tracker

Status values: NOT STARTED, IN PROGRESS, BLOCKED, COMPLETED. Validated status
changed only after the acceptance criteria passed. Evidence names the command or
test that proves completion; counts are from the run on 2026-10-21.

Test totals (v0.3, 2026-10-22): `cargo test` → **174 passed, 0 failed**
(61 unit, 113 integration).

| ID | Description | Workstream | Priority | Depends on | Status | Acceptance criteria | Validation evidence |
|---|---|---|---|---|---|---|---|
| T-001 | Initialize repository skeleton, `.gitignore`, initial commit | Maintainer | P0 | — | COMPLETED | Repo has a git branch and commit; build artifacts ignored | `git log --oneline -1`; `.gitignore` excludes `compiler/target/`, `build/`, `*.o`, `*.ll` |
| T-002 | Install user-local Rust toolchain | Maintainer (Environment) | P0 | — | COMPLETED | `rustc` and `cargo` resolve without root | `rustc 1.99.1`; `cargo 1.99.1`; installed under `$HOME/.cargo` / `$HOME/.rustup` |
| T-003 | Install user-local LLVM 20 toolchain | Maintainer (Environment) | P0 | — | COMPLETED | `llvm-as`, `llc`, `opt`, `llvm-config` run; freestanding static ELF links and executes | `llvm-as --version` → 20.1.8; `llc --version` → 20.1.8; hand-written asm binary exits 42 |
| T-004 | Research existing systems languages and compiler patterns | Research (parallel workstream) | P0 | T-001 | COMPLETED | Evidence-labelled findings with source URLs and dates | `docs/research.md` (868 lines, 46 cited URLs; spot-checked HTTP 200); `docs/competitor-analysis.md` (140 lines) |
| T-005 | Product requirements and non-functional requirements | Product Requirements (parallel workstream) | P0 | T-004 | COMPLETED | Vision, users, use cases, numbered FRs, MVP, non-goals, risks, DoD, measurable NFRs | `docs/PRD.md` (35 FRs, 13 non-goals, 12-point DoD); `docs/NFR.md` (62 NFRs, each Enforced/Unvalidated/Aspirational) |
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
| T-049 | Dynamic arrays (`vec<T>`) for compiler data | Maintainer (Language + Backend) | P0 | T-046 | COMPLETED | `vec_new`/`vec_push`/`vec_free`/`vec_slice`, `len`, indexing, growth, element types incl. structs | `tests/compiler_caps.rs::dynamic_arrays_grow_index_and_free`, `recursive_structs_via_pointers`; IR verified |
| T-050 | Compile-time `const` declarations | Maintainer (Language) | P0 | T-046 | COMPLETED | Scalar/str/pointer constants, previous-constant references, no address, init errors | `tests/compiler_caps.rs::compile_time_constants`; `tests/diagnostics.rs` |
| T-051 | File I/O builtins | Maintainer (Runtime + Language) | P0 | T-049 | COMPLETED | `read_file` returns owned `vec<u8>`; `write_file` returns 0/-1; syscall helpers and failure behavior | `tests/compiler_caps.rs::file_io_round_trip_and_missing_file` |
| T-052 | Recursive imports and multi-file diagnostics | Maintainer (Driver) | P0 | T-049 | COMPLETED | Relative import resolution, dedup/cycles, flat namespace, per-file span mapping | `tests/compiler_caps.rs::imports_resolve_relative_files_and_diagnose_paths` |
| T-053 | Panic/assert error handling | Maintainer (Language + Backend) | P0 | T-049 | COMPLETED | Message + newline on fd 2, exit 101, panic satisfies return analysis | `tests/compiler_caps.rs::panic_and_assert_abort_with_messages`, `panic_satisfies_return_analysis` |
| T-054 | Compiler-data capability example and docs | Documentation (isolated workstream) | P0 | T-049…T-053 | COMPLETED | Tokenizer example using enums/structs/const/vec/strings/slices/assert; docs and grammar updated | `examples/compiler_data.cx`; `tests/compiler_caps.rs::compiler_data_example_tokenizes`; README/spec/grammar/memory/architecture/PRD updated |
| T-055 | Installation guide and curated resource index | Documentation (isolated workstream) | P0 | T-054 | COMPLETED | Reproducible install/build/verify/troubleshooting guide; project, source, test, example, and external reference map | `docs/installation.md`, `docs/resources.md`, README documentation index |

## Backlog disposition (v0.3)

The v0.2 backlog is fully dispositioned:

| ID | Was | Now |
|---|---|---|
| T-033 | modules + multi-object linking | T-061, T-062 |
| T-034 | `extern` declarations and C FFI | T-060 |
| T-035 | DWARF debug information | T-065 |
| T-036 | checked-arithmetic mode | T-071 (`--sanitize`, signed types) |
| T-037 | opt-in owned heap handles | superseded by explicit `Allocator`/arena (T-063); no hidden ownership model added |
| T-038 | ARM64/freestanding targets | still out of scope (Linux x86-64 only) |
| T-039 | property/fuzz testing | existing robustness/fuzz-style tests; parser nesting limits remain |
| T-040 | golden IR snapshots | `tests/ir.rs` + `emit-ir --verify`; no version-pinned corpus |

## Status summary

- COMPLETED: T-001 … T-028, T-041 … T-046, and T-049 … T-055 (41 tasks).
- IN PROGRESS: none.
- BLOCKED: none.

## C-class roadmap (v0.3)

All tasks below are COMPLETED and validated by the named test or command on
2026-10-22. Test totals are in the status summary at the end.

| ID | Description | Workstream | Priority | Status | Validation evidence |
|---|---|---|---|---|---|
| T-056 | Function types, addresses, indirect calls, null function pointers, dispatch tables | Language + Backend | P0 | COMPLETED | `tests/extended.rs::function_pointers_are_values_and_dispatch_indirectly`; `examples/dispatch.cx` runs at `--opt 0..3` |
| T-057 | `switch`/`case`/`default` with C fallthrough, `for`, `do while`, labels and `goto` | Language + Backend | P0 | COMPLETED | `tests/extended.rs::switch_fallthrough_for_do_while_and_goto`, `switch_works_at_every_optimization_level` |
| T-058 | Unions, bitfields, flexible array members, designated/partial initializers, compound literals | Language + Layout | P0 | COMPLETED | `tests/extended.rs::layout_introspection_unions_bitfields_and_compound_literals` |
| T-059 | `typedef`, `repr(C)`, packed and `align(N)` structs, `sizeof`/`alignof`/`offsetof`, C layout | Language + Layout | P0 | COMPLETED | same test (sizes/offsets); `docs/language-spec.md` §12.4 |
| T-060 | `extern "C"` declarations and exports, C strings, variadic calls, `main(argc, argv, envp)` | ABI + Backend | P0 | COMPLETED | `tests/ffi.rs::extern_c_calls_exports_aggregates_and_varargs`, `main_receives_argc_argv_and_envp` |
| T-061 | File modules, `pub`/private interfaces, deterministic `module$item` mangling | Modules | P0 | COMPLETED | `tests/modules.rs::file_modules_export_public_items_and_hide_private_ones`; `nm` symbol assertions |
| T-062 | Per-unit object compilation, object/archive/shared linking, `corex.toml`, `#[cfg]` | Modules + Driver | P0 | COMPLETED | `tests/modules.rs` (manifest exe, static archive, shared + dlopen), `cfg_attributes_select_items_and_target_keys` |
| T-063 | `malloc`/`calloc`/`realloc`/`free`, `Allocator` interface, arena allocator | Runtime | P0 | COMPLETED | `tests/stdlib.rs::allocator_memory_and_string_library` |
| T-064 | Freestanding memory, string, I/O, file, directory, process, time, math, formatting libraries | Runtime | P0 | COMPLETED | `tests/stdlib.rs::file_io_formatting_time_and_directories`; `lib/corex/*.cx` |
| T-065 | DWARF 5 line tables, subprograms, local variables | Backend | P0 | COMPLETED | `tests/tooling.rs::debug_info_has_line_tables_and_subprograms`; `readelf --debug-dump=decodedline/info` |
| T-066 | Formatter (`corex fmt`, `-w`, `--check`), idempotent | Tooling | P0 | COMPLETED | `format::tests::*`; `tests/tooling.rs::formatter_is_idempotent_and_semantics_preserving` |
| T-067 | Linter (`W0001`–`W0004`, `--deny-warnings`) | Tooling | P0 | COMPLETED | `tests/tooling.rs::linter_reports_unused_and_unreachable` |
| T-068 | Language server: diagnostics, completion, definition, references, rename, hover, formatting | Tooling | P0 | COMPLETED | `tests/tooling.rs::language_server_reports_diagnostics_and_edits`; manual framed session |
| T-069 | Markdown documentation generator from `///` comments | Tooling | P0 | COMPLETED | `docgen::tests::generates_docs_for_items`; `corex doc examples/features.cx` |
| T-070 | Test runner for `fn test_*() -> bool` with pass/fail reporting | Tooling | P0 | COMPLETED | `tests/tooling.rs::test_runner_reports_pass_and_failure` |
| T-071 | Sanitizers: checked signed arithmetic, allocator poisoning and double-free detection | Tooling + Backend | P0 | COMPLETED | `tests/tooling.rs::sanitizer_aborts_on_signed_overflow_and_double_free` |
| T-072 | Volatile globals/pointers, atomic load/store/RMW/cmpxchg/fence builtins | Language + Backend | P1 | COMPLETED | `tests/concurrency.rs::atomics_volatile_and_inline_asm` |
| T-073 | Threads (`clone`), futex mutex/condvar/spinlock, explicit TLS | Runtime | P1 | COMPLETED | `tests/concurrency.rs::threads_share_memory_and_a_futex_mutex`, `explicit_thread_local_storage_round_trips` |
| T-074 | Inline assembly with constraints, clobbers, and barriers | Language + Backend | P1 | COMPLETED | `tests/concurrency.rs::atomics_volatile_and_inline_asm`; `std_thread` trampoline |
| T-075 | Declarative expression macros | Language | P1 | COMPLETED | `tests/extended.rs::declarative_macros_expand_at_compile_time` |
| T-076 | Shared libraries and `dlopen` | Runtime + Driver | P1 | COMPLETED | `tests/modules.rs::shared_library_can_be_loaded_with_dlopen` |
| T-077 | Benchmark suite and measured results | Performance | P1 | COMPLETED | `scripts/bench.sh`; `docs/benchmarks.md` (5 kernels + compiler throughput) |

### Known gaps (explicit)

- No thread join; `std_thread` exposes spawn/yield/exit and stack cleanup only.
- TLS is an explicit table keyed by thread id, not a compiler `thread_local`.
- The `fib` benchmark remains ~1.6× slower than GCC; documented in
  `docs/benchmarks.md`, not hidden.
- `volatile` applies to global declarations; pointers use the builtin pair.
- Macros are expression macros, unit-local and non-hygienic.

## Status summary (v0.3)

- COMPLETED: T-001 … T-028, T-041 … T-046, T-049 … T-055, T-056 … T-077.
- IN PROGRESS: none.
- BLOCKED: none.

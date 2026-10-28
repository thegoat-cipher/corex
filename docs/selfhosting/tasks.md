# Self-Hosting Task Tracker

Last updated: 2026-10-28. Statuses: NOT STARTED, IN PROGRESS, BLOCKED,
COMPLETED. A task is only COMPLETED with the evidence named in the row.

| ID | Agent | Task | Priority | Depends on | Status | Acceptance / evidence |
|---|---|---|---|---|---|---|
| SH-001 | 01 Auditor | Audit existing compiler and repository | P0 | — | COMPLETED | `repository-audit.md`; baseline `cargo test` 205/205; release binary works |
| SH-002 | 03 Language analyst | Verify language can express the compiler | P0 | SH-001 | COMPLETED | `language-readiness.md`; probes all `rc=0`; no language change required |
| SH-003 | 02 Bootstrap architect | Define stages, artifacts, toolchain, determinism | P0 | SH-001 | COMPLETED | `bootstrap-architecture.md`; Stage 0–4 model |
| SH-004 | 04 Memory engineer | Arena/buffers/strings/files/process support | P0 | SH-003 | COMPLETED | `runtime.cx` (573 lines); arena stress 128 blocks, `used=4096`; size parity |
| SH-005 | 05 Lexer engineer | Lexer in CoreX, tokens, spans, errors | P0 | SH-004 | COMPLETED | `lexer.cx`; lexes all 7,721 selfhost lines; token overflow check |
| SH-006 | 06 Parser engineer | AST and recursive-descent parser | P0 | SH-005 | COMPLETED | `ast.cx`, `parser.cx`; parses `corexc.cx` with 0 diagnostics; 244 functions |
| SH-007 | 07 Semantic engineer | Symbols, types, layout, consts, checks, flow | P0 | SH-006 | COMPLETED | `types.cx`, `semantic.cx`; 0 diagnostics on full source; 6 negative cases rejected |
| SH-008 | 08 Codegen engineer | LLVM IR backend and ABI | P0 | SH-007 | COMPLETED | `codegen.cx`; `llvm-as` accepts every build; 64 positive runs × 3 stages |
| SH-009 | 09 Integration | Driver, CLI, tool discovery, linking | P0 | SH-008 | COMPLETED | `driver.cx`; `check`, `emit-ir`, `build --opt 0..3`; static ELF |
| SH-010 | 09 Integration | Build Stage 1 with Stage 0 | P0 | SH-009 | COMPLETED | `build/stage1/corexc`, 0.272 s, hash `7b2f8837…` |
| SH-011 | 09 Integration | Build Stage 2 with Stage 1 | P0 | SH-010 | COMPLETED | `build/stage2/corexc`, 0.342 s, hash `644c22c5…` |
| SH-012 | 09 Integration | Build Stage 3 with Stage 2 | P0 | SH-011 | COMPLETED | `build/stage3/corexc`, 0.347 s, hash `644c22c5…` |
| SH-013 | 09 Integration | Compare Stage 2/3 and Stage 3/4 | P0 | SH-012 | COMPLETED | SHA-256 equal (`644c22c5…`); repeated build equal; `bootstrap.sh` exits 0 |
| SH-014 | 10 Validation | Differential/negative/native/opt-level tests | P0 | SH-012 | COMPLETED | `tests/selfhosting/`; 79 tests × Stage 1/2/3 = 237 passes; opt 0–3 |
| SH-015 | 11 Documentation | Architecture, results, validation, ADRs, logs | P1 | SH-014 | COMPLETED | All files in `docs/selfhosting/`; CI step added |
| SH-016 | 12 Bootstrap subset | Slice support in the bootstrap compiler | P1 | SH-014 | COMPLETED | `slices.cx` at `--opt 0..3`; 3 negative slice cases; corpus 49 at completion (62 after SH-017, 79 after SH-018); Stage 2/3/4 `ff6677ca…`; ADR-SH-012 |
| SH-017 | 12 Bootstrap subset | Runtime bounds checks and `str`/`[u8]` compatibility | P1 | SH-016 | COMPLETED | `bounds_array/slice/str` abort cases (exit 101); `index_literal` negative case; corpus 62 at completion (79 after SH-018); Stage 2/3/4 `c417b392…`; ADR-SH-013 |
| SH-018 | 12 Bootstrap subset | `vec<T>` support | P1 | SH-017 | COMPLETED | `vec.cx` + `vec_elements.cx`; `bounds_vec` abort case; 5 negative vec cases; corpus 79 × Stage 1/2/3; Stage 2/3/4 `644c22c5…`; ADR-SH-014 |

## Remaining Backlog (2026-10-28)

Statuses follow the tracker rules. "Oracle" means Rust `corex` from
`compiler/`. Each task is complete only with the acceptance evidence named in
the row; the fixed-point gate is Stage 2 == Stage 3 == Stage 4 plus the corpus.

### P0 — next critical path

| ID | Task | Owner | Priority | Depends on | Status | Acceptance / evidence |
|---|---|---|---|---|---|---|
| SH-019 | `f32`/`f64` type model and literals | unassigned | P0 | — | NOT STARTED | `let x: f64 = 1.5;` parses/types/builds; `sizeof/alignof` match oracle; invalid float forms rejected |
| SH-020 | Float arithmetic, comparisons, casts, ABI | unassigned | P0 | SH-019 | NOT STARTED | `+ - * / %`, ordered comparisons, int↔float and f32↔f64 casts, params/returns; oracle parity at `--opt 0..3` |
| SH-021 | Float `print` formatting | unassigned | P0 | SH-019, SH-020 | NOT STARTED | six fractional digits, `nan`/`inf`/`-inf`, ≥1e18 abort; byte-identical to oracle |
| SH-022 | Division/remainder runtime checks | unassigned | P0 | — | NOT STARTED | `a/0`, `a%0`, `MIN/-1` abort 101 with `corex: division by zero or signed overflow`; unsigned/signed cases |
| SH-023 | Static aggregate/global initializers | unassigned | P0 | — | NOT STARTED | `static A: [i32; 3] = [7,8,9]`, struct/fn-pointer tables; `llvm-as` valid; values match oracle |
| SH-024 | `str_to_cstr` codegen; `read_file`/`write_file` builtins | unassigned | P0 | — | NOT STARTED | `str_to_cstr` emits valid IR; file builtins round-trip; failure contracts match spec |
| SH-025 | Diagnostics, parser recovery, malformed-input robustness | unassigned | P0 | — | NOT STARTED | Fuzz/malformed corpus never panics; diagnostic codes/positions match oracle where defined; negative tests added |

### P1 — language surface and compatibility

| ID | Task | Owner | Priority | Depends on | Status | Acceptance / evidence |
|---|---|---|---|---|---|---|
| SH-026 | Example and `lib/corex` compatibility matrix | unassigned | P1 | SH-019…024, SH-030…033 | NOT STARTED | All `examples/*.cx` and `lib/corex/*.cx` compile/run or have a documented subset reason |
| SH-027 | Function types and function pointers | unassigned | P1 | SH-023 | NOT STARTED | `fn(T)->R` types, `&fn`, indirect calls, tables, params/returns; oracle parity |
| SH-028 | Unions, bitfields, flexible arrays, packed/`align(N)` structs | unassigned | P1 | — | NOT STARTED | Layout, field access, `sizeof/alignof/offsetof` match oracle |
| SH-029 | `for`, `do while`, labels, `goto` | unassigned | P1 | — | NOT STARTED | Control-flow semantics and return analysis match oracle |
| SH-030 | Modules, interfaces, mangling, `corex.toml`, `#[cfg]` | unassigned | P1 | — | NOT STARTED | Multi-unit project builds; `module$item` symbols; interfaces; manifest; cfg |
| SH-031 | Expression macros and cfg attributes | unassigned | P1 | SH-030 | NOT STARTED | Expansion parity; nested/edge cases; negative diagnostics |
| SH-032 | C `extern`, variadics, hosted mode, C aggregate ABI, extra objects/archives/shared | unassigned | P1 | — | NOT STARTED | C helper called both directions; `--hosted`; archives/shared/dlopen |
| SH-033 | Threads, atomics, volatile, TLS, inline asm | unassigned | P2 | — | NOT STARTED | Concurrency/futex/TLS/asm tests match oracle |
| SH-034 | Build `lib/corex` with the bootstrap compiler | unassigned | P1 | SH-030…033 | NOT STARTED | All 16 stdlib modules build; tests using them pass |

### P2 — driver, tooling, validation

| ID | Task | Owner | Priority | Depends on | Status | Acceptance / evidence |
|---|---|---|---|---|---|---|
| SH-035 | CLI parity: `run`, `exec`, `repl`, `fmt`, `lint`, `doc`, `test`, `emit-ir --verify`, stdin `-` | unassigned | P2 | — | NOT STARTED | Commands, flags, exit codes and source-from-stdin match the Rust CLI |
| SH-036 | Link/emit flags: `--hosted`, `--shared`, `--emit`, `-g`, `--sanitize`, `-I`, `--cfg`, `--link`, `-L`, `-l` | unassigned | P2 | SH-032 | NOT STARTED | Each flag exercised; artifacts validated; failures diagnostic |
| SH-037 | LSP, line editor, remaining tooling | unassigned | P3 | SH-035 | NOT STARTED | LSP session and tooling tests pass against the bootstrap compiler |
| SH-038 | Differential token/AST/IR dumps | unassigned | P2 | — | NOT STARTED | Normalized dumps match oracle on the corpus; divergences documented |
| SH-039 | Run the full Rust behavior suite against `corexc` | unassigned | P2 | SH-019…036 | NOT STARTED | 205 behavioral tests parameterized; same stdout/exit/diagnostics |
| SH-040 | Memory validation and fuzzing | unassigned | P2 | — | NOT STARTED | ASan/Valgrind on bootstrap build; malformed-input fuzz; no UAF/overflow/leaks beyond arena lifetime |
| SH-041 | Performance baseline, parallel units, arena sizing | unassigned | P2 | — | NOT STARTED | Measured self-compile/frontend throughput; documented budget; optional fork-per-unit |
| SH-042 | Reproducibility at `--opt 1..3` | unassigned | P3 | SH-019…024 | NOT STARTED | Optimized self-build succeeds; document whether Stage2/3 hashes are expected to match |
| SH-043 | Commit/push and remote CI validation | unassigned | P1 | — | NOT STARTED | Branch pushed; GitHub CI runs the 79-test gate and `bootstrap.sh`; hashes recorded |
| SH-044 | Bootstrap code cleanup/review | unassigned | P3 | — | NOT STARTED | Remove or justify defensive `ck`/`type_ok` paths; reduce duplicated `unsafe` wrappers; no dead code |
| SH-045 | Documentation maintenance per milestone | unassigned | P3 | — | NOT STARTED | tasks/ADRs/results/limits updated with evidence after each task |
| SH-046 | Full self-hosting acceptance (AC-1…AC-9) | unassigned | P1 | SH-019…039 | NOT STARTED | Corpus/negative/IR/determinism/performance gates measured and recorded in `docs/self-hosting.md` terms |

## Non-Goals (not tasks)

These are outside the language's stated scope, not bootstrap gaps: `match` and
payload enums, generics beyond `vec<T>`, closures, traits, GC, exceptions,
`async`, operator overloading, cross-compilation, non-Linux targets, package
registry, and a full standard library. Rust Stage 0 remains the reference and
is not scheduled for deletion.


## Notes

- SH-001…SH-003 were completed by independent subagents (audit and language
  readiness) and by the master orchestrator (architecture); the implementation
  workstreams SH-004…SH-015 were executed by the master orchestrator. The
  self-hosted compiler is a single tightly coupled translation unit; splitting
  it across concurrent agents in the same checkout would have introduced
  conflicting edits. See `agent.md` for the coordination rule.
- SH-016 (slices) was executed by the dedicated bootstrap-subset agent after
  the Stage 3 fixed point; it touched `ast.cx`, `parser.cx`, `semantic.cx`,
  `codegen.cx`, tests, and docs only.
- SH-017 (bounds checks, `str`/`[u8]` compatibility) followed in the same
  workstream and reused the shared abort helper in `codegen.cx`.
- SH-018 (`vec<T>`) completed the subset's dynamic-array milestone; the
  compiler changes were audited against the Rust oracle and the goldens were
  regenerated from it.
- No completed task is blocked. The remaining work is scheduled as SH-019…SH-046
  above; none is IN PROGRESS or BLOCKED. Estimates are replaced by measured
  evidence in `bootstrap-results.md`.

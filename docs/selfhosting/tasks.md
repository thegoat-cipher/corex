# Self-Hosting Task Tracker

Last updated: 2026-10-29. Statuses: NOT STARTED, IN PROGRESS, BLOCKED,
COMPLETED. A task is only COMPLETED with the evidence named in the row.

Final integrated compiler: **545 functions**, **17,747 lines** across 15
`selfhost/*.cx` files (LSP included; SH-044 cleanup applied).
`selfhost/bootstrap.sh` builds Stage 1–4, runs the behavioural corpus (270
checks per stage), CLI parity (80), the LSP protocol suite (35), differential
dumps (81), fuzzing/ASan (330), concurrency parity (33), the `lib/corex`
matrix (16), and the performance baseline;
`selfhost/repro-opt.sh --corpus` covers `--opt 1..3`. The parameterized Rust
behaviour suite is `tests/selfhosting/behavior.py` (205 rows:
137 passed / 0 failed / 68 blocked / 0 timeout; the blocked rows are the
Rust-internal `--lib` unit tests) — see `worklogs/semantic-hang.md`.

| ID | Task | Status | Evidence |
|---|---|---|---|
| SH-001 | Audit compiler/repository | COMPLETED | `repository-audit.md`; baseline 205/205 |
| SH-002 | Verify language can express the compiler | COMPLETED | `language-readiness.md`; probes |
| SH-003 | Define bootstrap stages/toolchain | COMPLETED | `bootstrap-architecture.md` |
| SH-004 | Arena/buffers/strings/files/process runtime | COMPLETED | `runtime.cx` |
| SH-005 | Lexer | COMPLETED | `lexer.cx` |
| SH-006 | AST and parser | COMPLETED | `ast.cx`, `parser.cx` |
| SH-007 | Types and semantic analysis | COMPLETED | `types.cx`, `semantic.cx` |
| SH-008 | LLVM IR backend/ABI | COMPLETED | `codegen.cx` |
| SH-009 | Driver/CLI/linking | COMPLETED | `driver.cx` |
| SH-010 | Build Stage 1 with Rust Stage 0 | COMPLETED | Stage 1 `96f1ef30…` |
| SH-011 | Build Stage 2 with Stage 1 | COMPLETED | Stage 2 `f3e770a3…` |
| SH-012 | Build Stage 3 with Stage 2 | COMPLETED | Stage 3 `f3e770a3…` |
| SH-013 | Compare Stage 2/3 and 3/4 | COMPLETED | `stage2 == stage3 == stage4` |
| SH-014 | Differential/negative/native/opt tests | COMPLETED | `tests/selfhosting/`; 270 per stage |
| SH-015 | Documentation and CI | COMPLETED | `docs/selfhosting/`; `.github/workflows/ci.yml` |
| SH-016 | Slices | COMPLETED | `slices.cx`; corpus |
| SH-017 | Runtime bounds checks and `str`/`[u8]` | COMPLETED | abort cases; `index_literal` |
| SH-018 | `vec<T>` | COMPLETED | `vec.cx`, `vec_elements.cx` |
| SH-019 | `f32`/`f64` type model and literals | COMPLETED | P0 floats; `floats.cx`, `float_compound.cx`; oracle parity |
| SH-020 | Float arithmetic/comparisons/casts/ABI | COMPLETED | P0; compound `+= -= *= /=` and hosted `%=`; differential |
| SH-021 | Float `print` | COMPLETED | six-digit format; oracle parity |
| SH-022 | Division/remainder checks | COMPLETED | abort 101 zero/`MIN/-1` |
| SH-023 | Static aggregate/global initializers | COMPLETED | `statics.cx`; plain aggregates only; non-plain (`packed`/union/bitfield/`align`) statics rejected E0226 |
| SH-024 | `str_to_cstr`, `read_file`, `write_file` | COMPLETED | P0 IO builtins; CLI/file tests |
| SH-025 | Diagnostics/parser recovery/fuzz | COMPLETED | 65/65 negative cases assert a code matching the oracle; malformed corpus; fuzz/ASan |
| SH-026 | Example and `lib/corex` compatibility matrix | COMPLETED | 8/8 examples byte-identical to the oracle; `lib/corex` 16/16 |
| SH-027 | Function types/pointers | COMPLETED | `fn_pointers.cx`; indirect calls |
| SH-028 | Unions/bitfields/FAM/packed/align | COMPLETED | layout/access differential |
| SH-029 | `for`/`do while`/labels/`goto` | COMPLETED | control-flow cases |
| SH-030 | Modules/mangling/`corex.toml`/cfg | COMPLETED | multi-unit projects; `math$add`; manifest; built-in `target_os`/`target_arch`/`sanitize` cfg keys; `--cfg`/`-I` on `check`/`emit-ir`/stdin; `mod` search honors `-I` |
| SH-031 | Expression macros | COMPLETED | expansion parity; negative cases |
| SH-032 | C `extern`/variadics/hosted/shared/obj | COMPLETED | C↔CoreX, dlopen, `--emit` |
| SH-033 | Threads, atomics, volatile, TLS, inline asm | COMPLETED | volatile globals/`volatile_load`/`volatile_store`; 12 atomic builtins; `asm`; `std_sync`/`std_tls`/`std_thread` runtime parity at `--opt 0..3`; `concurrency.sh` 33/33 |
| SH-034 | Build `lib/corex` | COMPLETED | 16/16 modules build with the bootstrap and the oracle (`tests/selfhosting/lib.sh`) |
| SH-035 | CLI parity | COMPLETED | `run`/`exec`/`fmt`/`lint`/`doc`/`test`/REPL/stdin; build failure and signal status propagate; CLI 80/80 |
| SH-036 | Link/emit flags | COMPLETED | `--emit obj/shared/staticlib`, `--hosted`, `--shared`, `--sanitize`, `-I`, `--cfg`, `--link`, `-L`, `-l`; `-g` emits DWARF (compile unit, per-function `DW_TAG_subprogram`, line locations); `build --test` links the test runner |
| SH-037 | LSP/line editor/remaining tooling | COMPLETED | `selfhost/lsp.cx` (1,499 lines) implements initialize/initialized/shutdown/exit, didOpen/didChange/didClose + publishDiagnostics, hover, definition, references, rename, completion, and whole-document formatting; `corexc lsp` wired in `driver.cx`; `lsp.py` 35/35 in bootstrap and CI |
| SH-038 | Differential token/AST/IR dumps | COMPLETED | Rust oracle; 81/81; known IR divergences recorded |
| SH-039 | Full Rust behavior suite against `corexc` | COMPLETED | `behavior.py` runs all 205 rows with per-process 60 s timeouts: 137 passed, 0 failed, 68 blocked, 0 timeout (exit 0). Every CLI-expressible integration test passes; the 68 `--lib` unit tests call Rust-internal modules and are reported as blocked with that reason. See `worklogs/semantic-hang.md` |
| SH-040 | Memory validation and fuzzing | COMPLETED | 330/330 ASan/fuzz; ASan required in CI; no signals |
| SH-041 | Performance baseline | COMPLETED | Integrated table in `bootstrap-results.md` §3 (self-compile and frontend/RSS) |
| SH-042 | Reproducibility at `--opt 1..3` | COMPLETED | `repro-opt.sh`; opt1 `0745f798…`, opt2 `2cd22d29…`, opt3 `37d52093…` |
| SH-043 | Commit/push and remote CI validation | COMPLETED | Branch pushed to `origin/room/corex-tooling-hang-fix-g3gb`; workflow_dispatch run [41095491312](https://github.com/thegoat-cipher/corex/actions/runs/41095491312) on `47b60b5` completed green: all 13 steps (cargo 205, examples, bootstrap 270/80/35/81/330/33/16 + ASan, repro-opt, install smoke). Main pushed as `96c3790`; main run [41097021388](https://github.com/thegoat-cipher/corex/actions/runs/41097021388) completed green. |
| SH-044 | Bootstrap code cleanup/review | COMPLETED | Reapplied on the integrated tree by `room/corex-cleanup-blockers-ppaf` in four commits (`33a8094` unsafe unwrap, `fa135cb` dead helpers/merges, `c53a580` `Program` fields, `8320b92` `parse_program` cutover), rebased onto `b53be92`. 558 → 534 functions, 17,653 → 17,236 lines (pre-tooling-merge tree 487 → 463 / 15,787 → 15,370). Gates unchanged: 270/80/35/81/330/33/16, 205 `cargo test`, 8/8 examples; Stage 2/3/4 `f3e770a3…`. Details in `worklogs/p3.md` |
| SH-045 | Documentation maintenance | COMPLETED | P0/P1/P2/P3 worklogs plus `worklogs/sh033-034.md`; this tracker, `master.md`, results/validation |
| SH-046 | Full self-hosting acceptance (AC-1…AC-9) | COMPLETED | All AC rows pass (AC-4 in scope); local gates green on `47b60b5`; remote CI run [41095491312](https://github.com/thegoat-cipher/corex/actions/runs/41095491312) green; main push `96c3790` run [41097021388](https://github.com/thegoat-cipher/corex/actions/runs/41097021388) completed green. Independent master audit remains external |

## Final gate evidence

```text
compiler/target/release/corex check selfhost/corexc.cx -> ok: 545 function(s), entry `main`
stage1 57075baed1bb96564739e1274f16716fcd5fa693ee22c5a916eef7e5433796c0
stage2 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage3 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage4 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
reproducible: stage2 == stage3 == stage4
selfhosting tests: 270 passed, 0 failed (Stage 1/2/3)
cli tests: 80 passed, 0 failed
lsp tests: 35 passed, 0 failed
dump tests: 81 passed, 0 failed
fuzz tests: 330 passed, 0 failed
concurrency tests: 33 passed, 0 failed
lib tests: 16 passed, 0 failed
cargo test: 205 passed, 0 failed
behavior suite: 137 passed, 0 skipped, 0 failed, 68 blocked, 0 timeout (205 rows)
repro-opt: opt1 0745f798…, opt2 2cd22d29…, opt3 37d52093… (deterministic, fixed point)
examples: 8/8 match the oracle
lib/corex: 16/16 build
```

## Non-Goals

`match`/payload enums, generics beyond `vec<T>`, closures, traits, GC,
exceptions, `async`, operator overloading, cross-compilation, non-Linux
targets, a package registry, and a full standard library remain outside scope.
Rust Stage 0 is retained as the reference compiler.

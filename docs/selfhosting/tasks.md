# Self-Hosting Task Tracker

Last updated: 2026-10-29. Statuses: NOT STARTED, IN PROGRESS, BLOCKED,
COMPLETED. A task is only COMPLETED with the evidence named in the row.

Final integrated compiler: **487 functions**, **15,787 lines** across 14
`selfhost/*.cx` files. `selfhost/bootstrap.sh` builds Stage 1–4, runs the
behavioural corpus (266 checks per stage), CLI parity (80), differential dumps
(79), fuzzing/ASan (329), concurrency parity (33), the `lib/corex` matrix (16),
and the performance baseline; `selfhost/repro-opt.sh --corpus` covers
`--opt 1..3`.

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
| SH-010 | Build Stage 1 with Rust Stage 0 | COMPLETED | Stage 1 `926ecdec…` |
| SH-011 | Build Stage 2 with Stage 1 | COMPLETED | Stage 2 `dcccb0e0…` |
| SH-012 | Build Stage 3 with Stage 2 | COMPLETED | Stage 3 `dcccb0e0…` |
| SH-013 | Compare Stage 2/3 and 3/4 | COMPLETED | `stage2 == stage3 == stage4` |
| SH-014 | Differential/negative/native/opt tests | COMPLETED | `tests/selfhosting/`; 266 per stage |
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
| SH-036 | Link/emit flags | COMPLETED | `--emit obj/shared/staticlib`, `--hosted`, `--shared`, `--sanitize`, `-I`, `--cfg`, `--link`, `-L`, `-l`; `-g` is rejected by contract (exit 2) because no DWARF is emitted |
| SH-037 | LSP/line editor/remaining tooling | BLOCKED | CLI prerequisite is done; LSP/editor not ported |
| SH-038 | Differential token/AST/IR dumps | COMPLETED | Rust oracle; 79/79; known IR divergences recorded |
| SH-039 | Full Rust behavior suite against `corexc` | BLOCKED | SH-033 coverage is ported; remaining work is the broader suite plus LSP/tooling and ABI edges |
| SH-040 | Memory validation and fuzzing | COMPLETED | 329/329 ASan/fuzz; ASan required in CI; no signals |
| SH-041 | Performance baseline | COMPLETED | Integrated table in `bootstrap-results.md` §3 (self-compile and frontend/RSS) |
| SH-042 | Reproducibility at `--opt 1..3` | COMPLETED | `repro-opt.sh`; opt1 `58e5c30d…`, opt2 `d4e106cb…`, opt3 `ab469e2f…` |
| SH-043 | Commit/push and remote CI validation | NOT STARTED | Local commits only; remote push/CI not run |
| SH-044 | Bootstrap code cleanup/review | BLOCKED | P3 cleanup commits conflict with the larger integrated language branches; non-functional and not merged. See `worklogs/p3.md` |
| SH-045 | Documentation maintenance | COMPLETED | P0/P1/P2/P3 worklogs plus `worklogs/sh033-034.md`; this tracker, `master.md`, results/validation |
| SH-046 | Full self-hosting acceptance (AC-1…AC-9) | BLOCKED | Depends on SH-037, SH-039, SH-043 |

## Final gate evidence

```text
compiler/target/release/corex check selfhost/corexc.cx -> ok: 487 function(s), entry `main`
stage1 926ecdec940f571bcabaae07194bf8688e07bb0e69f57ab554746e4da454571f
stage2 dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad
stage3 dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad
stage4 dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad
reproducible: stage2 == stage3 == stage4
selfhosting tests: 266 passed, 0 failed (Stage 1/2/3)
cli tests: 80 passed, 0 failed
dump tests: 79 passed, 0 failed
fuzz tests: 329 passed, 0 failed
concurrency tests: 33 passed, 0 failed
lib tests: 16 passed, 0 failed
cargo test: 205 passed, 0 failed
repro-opt: opt1 58e5c30d…, opt2 d4e106cb…, opt3 ab469e2f… (deterministic, fixed point)
examples: 8/8 match the oracle
lib/corex: 16/16 build
```

## Non-Goals

`match`/payload enums, generics beyond `vec<T>`, closures, traits, GC,
exceptions, `async`, operator overloading, cross-compilation, non-Linux
targets, a package registry, and a full standard library remain outside scope.
Rust Stage 0 is retained as the reference compiler.

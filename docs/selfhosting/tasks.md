# Self-Hosting Task Tracker

Last updated: 2026-10-28. Statuses: NOT STARTED, IN PROGRESS, BLOCKED,
COMPLETED. A task is only COMPLETED with the evidence named in the row.

Final integrated compiler: **469 functions**, **15,065 lines** across 14
`selfhost/*.cx` files. `selfhost/bootstrap.sh` builds Stage 1–4, runs the
behavioural corpus (241 checks per stage), CLI parity (62), differential dumps
(73), fuzzing/ASan (326), and the performance baseline; `selfhost/repro-opt.sh
--corpus` covers `--opt 1..3`.

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
| SH-010 | Build Stage 1 with Rust Stage 0 | COMPLETED | Stage 1 `ccffc972…` |
| SH-011 | Build Stage 2 with Stage 1 | COMPLETED | Stage 2 `d270cd67…` |
| SH-012 | Build Stage 3 with Stage 2 | COMPLETED | Stage 3 `d270cd67…` |
| SH-013 | Compare Stage 2/3 and 3/4 | COMPLETED | `stage2 == stage3 == stage4` |
| SH-014 | Differential/negative/native/opt tests | COMPLETED | `tests/selfhosting/`; 241 per stage |
| SH-015 | Documentation and CI | COMPLETED | `docs/selfhosting/`; `.github/workflows/ci.yml` |
| SH-016 | Slices | COMPLETED | `slices.cx`; corpus |
| SH-017 | Runtime bounds checks and `str`/`[u8]` | COMPLETED | abort cases; `index_literal` |
| SH-018 | `vec<T>` | COMPLETED | `vec.cx`, `vec_elements.cx` |
| SH-019 | `f32`/`f64` type model and literals | COMPLETED | P0 floats; `floats.cx`; oracle parity |
| SH-020 | Float arithmetic/comparisons/casts/ABI | COMPLETED | P0; differential at `--opt 0..3` |
| SH-021 | Float `print` | COMPLETED | six-digit format; oracle parity |
| SH-022 | Division/remainder checks | COMPLETED | abort 101 zero/`MIN/-1` |
| SH-023 | Static aggregate/global initializers | COMPLETED | `statics.cx`; `const_value`/`const_init` |
| SH-024 | `str_to_cstr`, `read_file`, `write_file` | COMPLETED | P0 IO builtins; CLI/file tests |
| SH-025 | Diagnostics/parser recovery/fuzz | COMPLETED | malformed corpus; fuzz/ASan |
| SH-026 | Example and `lib/corex` compatibility matrix | COMPLETED | 7/8 examples byte-identical; `features.cx` blocked by float compound assignment; SH-034 tracks lib gaps |
| SH-027 | Function types/pointers | COMPLETED | `fn_pointers.cx`; indirect calls |
| SH-028 | Unions/bitfields/FAM/packed/align | COMPLETED | layout/access differential |
| SH-029 | `for`/`do while`/labels/`goto` | COMPLETED | control-flow cases |
| SH-030 | Modules/mangling/`corex.toml`/cfg | COMPLETED | multi-unit projects; `math$add`; manifest |
| SH-031 | Expression macros | COMPLETED | expansion parity; negative cases |
| SH-032 | C `extern`/variadics/hosted/shared/obj | COMPLETED | C↔CoreX, dlopen, `--emit` |
| SH-033 | Threads, atomics, volatile, TLS, inline asm | NOT STARTED | No parser/semantic/codegen support; blocks concurrency tests and std_sync/tls/thread |
| SH-034 | Build `lib/corex` | BLOCKED | 10/16 modules at P1 time; floats now integrated; SH-033 and remaining gaps block the rest |
| SH-035 | CLI parity | COMPLETED | `run`/`exec`/`fmt`/`lint`/`doc`/`test`/REPL/stdin; CLI 62/62 |
| SH-036 | Link/emit flags | COMPLETED | `--emit obj/shared/staticlib`, `--hosted`, `--shared`, `--sanitize`, `-I`, `--cfg`, `--link`, `-L`, `-l`; `-g` is rejected because no DWARF is emitted |
| SH-037 | LSP/line editor/remaining tooling | BLOCKED | CLI prerequisite is done; LSP/editor not ported |
| SH-038 | Differential token/AST/IR dumps | COMPLETED | Rust oracle; 73/73; known IR divergences recorded |
| SH-039 | Full Rust behavior suite against `corexc` | BLOCKED | Suite not ported; depends on SH-033, LSP/tooling, and remaining attribute/ABI gaps |
| SH-040 | Memory validation and fuzzing | COMPLETED | 326/326 ASan/fuzz; no signals |
| SH-041 | Performance baseline | COMPLETED | self-compile 496 ms; frontend 10.08 MB/s; RSS recorded |
| SH-042 | Reproducibility at `--opt 1..3` | COMPLETED | `repro-opt.sh`; opt1 `a5a09716…`, opt2 `e076f652…`, opt3 `76445b56…` |
| SH-043 | Commit/push and remote CI validation | NOT STARTED | Local commits only; remote push/CI not run |
| SH-044 | Bootstrap code cleanup/review | BLOCKED | P3 cleanup commits conflict with the larger integrated language branches; non-functional and not merged. See `worklogs/p3.md` |
| SH-045 | Documentation maintenance | COMPLETED | P0/P1/P2/P3 worklogs; this tracker, `master.md`, results/validation |
| SH-046 | Full self-hosting acceptance (AC-1…AC-9) | BLOCKED | Depends on SH-033, SH-034, SH-037, SH-039, SH-043 |

## Final gate evidence

```text
compiler/target/release/corex check selfhost/corexc.cx -> ok: 469 function(s), entry `main`
stage1 ccffc9721da61cd5cf880a1616e01f7c762fba53939e7618c29b299321447032
stage2 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
stage3 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
stage4 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
reproducible: stage2 == stage3 == stage4
selfhosting tests: 241 passed, 0 failed (Stage 1/2/3)
cli tests: 62 passed, 0 failed
dump tests: 73 passed, 0 failed
fuzz tests: 326 passed, 0 failed
cargo test: 205 passed, 0 failed
repro-opt: deterministic and self-reproducing at --opt 1..3
```

## Non-Goals

`match`/payload enums, generics beyond `vec<T>`, closures, traits, GC,
exceptions, `async`, operator overloading, cross-compilation, non-Linux
targets, a package registry, and a full standard library remain outside scope.
Rust Stage 0 is retained as the reference compiler.

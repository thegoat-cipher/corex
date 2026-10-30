# Validation Report — CoreX Self-Hosting

Date: 2026-10-29
Compiler under test: integrated `selfhost/` (released as CoreX 0.4.0) with the
SH-033 concurrency surface, SH-034 library closure, the SH-037 LSP, the SH-039
behaviour suite, and the SH-044 cleanup (545 functions, 17,747 lines after this
tooling change; 534 / 17,236 before it).

## 1. Result summary

| Gate | Result | Evidence |
|---|---|---|
| Stage 0 builds Stage 1 | PASS | Stage 1 `57075bae…`; runs the corpus |
| Stage 1 builds Stage 2 | PASS | Stage 2 `bcf17bc8…` |
| Stage 2 builds Stage 3 | PASS | Stage 3 identical to Stage 2 |
| Stage 3 builds Stage 4 | PASS | Stage 4 identical |
| Stage 2/3 compile user programs | PASS | 270 checks per stage; outputs byte-identical |
| Compiler behavior across stages | PASS | same corpus on Stage 1/2/3 |
| Artifact reproducibility | PASS | `sha256(stage2) == sha256(stage3) == sha256(stage4)` |
| Optimized reproducibility | PASS | `repro-opt.sh --corpus`, `--opt 1..3` |
| Rust suite unaffected | PASS | 205/205 `cargo test` |
| Remote CI | PASS | workflow_dispatch run [41095491312](https://github.com/thegoat-cipher/corex/actions/runs/41095491312) on `47b60b5`: 13/13 steps green |

## 2. Behavioural corpus

`selfhost/bootstrap.sh` runs 270 checks against Stage 1, Stage 2, and Stage 3.
The corpus covers arithmetic, aggregates, pointers, slices, `vec<T>`, floats
(including compound assignment), static initializers, modules, macros, function
pointers, volatile/atomic/`asm` paths, C FFI, labels/`goto`, and runtime
bounds/check aborts. All expected stdout and exit statuses are byte-for-byte
oracle output.

Composition: 40 positive cases × 4 opt levels = 160 (including
`pointer_chain.cx`, the 64-term left-deep regression); 65 negative cases; 28
malformed inputs; 4 module projects (3 normal × 4 + 1 `must_fail` = 13); 1 FFI
project × 4 = 4.

Additional gates:

| Gate | Result |
|---|---|
| CLI parity | 80/80 |
| LSP protocol (`lsp.py`) | 35/35 |
| Differential token/AST/IR dumps | 81/81 |
| Fuzz/ASan inputs | 330/330 |
| Parameterized Rust suite (`behavior.py`, Stage 2) | 137 passed, 0 failed, 68 blocked, 0 timeout (205 rows) |
| Concurrency runtime parity | 33/33 |
| `lib/corex` modules | 16/16, both compilers |
| Rust reference tests | 205/205 |
| Example matrix vs oracle | 8/8 |
| `emit-ir --verify` / `llvm-as` | all builds pass |

Known IR signature divergences are in
`tests/selfhosting/dump-ir-expected.txt`; they are ABI differences (not
miscompilation) recorded from the differential oracle.

## 3. Frontend and IR

- `compiler/target/release/corex check selfhost/corexc.cx` → `ok: 545
  function(s), entry main`; the bootstrap compiler produces the same result.
- Every build invokes `llvm-as`; malformed IR aborts the build and `build`
  exits nonzero (exit 3 for tool failures) without printing `compiled`.
- A tool killed by a signal is reported as failure (`128 + signal`), not exit 0.
- `llc -filetype=obj -relocation-model=static` and `cc -nostdlib -static
  -no-pie` produce a static ELF.
- The module emits weak freestanding `memset`/`memcpy`/`memmove`/`memcmp` so
  LLVM loop-idiom synthesis links without libc.

## 4. Native execution

The 40 positive cases run at `--opt 0,1,2,3`. Runtime abort cases (array,
slice, `str`, `vec` bounds; division by zero; signed `MIN/-1`) exit 101 with
the expected message on Stage 1 and Stage 2. All 65 negative cases are
rejected with a diagnostic code that matches the Rust oracle: the original
52-case corpus has a `.code` for every case, four packed/union/bitfield/`align`
static cases assert E0226, and nine new atomic/volatile/`asm` cases assert
E0203/E0204/E0209/E0216. `tests/selfhosting/concurrency.sh` checks those nine
against both compilers.

## 5. Bootstrap validation

```text
stage1 57075baed1bb96564739e1274f16716fcd5fa693ee22c5a916eef7e5433796c0
stage2 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage3 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
stage4 bcf17bc8ecd73f13fbce75ea24034ea160c5d027bd7e9b40077e17fc10541f64
reproducible: stage2 == stage3 == stage4
```

`file build/stage2/corexc` → static x86-64 ELF; `ldd` → not a dynamic
executable. Repeated Stage 2 builds are byte-identical.

Optimized levels are deterministic and self-reproducing:

| Level | SHA-256 |
|---|---|
| `--opt 1` | `0745f798dc417c21868f697161e1a7b923f1b0284a6304076f0ee8fad8b1da42` |
| `--opt 2` | `2cd22d2900ac4c9c2f3b1fc46473fe495841655a90cc3e394d928d665a1214f9` |
| `--opt 3` | `37d52093a0ab10ac304aecc5fa58e6a0299f5bc8a5322e1a607f5ec908752bf5` |

The integrated performance table (1304 ms best self-compile, 113 ms frontend,
4.72 MB/s closure throughput, 99,456/24,720 KiB peak RSS) is recorded once
in `bootstrap-results.md` §3.

## 6. Acceptance criteria

| ID | Status | Note |
|---|---|---|
| AC-1 corpus parity | PASS (scope) | 270-check corpus at four opt levels; 8/8 examples match the oracle; `lib/corex` 16/16 |
| AC-2 negative parity | PASS | 65/65 negative cases assert a code and match the Rust oracle |
| AC-3 IR validity | PASS | Every build is assembled; `emit-ir --verify` covers unreachable-label IR |
| AC-4 full 205-test behavior suite | PASS (scope) | `behavior.py` runs all 205 rows: 137 passed, 0 failed, 68 blocked, 0 timeout. Every CLI-expressible integration test passes; the 68 `--lib` unit tests call Rust-internal modules and are reported as blocked with that reason. See `worklogs/semantic-hang.md` |
| AC-5 modules/FFI | PASS (scope) | Module projects, archives/shared objects, C both directions |
| AC-6 determinism | PASS | Twin builds byte-identical |
| AC-7 fixed point | PASS | Stage 2 == Stage 3 == Stage 4 |
| AC-8 performance | PASS | Integrated table in `bootstrap-results.md` §3 |
| AC-9 no regression | PASS | Rust `corex` unchanged; 205/205 |

## 7. Known limitations

- Threads/TLS are library-level (`std_thread`/`std_tls`); TLS is an explicit
  table and there is no thread join. Inline-asm constraints are passed to LLVM
  verbatim, so a bad constraint is reported by `llvm-as`, not the frontend.
- `lib/corex` builds 16/16 modules with both compilers.
- The LSP is implemented (`corexc lsp`, 35 protocol checks); the line editor is
  the plain `repl` with no prompt output.
- `-g` emits minimal DWARF (compile unit, subprogram DIEs, line tables); local
  variable and type DIEs are not described.
- The behaviour suite has no failing or timing-out rows; only the 68
  Rust-internal unit tests are blocked (see `worklogs/semantic-hang.md`).
- Static initializers for non-plain aggregates (packed/union/bitfield/`align`)
  are rejected with E0226 rather than lowered to byte constants.
- Aggregate/`bool` calling conventions differ from Rust `corex`, so objects
  from the two compilers are not link-compatible for aggregate parameters.
- Module builds are whole-program; no generated interface files.
- Vec globals/constants are rejected and vec headers copy shallowly.
- REPL prints no prompts.
- ASan covers the compiler `check` path with `detect_leaks=0`; `fmt`, `lint`,
  `doc`, `dump`, and `repl` are exercised by CLI tests but not sanitized.

See `master.md`, `tasks.md`, `bootstrap-architecture.md`,
`memory-audit.md`, and the worklogs for details.

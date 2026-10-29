# Validation Report — CoreX Self-Hosting

Date: 2026-10-29
Compiler under test: integrated `selfhost/` with the SH-033 concurrency surface
and SH-034 library closure (487 functions, 15,787 lines).

## 1. Result summary

| Gate | Result | Evidence |
|---|---|---|
| Stage 0 builds Stage 1 | PASS | Stage 1 `926ecdec…`; runs the corpus |
| Stage 1 builds Stage 2 | PASS | Stage 2 `dcccb0e0…` |
| Stage 2 builds Stage 3 | PASS | Stage 3 identical to Stage 2 |
| Stage 3 builds Stage 4 | PASS | Stage 4 identical |
| Stage 2/3 compile user programs | PASS | 266 checks per stage; outputs byte-identical |
| Compiler behavior across stages | PASS | same corpus on Stage 1/2/3 |
| Artifact reproducibility | PASS | `sha256(stage2) == sha256(stage3) == sha256(stage4)` |
| Optimized reproducibility | PASS | `repro-opt.sh --corpus`, `--opt 1..3` |
| Rust suite unaffected | PASS | 205/205 `cargo test` |

## 2. Behavioural corpus

`selfhost/bootstrap.sh` runs 266 checks against Stage 1, Stage 2, and Stage 3.
The corpus covers arithmetic, aggregates, pointers, slices, `vec<T>`, floats
(including compound assignment), static initializers, modules, macros, function
pointers, volatile/atomic/`asm` paths, C FFI, labels/`goto`, and runtime
bounds/check aborts. All expected stdout and exit statuses are byte-for-byte
oracle output.

Composition: 39 positive cases × 4 opt levels = 156; 65 negative cases; 28
malformed inputs; 4 module projects (3 normal × 4 + 1 `must_fail` = 13); 1 FFI
project × 4 = 4.

Additional gates:

| Gate | Result |
|---|---|
| CLI parity | 80/80 |
| Differential token/AST/IR dumps | 79/79 |
| Fuzz/ASan inputs | 329/329 |
| Concurrency runtime parity | 33/33 |
| `lib/corex` modules | 16/16, both compilers |
| Rust reference tests | 205/205 |
| Example matrix vs oracle | 8/8 |
| `emit-ir --verify` / `llvm-as` | all builds pass |

Known IR signature divergences are in
`tests/selfhosting/dump-ir-expected.txt`; they are ABI differences (not
miscompilation) recorded from the differential oracle.

## 3. Frontend and IR

- `compiler/target/release/corex check selfhost/corexc.cx` → `ok: 487
  function(s), entry main`; the bootstrap compiler produces the same result.
- Every build invokes `llvm-as`; malformed IR aborts the build and `build`
  exits nonzero (exit 3 for tool failures) without printing `compiled`.
- A tool killed by a signal is reported as failure (`128 + signal`), not exit 0.
- `llc -filetype=obj -relocation-model=static` and `cc -nostdlib -static
  -no-pie` produce a static ELF.
- The module emits weak freestanding `memset`/`memcpy`/`memmove`/`memcmp` so
  LLVM loop-idiom synthesis links without libc.

## 4. Native execution

The 39 positive cases run at `--opt 0,1,2,3`. Runtime abort cases (array,
slice, `str`, `vec` bounds; division by zero; signed `MIN/-1`) exit 101 with
the expected message on Stage 1 and Stage 2. All 65 negative cases are
rejected with a diagnostic code that matches the Rust oracle: the original
52-case corpus has a `.code` for every case, four packed/union/bitfield/`align`
static cases assert E0226, and nine new atomic/volatile/`asm` cases assert
E0203/E0204/E0209/E0216. `tests/selfhosting/concurrency.sh` checks those nine
against both compilers.

## 5. Bootstrap validation

```text
stage1 926ecdec940f571bcabaae07194bf8688e07bb0e69f57ab554746e4da454571f
stage2 dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad
stage3 dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad
stage4 dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad
reproducible: stage2 == stage3 == stage4
```

`file build/stage2/corexc` → static x86-64 ELF; `ldd` → not a dynamic
executable. Repeated Stage 2 builds are byte-identical.

Optimized levels are deterministic and self-reproducing:

| Level | SHA-256 |
|---|---|
| `--opt 1` | `58e5c30d05631e49b9091a5416c165afda8c462e16ea31ecee11887d0654557f` |
| `--opt 2` | `d4e106cb35579458dd79b6f68632388e33f891500ade1d70a8d6bff31d81d270` |
| `--opt 3` | `ab469e2f1864c8a6ac29d1963b5891d7da256454a3f9ed3add3296f27ec724a2` |

The integrated performance table (731 ms best self-compile, 67 ms frontend,
6.98 MB/s closure throughput, 95,352/21,916 KiB peak RSS) is recorded once
in `bootstrap-results.md` §3.

## 6. Acceptance criteria

| ID | Status | Note |
|---|---|---|
| AC-1 corpus parity | PASS (scope) | 266-check corpus at four opt levels; 8/8 examples match the oracle; `lib/corex` 16/16 |
| AC-2 negative parity | PASS | 65/65 negative cases assert a code and match the Rust oracle |
| AC-3 IR validity | PASS | Every build is assembled; `emit-ir --verify` covers unreachable-label IR |
| AC-4 full 205-test behavior suite | BLOCKED | Concurrency behaviours are ported; remaining work is tooling/LSP and ABI edges |
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
- LSP/line-editor parity is not ported (SH-037).
- `-g` is rejected by contract (exit 2) because the bootstrap emits no DWARF.
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

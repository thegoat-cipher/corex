# Validation Report — CoreX Self-Hosting

Date: 2026-10-28
Compiler under test: integrated `selfhost/` after the P0–P3 audit fixes
(474 functions, 15,228 lines).

## 1. Result summary

| Gate | Result | Evidence |
|---|---|---|
| Stage 0 builds Stage 1 | PASS | Stage 1 `f4192f40…`; runs the corpus |
| Stage 1 builds Stage 2 | PASS | Stage 2 `c33aff55…` |
| Stage 2 builds Stage 3 | PASS | Stage 3 identical to Stage 2 |
| Stage 3 builds Stage 4 | PASS | Stage 4 identical |
| Stage 2/3 compile user programs | PASS | 249 checks per stage; outputs byte-identical |
| Compiler behavior across stages | PASS | same corpus on Stage 1/2/3 |
| Artifact reproducibility | PASS | `sha256(stage2) == sha256(stage3) == sha256(stage4)` |
| Optimized reproducibility | PASS | `repro-opt.sh --corpus`, `--opt 1..3` |
| Rust suite unaffected | PASS | 205/205 `cargo test` |

## 2. Behavioural corpus

`selfhost/bootstrap.sh` runs 249 checks against Stage 1, Stage 2, and Stage 3.
The corpus covers arithmetic, aggregates, pointers, slices, `vec<T>`, floats
(including compound assignment), static initializers, modules, macros, function
pointers, C FFI, labels/`goto`, and runtime bounds/check aborts. All expected
stdout and exit statuses are byte-for-byte oracle output.

Composition: 37 positive cases × 4 opt levels = 148; 56 negative cases; 28
malformed inputs; 4 module projects (3 normal × 4 + 1 `must_fail` = 13); 1 FFI
project × 4 = 4.

Additional gates:

| Gate | Result |
|---|---|
| CLI parity | 80/80 |
| Differential token/AST/IR dumps | 75/75 |
| Fuzz/ASan inputs | 327/327 |
| Rust reference tests | 205/205 |
| Example matrix vs oracle | 8/8 |
| `lib/corex` modules | 13/16 (SH-033 blocks `std_sync`, `std_tls`, `std_thread`) |
| `emit-ir --verify` / `llvm-as` | all builds pass |

Known IR signature divergences are in
`tests/selfhosting/dump-ir-expected.txt`; they are ABI differences (not
miscompilation) recorded from the differential oracle.

## 3. Frontend and IR

- `compiler/target/release/corex check selfhost/corexc.cx` → `ok: 474
  function(s), entry main`; the bootstrap compiler produces the same result.
- Every build invokes `llvm-as`; malformed IR aborts the build and `build`
  exits nonzero (exit 3 for tool failures) without printing `compiled`.
- A tool killed by a signal is reported as failure (`128 + signal`), not exit 0.
- `llc -filetype=obj -relocation-model=static` and `cc -nostdlib -static
  -no-pie` produce a static ELF.
- The module emits weak freestanding `memset`/`memcpy`/`memmove`/`memcmp` so
  LLVM loop-idiom synthesis links without libc.

## 4. Native execution

The 37 positive cases run at `--opt 0,1,2,3`. Runtime abort cases (array,
slice, `str`, `vec` bounds; division by zero; signed `MIN/-1`) exit 101 with
the expected message on Stage 1 and Stage 2. All 56 negative cases are
rejected with a diagnostic code that matches the Rust oracle: the original
52-case corpus now has a `.code` for every case (30 existing files plus 22
generated from the oracle), and four new packed/union/bitfield/`align` static
cases assert E0226.

## 5. Bootstrap validation

```text
stage1 f4192f40a4571bdfec5f4514bf035a1246486f80ad94fa54e2fedf79a17af884
stage2 c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78
stage3 c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78
stage4 c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78
reproducible: stage2 == stage3 == stage4
```

`file build/stage2/corexc` → static x86-64 ELF; `ldd` → not a dynamic
executable. Repeated Stage 2 builds are byte-identical.

Optimized levels are deterministic and self-reproducing:

| Level | SHA-256 |
|---|---|
| `--opt 1` | `0f6bd2355968070f61c4097e966effd36ba7953415b801cb71bcfc3a0a3db81d` |
| `--opt 2` | `eda405ad9c109c673343d4885c9952ec475a48cb1ad1f4047a1cd4139b1a0981` |
| `--opt 3` | `58067778faf2bfe10ca6ccc0b37523063e199a8558e0dabd681eeb98b629ed25` |

The integrated performance table (708 ms best self-compile, 64 ms frontend,
7.04 MB/s closure throughput, 94,008/21,184 KiB peak RSS) is recorded once
in `bootstrap-results.md` §3.

## 6. Acceptance criteria

| ID | Status | Note |
|---|---|---|
| AC-1 corpus parity | PASS (scope) | 249-check corpus at four opt levels; 8/8 examples match the oracle; `lib/corex` 13/16, limited by SH-033 |
| AC-2 negative parity | PASS | 56/56 negative cases assert a code and match the Rust oracle |
| AC-3 IR validity | PASS | Every build is assembled; `emit-ir --verify` covers unreachable-label IR |
| AC-4 full 205-test behavior suite | BLOCKED | Depends on SH-033 and remaining tooling/LSP gaps |
| AC-5 modules/FFI | PASS (scope) | Module projects, archives/shared objects, C both directions |
| AC-6 determinism | PASS | Twin builds byte-identical |
| AC-7 fixed point | PASS | Stage 2 == Stage 3 == Stage 4 |
| AC-8 performance | PASS | Integrated table in `bootstrap-results.md` §3 |
| AC-9 no regression | PASS | Rust `corex` unchanged; 205/205 |

## 7. Known limitations

- Threads, atomics, `volatile`, TLS, and inline asm are not implemented
  (SH-033); this blocks `std_sync`, `std_tls`, and `std_thread`.
- `lib/corex` builds 13/16 modules; `std_math` is unblocked by float compound
  assignment, the remaining three need SH-033.
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

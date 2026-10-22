# CoreX Benchmarks — v0.2

Purpose: validate (or refute) performance claims with measurements. Every
number here is produced by `scripts/bench.sh` on the machine below; re-run it
before trusting a claim.

## Environment

| Item | Value |
|---|---|
| Date | 2026-10-22 |
| CPU | AMD Ryzen 7 6800H with Radeon Graphics (16 threads visible) |
| OS | Linux 6.17.0 x86-64 |
| C compiler | gcc 15.2.0 (`-O3`) |
| LLVM | 20.1.8 (`opt -passes=default<O3>`, `llc -O3`) |
| Rust | 1.99.1 (compiler itself) |
| Link flags | `-nostdlib -static -no-pie` (CoreX), default (C) |
| Runs | 7 per binary, wall clock, best and median of milliseconds |

Both sides are compiled from equivalent source, and the harness checks that the
exit statuses agree before timing.

## Kernels

| Kernel | Source | What it exercises |
|---|---|---|
| `mix` | `bench/mix.cx`, `bench/mix.c` | 10^9 iterations of a data-dependent 64-bit recurrence (multiply, xor-shift, add). N comes from stdin, so it cannot be folded. |
| `fib` | `bench/fib.cx`, `bench/fib.c` | `fib(35)` naive double recursion: call/return, frames, no closed form. |
| `array` | `bench/array.cx`, `bench/array.c` | 4M-element `i64` heap array: fill through a `*mut i64`, then sum through it. Tests pointer arithmetic and loop codegen. |
| `struct` | `bench/struct.cx`, `bench/struct.c` | 2M-element array of 24-byte structs: field stores, read-modify-write, and struct strides. |
| `f64` | `bench/f64.cx`, `bench/f64.c` | 2M-element `f64` multiply-add over two heap arrays; LLVM/GCC can vectorize it. |

## Results (median, milliseconds)

| Kernel | CoreX `--opt 3` | C `gcc -O3` | Ratio (CoreX / C) |
|---|---:|---:|---:|
| `mix` (10^9) | 1587 | 1561 | **1.017** |
| `fib` (fib 35) | 18 | 11 | **1.636** |
| `array` (4M i64) | 19 | 18 | **1.056** |
| `struct` (2M) | 27 | 28 | **0.964** |
| `f64` (2M) | 18 | 19 | **0.947** |

Interpretation:

- Arithmetic, pointer-heavy, struct, and floating-point kernels are at parity
  with C (0.95–1.06×). The generated code has no runtime layer: every CoreX
  program starts at `_start`, uses direct syscalls, and links statically.
- `fib` remains ~60% slower. The IR is valid and optimized, but LLVM's inliner
  and frame layout for leaf-recursive functions do not match GCC's here. This is
  a backend code-quality gap, not a language runtime cost.

## Compiler throughput

Synthetic unit: 2,000 functions plus a `main` that calls all of them
(4,001 lines). Release build of the compiler, 5 runs:

| Measurement | Median |
|---|---:|
| `corex check` (lex, parse, semantic) | 22 ms (≈ 180k lines/s) |
| `corex build --opt 2` (frontend + LLVM + link) | 798 ms (≈ 5.0k lines/s) |

Full builds are dominated by `llvm-as`/`opt`/`llc` subprocesses. The frontend
itself is allocation-light and has no dependency on LLVM until code generation.

## Methodology notes and caveats

1. Wall-clock timing includes process startup (~1 ms) and is quantized to 1 ms.
2. 5–7 runs are enough to see large regressions, not enough for precise
   micro-architecture conclusions. Treat ratios as ±5%.
3. The compiler still uses textual LLVM IR and subprocess tools, so full-build
   throughput includes three process launches per compilation unit.
4. Separate compilation multiplies process launches per module; a manifest build
   of N modules costs roughly N× the single-unit pipeline.
5. All benchmarks use `--opt 3`; the test suite also runs every example at
   `--opt 0..3` to prove semantic identity across levels.

## Reproducing

```sh
cd compiler && cargo build --release
cd .. && scripts/bench.sh 7
```

The script prints compiler/LLVM/cc versions, the CPU, and a ratio for each
kernel; it fails if the CoreX and C binaries disagree on the exit status.

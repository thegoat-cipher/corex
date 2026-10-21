# CoreX Benchmarks — v0.1

Purpose: validate (or refute) performance claims with measurements. This file
records one run; the harness is reproducible with `scripts/bench.sh`.

## Environment

| Item | Value |
|---|---|
| Date | 2026-10-21 |
| CPU | AMD Ryzen 7 6800H with Radeon Graphics |
| OS | Linux 6.17.0 x86-64 |
| C compiler | gcc 15.2.0 (`-O2`) |
| LLVM | 20.1.8 (`llc -O2` after `opt -passes=default<O2>`) |
| Rust | 1.99.1 (compiler itself, irrelevant to runtime) |
| Link flags | `-nostdlib -static -no-pie` (CoreX), default (C) |
| Runs | 5 per binary, wall clock, best and median of milliseconds |

## Kernels

| Kernel | Source | What it exercises |
|---|---|---|
| `mix` | `bench/mix.cx`, `bench/mix.c` | 10^9 iterations of a data-dependent 64-bit recurrence (multiply, xor-shift, add). Iteration count is read from stdin, so it is unknown at compile time; the recurrence is non-affine, so the optimizer cannot close-form it. |
| `fib` | `bench/fib.cx`, `bench/fib.c` | `fib(35)` by naive double recursion: call/return overhead, stack frames, no inlining across 35 levels. |

Both programs are compiled from source with equivalent semantics, and the
harness asserts that both binaries return the same exit status before timing.

## Results

### mix (10^9 iterations)

| Binary | Best (ms) | Median (ms) |
|---|---|---|
| CoreX `--opt 2` | 1586 | 1590 |
| C `gcc -O2` | 1588 | 1594 |

**Ratio (median, CoreX / C): 0.997** — parity within run-to-run noise. The loop
is arithmetic-bound; both backends emit comparable scalar code and neither can
vectorize the dependency chain.

### fib (fib(35))

| Binary | Best (ms) | Median (ms) |
|---|---|---|
| CoreX `--opt 2` | 21 | 22 |
| C `gcc -O2` | 14 | 15 |

**Ratio (median, CoreX / C): 1.467** — CoreX is ~47% slower on this kernel.
A later run of the same harness on the same machine measured 0.997 (`mix`) and
1.500 (`fib`); treat ratios as reproducible within roughly 5% run-to-run, not as
exact values.
Observed cause: our functions are emitted with one `alloca` per parameter and
local plus a load/store bridge, and LLVM's inliner heuristics treat these
small functions differently from GCC's. This is a **backend code-quality gap,
not a language-level runtime overhead** (there is no runtime to pay for).

## Methodology notes and caveats

1. Wall-clock timing includes process startup (~1 ms) and is quantized to 1 ms
   by the shell harness; treat single-digit differences as noise.
2. No statistical confidence intervals are reported. Numbers are one run on one
   machine; they are evidence, not a general performance claim.
3. LLVM optimizations can change the algorithm being measured. An earlier
   version of this benchmark summed `i*i` with a compile-time bound; LLVM's
   `default<O2>` pipeline closed-formed it and the loop disappeared (3 ms vs
   232 ms for GCC). The kernels above were chosen specifically to resist that.
   Any new benchmark must assert that the loop is actually present (or compare
   equivalent binaries) before its numbers are reported.
4. CoreX's `--opt 2` means `opt -passes=default<O2>` followed by `llc -O2`
   (ADR-0012). Without the `opt` stage, the same loop ran at ~2.5× the C time,
   because `llc` alone does not run the IR optimizer (ADR-0012 records this).
5. `fib` is recursive and stack-heavy; `mix` is a tight loop. Neither measures
   I/O, allocation, or syscall overhead, which are dominated by the kernel.

## How to reproduce

```
source environment with LLVM 20 tools on PATH (or set COREX_LLVM_BIN)
cd compiler && cargo build --release
cd .. && ./scripts/bench.sh 5
```

# Bootstrap Results

Date: 2026-10-28
Command: `selfhost/bootstrap.sh` on the integrated compiler (after the P0–P3
audit fixes).

## 1. Environment

| Item | Value |
|---|---|
| Host | Linux 6.17.0-41-generic x86-64 |
| Rust | rustc 1.99.1 (58cf8f9c3 2026-10-13) |
| LLVM | LLVM 20.1.8 (`llvm-as`, `llc`, `opt`) |
| C driver | cc (Ubuntu 15.2.0-4ubuntu4) 15.2.0 |
| Target | `x86_64-unknown-linux-gnu` |
| Stage 0 | `corex 0.3.0 (LLVM 20 backend)` |
| Link flags | `cc -nostdlib -static -no-pie` |

## 2. Artifacts and hashes

| Stage | SHA-256 | Size |
|---|---|---:|
| Stage 1 (Rust-built from CoreX source) | `f4192f40a4571bdfec5f4514bf035a1246486f80ad94fa54e2fedf79a17af884` | 489,088 bytes |
| Stage 2 (Stage 1-built) | `c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78` | 548,016 bytes |
| Stage 3 (Stage 2-built) | `c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78` | 548,016 bytes |
| Stage 4 (Stage 3-built) | `c33aff55ae3c74ab87358b4db50062a548fed238b7cae4cd9e7ac8f202eb5f78` | 548,016 bytes |

`stage2 == stage3 == stage4` byte-for-byte. Stage 1 differs because it is
compiled by the Rust backend (Rust ABI/runtime helper spellings), while
Stage 2/3/4 are compiled by the self-hosted backend. The fixed point is
Stage 2 → Stage 3.

```text
file build/stage2/corexc
ELF 64-bit LSB executable, x86-64, statically linked, not stripped
Entry point address: 0x401000

ldd build/stage2/corexc
not a dynamic executable
```

## 3. Timings and performance

Stage builds (`--opt 0`, wall time, final run):

| Step | Wall time |
|---|---:|
| Stage 1 (Rust Stage 0 compiles CoreX source) | 0.604 s |
| Stage 2 (Stage 1 compiles itself) | 0.727 s |
| Stage 3 (Stage 2 compiles itself) | 0.722 s |
| Stage 4 (Stage 3 compiles itself) | 0.752 s |

Integrated performance table. This is the only performance baseline cited by
the self-hosting docs. Numbers are best-of-N wall time on the host above
(2026-10-28, LLVM 20.1.8, cc 15.2.0); self-compile varies with host load, so
the observed range across final runs is shown.

| Metric | Observed |
|---|---|
| Compiler source closure | 15,228 lines, 472,690 bytes (`selfhost/*.cx`) |
| Self-compile `build --opt 0` (best of 3) | **708 ms** (repeated runs vary with host load) |
| Frontend `check` of the compiler source (best of 5) | **64 ms** |
| Frontend corpus (37 files, 24,780 bytes) | 32 ms (0.74 MB/s) |
| Frontend throughput (immutable closure) | **7.04 MB/s** |
| Peak RSS, self-compile | 94,008 KiB (≈91.8 MiB) |
| Peak RSS, frontend `check` | 21,184 KiB (≈20.7 MiB) |

`tests/selfhosting/perf.sh` applies only a 60 s smoke bound; the numbers above
are observations, not a gate.

## 4. Behavioural corpus

```text
selfhosting tests: 249 passed, 0 failed (Stage 1)
selfhosting tests: 249 passed, 0 failed (Stage 2)
selfhosting tests: 249 passed, 0 failed (Stage 3)
```

Composition: 37 positive cases × 4 opt levels = 148; 56 negative cases (each
asserts a `.code` that matches the Rust oracle); 28 malformed inputs; 4 module
projects (3 normal × 4 + 1 `must_fail` = 13); 1 FFI project × 4 = 4; total 249.
Expected stdout/exit values were produced by the Rust oracle and are checked
byte-for-byte.

Additional gates:

```text
cli tests: 80 passed, 0 failed (Stage 1)
cli tests: 80 passed, 0 failed (Stage 2)
dump tests: 75 passed, 0 failed (Stage 2)
fuzz tests: 327 passed, 0 failed (Stage 2)
ASan: enabled (60 mutated inputs + corpus)
```

CLI coverage now includes build-failure status, signal-killed tools, built-in
cfg selection, `check`/`emit-ir` frontend options, stdin builds, dead-label IR
verification, and hosted float `%=`. Fuzz/ASan is 215 unsanitized checks
(13 checked-in fuzz inputs, 2 generated, 200 mutants) plus 112 ASan checks
(2 self-host, 37 positive, 13 fuzz, 60 mutants). CI sets
`COREX_REQUIRE_ASAN=1`, so a skipped sanitizer run fails the gate; locally a
missing ASan runtime is reported and skipped.

The example matrix is **8/8** against the oracle (stdout and exit status,
`--opt 0`), and `lib/corex` builds **13/16** modules; `std_sync`, `std_tls`, and
`std_thread` remain blocked by SH-033. The Rust reference suite remains
**205/205**.

## 5. Optimized reproducibility

`selfhost/repro-opt.sh --corpus` builds the compiler twice at each optimization
level and checks determinism, the self-fixed point, and Stage 1/Stage 2
agreement. All three levels pass, and the 249-check corpus passes with each
optimized compiler:

| Level | SHA-256 | Size |
|---|---|---:|
| `--opt 1` | `0f6bd2355968070f61c4097e966effd36ba7953415b801cb71bcfc3a0a3db81d` | 424,416 bytes |
| `--opt 2` | `eda405ad9c109c673343d4885c9952ec475a48cb1ad1f4047a1cd4139b1a0981` | 472,120 bytes |
| `--opt 3` | `58067778faf2bfe10ca6ccc0b37523063e199a8558e0dabd681eeb98b629ed25` | 876,072 bytes |

Hashes differ *across* levels by design; within each level,
`repeat == self == fixed == stage1 oracle`.

## 6. Reproduce

```sh
cd /path/to/corex
export PATH="$HOME/opt/llvm20/bin:$PATH"   # or set COREX_LLVM_BIN
selfhost/bootstrap.sh
selfhost/repro-opt.sh --corpus
```

`bootstrap.sh` rebuilds Stage 0 if missing, builds Stage 1–4, checks hashes, and
runs the corpus, CLI, dump, fuzz, and performance gates. It exits non-zero on a
bad stage or a failing gate. CI additionally exports `COREX_REQUIRE_ASAN=1`.

## 7. Notes

- The compiler is freestanding: it uses Linux syscalls and external LLVM/cc
  tools at compile time.
- The source is 15,228 lines across 14 files and 474 functions.
- Known gaps are listed in `master.md` §9, `bootstrap-architecture.md` §10, and
  `tasks.md`: threads/atomics/volatile/TLS/asm, LSP/editor parity, DWARF
  (`-g`), full 205-test parity, and `lib/corex` completion (13/16).

# Bootstrap Results

Date: 2026-10-29
Command: `selfhost/bootstrap.sh` on the integrated compiler with the SH-033
concurrency surface and SH-034 library closure.

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
| Stage 1 (Rust-built from CoreX source) | `926ecdec940f571bcabaae07194bf8688e07bb0e69f57ab554746e4da454571f` | 510,960 bytes |
| Stage 2 (Stage 1-built) | `dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad` | 575,128 bytes |
| Stage 3 (Stage 2-built) | `dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad` | 575,128 bytes |
| Stage 4 (Stage 3-built) | `dcccb0e017843e6d5dc6f2ca4d488cb74427a33c814f2639894c48633e7361ad` | 575,128 bytes |

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
| Stage 1 (Rust Stage 0 compiles CoreX source) | 0.567 s |
| Stage 2 (Stage 1 compiles itself) | 0.725 s |
| Stage 3 (Stage 2 compiles itself) | 0.717 s |
| Stage 4 (Stage 3 compiles itself) | 0.719 s |

Integrated performance table. This is the only performance baseline cited by
the self-hosting docs. Numbers are best-of-N wall time on the host above
(2026-10-29, LLVM 20.1.8, cc 15.2.0); self-compile varies with host load, so
the observed range across final runs is shown.

| Metric | Observed |
|---|---|
| Compiler source closure | 17,236 lines, 541,991 bytes (`selfhost/*.cx`) |
| Self-compile `build --opt 0` (best of 3) | **835 ms** (repeated runs vary with host load) |
| Frontend `check` of the compiler source (best of 5) | **75 ms** |
| Frontend corpus (40 files, 27,620 bytes) | 29 ms (0.91 MB/s) |
| Frontend throughput (immutable closure) | **6.89 MB/s** |
| Peak RSS, self-compile | 98,880 KiB (≈96.6 MiB) |
| Peak RSS, frontend `check` | 23,940 KiB (≈23.4 MiB) |

`tests/selfhosting/perf.sh` applies only a 60 s smoke bound; the numbers above
are observations, not a gate.

## 4. Behavioural corpus

```text
selfhosting tests: 270 passed, 0 failed (Stage 1)
selfhosting tests: 270 passed, 0 failed (Stage 2)
selfhosting tests: 270 passed, 0 failed (Stage 3)
```

Composition: 40 positive cases × 4 opt levels = 160 (including the 64-term
`pointer_chain.cx` regression); 65 negative cases (each asserts a `.code` that
matches the Rust oracle); 28 malformed inputs; 4 module projects
(3 normal × 4 + 1 `must_fail` = 13); 1 FFI project × 4 = 4; total 270.
Expected stdout/exit values were produced by the Rust oracle and are checked
byte-for-byte. Positive `check`/`build` and negative `check` run under
`timeout ${COREX_CASE_TIMEOUT:-30}`, so a checker regression fails instead of
hanging the gate.

Additional gates:

```text
cli tests: 80 passed, 0 failed (Stage 1)
cli tests: 80 passed, 0 failed (Stage 2)
lsp tests: 35 passed, 0 failed (Stage 2)
dump tests: 81 passed, 0 failed (Stage 2)
fuzz tests: 330 passed, 0 failed (Stage 2)
ASan: enabled (60 mutated inputs + corpus)
concurrency tests: 33 passed, 0 failed (Stage 2)
lib tests: 16 passed, 0 failed (Stage 2)
```

CLI coverage now includes build-failure status, signal-killed tools, built-in
cfg selection, `check`/`emit-ir` frontend options, stdin builds, dead-label IR
verification, and hosted float `%=`. Fuzz/ASan is 215 unsanitized checks
(13 checked-in fuzz inputs, 2 generated, 200 mutants) plus 115 ASan checks
(2 self-host, 40 positive, 13 fuzz, 60 mutants). CI sets
`COREX_REQUIRE_ASAN=1`, so a skipped sanitizer run fails the gate; locally a
missing ASan runtime is reported and skipped.

`concurrency.sh` ports the three `compiler/tests/concurrency.rs` behaviours
(atomics/volatile/asm, TLS, threads + futex mutex) and runs the bootstrap at
`--opt 0..3` plus the oracle at `--opt 2`. `lib.sh` builds every `lib/corex`
module with both compilers.

The example matrix is **8/8** against the oracle (stdout and exit status,
`--opt 0`), and `lib/corex` builds **16/16** modules. The Rust reference suite
remains **205/205**.

## 5. Optimized reproducibility

`selfhost/repro-opt.sh --corpus` builds the compiler twice at each optimization
level and checks determinism, the self-fixed point, and Stage 1/Stage 2
agreement. All three levels pass, and the 270-check corpus passes with each
optimized compiler:

| Level | SHA-256 | Size |
|---|---|---:|
| `--opt 1` | `e3e6335e32182abcd258a104b8693571130e00493d7ef5a41adbda85ababd313` | 504,544 bytes |
| `--opt 2` | `9af3b41a132a0c1cc698bd7d5445a48041c5b49215b2910ad84063ebdecedd84` | 563,736 bytes |
| `--opt 3` | `51c025a044009057505d4baa7cfa8b5882c3193d75f2f494ba015ee2528baa02` | 906,440 bytes |

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
- The source is 17,236 lines across 15 files and 534 functions.
- Known gaps are listed in `master.md` §9, `bootstrap-architecture.md` §10, and
  `tasks.md`: DWARF (`-g`) and full 205-test parity (SH-039; see
  `worklogs/semantic-hang.md`). The LSP is implemented and gated (35/35).
  `lib/corex` is complete (16/16).

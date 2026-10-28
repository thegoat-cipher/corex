# Bootstrap Results

Date: 2026-10-28
Command: `selfhost/bootstrap.sh` on the integrated compiler.

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
| Stage 1 (Rust-built from CoreX source) | `ccffc9721da61cd5cf880a1616e01f7c762fba53939e7618c29b299321447032` | 484,184 bytes |
| Stage 2 (Stage 1-built) | `d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20` | 542,128 bytes |
| Stage 3 (Stage 2-built) | `d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20` | 542,128 bytes |
| Stage 4 (Stage 3-built) | `d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20` | 542,128 bytes |

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

## 3. Timings

| Step | Wall time |
|---|---:|
| Stage 1 (Rust Stage 0 compiles CoreX source) | 0.574 s |
| Stage 2 (Stage 1 compiles itself) | 0.720 s |
| Stage 3 (Stage 2 compiles itself) | 0.732 s |
| Stage 4 (Stage 3 compiles itself) | 0.711 s |

Performance baseline (`tests/selfhosting/perf.sh`):

```text
self-compile --opt 0 (best of 3): 695 ms
frontend check compiler source (best of 5): 63 ms
frontend corpus: 36 files, 24441 bytes in 29 ms (0.80 MB/s)
frontend throughput (immutable closure): 7.07 MB/s
peak RSS self-compile: 94120 KiB
peak RSS frontend check: 20608 KiB
```

## 4. Behavioural corpus

```text
selfhosting tests: 241 passed, 0 failed (Stage 1)
selfhosting tests: 241 passed, 0 failed (Stage 2)
selfhosting tests: 241 passed, 0 failed (Stage 3)
```

The corpus includes 36 positive cases at `--opt 0..3`, 52 negative cases,
4 multi-unit module projects, FFI cases, and runtime abort cases. Expected
stdout/exit values were produced by the Rust oracle and are checked byte-for-byte.

CLI parity: **62/62**. Differential dumps: **73/73**. Fuzzing/ASan: **326/326**.
The Rust reference suite remains **205/205**.

## 5. Optimized reproducibility

`selfhost/repro-opt.sh --corpus` builds the compiler twice at each optimization
level and checks determinism, the self-fixed point, and Stage 1/Stage 2
agreement. All three levels pass, and the 241-check corpus passes with each
optimized compiler:

| Level | SHA-256 | Size |
|---|---|---:|
| `--opt 1` | `a5a097167d9f1e83f217384ef134792cc2bc7eff569eb9fd0a6e6b868808519b` | 422,968 bytes |
| `--opt 2` | `e076f652dbbd03ec7357bb18e20b71d26e194c9dd14507673ee6e9813a7e1aac` | 470,720 bytes |
| `--opt 3` | `76445b562f9bf982b410af38b41992722ae112df4999c4bd1d4549564b992b36` | 874,760 bytes |

Hashes differ *across* levels by design; within each level,
`repeat == self == fixed == oracle`.

## 6. Reproduce

```sh
cd /path/to/corex
export PATH="$HOME/opt/llvm20/bin:$PATH"   # or set COREX_LLVM_BIN
selfhost/bootstrap.sh
selfhost/repro-opt.sh --corpus
```

`bootstrap.sh` rebuilds Stage 0 if missing, builds Stage 1–4, checks hashes, and
runs the corpus, CLI, dump, fuzz, and performance gates. It exits non-zero on a
bad stage or a failing gate.

## 7. Notes

- The compiler is freestanding: it uses Linux syscalls and external LLVM/cc
  tools at compile time.
- The source is 15,065 lines across 14 files and 469 functions.
- Known gaps are listed in `master.md` §9 and `tasks.md`: threads/atomics/
  volatile/TLS/asm, LSP/editor parity, DWARF (`-g`), full 205-test parity, and
  `lib/corex` completion.

# Bootstrap Results

Date: 2026-10-28
Command: `selfhost/bootstrap.sh`

## 1. Environment

| Item | Value |
|---|---|
| Host | `commandcode/deepseek/deepseek-v4.1-flash` / Linux 6.17.0-41-generic x86-64 |
| Rust | rustc 1.99.1 (58cf8f9c3 2026-10-13) |
| LLVM | Ubuntu LLVM 20.1.8 (`llvm-as`, `llc`, `opt`) |
| C driver | cc (Ubuntu 15.2.0-4ubuntu4) 15.2.0 |
| Target | `x86_64-unknown-linux-gnu` |
| Stage 0 | `corex 0.3.0 (LLVM 20 backend)` |
| Link flags | `cc -nostdlib -static -no-pie` |

## 2. Artifacts and hashes

Final run:

| Stage | SHA-256 | Size |
|---|---|---:|
| Stage 1 (Rust-built from CoreX source) | `7b2f8837093d0dd5b6a4e4bce01c2e5dce2d908b7dfdd843fd6e2354df989601` | 222,144 bytes |
| Stage 2 (Stage 1-built) | `644c22c5bd3772b8a71365798731c52387b7c9349385030b62a573489244c7a8` | 253,432 bytes |
| Stage 3 (Stage 2-built) | `644c22c5bd3772b8a71365798731c52387b7c9349385030b62a573489244c7a8` | 253,432 bytes |
| Stage 4 (Stage 3-built) | `644c22c5bd3772b8a71365798731c52387b7c9349385030b62a573489244c7a8` | 253,432 bytes |

`stage2 == stage3 == stage4` byte-for-byte. A repeated Stage 2 build also
produced `644c22c5…`. Stage 1 differs from Stage 2 because Stage 1 is compiled by
the Rust backend (Rust internal ABI, Rust runtime helper spellings), while
Stage 2/3/4 are compiled by the self-hosted backend (first-class aggregate ABI).
That difference is expected and is not a reproducibility failure; the fixed
point is Stage 2 → Stage 3.

`file build/stage2/corexc`:

```text
ELF 64-bit LSB executable, x86-64, statically linked, not stripped
Entry point address: 0x401000
```

`ldd build/stage2/corexc` → `not a dynamic executable`.

## 3. Timings

| Step | Wall time |
|---|---:|
| Stage 1 (Rust Stage 0 compiles CoreX source) | 0.272 s |
| Stage 2 (Stage 1 compiles itself) | 0.342 s |
| Stage 3 (Stage 2 compiles itself) | 0.347 s |
| Stage 4 (Stage 3 compiles itself) | 0.337 s |
| Full bootstrap including three corpus runs | 15.8 s |

## 4. Behavioural corpus

```text
selfhosting tests: 79 passed, 0 failed (compiler: build/stage1/corexc)
selfhosting tests: 79 passed, 0 failed (compiler: build/stage2/corexc)
selfhosting tests: 79 passed, 0 failed (compiler: build/stage3/corexc)
```

79 = 16 positive cases × `--opt 0,1,2,3` (64) + 15 negative cases.

Positive cases: `arith`, `arrays`, `bounds_array`, `bounds_slice`, `bounds_str`,
`bounds_vec`, `control`, `enums`, `fib`, `globals`, `pointers`, `slices`,
`strings`, `structs`, `vec`, `vec_elements`. The expected stdout/exit values
were produced by the Rust compiler (`corex run`) and are checked byte-for-byte.

Negative cases: `syntax`, `undefined`, `types`, `args`, `return`, `immutable`,
`index_literal`, `slice_immutable`, `slice_mix`, `slice_scalar`,
`vec_element_type`, `vec_global`, `vec_immutable`, `vec_slice_ref`,
`vec_untyped`. All are rejected by `check` with a non-empty diagnostic.

## 5. Stage-by-stage behavior

- Stage 1 can `check`, `emit-ir`, and `build` the corpus at all four
  optimization levels. Its binaries behave identically to Stage 2 binaries even
  though the machine code differs.
- Stage 2 accepts its own source (`selfhost/corexc.cx`) with zero diagnostics
  and builds Stage 3.
- Stage 3 repeats the result and builds Stage 4 with the same hash.
- `emit-ir` output for `tests/selfhosting/cases/positive/enums.cx` is accepted
  by `llvm-as` (`IR_VERIFY_OK`); every `build` in the harness also runs
  `llvm-as`.
- Slice support: `[T]` values, `&array`/`&mut array` coercion, params/returns,
  indexing, range slicing, `[u8]` print, and dynamic range bounds checks. The
  `slices` case covers all of it at `--opt 0..3`; a runtime `s[2..5]` on length 3
  aborts with `corex: slice range out of bounds`, exit 101.
- Element indexing on arrays, slices, and `str` is bounds-checked (`corex: index
  out of bounds`, exit 101); constant array indices out of range are rejected
  by semantic analysis. `str` and `[u8]` are compatible per spec §3.3.
- `vec<T>`: `{ptr,len,cap}` layout, `vec_new`/`vec_push`/`vec_free`/`vec_slice`,
  growth `max(4, 2*cap)`, len/indexing, nested `vec<vec<T>>`, `vec<str>`,
  `vec<[T]>`, `vec<u8>` print via `vec_slice`, and bounds-checked indexing.
- Optimized self-compile: `stage2 build selfhost/corexc.cx --opt 2` succeeds,
  the resulting compiler checks the source, and it builds the corpus. This is
  why the module emits weak freestanding `memset`/`memcpy`/`memmove`/`memcmp`
  definitions; LLVM's loop-idiom pass can synthesize them at `--opt 1..3`.

## 6. Reproduce

```sh
cd /path/to/corex
export PATH="$HOME/opt/llvm20/bin:$PATH"   # or set COREX_LLVM_BIN
selfhost/bootstrap.sh
```

The script rebuilds Stage 0 if missing, then Stage 1–4, checks hashes, and runs
the corpus against Stage 1–3. It exits non-zero if any stage fails or if
Stage 2/3/4 hashes differ.

## 7. Notes

- The first successful fixed point was reached before the type-model extraction
  (`selfhost/types.cx`); the final hashes above are after that refactor. All
  results were re-run after the refactor.
- The bootstrap source is 7,263 lines of CoreX across 9 files.
- The compiler binary is freestanding; it uses only Linux syscalls and external
  LLVM/cc tools at compile time.

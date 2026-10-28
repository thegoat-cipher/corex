# Validation Report — CoreX Self-Hosting

Date: 2026-10-28
Compiler under test: `build/stage1/corexc`, `build/stage2/corexc`,
`build/stage3/corexc` (all built back to back by `selfhost/bootstrap.sh`).

## 1. Result summary

| Gate | Result | Evidence |
|---|---|---|
| Stage 0 builds Stage 1 | PASS | Stage 1 hash `7b2f8837…`, runs corpus |
| Stage 1 builds Stage 2 | PASS | Stage 2 hash `644c22c5…` |
| Stage 2 builds Stage 3 | PASS | Stage 3 hash identical to Stage 2 |
| Stage 3 builds Stage 4 | PASS | Stage 4 hash identical |
| Stage 2 compiles user programs | PASS | 64 positive runs, all outputs match |
| Stage 3 compiles user programs | PASS | 64 positive runs, all outputs match |
| Compiler behavior consistent across stages | PASS | same corpus on Stage 1/2/3 |
| Artifact reproducibility | PASS | `sha256(stage2) == sha256(stage3) == sha256(stage4)` |
| Rust suite unaffected | PASS | 205/205 `cargo test` (baseline re-run) |

## 2. Layered tests

### Level 1 — unit/integration behavior of compiler components

The bootstrap compiler has no separate unit-test binary; component behavior is
exercised through the frontend and codegen corpus, plus the structural checks
below. The Rust compiler's own 205 tests remain the reference suite and still
pass.

### Level 2 — frontend parity

- `selfhost/corexc.cx` (all 9 files, 7,721 lines, 244 functions) parses and checks with **0
  diagnostics** on Stage 1, Stage 2, and Stage 3.
- The Rust compiler parses/checks the same source with 0 diagnostics.
- `check` on every positive corpus case succeeds; every negative case fails with
  a non-empty diagnostic.

### Level 3 — IR validation

- Every `build` invokes `llvm-as`; a malformed module aborts the build.
- Explicit check: `corexc emit-ir tests/selfhosting/cases/positive/enums.cx -o
  /tmp/enums.ll && llvm-as /tmp/enums.ll -o /tmp/enums.bc` → success
  (`IR_VERIFY_OK`).
- `llc -filetype=obj -relocation-model=static` and `cc -nostdlib -static
  -no-pie` produce a static ELF.

### Level 4 — native execution

Positive corpus (each at `--opt 0,1,2,3`):

| Case | Coverage | Expected |
|---|---|---|
| `arith` | wrapping, signed division/remainder, shifts, widths, comparisons, logical ops | 14 lines, exit 0 |
| `arrays` | arrays, array fields, arrays of structs, repeat literals, mutation, `len` | 8 lines |
| `control` | `while`, `continue`, `break`, `loop`, `switch` fallthrough/default | 6 lines |
| `enums` | implicit/explicit/negative discriminants, `switch`, casts | 8 lines |
| `fib` | recursion, loops, accumulation | 2 lines |
| `globals` | `const` lengths, `static`, `static mut`, `sizeof`, `alignof` | 6 lines |
| `pointers` | `alloc/free`, pointer arithmetic, `*mut`/`*`, address-of locals | 4 lines |
| `strings` | `str` params/returns, indexing, `len`, equality loop | 7 lines |
| `structs` | nested structs, struct return, str field, pointer field, copy | 6 lines |
| `slices` | `[T]` params/returns/locals, `&arr`/`&mut arr`, struct slice fields, arrays of slices, field/element range bases, indexing, two-sided/one-sided/full ranges on arrays, slices and `str`, dynamic bounds, `len`, `data_ptr`, `[u8]` print, `str` ↔ `[u8]`, mutation through a mutable slice | 56 lines |
| `bounds_array` | dynamic array index 3 on `[i32; 3]` prints the in-range read, then aborts | 1 line, exit 101 |
| `bounds_slice` | dynamic slice index 3 on length 3 aborts | 1 line, exit 101 |
| `bounds_str` | dynamic `str` index 5 on length 5 aborts | 1 line, exit 101 |
| `vec` | `vec_new`/`vec_push`/`vec_free`/`vec_slice`, growth from 0 and 2, indexing read/write, `len`, struct elements, vec params/returns, struct fields, arrays of vecs, shallow header copies, layout | 39 lines |
| `vec_elements` | nested `vec<vec<i32>>`, `vec<str>`, `vec<[i32]>`, `vec<u8>` printed via `vec_slice` | 8 lines |
| `bounds_vec` | dynamic vec index 2 on length 2 aborts | 1 line, exit 101 |

All stdout and exit statuses match the Rust compiler's output exactly.

### Level 5 — negative testing

| Case | Rejection |
|---|---|
| `syntax.cx` | `expected expression` |
| `undefined.cx` | `undefined name` |
| `types.cx` | initializer type mismatch |
| `args.cx` | too many function arguments |
| `return.cx` | function can reach the end without returning |
| `immutable.cx` | cannot assign to an immutable place |
| `slice_mix.cx` | argument type mismatch (`[i32]` where `[u8]` is expected) |
| `slice_immutable.cx` | cannot assign to an immutable place (write through an immutable slice) |
| `slice_scalar.cx` | cannot slice a value of this type |
| `index_literal.cx` | constant array index out of bounds |
| `vec_untyped.cx` | cannot infer the element type of `vec_new` |
| `vec_element_type.cx` | `vec_push` value type mismatch |
| `vec_immutable.cx` | `&mut` requires a mutable place |
| `vec_slice_ref.cx` | `vec_slice` requires `*vec<T>` |
| `vec_global.cx` | vec globals are not supported |

All fifteen exit non-zero and print a source location + message.

### Optimized self-compile

`build/stage2/corexc build selfhost/corexc.cx -o /tmp/corexc_opt2 --opt 2`
succeeded; `/tmp/corexc_opt2 check selfhost/corexc.cx` returned 0, and it built
`control.cx` at `--opt 2` with the expected output.

The range and index checks survive optimization: `arr[2..5]` on a length-3
array aborts with `corex: slice range out of bounds`, and `arr[i]` with
`i == 3` on the same array aborts with `corex: index out of bounds`; both exit
101 on Stage 1 and Stage 2 and match the Rust compiler. The module includes weak
freestanding `memset`/`memcpy`/`memmove`/`memcmp` so LLVM's loop-idiom pass can
link without libc.

### Level 6 — bootstrap validation

- `sha256(stage1) =
  7b2f8837093d0dd5b6a4e4bce01c2e5dce2d908b7dfdd843fd6e2354df989601`.
- `sha256(stage2) = sha256(stage3) = sha256(stage4) =
  644c22c5bd3772b8a71365798731c52387b7c9349385030b62a573489244c7a8`.
- Repeated Stage 2 build produces the same hash.
- Stage 2/3/4 are static ELF x86-64 with entry `_start` at `0x401000`.
- Stage 1 differs (expected; Rust backend/ABI vs self-hosted backend/ABI) but
  passes the same behavioral corpus.

## 3. Structural and memory checks

- 17 representative struct sizes (`Arena`, `Buf`, `Tok`, `Lex`, `Type`, `Expr`,
  `Stmt`, `Decl`, `Item`, `Program`, `Src`, `Diag`, `Sem`, `Val`, `Cg`, `Fn`,
  `P`) were printed by binaries compiled by Rust Stage 0 and by Stage 1; all
  pairs match (e.g. `Expr` 152, `Decl` 184, `Stmt` 112, `Tok` 56,
  `Arena` 24, `Buf` 32).
- Slice layout matches the spec and the oracle: `sizeof([i32]) = 16`,
  `alignof([i32]) = 8`, `sizeof(str) = 16`, `sizeof([[i32]; 2]) = 32`, and
  `len` of a 3-element slice is 3; Rust-, Stage 1-, and Stage 2-compiled
  binaries print identical values.
- Vec layout matches the spec and the oracle: `sizeof(vec<i32>) = 24`,
  `alignof(vec<i32>) = 8`, and `sizeof(vec<vec<u8>>) = 24` (the header is
  element-independent); Rust-, Stage 1-, and Stage 2-compiled binaries print
  identical values (corpus `vec` and `vec_elements`).
- Arena stress: 128 blocks of `sizeof(B) = 32` were allocated, written, read
  back, and the arena reported `used = 4096`; no overlap/corruption.
- Token array capacity is `len(source) + 2`; `lx_push` aborts on overflow.
- Arena allocation aborts on capacity exhaustion; alignment is 16 bytes.
- No `free`/`munmap` happens during compilation; the arena is process-lifetime.
- The compiler source initializes every pointer field it later reads
  (`Src.next`, `Variant.expr`, etc.); `rt_alloc` memory is zero-filled by
  `mmap`.

## 4. Reproducibility conditions

- Stage builds use `--opt 0`; no debug info, no timestamps, no absolute paths
  in IR; fixed `source_filename`; counter-based temporaries/labels/string IDs.
- Temporary paths are `/tmp/corexc.<pid>.<ext>` and are not embedded.
- LLVM 20.1.8 and cc 15.2.0 pinned in `selfhost/bootstrap.sh` / CI.

## 5. Known validation limits

- The bootstrap subset is narrower than the full language: no floats,
  modules, function pointers, unions, bitfields, threads, or C extern.
  Programs using those are rejected, not miscompiled.
- Slices are supported. Element indexing and slice ranges are bounds-checked at
  runtime (`corex: index out of bounds` / `corex: slice range out of bounds`,
  exit 101), and constant array indices are rejected in the frontend. `str` and
  `[u8]` are mutually compatible per spec §3.3.
- `vec<T>` is supported (`vec_new`, `vec_push`, `vec_free`, `vec_slice`, `len`,
  checked indexing, any sized element type). Vec constants and globals are
  rejected because `vec_new` is a runtime call; vec headers copy shallowly.
- The original Rust suite does not exercise the CoreX-written compiler; the new
  `tests/selfhosting` suite does. CI runs both.
- No sanitizer build of the self-hosted compiler exists; memory evidence is
  structural + stress-based, not ASan/Valgrind.

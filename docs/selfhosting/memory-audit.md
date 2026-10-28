# Memory Audit — CoreX Bootstrap Compiler

Date: 2026-10-28
Scope: `selfhost/runtime.cx`, `selfhost/lexer.cx`, `selfhost/parser.cx`,
`selfhost/semantic.cx`, `selfhost/codegen.cx`, `selfhost/driver.cx`, and the
generated runtime helpers in `selfhost/codegen.cx`.

## 1. Memory model

The bootstrap compiler preserves CoreX's low-level model:

- no garbage collector, no reference counting, no hidden allocation;
- no background threads or runtime;
- one explicit arena per compiler process;
- raw pointers and explicit `unsafe` regions;
- no destructors; resources are released by process exit or by explicit
  `free`/`unmap` in user programs.

The compiler never calls `free` on its own allocations. It allocates one
512 MiB anonymous mapping (`alloc(536870912)` → `mmap`) and bump-allocates from
it. `mmap` pages are zero-filled and are not reused, so:

- no allocator metadata, no double-free, no use-after-free inside the compiler;
- reads of never-written memory return zero;
- the maximum resident set is bounded by the source/AST/IR footprint, not by
  the 512 MiB reservation (untouched pages are not resident);
- leaks are limited to the compiler process lifetime and released by the kernel.

## 2. Allocation paths

| Path | Allocation | Failure behavior |
|---|---|---|
| `rt_arena(cap)` | `alloc(cap)` (mmap) | `alloc` aborts 101 on mmap failure |
| `rt_alloc(a, size)` | bump, 16-byte aligned | `rt_die` if `used + size > cap` |
| `rt_buf_cap` / `rt_reserve` | arena block, copy on grow | arena exhaustion aborts |
| Lexer token array | `(len(source) + 2) * sizeof(Tok)` | `lx_push` aborts if `ntoks >= cap` |
| AST nodes | `rt_alloc(sizeof(Node))` per node | arena exhaustion aborts |
| IR text | growable `Buf` | arena exhaustion aborts |
| Generated user code | `alloc`/`free` builtins | `_cx_alloc` aborts on mmap failure |

`rt_alloc` aligns `used` with `(used + 15) & (0 - 16)`. `usize` arithmetic wraps
by language definition; the capacity guard runs after alignment, before the
pointer is returned.

## 3. Object initialization

`rt_alloc` returns zeroed pages because the arena is a fresh `mmap` and never
reuses bytes. Every constructor still initializes the fields it later reads.
Examples: `type_new` sets all `Type` fields; `expr_new`/`stmt_new`/`decl_new`
set all fields; `srcs_add` sets `Src.next`; `parse_enum` sets `Variant.expr`;
`Case.lbl` is assigned before switch emission; `Val`, `LoopCtx`, `Cg`, `Fn`, and
`P` are built with complete struct literals. This makes uninitialized reads a
defect rather than a routine occurrence.

## 4. Structural parity

The compiler's layout of its own structs must match LLVM's layout, because
`sizeof`/array element storage use the compiler's computed sizes. A probe printed
17 representative sizes from binaries compiled by Rust Stage 0 and by Stage 1:

```text
Arena 24  Buf 32  Tok 56  Lex 80  Type 64  Expr 152  Stmt 112  Decl 184
Item 48  Program 32  Src 48  Diag 40  Sem 88  Val 48  Cg 80  Fn 56  P 96
```

All values were identical. `Tok` is `{i32,i32,i32,i32,usize,usize,str,i64}` =
56 bytes with natural alignment; `Expr` = 152; `Decl` = 184.

## 5. Bounds and corruption checks

Implemented:

- token array: hard capacity check in `lx_push`;
- arena: capacity check in `rt_alloc`; no silent overrun;
- source reads: `rt_slice` clamps `end` to `len(s)` and `end >= start`;
- file reads: size from `lseek`, buffer `n + 1`, read loop bounded by `n`;
- string globals: lengths from `len(text)`, IR escaping emits `\XX` for quotes,
  backslashes, NUL, and non-printable bytes;
- driver argv arrays: `rt_argv` allocates `n + 1` zeroed pointers (NULL
  terminator), and every call sets exactly `n` entries;
- pointer null checks in semantic type helpers; the parser/checker now reject
  malformed inputs instead of dereferencing null.

Not implemented (documented limits):

- generated programs do not get dynamic array/str bounds checks from the
  bootstrap backend; invalid indices are undefined at the machine level. The
  corpus uses valid indices. The Rust compiler still checks them, so this is a
  bootstrap-subset behavior, not a language change.
- no ASan/Valgrind/sanitizer pass for the self-hosted compiler;
- no guard pages around the arena; overflow is prevented by the explicit size
  check, not hardware.

## 6. Double-free / use-after-free

The compiler performs no `free` calls. `rt_unlink` removes temp files; it does
not free memory. `_cx_free` exists only in generated user code and maps to
`munmap` with the caller-supplied size. The compiler's own process has a single
arena owned by `main`; nested contexts receive `*mut Arena`.

## 7. Leaks

Intentional process-lifetime retention:

- source buffers, tokens, AST, symbols, IR text, tool paths;
- per-function temp allocas are compile-time only (IR text), not runtime
  allocations.

Every path that aborts (lex/parse/semantic error, tool failure, arena exhaustion)
exits the process, releasing the mapping. No library-style API is exposed, so
there is no long-lived leak surface.

## 8. Alignment

- Arena returns 16-byte-aligned blocks; `malloc`-style user allocations are
  16-byte aligned.
- LLVM struct/array layout uses natural alignment from the emitted type
  definitions; the compiler's `type_align` matches (1/2/4/8 for scalars,
  8 for pointers/`str`, `alignof(elem)` for arrays, max field alignment for
  structs).
- `sizeof`/`alignof` probes match between Rust-built and self-built binaries.

## 9. Verification evidence

- `selfhost/bootstrap.sh`: Stage 2 == Stage 3 == Stage 4; three corpus runs.
- 79 tests per compiler stage, 237 total; all pass.
- Arena stress: 128 × 32-byte blocks, patterns verified, `used = 4096`.
- Struct-size parity between Rust Stage 0 and Stage 1 binaries.
- Static ELF, no libc (`ldd` reports "not a dynamic executable").

## 10. Residual risks

1. Semantic/codegen bugs could still write through a bad pointer; null guards
   and the corpus reduce but do not eliminate this. The Rust compiler remains
   the reference for untrusted input.
2. The arena reservation is 512 MiB; a pathological input exceeding it aborts
   rather than growing. This is a deliberate fixed-memory policy.
3. No sanitizer validation of the self-hosted compiler.
4. Generated code has no bounds checks; programs must index validly.

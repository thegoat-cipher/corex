# Bootstrap Architecture — CoreX Self-Hosting

Date: 2026-10-28
Status: implemented and verified (see `bootstrap-results.md`)

## 1. Goal

Replace the Rust compiler as the *producer of the CoreX compiler binary* with a
compiler written in CoreX, while keeping Rust `corex` as the trusted Stage 0
oracle. The bootstrap must be reproducible from a clean checkout.

## 2. Stage model

| Stage | Compiler | Input | Output | Meaning |
|---|---|---|---|---|
| 0 | Rust `compiler/target/release/corex` | `selfhost/corexc.cx` | `build/stage1/corexc` | Trusted reference; Rust semantics are the oracle |
| 1 | Stage 0 | `selfhost/corexc.cx` | `build/stage2/corexc` | First CoreX-written compiler binary |
| 2 | Stage 1 | `selfhost/corexc.cx` | `build/stage3/corexc` | Compiler compiles itself |
| 3 | Stage 2 | `selfhost/corexc.cx` | `build/stage4/corexc` | Fixed-point confirmation |

Stage 2 and Stage 3 are compared byte-for-byte. Stage 4 is a third
self-hosted generation and must match Stage 3.

```text
cargo build --release                    (Stage 0, Rust)
        |
        v
stage0 build selfhost/corexc.cx          -> build/stage1/corexc
        |
        v
stage1 build selfhost/corexc.cx          -> build/stage2/corexc
        |
        v
stage2 build selfhost/corexc.cx          -> build/stage3/corexc
        |
        +--> sha256(stage2) == sha256(stage3)
        |
        v
stage3 build selfhost/corexc.cx          -> build/stage4/corexc
        |
        +--> sha256(stage3) == sha256(stage4)
        |
        v
tests/selfhosting/run.sh (stage1, stage2, stage3)
```

## 3. Source layout

| File | Lines | Responsibility |
|---|---:|---|
| `selfhost/corexc.cx` | 12 | Translation-unit root; textual imports only |
| `selfhost/types.cx` | 59 | `K` type kinds, `Type` node, type constructors |
| `selfhost/runtime.cx` | 573 | Arena, growable bytes, strings/paths, files, env, fork/exec |
| `selfhost/lexer.cx` | 695 | Sources, diagnostics, tokens, token kinds, lexer |
| `selfhost/ast.cx` | 307 | AST node kinds, tagged structs, declarations, items |
| `selfhost/parser.cx` | 1,509 | Recursive descent, imports, types, statements, expressions |
| `selfhost/semantic.cx` | 1,929 | Symbols, types, layout, const eval, checks, return flow |
| `selfhost/codegen.cx` | 2,373 | LLVM IR emission, ABI, runtime helpers, module assembly |
| `selfhost/driver.cx` | 264 | CLI, IR writing, `llvm-as`/`opt`/`llc`/`cc` orchestration |

`selfhost/corexc.cx` imports `types.cx` before `ast.cx`; type references to
`Expr`/`Decl` are resolved after all declarations are collected. All files are
imported textually into one translation unit; there are no `module` units and
therefore no symbol mangling in the bootstrap.

## 4. Bootstrap subset

The self-hosted compiler implements the subset used by its own source plus a
useful user-program subset. Unsupported constructs are rejected with a
diagnostic; they are not silently miscompiled.

Supported:

- `i8..i64`, `u8..u64`, `usize`, `isize`, `bool`, `char`;
- raw pointers `*T`/`*mut T`, pointer arithmetic, dereference, casts;
- `str`, `cstr`, tuple-layout `{ptr,len}` values, indexing, `len`, and
  spec §3.3 `str` ↔ `[u8]` compatibility;
- slices `[T]` (`{ ptr, i64 }`): `&array`/`&mut array` coercion in a slice
  context, slice parameters/returns/locals/struct fields, arrays of slices,
  `s[i]`, range slicing `s[a..b]`/`s[a..]`/`s[..b]`/`s[..]` on arrays, slices,
  and `str`, `len`, `data_ptr`; element indexing and ranges are bounds-checked
  at runtime (exit 101), and constant array indices are rejected in the
  frontend;
- `vec<T>` (`{ ptr, i64, i64 }`): `vec_new` (element type from context),
  `vec_push`, `vec_free`, `vec_slice`, `len`, bounds-checked indexing, elements
  of any sized type (scalars, structs, slices, `str`, nested `vec`),
  params/returns/locals, struct fields, arrays of vecs; header copies are
  shallow;
- fixed arrays `[T; N]`, structs (by value, nested), enums with discriminants;
- functions, recursion, `main(argc, argv, envp)`, user `_start`;
- `let`, assignment, compound assignment, `if/else`, `while`, `loop`,
  `break`, `continue`, `return`, `unsafe`, bare blocks, `switch` with
  fallthrough;
- `const`, `static`/`static mut` scalars, `sizeof`, `alignof`, `offsetof`;
- `import "file"` textual inclusion with relative/absolute paths;
- builtins: `print`, `len`, `syscall`, `alloc`, `free`, `assert`, `panic`,
  `data_ptr`, `str_from_raw`, `cstr_len`, `cstr_to_str`, `str_to_cstr`,
  `vec_new`, `vec_push`, `vec_free`, `vec_slice`;
- sizes/layout: natural C layout, `str` and `[T]` = `{ ptr, i64 }`,
  `vec<T>` = `{ ptr, i64, i64 }` (24 bytes, align 8), enum =
  `i32`, arrays inline.

Intentionally unsupported in the bootstrap (rejected):

- `match`, payload enums, generics,
  closures, traits, function types/pointers, unions, bitfields, flexible array
  members, packed/`align` structs, `for`, `do while`, labels/goto, macros, cfg
  attributes, `module`/`mod`, floats, variadics, C `extern` declarations,
  threads/atomics/TLS, inline asm.

The source is written to this subset, so Stage 1/2/3 never need the unsupported
features. Extending the subset does not require changing the bootstrap model;
it requires adding the corresponding parser/semantic/codegen paths and tests.

## 5. Toolchain dependencies

Required for every stage after Stage 0:

- `llvm-as` (assemble textual IR; also validates it),
- `opt` (only for `--opt 1..3`; `--opt 0` is the bootstrap default),
- `llc` (`-filetype=obj -relocation-model=static`),
- a C driver (`cc`, default) for `-nostdlib -static -no-pie` linking.

Stage 0 additionally requires `cargo`/`rustc`. Discovery:

1. `COREX_LLVM_BIN` for `llvm-as`/`opt`/`llc`;
2. `PATH`;
3. `/usr/bin`, `/bin` fallback.

`COREX_CC` overrides the linker driver. The compiler does not link libc and
emits a static ELF with `_start` as the ELF entry.

## 6. Determinism rules

- The compiler emits a fixed `source_filename = "corexc"` and no file paths,
  timestamps, hostnames, or random IDs.
- Temporary files are `/tmp/corexc.<pid>.<ext>` and never appear in the IR.
- Stage builds use `--opt 0`; `llc` gets `-O0` unless `--opt` says otherwise.
- Struct/type emission follows source order; string literals get monotonically
  increasing IDs; temporaries and labels are counter-based.
- Maps/sets are not used by the compiler; symbol lookup is deterministic list
  order.
- The driver invokes tools with a fixed argv order.
- Verification: byte-identical Stage 2/3/4; repeated Stage 2 builds are also
  byte-identical.

## 7. Artifact and ABI decisions

- Aggregates (`str`, structs, arrays) are passed/returned as first-class LLVM
  aggregate values in the self-hosted ABI. Stage 1 is compiled by Rust with the
  Rust compiler's ABI; Stage 2/3/4 are compiled by the self-hosted compiler with
  this ABI. This is intentional and is why Stage 1 differs byte-wise from
  Stage 2.
- The self-hosted compiler emits its own `_cx_syscall`, `_cx_alloc`,
  `_cx_free`, print helpers, and `_cx_cstr_len`/`_cx_cstr_str` in the module; it
  does not depend on Rust runtime symbols.
- `main(argc, argv, envp)` uses the same `_start` trampoline as the Rust
  compiler: `movq %rsp, %rdi; andq $-16, %rsp; call __corex_entry`.
- No DWARF, no unwind tables, no constructors, no TLS.

## 8. CLI

```text
corexc check   <file.cx>
corexc emit-ir <file.cx> [-o <out.ll>]
corexc build   <file.cx> [-o <out>] [--opt 0..3]
```

`emit-ir` writes to stdout when `-o` is absent. `build` requires the LLVM tools
and `cc`; it removes its temporary `.ll`, `.bc`, and `.o` files on success.

## 9. Verification gates

1. Stage 0 builds Stage 1.
2. Stage 1 builds Stage 2.
3. Stage 2 builds Stage 3.
4. `sha256(stage2) == sha256(stage3)` and `sha256(stage3) == sha256(stage4)`.
5. `tests/selfhosting/run.sh` passes against Stage 1, Stage 2, and Stage 3
   (79 tests: 16 positive cases × `--opt 0..3` + 15 negative cases).
6. Every `build` in the harness is implicitly validated by `llvm-as`; the
   harness runs each case at `--opt 0,1,2,3`.
7. Negative cases are rejected with a diagnostic.

## 10. Known limits

- The subset is not the full CoreX v0.3 language (see §4).
- Element indexing and slice ranges are bounds-checked at runtime in generated
  programs (`corex: index out of bounds` / `corex: slice range out of bounds`,
  exit 101); constant array indices are rejected in the frontend. The compiler
  source itself relies on valid indices.
- `vec` constants and globals are rejected (`vec_new` needs a runtime call);
  vec headers copy shallowly and ownership is manual, per the language spec.
- No floating point, modules, function pointers, or C ABI in the bootstrap.
- The compiler is single-threaded and allocates from one arena per process.
- Stage 0 (Rust) remains the oracle and the tool used for development tests;
  it is not replaced.

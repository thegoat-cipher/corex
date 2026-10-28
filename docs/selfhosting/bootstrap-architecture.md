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
| `selfhost/corexc.cx` | 17 | Translation-unit root; textual imports only |
| `selfhost/runtime.cx` | 1,116 | Arena, growable bytes, strings/paths, files, env, fork/exec |
| `selfhost/types.cx` | 125 | `K` type kinds, `Type` node, constructors, plain-layout predicate |
| `selfhost/lexer.cx` | 801 | Sources, diagnostics, tokens, token kinds, lexer |
| `selfhost/ast.cx` | 365 | AST node kinds, tagged structs, declarations, items |
| `selfhost/parser.cx` | 2,377 | Recursive descent, imports, types, statements, expressions, macros, cfg |
| `selfhost/semantic.cx` | 2,915 | Symbols, types, layout, const eval, checks, return flow |
| `selfhost/codegen.cx` | 3,535 | LLVM IR emission, ABI, runtime helpers, module assembly |
| `selfhost/fmt.cx` | 287 | Whitespace formatter |
| `selfhost/lint.cx` | 409 | Unused/unreachable/constant-condition lint |
| `selfhost/doc.cx` | 394 | `///` documentation generator |
| `selfhost/dump.cx` | 508 | Token/AST dumps for differential tests |
| `selfhost/repl.cx` | 726 | Interactive session |
| `selfhost/driver.cx` | 1,634 | CLI, IR writing, `llvm-as`/`opt`/`llc`/`cc` orchestration |

`selfhost/corexc.cx` imports `types.cx` before `ast.cx`; type references to
`Expr`/`Decl` are resolved after all declarations are collected. All files are
imported textually into one translation unit. Modules are a whole-program
construct: `module`/`mod` units and `corex.toml` manifests are parsed into one
program, item names are mangled (`math$add`), and visibility is enforced in
semantic analysis. No separate interface files are generated.

## 4. Bootstrap subset

The self-hosted compiler implements the subset used by its own source plus a
useful user-program subset. Unsupported constructs are rejected with a
diagnostic; they are not silently miscompiled.

Supported:

- `i8..i64`, `u8..u64`, `usize`, `isize`, `bool`, `char`;
- `f32`/`f64` literals, arithmetic, comparisons, casts, ABI passing/returning,
  and six-digit `print`; compound assignment for integer operators and float
  `+ - * / %` (`float %` emits `frem` and needs libm when linking, as in C);
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
- unions, bitfields, flexible array members, packed structs, and `align(N)`
  structs, with C-compatible layout and byte-offset access through `[N x i8]`
  lowering;
- `const`, `static`/`static mut` scalars and plain-layout aggregates,
  `sizeof`, `alignof`, `offsetof`; non-plain static aggregate initializers
  (packed/union/bitfield/`align(N)`) are rejected with E0226;
- functions, recursion, `main(argc, argv, envp)`, user `_start`, function
  types/pointers, indirect calls, struct fields holding function pointers;
- `let`, assignment, compound assignment, `if/else`, `while`, `loop`, `for`,
  `do while`, `break`, `continue`, `return`, `unsafe`, bare blocks, `switch`
  with fallthrough, `name:` labels and `goto`;
- expression macros, `#[cfg(...)]` attributes (`all`/`any`/`not` and explicit
  `--cfg` keys; `target_os=linux`, `target_arch=x86_64`, and `sanitize=true`
  when active are built in);
- `import "file"` textual inclusion with relative/absolute paths;
- `module`/`mod` units, `corex.toml` manifests, `mod$item` mangling, and `pub`
  visibility enforced in semantic analysis;
- `extern "C"` declarations, variadics, `--hosted`, `--shared`,
  `--emit obj|shared|staticlib`, and `--link`/`-L`/`-l`;
- builtins: `print`, `len`, `syscall`, `alloc`, `free`, `assert`, `panic`,
  `data_ptr`, `str_from_raw`, `cstr_len`, `cstr_to_str`, `str_to_cstr`,
  `vec_new`, `vec_push`, `vec_free`, `vec_slice`, `read_file`, `write_file`;
- sizes/layout: natural C layout (plus packed/`align`/union/bitfield rules),
  `str` and `[T]` = `{ ptr, i64 }`, `vec<T>` = `{ ptr, i64, i64 }`
  (24 bytes, align 8), enum = `i32`, arrays inline.

Intentionally unsupported in the bootstrap (rejected):

- `match`, payload enums, generics beyond `vec<T>`, closures, traits;
- threads, atomics, `volatile`, TLS, and inline asm (SH-033);
- DWARF debug info (`-g`), LSP/line-editor parity (SH-037), and generated
  module interface files (modules are whole-program);
- static initializers for aggregates whose layout is not plain (E0226).

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
corexc check   <file.cx | -> [-I <dir>] [--cfg <key>]
corexc emit-ir <file.cx | -> [-o <out.ll>] [--verify] [-I <dir>] [--cfg <key>]
corexc build   <file.cx | dir | -> [-o <out>] [--opt 0..3]
                [--emit obj|shared|staticlib] [--hosted] [--shared]
                [--sanitize] [-I <dir>] [--cfg <key>]
                [--link <file>] [-L <dir>] [-l <lib>]
corexc run     <file.cx | -> [-I <dir>] [--cfg <key>] [--hosted]
                [--sanitize] [--opt 0..3] [-- <program args>...]
corexc exec    '<source>' [-I <dir>] [--cfg <key>] [--opt 0..3]
                [-- <program args>...]
corexc test    <file.cx | -> [--opt 0..3]
corexc repl    [--opt 0..3]
corexc fmt     <file.cx | -> [-w | --check]
corexc lint    <file.cx | -> [--deny-warnings]
corexc doc     <file.cx | -> [-o <out.md>]
corexc dump-tokens <file.cx | ->
corexc dump-ast    <file.cx | ->
```

`emit-ir` writes to stdout when `-o` is absent; `--verify` assembles the IR
with `llvm-as` and fails the command on invalid IR. `--cfg` and `-I` are
frontend options accepted by every command that parses source, including
`check`, `emit-ir`, `run`, `exec`, and stdin (`-`) input. `build` accepts a
`corex.toml` directory as well as a file.

Exit codes: 0 success, 1 compile error, 2 usage/I/O error, 3 external-tool
failure. `run` and `exec` return the compiled program's status; a child killed
by a signal reports `128 + signal`. `build` prints `compiled ...` only after
the toolchain succeeds. `-g` is rejected with exit 2 because the bootstrap
emits no DWARF.

`build` requires the LLVM tools and `cc`; it removes its temporary `.ll`,
`.bc`, and `.o` files on success.

## 9. Verification gates

1. Stage 0 builds Stage 1.
2. Stage 1 builds Stage 2.
3. Stage 2 builds Stage 3.
4. `sha256(stage2) == sha256(stage3)` and `sha256(stage3) == sha256(stage4)`.
5. `tests/selfhosting/run.sh` passes against Stage 1, Stage 2, and Stage 3
   (249 checks: positive cases at `--opt 0..3`, negative cases, module
   projects, FFI, and abort cases). `cli.sh`, `dump.sh`, and `fuzz.sh` add
   CLI, differential, and sanitizer coverage.
6. Every `build` in the harness is implicitly validated by `llvm-as`; the
   harness runs each case at `--opt 0,1,2,3`.
7. Every negative case asserts a diagnostic code and matches the Rust oracle.
8. `fuzz.sh` builds a hosted ASan compiler and requires no signal or sanitizer
   report. CI sets `COREX_REQUIRE_ASAN=1`, so an unavailable sanitizer fails the
   gate instead of silently skipping it.

## 10. Known limits

- The subset is not the full CoreX v0.3 language (see §4).
- Element indexing and slice ranges are bounds-checked at runtime in generated
  programs (`corex: index out of bounds` / `corex: slice range out of bounds`,
  exit 101); constant array indices are rejected in the frontend. The compiler
  source itself relies on valid indices.
- Static initializers for non-plain aggregates are rejected with E0226; they
  are not lowered to byte constants yet.
- `vec` constants and globals are rejected (`vec_new` needs a runtime call);
  vec headers copy shallowly and ownership is manual, per the language spec.
- Threads, atomics, `volatile`, TLS, and inline asm are not implemented
  (SH-033); LSP/line-editor parity and DWARF (`-g`) are also absent.
- Modules are built as one whole program; visibility is enforced in semantic
  analysis rather than through generated interface files.
- The compiler is single-threaded and allocates from one arena per process.
- Stage 0 (Rust) remains the oracle and the tool used for development tests;
  it is not replaced.

# CoreX Resources

A map of the project and the external references worth reading. Start with the
goal, then follow the links.

- **Install the compiler:** [`docs/installation.md`](installation.md)
- **Learn the language:** [`docs/language-spec.md`](language-spec.md) and
  [`examples/`](../examples)
- **Work on the compiler:** [`docs/architecture.md`](architecture.md) and
  [`compiler/src/`](../compiler/src)
- **Check behavior:** [`compiler/tests/`](../compiler/tests) and
  [`docs/tasks.md`](tasks.md)
- **Understand a decision:** [`docs/ADRs.md`](ADRs.md)

---

## 1. Project documentation

| Document | Use it for |
|---|---|
| [`README.md`](../README.md) | Project overview, quick start, CLI surface, language tour, limitations |
| [`docs/installation.md`](installation.md) | Toolchain installation, build, verification, environment variables, troubleshooting |
| [`docs/language-spec.md`](language-spec.md) | Normative syntax and semantics; diagnostic catalog in §11 |
| [`docs/grammar.ebnf`](grammar.ebnf) | Formal grammar matching `parser.rs` |
| [`docs/memory-model.md`](memory-model.md) | Stack/heap ownership, pointers, vectors, slices, files, undefined behavior |
| [`docs/architecture.md`](architecture.md) | Compiler modules, AST/semantic tables, IR layout, helpers, test architecture |
| [`docs/PRD.md`](PRD.md) | Product vision, users, functional requirements, scope, risks |
| [`docs/NFR.md`](NFR.md) | Measurable non-functional requirements and their evidence |
| [`docs/ADRs.md`](ADRs.md) | Append-only architecture decisions, including the extended-feature and runtime designs |
| [`docs/tasks.md`](tasks.md) | Completed work with acceptance criteria and test evidence |
| [`docs/request-log.md`](request-log.md) | Historical request/outcome log |
| [`docs/research.md`](research.md) | Evidence-labelled research on comparable languages |
| [`docs/competitor-analysis.md`](competitor-analysis.md) | Language comparison and design tradeoffs |
| [`docs/benchmarks.md`](benchmarks.md) | Benchmark methodology and measured results |
| [`ENGINEERING.md`](../ENGINEERING.md) | Repository conventions, testing and documentation rules |
| [`master.md`](../master.md) | Current milestone and release status |

## 2. Examples

| File | Demonstrates | Expected |
|---|---|---|
| [`hello.cx`](../examples/hello.cx) | `_start`, direct `write`/`exit` syscalls, no libc | `Hello, World!`, exit 0 |
| [`factorial.cx`](../examples/factorial.cx) | Recursion, loops, arrays, integer formatting | `3628800`, exit 0 |
| [`pointers.cx`](../examples/pointers.cx) | `alloc`, `*mut u64`, casts, `free` | `123456789`, exit 0 |
| [`loop_sum.cx`](../examples/loop_sum.cx) | `while`, `continue`, `usize` arithmetic | exit status 51 |
| [`features.cx`](../examples/features.cx) | Enums, structs, globals, floats, chars, inference, compound assignment, slices, strings, `len` | deterministic feature tour, exit 0 |
| [`compiler_data.cx`](../examples/compiler_data.cx) | Tokenizer using `enum`, `struct`, `const`, `vec<T>`, strings, slices, `assert` | `9`, `0`, `fn`, `4`, `1` |
| [`playground.cx`](../examples/playground.cx) | Miscellaneous syntax/language checks | see file |

## 3. Test suites

All integration tests compile with the same `driver` API used by the CLI and, for
`native`/`features`/`compiler_caps`, execute the resulting ELF.

| File | Covers |
|---|---|
| [`compiler/tests/diagnostics.rs`](../compiler/tests/diagnostics.rs) | Rejection paths by stable diagnostic code, including `vec`, `const`, imports, and panic type errors |
| [`compiler/tests/features.rs`](../compiler/tests/features.rs) | Compound assignment, inference, chars, globals, enums, floats, structs, slices, strings, ranges, bounds aborts |
| [`compiler/tests/compiler_caps.rs`](../compiler/tests/compiler_caps.rs) | `vec<T>`, `const`, file I/O, imports, recursive structs, panic/assert, checked-in examples |
| [`compiler/tests/native.rs`](../compiler/tests/native.rs) | Hello/factorial/pointers/loop examples, wrapping, shifts, heap, exit statuses |
| [`compiler/tests/opt_levels.rs`](../compiler/tests/opt_levels.rs) | Identical observable behavior at `--opt 0..3`; no `nsw`/`nuw` |
| [`compiler/tests/ir.rs`](../compiler/tests/ir.rs) | Every example assembles under LLVM 20 `llvm-as`; structural IR checks |
| [`compiler/tests/cli.rs`](../compiler/tests/cli.rs) | stdin, `exec`, flags, REPL accumulation, blank-line submission |
| [`compiler/tests/robustness.rs`](../compiler/tests/robustness.rs) | Malformed/adversarial input produces diagnostics instead of panics |

Run everything from `compiler/`:

```bash
cargo test
```

## 4. Compiler source map

| File | Responsibility |
|---|---|
| [`compiler/src/token.rs`](../compiler/src/token.rs) | Token kinds, symbols, diagnostic descriptions |
| [`compiler/src/lexer.rs`](../compiler/src/lexer.rs) | Byte-oriented lexing: comments, integers, floats, chars, strings, operators |
| [`compiler/src/ast.rs`](../compiler/src/ast.rs) | AST, types, struct layout information, source spans |
| [`compiler/src/parser.rs`](../compiler/src/parser.rs) | Items, globals, `const`, imports, enums, structs, expressions, ranges |
| [`compiler/src/semantic.rs`](../compiler/src/semantic.rs) | Scopes, symbol tables, type checking, constant evaluation, mutability, `unsafe`, return flow, builtins |
| [`compiler/src/codegen.rs`](../compiler/src/codegen.rs) | LLVM 20 textual IR, aggregate ABI, vectors, file helpers, entry synthesis |
| [`compiler/src/diagnostics.rs`](../compiler/src/diagnostics.rs) | Spans, diagnostics, multi-file `SourceFile` segments and rendering |
| [`compiler/src/driver.rs`](../compiler/src/driver.rs) | Import expansion, LLVM tool discovery, verify → optimize → object → link, execution |
| [`compiler/src/repl.rs`](../compiler/src/repl.rs) | Interactive session and REPL-only commands |
| [`compiler/src/lineedit.rs`](../compiler/src/lineedit.rs) | TTY raw-mode line editing and history |
| [`compiler/src/main.rs`](../compiler/src/main.rs) | CLI parsing, source loading, diagnostic rendering, exit codes |

The emitter writes IR text rather than linking `libLLVM`; the driver runs
`llvm-as`, `opt`, `llc`, and the host linker as subprocesses.

## 5. Building compiler-style programs in CoreX

The language now covers the data shapes a frontend needs:

| Need | CoreX facility |
|---|---|
| Token kinds, error codes, AST node tags | `enum` with explicit `i32` discriminants |
| Tokens, diagnostics, symbol tables | `struct` values; nested structs; arrays of structs |
| Recursive ASTs | Structs containing `*T`/`*mut T` fields |
| Source buffers | `str`, `[u8]`, string literals, indexing, `s[a..b]` |
| Token/error/node lists | `vec<T>` with `vec_new`, `vec_push`, `vec_slice`, `vec_free`, `len`, `v[i]` |
| File readers/writers | `read_file(path) -> vec<u8>`, `write_file(path, data) -> i32` |
| Split frontend/backend files | `import "file.cx";` (flat namespace) |
| Named limits/opcodes | `const` values |
| Fatal compiler errors | `panic(msg)` / `assert(cond, msg)` |
| Recoverable parse errors | Ordinary return values (`bool`, enums, error-code structs); collect diagnostics in a `vec` |

See the tokenizer in [`examples/compiler_data.cx`](../examples/compiler_data.cx)
for a compact example.

## 6. External references

### Rust and Cargo

- [The Rust Programming Language](https://doc.rust-lang.org/book/)
- [The Cargo Book](https://doc.rust-lang.org/cargo/)
- [rustup](https://rustup.rs/)

### LLVM 20

- [LLVM 20.1.0 documentation](https://releases.llvm.org/20.1.0/docs/)
- [LLVM Language Reference](https://llvm.org/docs/LangRef.html) — check syntax
  against LLVM 20 when in doubt; the local pinned toolchain is authoritative.
- [LLVM Kaleidoscope tutorial](https://llvm.org/docs/tutorial/)
- [`llc` command guide](https://llvm.org/docs/CommandGuide/llc.html)
- [`opt` command guide](https://llvm.org/docs/CommandGuide/opt.html)
- [LLVM 20.1.8 release](https://github.com/llvm/llvm-project/releases/tag/llvmorg-20.1.8)

### Linux / x86-64 systems programming

- [Linux `syscall(2)` manual page](https://man7.org/linux/man-pages/man2/syscall.2.html)
- [Linux man-pages project](https://man7.org/linux/man-pages/)
- [x86-64 System V ABI](https://gitlab.com/x86-psABIs/x86-64-ABI)
- [System V gABI / ELF specification](https://refspecs.linuxfoundation.org/elf/gabi4+/contents.html)
- [Intel 64 and IA-32 Architectures Software Developer Manuals](https://www.intel.com/content/www/us/en/developer/articles/technical/intel-sdm.html)

### Compiler construction

- [Crafting Interpreters](https://craftinginterpreters.com/) — lexer, parser,
  bytecode/VM, and tree-walking interpreter implementation walkthroughs
- [LLVM Kaleidoscope](https://llvm.org/docs/tutorial/) — a small LLVM-backed
  language frontend end to end

## 7. Diagnostics and CLI

- Diagnostic catalog: [`docs/language-spec.md` §11](language-spec.md#11-diagnostics)
- CLI usage and flags: [`README.md` §CLI](../README.md#cli)
- Optimization-level guarantees: [`docs/ADRs.md`](ADRs.md) ADR-0019 and
  [`compiler/tests/opt_levels.rs`](../compiler/tests/opt_levels.rs)
- File/build behavior: [`docs/architecture.md` §5](architecture.md)

## 8. Getting help

- Open an issue at <https://github.com/thegoat-cipher/corex/issues>.
- Include the compiler command, source snippet, expected behavior, actual
  behavior, and the output of `corex --version`, `llvm-as --version`, and
  `cc --version` when relevant.
- Do not paste secrets or private source. Trim a reproducer to the smallest
  failing program.

## 9. License and acknowledgements

CoreX is dual-licensed under MIT OR Apache-2.0. See
[`LICENSE-MIT`](../LICENSE-MIT), [`LICENSE-APACHE`](../LICENSE-APACHE), and
[`THIRD_PARTY.md`](../THIRD_PARTY.md). The compiler uses no external Rust crates;
LLVM and the host C toolchain are build tools, not redistributed components.

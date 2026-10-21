# CoreX Compiler Architecture — v0.1

Status: describes the implementation in `compiler/` as of 2026-10-21. Facts here
were read from the code, not proposed.

- Rust 1.99.1, edition 2021, **zero external crates**.
- LLVM 20.1.8 tools invoked as processes (`llvm-as`, `llc`).
- Target: `x86_64-unknown-linux-gnu`, statically linked, freestanding ELF.
- Test suite: 122 tests (53 unit, 69 integration), all passing.

---

## 1. Pipeline

```
source .cx
   │
   ▼  lexer::lex            token.rs        tokens + spans
   │
   ▼  parser::parse         ast.rs          AST + NodeIds
   │
   ▼  semantic::analyze                     Analysis side tables
   │
   ▼  codegen::generate                    LLVM 20 IR text
   │
   ▼  llvm-as  (assembles + verifies)
   │
   ▼  opt -passes=default<O<n>>          (only when --opt > 0)
   │
   ▼  llc -filetype=obj -O<n> -relocation-model=static
   │
   ▼  cc -nostdlib -static -no-pie
   │
   ▼  ELF executable
```

Each stage consumes only the previous stage's output. No stage re-reads source
text except the diagnostics renderer, which needs it to print carets.

---

## 2. Modules

| Module | Lines | Responsibility | Depends on |
|---|---|---|---|
| `lib.rs` | 19 | Module declarations for the pipeline stages | — |
| `token.rs` | 164 | Token kinds, symbol/description text for diagnostics | `diagnostics` |
| `diagnostics.rs` | 222 | `Span`, `Diagnostic`, `SourceFile` (line/column mapping, rendering) | — |
| `lexer.rs` | 520 | Byte-oriented lexer, keywords, numbers, strings, comments; structured errors | `token`, `diagnostics` |
| `ast.rs` | 394 | Untyped AST, `NodeId`, `Type`, `IntTy`, size/alignment/LLVM spelling | `diagnostics` |
| `parser.rs` | 853 | Recursive descent + precedence climbing; recovers per statement and per function | `ast`, `token`, `diagnostics` |
| `semantic.rs` | 1515 | Scopes, name resolution, type checking, mutability, `unsafe` gating, return-flow analysis, builtin signatures | `ast`, `diagnostics` |
| `codegen.rs` | 1249 | LLVM 20 textual IR emission, runtime helpers, entry-point synthesis | `ast`, `semantic` |
| `driver.rs` | 294 | Tool discovery, temp directories, `llvm-as` verification, `opt`, `llc`, linking, execution | all of the above |
| `repl.rs` | 472 | Interactive session: per-function source accumulation, candidate selection (items / printed expression / statements), completeness heuristic, `:` commands | `driver`, `parser`, `semantic`, `lineedit` |
| `lineedit.rs` | 273 | TTY line editing and history via `stty` raw mode; plain line reads when stdin is not a terminal | — |
| `main.rs` | 423 | CLI parsing (files, stdin, inline source), diagnostic rendering, exit codes, `emit-ir` verification | `driver`, `repl` |

Line counts include inline `#[cfg(test)]` modules.

---

## 3. Frontend data flow

### 3.1 Tokens

`Token { kind: TokenKind, span: Span }`. `Span` is a half-open byte range;
line/column is computed only when a diagnostic is rendered. Multi-character
operators are lexed greedily; `&&` and `->` are single tokens, while `<<=` is
`<<` followed by `=` (compound assignment does not exist, so it fails to parse).

### 3.2 AST

- `Program { fns: Vec<FnDecl> }`. Functions are the only items.
- Every `Expr` carries a unique `NodeId`. Semantic analysis stores types and
  resolutions in maps keyed by `NodeId`, so the tree is never rewritten.
- Statements are a flat enum (`Let`, `Assign`, `If`, `While`, `Loop`, `Break`,
  `Continue`, `Return`, `Unsafe`, `Block`, `Empty`).
- Blocks are statement lists; there are no block expressions.

### 3.3 Analysis side tables

| Table | Key | Value | Consumer |
|---|---|---|---|
| `expr_types` | `NodeId` | `Type` | codegen, mutability checks |
| `idents` | `NodeId` | `LocalRef { slot }` | codegen (which `alloca` to use) |
| `let_slots` | initializer `NodeId` | slot index | codegen (`let` statement) |
| `calls` | call `NodeId` | `Callee::User/Syscall/Alloc/Free/Print` | codegen |
| `fn_sigs` | name | `FnSig { params, ret }` | arity/return checks |
| `layouts` | function name | `FnLayout { slot_types }` | codegen (`alloca` per slot) |
| `builtins` | — | usage flags | codegen (which helpers to emit) |
| `entry` | — | `"_start"` or `"main"` | codegen (entry synthesis) |

### 3.4 Type checking strategy

Bidirectional: `check_expr(expr, expected: Option<&Type>)`.

- Integer literals take the expected integer type, the other operand's type, or
  default to `i32`. Out-of-range literals are rejected at the literal's span.
- Negative literals are matched as `Unary Neg(Int)` and range-checked against the
  target minimum, so `-128i8` is accepted.
- Binary operators unify operands; the only adaptation is literal adoption. No
  implicit widening or narrowing exists.
- Pointer arithmetic is a special case: `*T + usize` yields `*T`; the offset must
  be `usize`; `unsafe` is required.
- Every expression's type is recorded, so codegen never infers.

### 3.5 Scope and mutability

- A scope stack of `HashMap<String, LocalRef>`; slots are assigned monotonically
  per function and never reused.
- Parameters occupy the first slots. The body block is a nested scope, so a
  local may shadow a parameter.
- Mutability is tracked per slot (`slot_mut`) and consulted by `check_place`,
  which returns `(Type, is_mutable)`.
- Parameters are immutable slots: assigning to a parameter is `E0205`, and
  `&mut` of a parameter is `E0220`. Copy into a `let mut` binding to mutate.

### 3.6 Return-flow analysis

`block_diverges` / `stmt_diverges` decide whether a body can fall off its end.
`loop` and `while true` diverge unless a `break` belonging to them appears
(breaks in nested loops are ignored). A tail `syscall(60, ...)` or
`syscall(231, ...)` diverges. `if` diverges only when both branches do. A non-void function that
can fall through is `E0206`.

---

## 4. Code generation

### 4.1 Module shape

```
; CoreX module: <file>
source_filename = "<file>"
target triple = "x86_64-unknown-linux-gnu"

<globals: string literals, abort messages, print fragments>
<function definitions>
<synthesized _start if entry is main>
<runtime helpers, emitted only when used>
```

- **No `target datalayout`** is emitted. `llc` fills in the x86-64 layout,
  avoiding a hard dependency on one LLVM build's layout string.
- Opaque pointers only (`ptr`), as required by LLVM 20.
- Globals are `private unnamed_addr constant [...]`; strings are escaped byte
  arrays with explicit lengths (no NUL terminator).

### 4.2 Values and places

- Scalars (integers, `bool`, pointers) are SSA values.
- Aggregates (`[T; N]`) are represented by a **pointer to storage**; copying is
  an explicit `load`/`store` of the aggregate.
- `gen_place` maps an expression to `(pointer, type)` for identifiers,
  dereferences, and indices; `gen_expr` loads from a place when a scalar value is
  needed.
- Every `alloca` (locals, array literals, array repeats) is emitted in the entry
  block, so no allocation happens inside a loop.

### 4.3 Control flow

The emitter tracks `terminated` and `current_label` plus a loop stack of
`(break_label, continue_label)`:

- `terminate(instr)` appends an instruction and marks the block closed.
- `label(name)` closes the current block with a fallthrough branch when needed,
  then starts `name:`.
- `if` emits then/else/end blocks; both branches branch to `end` unless they are
  already terminated.
- `while` re-evaluates its condition in a dedicated header block; `continue`
  targets the header, `break` targets the exit.
- `loop` uses a body block; `continue` targets the body, `break` the exit.

A function whose body can fall through gets a final `ret void`, `ret <value>`
(unreachable, rejected earlier by semantic analysis), or — for `_start` — `exit(0)`.

### 4.4 Integer and memory operations (all defined)

| Language | IR | Guard |
|---|---|---|
| `+ - *` | `add`/`sub`/`mul` (no `nsw`/`nuw`) | — (wrapping is defined) |
| unary `-` on a literal | folded constant | — |
| `/ %` | `sdiv`/`udiv`, `srem`/`urem` | zero and `MIN/-1` branch to `__corex_fatal` |
| `<< >>` | `shl`, `ashr`/`lshr` | count masked with `bits-1` |
| `& \| ^` | `and`/`or`/`xor` | — |
| `== != < <= > >=` | `icmp` + `zext i1 to i8` | signedness from the operand type |
| `&& \|\|` | branch + `phi i1` + `zext` | short-circuit |
| `as` int→int | no-op / `trunc` / `sext` / `zext` | source signedness |
| `as` ptr→int | `ptrtoint` (+`trunc`) | — |
| `as` int→ptr | `zext`/`sext` to i64, `inttoptr` | — |
| index | `getelementptr inbounds` | dynamic bounds check |
| array copy | aggregate `load` + `store` | — |
| `alloc` | `__corex_alloc` helper | negative `mmap` result → abort |
| `free` | `__corex_free` helper | — |
| `syscall` | `__corex_syscall` helper (inline asm) | — |
| `print` | `__corex_print_i64` / `__corex_print_u64` / `__corex_print_bool` | decimal formatting into a 20-byte stack buffer, then `write` + newline |

### 4.5 Runtime helpers

Emitted only when referenced (`analysis.builtins`):

```llvm
define internal i64 @__corex_syscall(i64 %nr, i64 %a1, ..., i64 %a6) {
entry:
  %r = call i64 asm sideeffect "syscall",
        "={rax},{rax},{rdi},{rsi},{rdx},{r10},{r8},{r9},~{rcx},~{r11},~{memory}"(...)
  ret i64 %r
}
```

- Register constraints model the Linux syscall ABI exactly; `~{memory}` prevents
  the optimizer from moving memory operations across a syscall.
- `__corex_alloc` performs `mmap` (syscall 9, `PROT_READ|WRITE`,
  `MAP_PRIVATE|ANONYMOUS`) and aborts on a negative result.
- `__corex_free` performs `munmap` (syscall 11) and ignores the result.
- `__corex_fatal(ptr msg, i64 len)` writes to fd 2 (syscall 1) and exits 101
  (syscall 60).
- The `main` entry is wrapped by a synthesized `_start` that calls `main`,
  sign-extends the `i32` result, and exits with it.

### 4.6 Trust boundary

Codegen assumes a program that semantic analysis accepted. It uses
`unwrap_or` fallbacks for table lookups, and `debug_assert`s on invariants such
as aggregate type agreement. Invalid IR is impossible by construction for
accepted programs; if it happens anyway, `llvm-as` rejects it and the driver
reports `E0300` with the verifier message (this is how one real emitter bug — a
constant expression wrapping a function-local `ptrtoint` — was caught and fixed;
it is now a regression test).

---

## 5. Driver and CLI

### 5.1 Build steps

1. `check` — lex, parse, analyze. Diagnostics abort before IR generation.
2. `codegen` — produce IR text in memory.
3. Write `module.ll` to a unique temp directory.
4. `llvm-as module.ll -o module.bc` — assembles **and runs the LLVM verifier**.
   Failure → `E0300` carrying the verifier text.
5. `opt -passes=default<O{n}> module.bc -o module.opt.bc` (only for `--opt` >
   0); `opt` runs the IR optimizer that `llc` does not.
6. `llc -filetype=obj -O<n> -relocation-model=static <bc> -o module.o`.
7. `cc -nostdlib -static -no-pie -o <output> module.o`.
8. Clean the temp directory unless `COREX_KEEP_TEMP` is set.

The synthesized and user-provided `_start` both carry `alignstack=16` because
the kernel enters the process with `%rsp` 16-byte aligned, not in the SysV callee
state (ADR-0012).

### 5.2 Tool discovery

`COREX_LLVM_BIN` → `PATH` (plain name, then name-25…name-14) →
`/usr/lib/llvm-25/bin` … `/usr/lib/llvm-14/bin`. The linker driver is `COREX_CC`
or `cc`. Missing tools are `E0303` (exit 3), distinct from program errors.

### 5.3 REPL behavior

`repl.rs` keeps the source text of each top-level function and recompiles the
combined program per submission. A submission becomes, in order of preference:

1. item definitions (merged by name; redefinition replaces);
2. a printed expression — the chunk with no trailing `;` is wrapped as
   `print(<chunk>)` and analyzed together with the session's definitions;
3. a statement block wrapped in a temporary `fn main`.

The first candidate that passes semantic analysis wins, so `print(x)` does not
become `print(print(x))`. A one-line chunk runs on Enter; multi-line chunks run
when they define `main`/`_start` or when the user enters a blank line.
`lineedit.rs` provides raw-mode editing (arrows, history, Ctrl-A/E/U) when stdin
is a terminal, and falls back to line reads otherwise (tests, pipes).

### 5.4 Commands

| Command | Behavior |
|---|---|
| `corex build <file\|-> [-o out] [--opt 0-3]` | Produces the static ELF; prints `compiled <in> -> <out>`; `--opt > 0` runs `opt` then `llc` |
| `corex exec '<source>'` | Runs source supplied on the command line; forwards the program's exit status |
| `corex repl` | Interactive session; a blank line compiles and runs the accumulated definitions |
| `corex run <file\|-> [--opt n] [-- args...]` | Builds into a temp directory, runs, removes it, forwards the exit status (128+signal on Unix); `-` reads source from stdin |
| `corex emit-ir <file> [--verify]` | Prints IR; `--verify` first assembles it with `llvm-as` |
| `corex check <file>` | Frontend only; prints `ok: N function(s), entry ...` |

Exit codes: 0 success, 1 compile error, 2 usage/I/O, 3 toolchain failure.

### 5.5 Error model

A single `Diagnostic { code, message, span, note }` type is used by every stage.
Lexer and parser errors are `E0xxx`, semantic errors `E02xx`, toolchain errors
`E03xx`. `SourceFile` renders `error[CODE]: message`, a `--> file:line:col` line,
the source line, and a caret, plus an optional note. Multiple diagnostics are
rendered in order and counted.

Parser recovery is deliberately shallow: skip to the next `;`/`}` inside a
failed statement, and to the next `fn` after a failed item, so later declarations
still produce errors.

---

## 6. Testing architecture

| Test file | Kind | Count | What it proves |
|---|---|---|---|
| inline `#[cfg(test)]` in `lexer`, `parser`, `semantic`, `diagnostics` | unit | 53 | token streams, precedence, recovery, scope/type rules, layout tables, span rendering |
| `tests/diagnostics.rs` | integration | 24 | every documented rejection path, by code |
| `tests/ir.rs` | integration | 6 | all examples emit IR accepted by `llvm-as`; no libc symbols; no `undef`; regression for the invalid constant-expression bug; structural checks |
| `tests/opt_levels.rs` | integration | 5 | identical observable behavior at `--opt 0..3`; no `nsw`/`nuw` in emitted IR; generated differential arithmetic (1,104 assertions) |
| `tests/robustness.rs` | integration | 2 | malformed and adversarial input produces diagnostics, never a panic |
| `tests/native.rs` | integration | 18 | compile → link → execute: stdout, exit codes, wrapping, masked shifts, aborts (101), heap round-trip, pointer arithmetic, `_start` fallthrough |
| `tests/cli.rs` | integration | 14 | stdin source, inline `exec`, exit-status forwarding, REPL accumulation/redefinition/statement mode, pasted programs with blank lines |

`tests/native.rs` builds through the same `driver` API the CLI uses and executes
the produced binaries; `tests/ir.rs` assembles through `driver::emit_ir` and the
LLVM verifier, without linking. Together they cover lexing through the ELF.

Commands:

```
cargo test                 # everything (requires llvm-as, llc, cc)
cargo test --lib           # unit tests only
cargo test --test native   # end-to-end only
cargo test --test opt_levels  # same semantics at --opt 0..3
cargo test --test robustness  # malformed input never panics
```

---

## 7. Reproducibility

- Toolchain pinned in documentation: Rust 1.99.1, LLVM 20.1.8.
- No external crates, so no network access and no lockfile drift for the
  compiler itself.
- Deterministic emission: labels and SSA names come from per-function counters;
  globals are numbered in encounter order. The same input produces byte-identical
  IR.
- `llc` optimization level is explicit (`-O0..-O3`), and linking uses fixed flags.
- Known non-determinism: none in the compiler; `llc`/`cc` output may embed build
  IDs, which does not affect behavior.

---

## 8. Known limitations and technical debt

1. Single compilation unit; no modules, imports, or incremental compilation.
2. No debug information (no DWARF, no source maps in the binary).
3. No CoreX-level optimizer: `--opt 0` goes straight to `llc`, while `--opt 1..3`
   runs `opt -passes=default<On>` first (ADR-0012).
4. Array literals and repeats are unrolled; repeats are capped at 1024 elements
   to avoid pathological IR.
5. `alloc` sizes are page-rounded by the kernel; there is no size metadata.
6. Error recovery is coarse (statement/function granularity).
7. Runtime abort messages are duplicated per program as separate globals; a
   shared read-only message table would shrink the output.
8. `_start` bodies whose last statement is an `exit` syscall are recognized as
   diverging, so no synthesized exit is appended; other fallthrough bodies get
   `exit(0)`.
9. Fuzzing is limited to the deterministic generated corpus in
   `tests/robustness.rs`; there is no coverage-guided fuzzing or property testing.
10. No golden IR snapshots; IR tests assert structure and verifier acceptance.
11. Only Linux x86-64 is targeted; no cross-compilation, ARM64, or bare-metal
    variants.
12. Parser nesting is capped at 256 recursion levels (`E0111`) to bound
    recursion; deeply nested generated code is rejected rather than overflowing
    the stack.
13. The language has no structs, enums, floats, or function pointers, so pointer
    code must be expressed in terms of byte and integer types.

---

## 9. Extension guide

| Change | Files, in order |
|---|---|
| Add a keyword | `token.rs` (kind, symbol), `lexer.rs` (keyword map), `parser.rs` (use) |
| Add a type | `ast.rs` (`Type`, `IntTy`, `llvm()`, size/align), `parser.rs` (`parse_type`), `semantic.rs` (`check_expr`), `codegen.rs` (value/place handling) |
| Add an operator | `token.rs`, `lexer.rs`, `ast.rs` (`BinOp`/`UnOp`), `parser.rs` (`binding_power`), `semantic.rs` (`check_binary`), `codegen.rs` (`gen_binary`) |
| Add a builtin | `semantic.rs` (`BUILTINS`, `check_call`, `Callee`), `codegen.rs` (`gen_call`, `helpers`) |
| Change runtime behavior | `codegen.rs` (`helpers`, `fatal`), `docs/language-spec.md` §8–§10, `docs/memory-model.md`, add a native test |
| Add a diagnostic | emit in the stage that detects it, document the code in `docs/language-spec.md` §11, add a case to `tests/diagnostics.rs` |

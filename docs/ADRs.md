# Architecture Decision Records

APPEND ONLY. Never delete or rewrite an entry. Supersede a decision with a new
ADR that references the old one. Newest entries go at the bottom.

Every entry records: date, status, context, decision, consequences,
alternatives considered, and evidence. Entries marked ACCEPTED are implemented;
`PROPOSED` entries are not.

---

## ADR-0001 — Implement the compiler in Rust

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** CoreX needs a compiler that is correct, maintainable, and
  reproducible, written under time constraints. Candidate languages:
  Rust, C++, Zig, OCaml, Python.
- **Decision:** Implement in Rust (edition 2021) with **zero external crates**.
- **Consequences:**
  - Memory safety for compiler internals without a GC.
  - `cargo test` gives unit and integration testing with no extra framework.
  - No dependency resolution, no network needed to build.
  - Rust string/`HashMap` ergonomics are adequate for a lexer/parser.
  - Borrow-checker friction around AST + side tables was avoided by keeping the
    AST immutable during analysis and returning a separate `Analysis` struct.
- **Alternatives:**
  - C++: faster to write for some, but manual memory management in a compiler
    with error recovery is a liability; no built-in test harness.
  - OCaml: excellent for compilers, but adds an unfamiliar toolchain and runtime
    to the project, and the deliverable is a C-ABI-producing compiler.
  - Python: iteration speed, but runtime performance and type discipline are
    insufficient for a maintainable multi-thousand-line compiler.
- **Evidence:** `compiler/Cargo.toml` has no `[dependencies]`; `cargo test`
  passes 78 tests with `rustc 1.99.1`.

---

## ADR-0002 — Emit textual LLVM IR and shell out to `llvm-as`/`llc`

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** A backend must turn the AST into x86-64 machine code. Options:
  (a) link LLVM via `llvm-sys`/`inkwell` FFI bindings; (b) emit LLVM textual IR
  and invoke LLVM command-line tools; (c) write an x86-64 assembler backend;
  (d) use Cranelift.
- **Decision:** (b). `codegen.rs` produces IR text; `driver.rs` runs `llvm-as`
  (which assembles and runs the verifier), `llc` (object generation), and `cc`
  (`-nostdlib -static -no-pie` link).
- **Consequences:**
  - No `unsafe` FFI, no LLVM build dependency, no ABI coupling to a specific
    `libLLVM`.
  - Generated IR is text: reviewable, diffable, and directly testable
    (`emit-ir`, verifier-in-the-loop).
  - Verification is mandatory, not optional: a green build proves the verifier
    accepted the module.
  - Cost: process spawn per compilation step (milliseconds), external tool
    availability at runtime, and no LLVM API for advanced features (no
    target-machine introspection, no multi-module linking).
  - LLVM version is a documented runtime requirement (20.1.8 verified), not a
    compile-time link-time constant.
- **Alternatives:**
  - `inkwell`/`llvm-sys`: richer API, but pins the LLVM version at build time,
    requires LLVM dev libraries on every developer machine, and adds unsafe code
    to the trusted path.
  - Hand-written x86-64 backend: maximum control and no external tools, but it
    would dominate the schedule and lose LLVM's optimizer; it also makes the
    "LLVM IR passes verification" acceptance criterion impossible.
  - Cranelift: no textual IR, heavier dependency, weaker diagnostics.
- **Evidence:** `docs/ir-tests` (`tests/ir.rs`) assemble every example with
  `llvm-as`; the driver refuses to link IR the verifier rejects (`E0300`).

---

## ADR-0003 — Memory model: explicit allocation plus raw pointers gated by `unsafe`

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** The language must have no GC and no hidden allocation. Three
  candidate models were required: (A) explicit allocate/free, (B) ownership and
  borrowing, (C) hybrid manual allocation behind ownership-checked abstractions.
- **Decision:** Model A with a capability gate.
  - `alloc(bytes) -> *mut u8` (mmap) and `free(ptr, bytes)` (munmap).
  - Raw pointers `*T` / `*mut T` with no lifetimes or borrowing.
  - Dereference, pointer arithmetic, integer/pointer casts, `alloc`, `free`, and
    `syscall` require a lexical `unsafe` block; everything else is checked.
  - Compile-time checks: type correctness, mutable-place writes, `&mut` of
    mutable places, constant index bounds, `usize` indices, return paths.
  - Runtime checks: dynamic index bounds, division by zero, `MIN/-1`,
    allocation failure — all abort with exit 101.
- **Consequences:**
  - The compiler stays small enough to finish and test; no region inference,
    no move semantics, no lifetime errors.
  - CoreX **does not claim memory safety** for pointer code, and the docs say so
    in `docs/memory-model.md` §6–§7.
  - Unsafe code is greppable (`unsafe` blocks are lexical regions).
  - Aliasing is unrestricted; no `noalias`/TBAA is emitted, so optimizations
    that depend on strict aliasing are unavailable.
- **Alternatives:**
  - Model B: strong safety, but a borrow checker plus lifetimes would multiply
    compiler size and delay a working MVP; also conflicts with the freestanding,
    C-interoperable goal for raw addresses.
  - Model C: partial safety invites false confidence and needs a handle type
    system and escape analysis; recorded as a future opt-in layer, not v0.1.
- **Evidence:** `semantic.rs` enforces `unsafe` (`E0209`), mutability (`E0205`,
  `E0220`), and constant bounds (`E0222`); `codegen.rs` inserts the dynamic
  bounds check and the division guard; `tests/native.rs` asserts exit 101 for
  both abort paths; `docs/memory-model.md` documents the guarantee matrix.

---

## ADR-0004 — Defined integer semantics: wrapping arithmetic, masked shifts, aborting division

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** C leaves signed overflow, shift counts, and division by zero
  undefined; that makes programs unpredictable and blocks optimization-free
  reasoning. CoreX promises predictable execution.
- **Decision:**
  - `+ - *` and unary `-` wrap modulo 2^N (LLVM `add`/`sub`/`mul` without
    `nsw`/`nuw`).
  - Shift counts are masked with `bits-1`; signed `>>` is arithmetic.
  - Division or remainder by zero and `MIN / -1` / `MIN % -1` abort: message on
    fd 2, exit 101.
  - Dynamic array indices are bounds-checked and abort the same way.
- **Consequences:**
  - Every integer program has a defined result or a defined abort.
  - Guard branches cost a few instructions per division and dynamic index; the
    compiler does not hoist them out of loops.
  - Wrapping means overflow bugs are silent (by design); there is no checked
    arithmetic mode yet.
- **Alternatives:**
  - C-style undefined behavior: rejected (unpredictable, and LLVM may assume
    no overflow, producing surprising code).
  - Trapping overflow: rejected for v0.1 (would penalize every `+`).
  - Saturating arithmetic: rejected (different numeric model, surprising for
    systems code).
- **Evidence:** `tests/native.rs::integer_arithmetic_wraps_deterministically`,
  `shift_amounts_are_masked`, `division_by_zero_aborts_with_exit_101`,
  `dynamic_out_of_bounds_aborts_with_exit_101`.

---

## ADR-0005 — Freestanding Linux x86-64 target with direct syscalls

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** Bare-metal orientation requires that language semantics be
  independent of an OS runtime, while the first deliverable must be executable.
- **Decision:**
  - Target `x86_64-unknown-linux-gnu`, statically linked, `-nostdlib -no-pie`.
  - OS access only through the `syscall` builtin, lowered to inline asm:
    `"syscall"` with `{rax},{rdi},{rsi},{rdx},{r10},{r8},{r9}`, clobbering
    `rcx`, `r11`, and memory.
  - `alloc`/`free` are implemented as `mmap`/`munmap` helpers in IR, not as
    library calls.
  - Entry: user-provided `fn _start()`, or a synthesized `_start` that calls
    `fn main() -> i32` and exits with its result.
  - Language semantics (types, arithmetic, control flow) contain no syscall
    knowledge; only the builtins and the entry convention are platform-specific.
- **Consequences:**
  - The produced binary has no libc, no dynamic loader, no startup code.
  - `syscall` numbers are literal `usize` values chosen by the programmer;
    portability across OSes requires naming conventions, not compiler changes.
  - Future targets (freestanding x86-64, ARM64) need a new triple, a new syscall
    asm sequence, and an alternative alloc implementation behind the same
    builtins.
- **Alternatives:**
  - Link libc: rejected — violates "minimal runtime" and hides allocation.
  - Provide a CoreX standard library: deferred; the builtins are the minimum
    surface needed to write and test programs.
- **Evidence:** `examples/hello.cx` prints through `write` and exits through
  `exit`; `tests/ir.rs::ir_targets_linux_x86_64_without_libc_calls` rejects
  `@printf`/`@malloc`; `file` reports a statically linked ELF.

---

## ADR-0006 — No implicit numeric conversions; literals adapt, everything else is explicit

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** Implicit integer conversion rules are a major source of bugs and
  a frequent developer complaint in C/C++ research (`docs/research.md`).
- **Decision:**
  - Binary arithmetic requires identical operand types.
  - The only adaptation is an untyped integer literal adopting context: the other
    operand, the declared type, the parameter type, or the array element type.
  - Without context, an integer literal is `i32`.
  - Conversions are always `as`, with truncation or sign/zero-extension
    determined by the source signedness.
  - `*mut T` → `*T` is the only implicit pointer conversion.
- **Consequences:**
  - Mixed-width expressions require explicit casts; this is verbose but
    unambiguous.
  - Literal range errors are caught at the literal (`E0215`), including negative
    minimums.
  - No "usual arithmetic conversions" table to remember.
- **Alternatives:** C-style promotion rules (rejected: subtle and widely
  criticized); Rust-style inference across statements (rejected for v0.1:
  requires unification across the function and interacts poorly with
  diagnostics).
- **Evidence:** `semantic.rs::unify_operands`;
  `tests/diagnostics.rs::rejects_type_mismatches_without_implicit_conversion`.

---

## ADR-0007 — One compilation unit, functions as the only item

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** Modules, imports, and separate compilation add name mangling,
  linking policy, and visibility rules before the language has proven its core.
- **Decision:** A program is one `.cx` file containing only functions. All
  signatures are collected before bodies are checked, so declaration order does
  not matter and recursion works. No globals, `static`, `extern`, or imports.
- **Consequences:**
  - Simple compiler and simple linking (one object file).
  - No incremental compilation and no library story; string literals are the only
    data globals.
  - Adding modules later requires mangled names and a multi-file driver, but not
    a new IR strategy.
- **Alternatives:** Rust-style `mod` and Cargo-like manifests (rejected as
  premature); C-style headers (rejected: textual duplication, no namespacing).
- **Evidence:** `semantic.rs::collect_fns` performs two passes; examples compile
  as single objects; `docs/grammar.ebnf` has `program = { function }`.

---

## ADR-0008 — Runtime bounds checks on dynamic array indices

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** C arrays decay to pointers and indexing is unchecked; this is a
  top source of memory-safety bugs. CoreX arrays are inline values with a known
  compile-time length.
- **Decision:**
  - Constant indices are validated at compile time (`E0222`).
  - Dynamic indices are compared with an unsigned `< N` test; failure prints
    `corex: index out of bounds` and exits 101.
  - Pointers are not indexable (`E0213`); use pointer arithmetic inside `unsafe`
    if an unchecked walk is required.
- **Consequences:**
  - A whole class of bugs becomes a defined abort instead of a silent overwrite.
  - One compare + branch per dynamic access; no elimination pass yet.
  - Arrays larger than the stack limit (1 MiB) are rejected, so the `N` used in
    the check is always a sane constant.
- **Alternatives:** no checks (rejected: contradicts the predictability goal);
  optional `--bounds-check=off` flag (deferred until benchmarks justify it).
- **Evidence:** `codegen.rs::gen_place` emits the check;
  `tests/native.rs::dynamic_out_of_bounds_aborts_with_exit_101`;
  `tests/diagnostics.rs::rejects_bad_array_indexing`.

---

## ADR-0009 — Test strategy: stage unit tests plus compile-and-execute integration tests

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** "Source exists" is not evidence. The criteria require IR
  verification and native execution.
- **Decision:**
  - Unit tests per stage (lexer tokens, parser shape/precedence, semantic
    accept/reject, diagnostic rendering).
  - Integration tests call the same `driver` API as the CLI:
    - `tests/diagnostics.rs` asserts one diagnostic code per invalid program;
    - `tests/ir.rs` assembles every example with `llvm-as` and checks structure;
    - `tests/native.rs` links and executes programs, asserting stdout and exit
      status, including the abort paths.
  - No mocking of the toolchain; tests fail loudly if LLVM tools are missing.
- **Consequences:**
  - Tests require LLVM 20 on `PATH` or `COREX_LLVM_BIN`; this is a documented
    prerequisite, not a skipped test.
  - Execution tests catch ABI and codegen bugs that IR checks cannot.
- **Alternatives:** golden IR snapshots (deferred: brittle across LLVM versions);
  in-process emulation (rejected: does not prove the ELF works).
- **Evidence:** 78 tests pass, including 13 executed binaries.

---

## ADR-0010 — Append-only process records (ADRs, request log)

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** The project requires decision history that cannot be silently
  rewritten after the fact.
- **Decision:** `docs/ADRs.md` and `docs/request-log.md` are append-only.
  Changes of mind are recorded as new entries that supersede old ones by
  reference. `docs/tasks.md`, `master.md`, and `ENGINEERING.md` are living documents.
- **Consequences:** A superseded decision stays visible with its original
  rationale; readers must follow references to know what is current.
- **Alternatives:** a single mutable design document (rejected: loses history).
- **Evidence:** this file.

---

## ADR-0011 — Array repeat syntax `[value; N]` with a 1024-element unroll cap

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** Examples need initialized fixed-size buffers (`[0; 20]`).
  Writing twenty zeros by hand is error-prone; a runtime loop for initialization
  would allocate and branch where a constant is enough.
- **Decision:**
  - Add `[value; N]` as an array literal form (syntax in `grammar.ebnf`).
  - `N` must be a positive integer literal and at most 1024 (`E0221` above).
  - Codegen unrolls element stores in the entry block.
- **Consequences:**
  - Clean buffer initialization in examples and tests.
  - Large repeats produce large IR; the cap keeps compilation predictable.
  - `[expr; N]` evaluates `expr` `N` times (left to right), matching a literal
    list; no `Copy`/clone concept exists.
- **Alternatives:** `repeat(0, 20)` runtime helper (extra call, not constant);
  `llvm.memset` special case for zero (more codegen paths); no syntax (verbose,
  bug-prone examples).
- **Evidence:** `examples/factorial.cx`, `examples/pointers.cx`;
  `parser::tests::parses_array_repeat_literal`;
  `tests/diagnostics.rs::rejects_oversized_array_repeat`.

---

## ADR-0012 — Run the LLVM IR optimizer and realign the entry stack

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** The first pipeline was `llvm-as` → `llc`. `llc` runs the codegen
  pipeline, not the IR optimizer, so every local `alloca` and loop-carried load
  survived: a 10^9-iteration integer loop ran at ~2.5× the time of the same code
  built by gcc `-O2`. Enabling `opt` exposed a second, latent defect: optimized
  builds segfaulted because the kernel enters `_start` with `%rsp` 16-byte
  aligned, while LLVM assumes the SysV callee state (`%rsp % 16 == 8`) and may
  emit aligned SSE spills.
- **Decision:**
  1. For `--opt 1..3`, run `opt -passes=default<O{n}>` on the verified bitcode
     before `llc`.
  2. Mark `_start` (user-defined or synthesized) with `alignstack=16` so LLVM
     realigns the stack at process entry.
  3. Keep `--opt 0` as the unoptimized escape hatch.
- **Consequences:**
  - Loop-heavy code is no longer penalized by unoptimized IR (the measured
    gap closed to parity on the integer kernel; see `docs/benchmarks.md`).
  - `opt` becomes a required tool for `--opt > 0`; its absence is `E0303`.
  - Optimization can change the algorithm being measured, so benchmarks must
    verify that the workload still exists (an earlier loop was closed-formed
    away — recorded in `docs/benchmarks.md`).
  - `alignstack=16` costs one stack realignment instruction at process start.
- **Alternatives:**
  - Keep `llc` only: rejected — measurably slower generated code.
  - Hand-written prologue with `and rsp, -16` via inline asm: rejected — it
    breaks LLVM's frame assumptions.
  - Link a C runtime that provides a crt1 `_start`: rejected — violates the
    freestanding, no-libc constraint.
- **Evidence:** `driver.rs` runs `opt` before `llc`;
  `tests/native.rs` compiles at `--opt 2` (and one case at `--opt 3`) and asserts
  correct output; `docs/benchmarks.md` records the before/after measurement.

---

## ADR-0013 — Cap a single stack value at 1 MiB

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** Locals and array literals are `alloca`s in the entry block. An
  unbounded array type (`[u8; 18446744073709551615]`) either overflows size
  arithmetic or produces an allocation that cannot fit the 8 MiB default stack;
  either way the failure is opaque (LLVM error or `SIGSEGV`).
- **Decision:** Reject any binding, parameter, or return value whose type size
  exceeds `MAX_VALUE_BYTES = 1 MiB` with `E0223`. Heap allocation has no such
  limit; use `alloc` for larger buffers.
- **Consequences:**
  - Stack exhaustion from a single value becomes a compile error with a size
    report instead of a crash or verifier failure.
  - Programs needing more than 1 MiB must use the heap explicitly.
  - The limit is an implementation constraint, documented in the spec §3.1.
- **Alternatives:** allow arbitrary sizes (rejected: opaque failures); grow the
  process stack via `setrlimit` (out of scope, requires libc or a syscall at
  startup); allocate oversized values on the heap implicitly (rejected: hidden
  allocation violates a core constraint).
- **Evidence:** `semantic.rs::check_value_size`; `MAX_VALUE_BYTES`; documented in
  `docs/language-spec.md` §3.1 and §11 (`E0223`).

---

## ADR-0014 — Terminal run paths: stdin source, `exec`, and a definition-accumulating REPL

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** Trying a language should not require creating a file. Users asked
  for a way to "directly run the code into the terminal". A conventional
  read-eval-print loop is not possible for a compiled, freestanding language in
  the usual sense: every run is a new process, so there is no live value state to
  inspect, and CoreX has no print intrinsic.
- **Decision:**
  1. Every command accepts `-` as the input path and reads the source from stdin
     (`corex run - < examples/hello.cx`).
  2. `corex exec '<source>'` compiles and runs source passed as an argument,
     forwarding the program's exit status.
  3. `corex repl` accumulates submitted top-level functions keyed by name;
     redefining a function replaces the earlier definition. A blank line submits
     and runs the combined program, which is re-parsed and re-analyzed from
     combined source so diagnostics point at real text.
  4. A submission that is not a set of items is wrapped as
     `fn main() -> i32 { <submission> return 0; }`, replacing any previous
     `main`. This is disclosed in the session output.
  5. `:` commands: `:run`, `:ir`, `:show`, `:reset`, `:quit`, `:help`.
- **Consequences:**
  - A failed submission does not corrupt the session: parse errors are reported
    and nothing is merged; semantic errors leave the accumulated source intact.
  - Each submission recompiles the whole program (milliseconds), which keeps the
    REPL honest: no hidden incremental state, no divergence from batch behavior.
  - Programs that read stdin cannot be driven reliably from inside the REPL,
    because the REPL buffers stdin; use `corex run file.cx` for those. Documented
    in the module and README.
  - No expression evaluation: typing `1 + 2` is a statement block wrapped in a
    temporary `main`, not a printed value. A value-printing mode would need a
    language-level formatting facility, which v0.1 does not have.
- **Alternatives:**
  - A true read-eval-print loop with persistent state: rejected — requires a JIT
    and an interpreter, contradicting the AOT/freestanding design.
  - Auto-printing expression results: rejected — no print intrinsic and no
    generic formatting in v0.1.
  - A separate `corex-repl` binary: rejected — duplicates CLI plumbing for no
    benefit.
- **Evidence:** `compiler/src/repl.rs`, `compiler/tests/cli.rs` (6 tests covering
  stdin, `exec`, status forwarding, accumulation, redefinition, statement mode).

---

## ADR-0015 — Divergence analysis recognizes `while true` and exit syscalls

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** A user wrote a `main` whose last statement was `syscall(60, ...)`
  and got `E0206` ("can reach the end of its body"), even though the program
  never returns. `while true { }` had the same false positive. Both are ordinary
  ways to write non-returning programs, and the diagnostic wording also read
  "a `i32` value".
- **Decision:** Extend `stmt_diverges` with two rules:
  1. `while true { ... }` diverges when it contains no `break` belonging to it.
  2. A tail expression statement that calls `syscall` with the constant number
     60 (`exit`) or 231 (`exit_group`) diverges.
  Reword `E0206` to "without returning a value of type `i32`" and list all
  accepted diverging forms in the note.
- **Consequences:**
  - Exit-style programs compile without a dummy `return 0;`.
  - `_start` bodies ending in `exit` no longer receive a redundant synthesized
    exit call (codegen stops after a diverging tail).
  - The analysis stays conservative: non-constant syscall numbers and
    `if true` without `else` are not treated as diverging. A program that hides
    an exit inside a variable syscall number still needs an explicit `return`.
- **Alternatives:** Treat any `syscall` call as potentially non-returning
  (rejected: would accept programs that do fall through); require `_start` for
  exit-style programs (rejected: adds ceremony for a common case); make
  `E0206` a warning (rejected: leaves a function without a value on a reachable
  path, which would have to become an implicit trap).
- **Evidence:** `semantic.rs::{stmt_diverges,is_process_exit}`;
  unit tests `accepts_exit_syscall_as_a_diverging_tail`,
  `accepts_while_true_without_break`, `rejects_while_true_with_break`,
  `rejects_non_exit_syscall_fallthrough`;
  `tests/cli.rs::exec_accepts_a_program_terminating_with_the_exit_syscall`.

---

## ADR-0016 — REPL submits only syntactically complete blocks

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** The first REPL submitted on any blank line. Pasting a program
  whose function body contained a blank line (including `examples/hello.cx`)
  submitted a half-typed function, produced `E0100 expected '}'`, then wrapped
  the remainder in a temporary `main` and reported follow-on errors. The session
  looked broken.
- **Decision:**
  1. A blank line submits only when `needs_more_input(chunk)` is false. That
     helper tracks bracket depth and open strings/block comments outside those,
     and rejects a dangling trailing token (`=`, arithmetic/compare operators,
     `,`, `.`, `:`, or a trailing keyword such as `let`, `return`, `fn`).
  2. Blank lines encountered while a block is incomplete are preserved in the
     buffer (so line numbers in diagnostics stay correct).
  3. `:submit` (alias `:go`) forces compilation of a pending block, for when a
     user wants to see the errors of an intentionally incomplete fragment.
- **Consequences:**
  - Pasting a complete program, including blank lines, works: the block is
    submitted once, at the end.
  - A genuinely broken but balanced block still submits and shows diagnostics.
  - An unbalanced or dangling fragment waits for more input; the user escapes
    with `:submit` or Ctrl-D (EOF submits what is pending).
- **Alternatives:** Require an explicit terminator such as `.run` on every
  submission (rejected: noisy); detect completeness by trying to parse on every
  blank line and only submit on success (rejected: a block with a real syntax
  error could never be submitted); brace counting without the dangling-token
  heuristic (rejected: `let x: i32 =` would submit an error prematurely).
- **Evidence:** `repl.rs::needs_more_input`; `tests/cli.rs`:
  `repl_accepts_a_pasted_program_with_internal_blank_lines`,
  `repl_accumulates_definitions_and_runs_on_blank_line`.

---

## ADR-0017 — `print` is a language intrinsic, not a library

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** Nothing in the language could display a value: examples wrote
  raw bytes through `syscall`. That made a Python-like REPL impossible and made
  every program reimplement decimal formatting.
- **Decision:** Add `print(value: integer | bool) -> ()` as a reserved builtin.
  Codegen lowers it to `__corex_print_i64` (sign + magnitude),
  `__corex_print_u64`, or `__corex_print_bool`, each formatting into a 20-byte
  stack buffer and issuing `write(1, ...)` plus a newline. It is safe (no
  `unsafe` needed), allocates no heap memory, and is type-checked (pointers,
  arrays, and `void` are rejected).
- **Consequences:**
  - The REPL can evaluate and display expressions; examples and tests can show
    results without hand-written formatting loops.
  - One more reserved name (`E0202` if redeclared) and a fixed output format
    (decimal, `-` prefix, `true`/`false`, trailing newline) that is now part of
    the language.
  - A failed `write` is ignored; `print` has no error channel, consistent with
    the other builtins.
  - No formatting language, width, base, or floating-point support; a future
    `format` facility would supersede this decision.
- **Alternatives:** A `fmt`/`format` API (rejected for v0.1: needs a string
  type); REPL-only synthesized printing (rejected: hidden behavior, unusable in
  programs); leaving formatting to users (rejected: every example duplicated it).
- **Evidence:** semantic `print_*` tests; native test
  `print_formats_integers_and_bools` covers `0`, negatives, `i8`/`u8` bounds,
  `u64::MAX`, `i64::MIN` and both booleans.

---

## ADR-0018 — REPL: printed expressions, Enter-to-run, raw-mode editing

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** The REPL required a blank line for everything, printed nothing for
  an expression, and had no editing or history. Users expected a Python-like
  terminal.
- **Decision:**
  1. A chunk without a trailing `;` is first tried as `print(<chunk>)`, then as a
     statement block; the first candidate that analyzes cleanly is used.
     Candidates are analyzed together with the session's existing definitions.
  2. A complete one-line chunk runs on Enter. A multi-line chunk runs when it is
     complete and defines `main`/`_start` (so pasting a program works), otherwise
     a blank line or `:submit` runs it.
  3. Blank lines inside a block and between pasted top-level functions do not
     submit (see ADR-0016 and `should_blank_submit`).
  4. `lineedit.rs` implements raw-mode editing via the external `stty` utility:
     arrows, Home/End, Delete, Backspace, Ctrl-A/E/B/F/U, Ctrl-C to abandon the
     current input, Ctrl-D to exit, and Up/Down history persisted in
     `~/.corex_history`. Terminal settings are restored by `Drop`. Non-TTY
     stdin keeps the plain read path, so pipes and tests are unaffected.
- **Consequences:**
  - One-liners behave like a familiar REPL; multi-statement work needs a block
    (`{ ... }`) because each submission is a new process — bindings never persist
    across submissions, and the REPL says so when an undefined name appears.
  - `stty` is required only when stdin is a terminal; if it is missing, editing
    degrades to plain reads rather than failing.
  - History is per-user plain text; no search, no completion.
  - A definitions-only submission (for example a helper function followed by a
    blank line) installs silently: `E0212` (no entry point) is suppressed for
    automatic submissions and only reported by `:run`/`:submit`. This keeps paste
    from printing errors for every function separator.
- **Alternatives:** A readline dependency (rejected: zero-crate policy);
  termios through raw syscalls/FFI (rejected: unsafe code in the CLI for a
  convenience feature); requiring `;` to decide evaluation (rejected: Python-like
  typing is the point); persisting interpreter state (rejected: contradicts AOT
  compilation, see ADR-0014).
- **Evidence:** `tests/cli.rs` (`repl_evaluates_an_expression_and_prints_the_value`,
  `repl_statement_semicolon_suppresses_the_value`,
  `repl_explains_missing_cross_submission_bindings`,
  `repl_accepts_a_pasted_program_with_internal_blank_lines`); manual pty run via
  `script` verifying Enter, Up-arrow recall, and Backspace.

---

## ADR-0019 — Defined semantics must hold at every optimization level, and are tested

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** CoreX defines behavior that C leaves undefined: wrapping `+ - *`,
  masked shift counts, aborting division and bounds violations. Those guarantees
  are enforced by the IR the compiler emits, but LLVM is free to transform any
  module that lacks `nsw`/`nuw`/`noalias` metadata — and free to *add* flags when
  it proves them safe. A user reported the specific risk: `INT_MAX + 1` compared
  against `INT_MIN` inside an `if`, which must print 42 at `-O0` through `-O3`.
- **Decision:**
  1. The emitter never attaches `nsw` or `nuw` to integer arithmetic. A test
     asserts this over every example and every semantic case.
  2. `tests/opt_levels.rs` compiles and executes each semantic case at
     `--opt 0`, `1`, `2`, and `3`, asserting identical stdout, stderr markers, and
     exit status: signed/unsigned wrapping, `i64` boundaries, negation of the
     minimum, masked left/right shifts, division and remainder by zero, signed
     division overflow, dynamic bounds aborts and in-bounds reads, aliasing
     through pointers, `print` boundaries, and all four examples.
  3. A generated differential harness builds one program per integer type
     (`i32`, `u32`, `i8`, `u8`) containing 60 chained operations plus comparisons,
     computes the expected values in Rust from the specification's rules, and
     compares at all four levels. That is 1,104 value assertions per run.
  4. The harness itself is mutation-tested: masking shift counts with the width
     instead of `width-1`, and adding `nsw` to `add`, each make the suite fail.
- **Consequences:**
  - Semantic regressions caused by optimizer interaction are caught in CI, not by
    users.
  - The suite takes ~3 seconds extra; it is part of `cargo test`.
  - The optimizer may still add flags where it proves them sound; only observable
    behavior is asserted, so sound transformations remain allowed.
  - This covers integers, pointers, arrays, and I/O only. Floats do not exist in
    v0.1; if added, they need their own determinism policy (`fast-math` is off by
    default).
- **Alternatives:** trust LLVM (rejected: the guarantee would be unverified);
  test only `-O2` (rejected: optimizations differ per level); assert on optimized
  IR shape (rejected: brittle and wrong — sound flag insertion is legal);
  `opt-bisect` in CI (deferred: useful for diagnosing a failure once the coarse
  test fires).
- **Evidence:** `tests/opt_levels.rs` (5 tests): mutation of shift masking fails
  with value mismatches; mutation adding `nsw` fails with
  "`signed_add_wrap`: emitted IR contains ` nsw`"; the restored compiler passes
  the full 106-test suite.

---

## ADR-0020 — Correction to ADR-0018: blank-line submission is unconditional when complete

- **Date:** 2026-10-21
- **Status:** ACCEPTED (supersedes the blank-line clause of ADR-0018)
- **Context:** ADR-0018 introduced a `should_blank_submit` gate so blank lines
  between pasted top-level functions would not submit. That gate also swallowed a
  helper-only definition typed by hand (`fn twice(...) { }` followed by a blank
  line was never installed, so later calls failed with `E0201`). The gate was
  removed during the fact-check pass.
- **Decision:**
  1. A blank line submits whenever the buffer is syntactically complete
     (`needs_more_input` is false) and contains at least one token.
  2. Definitions without an entry point install silently: `E0212` is suppressed
     for automatic submissions and only reported by `:run`/`:submit`.
  3. Auto-submit is unchanged: one-line chunks run on Enter; multi-line chunks run
     when complete and defining `main`/`_start`.
- **Consequences:**
  - Pasting a program still works: each function separator installs definitions
    quietly, and the chunk that completes `main` runs the whole session once.
  - Hand-typed helpers install on the blank line, as expected.
  - The `should_blank_submit` helper no longer exists; any reference to it is
    historical.
- **Alternatives:** keep the gate (rejected: breaks helper-only definitions);
  require `:submit` for definitions (rejected: extra ceremony).
- **Evidence:** `tests/cli.rs::repl_installs_a_definition_submitted_on_a_blank_line`
  and `repl_accepts_a_pasted_program_with_internal_blank_lines`; manual paste of
  `examples/factorial.cx` runs once and prints `3628800`.

---

## ADR-0021 — ADR-0014 decisions 3–4 are superseded (expression evaluation and `print`)

- **Date:** 2026-10-21
- **Status:** ACCEPTED (supersedes ADR-0014 decisions 3 and 4)
- **Context:** ADR-0014 recorded that the REPL would not evaluate expressions and
  that no `print` facility existed. ADR-0017 added the `print` intrinsic and
  ADR-0018 added printed-expression evaluation in the REPL. The append-only rule
  requires a forward reference rather than an edit to the older record.
- **Decision:** ADR-0014 decisions 3–4 are superseded by ADR-0017 and ADR-0018.
  Decision 1 (source from stdin), decision 2 (`exec`), and decision 5
  (`:` commands) remain in force. There is still no value-printing *expression
  mode without* `print`: typed expressions are wrapped as `print(<expr>)`, and a
  trailing semicolon runs them as statements.
- **Consequences:** Readers following ADR-0014 must continue to ADR-0017/0018 for
  the current behavior. The original rationale (no interpreter, recompile per
  submission) still holds.
- **Evidence:** `tests/cli.rs::repl_evaluates_an_expression_and_prints_the_value`,
  `repl_statement_semicolon_suppresses_the_value`; semantic `print_*` tests.

---

## ADR-0022 — Dual-license the project under MIT OR Apache-2.0

- **Date:** 2026-10-21
- **Status:** ACCEPTED
- **Context:** The repository was public but unlicensed, which legally means
  "all rights reserved": nobody could fork, modify, or ship it. The user asked
  for full open source with forking allowed.
- **Decision:** Dual-license the compiler, tests, examples, benchmarks, scripts,
  and documentation under MIT OR Apache-2.0, at the recipient's option. No CLA,
  no copyright assignment; contributions are inbound=outbound under the same
  terms. Copyright holder is recorded as "CoreX contributors".
- **Consequences:**
  - Anyone may use, modify, redistribute, sublicense, and sell the project,
    including in proprietary products, and may fork it.
  - Apache-2.0 provides an express patent grant; MIT provides the shortest
    permissive terms. Users choose.
  - No copyleft: improvements need not be published. This is the standard Rust
    ecosystem trade-off and maximizes adoption.
  - `THIRD_PARTY.md` records that Rust, LLVM, `cc`, and `stty` are build tools
    that are not redistributed, and that compiled programs are not derivative
    works of the compiler.
- **Alternatives:** GPL-3.0 (rejected: copyleft discourages compiler adoption and
  linking into proprietary toolchains); BSL/unlicense (rejected: non-OSI or
  public-domain dedication is legally uneven across jurisdictions); single MIT
  (rejected: no patent grant); single Apache-2.0 (rejected: some users avoid it
  for license-compatibility reasons).
- **Evidence:** `LICENSE-MIT`, `LICENSE-APACHE`, the `license` field in
  `compiler/Cargo.toml`, README "License", and `THIRD_PARTY.md`.

---

## Open questions (PROPOSED, not implemented)

These are recorded so they are not lost. None is decided.

1. **Opt-in checked handles (Model C).** Would allow safe heap objects without a
   full borrow checker. Requires a handle type, escape analysis, and a `free`
   policy; not scheduled.
2. **Overflow checking mode.** A `--checked-arithmetic` flag could emit `sadd.with.overflow`
   and abort on wrap. Needs benchmarks to justify the cost.
3. **Module system and multi-object linking.** Requires name mangling, visibility,
   and a driver that accepts multiple inputs.
4. **Cross-compilation (ARM64, freestanding).** Requires a target abstraction for
   syscall sequences and allocator builtins.
5. **Debug information.** Requires source maps through the AST into IR metadata;
   significant work, high debugging value.

## ADR-0023 — Extended values: named types, aggregates, and slice representation

**Date:** 2026-10-21
**Status:** Accepted
**Context:** The language grew from scalar integers/booleans/pointers/arrays to
include characters, floats, globals, enums, structs, slices, and `str`. These
features share three compiler-wide problems: resolving names that the parser
cannot classify, choosing one runtime representation for aggregates, and keeping
the already-working value/place code path intact.

**Decision:**

1. **Named types are resolved in semantic analysis.** The parser emits
   `Type::Named` for every identifier in type position. Semantic analysis resolves
   it against the enum and struct tables, producing `Type::Enum` or
   `Type::Struct`. Analysis side tables (`expr_types`, `fn_sigs`, `cast_types`,
   `globals`) hold only resolved types, so codegen never interprets a name.
2. **Structs share a layout object.** Each struct is one
   `Rc<StructInfo>` containing its fields, size, alignment, and resolved flag.
   Pointer fields may reference the same object while fields are being resolved;
   a direct by-value cycle is detected during layout and rejected with `E0224`.
   Layout is natural (field alignment, final size rounded to struct alignment),
   matching LLVM's literal struct layout.
3. **All aggregates are pointers to storage while being generated.** Arrays,
   structs, slices, and `str` are represented by an SSA pointer to an entry-block
   allocation. Binding, assignment, and element/field stores perform an explicit
   aggregate `load` + `store`; scalar reads load from the place. This keeps one
   `gen_place` / `gen_expr` model for existing arrays and new types.
4. **Aggregate ABI is compiler-internal.** Aggregate parameters are passed as
   `ptr` and copied into the callee's slot in the entry block, preserving value
   semantics. Aggregate returns use a hidden `ptr %sret` first parameter and
   `ret void`; callers allocate the result slot. Scalars keep the System V
   register ABI. No C interoperability is promised for aggregates.
5. **Slices and `str` are two-word values.** `[T]` and `str` are laid out as
   `{ ptr, i64 }` (length in elements; bytes for `str`), copied shallowly.
   String literals are private byte constants plus a stack-built view. `&array`
   in a slice context records the array length; range expressions reuse the same
   view and check `start <= end <= len`.
6. **String literals are `str` by default, arrays on demand.** Context expecting
   `[u8; N]` of the same length makes the literal an array value; otherwise it is
   `str`. This preserves the existing array-literal programs while enabling
   `len`, indexing, slicing, printing, and slice parameters.
7. **Constant evaluation is semantic and explicit.** Globals store typed
   `ConstValue`s computed by a recursive evaluator restricted to literals,
   operators, casts, enum variants, string/array literals, and `&global`.
   Mutable or forward global reads are rejected with `E0226`; codegen only
   formats already-computed constants.

**Consequences:** New aggregate types reuse the established copy-on-bind code
path and can be nested without special cases. The cost is an extra alloca/copy
for aggregates and a non-C ABI. `Rc<StructInfo>` uses interior mutability, but
only semantic analysis mutates it; after analysis the layout is immutable in
practice. Forward references between constants are not supported, which is
documented rather than hidden.

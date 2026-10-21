# CoreX Product Requirements Document - v0.1 (MVP)

| Field | Value |
|---|---|
| Document | `docs/PRD.md` |
| Version | 0.1 |
| Date | 2026-10-21 |
| Status | Draft for implementation |
| Scope | CoreX v0.1 MVP only |
| Related | `docs/NFR.md` (non-functional requirements) |
| Owner | ASSUMPTION: CoreX maintainers; no named owner recorded in the repository |

All statements marked ASSUMPTION are decisions made while writing this document, not facts supplied by the product brief. They are listed again in section 11 and must be confirmed or overridden before v0.1 ships.

## 1. Product vision

CoreX is a low-level, statically typed, ahead-of-time compiled systems language. The reference compiler is written in Rust, emits LLVM 20 textual IR, and produces freestanding Linux x86-64 ELF executables that call the kernel directly: no libc, no runtime library, no garbage collector, no hidden allocation, no background threads, no exceptions, no reflection.

The product thesis is narrow: a systems language whose entire semantics fit in one document, whose memory behavior is fully explicit, and whose every semantic rule is enforced by an automated test. A programmer should be able to predict the machine-level behavior of a CoreX program by reading its source and the language reference.

v0.1 succeeds when a developer can install a pinned toolchain, run `corex build hello.cx`, receive a static ELF, execute it on Linux x86-64, and rely on the documented behavior of arithmetic, memory, and `unsafe`.

CoreX v0.1 is not a general-purpose application language. It is a single-compilation-unit language with no standard library, no floats, and no string type, intended for programs that own their memory layout and speak to the kernel directly.

## 2. Target developers

| Segment | Need | What v0.1 provides | What v0.1 does not provide |
|---|---|---|---|
| Systems programmers writing freestanding Linux tools (init processes, tiny utilities, containers' first process) | Small static binaries with explicit syscalls and no libc dependency | Freestanding ELF, direct `syscall`, explicit `alloc`/`free` | Standard library, C FFI, dynamic linking |
| OS and compiler course students and instructors | A language small enough to read completely in a semester, with a real LLVM backend | Small type system, small statement set, readable IR output | Modules, generics, an ecosystem of examples beyond the v0.1 corpus |
| Security and research engineers needing minimal trusted computing bases | Tiny binaries, visible syscall surface, no runtime initialization | No runtime, no constructors, no hidden threads, explicit unsafe | Formal verification, sandboxing, capability model |
| Compiler engineers studying LLVM textual IR pipelines | Inspectable, deterministic IR and a clean lowering path | `emit-ir`, LLVM 20 textual output, `llvm-as` verification | Custom optimization passes, debug info, incremental compilation |

Prerequisites for users: familiarity with two's-complement integers, memory layout, and the Linux x86-64 syscall ABI. Rust knowledge is required only to build the compiler, not to use the language.

Explicit non-audience for v0.1: application developers who need strings, floats, collections, or a package ecosystem; teams that need to call C libraries; anyone who requires memory safety without manual review.

## 3. Primary use cases

**UC-1: Freestanding hello.** Write a program that places a byte-array literal in a fixed-size array, calls `syscall` with the `write` number to emit it on file descriptor 1, then calls `syscall` with `exit`. Observable outcome: the bytes appear on stdout and the process exit status matches the program's chosen code; the ELF has no interpreter and no libc symbols.

**UC-2: Explicit heap.** Allocate a region with `alloc(n)`, write a bump-allocated structure through a `*mut u8` inside an `unsafe` block, and release it with `free(ptr, n)`. Observable outcome: the program runs without touching allocator metadata, because none exists; a syscall trace shows exactly the `mmap`/`munmap` calls the program requested.

**UC-3: File tool.** Use raw syscalls (`open`, `read`, `write`, `close`) to copy bytes between files. Observable outcome: the copy matches the input byte-for-byte and the exit status reflects errors returned by the kernel.

**UC-4: Integer algorithm.** Implement hashing, checksum, or recursive arithmetic over `u32`/`u64`/`i64`. Observable outcome: results match the two's-complement wrapping semantics defined in section 4, including overflow cases and masked shift amounts.

**UC-5: Inspect the pipeline.** Run `corex emit-ir prog.cx` and feed the result to `llvm-as` to see the generated module. Observable outcome: `llvm-as` accepts the module, and the IR is stable across repeated runs.

**UC-6: Learn from rejection.** Submit a program that dereferences a raw pointer outside `unsafe`, or adds an `i32` to an `i64`. Observable outcome: the compiler rejects it with a stable diagnostic code and a source span, exits nonzero, and never panics.

## 4. Functional requirements

Each requirement is testable and is covered by at least one automated test (see `docs/NFR.md`, NFR-T7). "Trap" means the process terminates by a hardware exception; on Linux x86-64 division by zero raises `SIGFPE`.

### 4.1 Types

**FR-1 Primitive integer and boolean types.** The compiler MUST accept `i8`, `i16`, `i32`, `i64`, `u8`, `u16`, `u32`, `u64`, `usize`, `isize`, and `bool`. Widths are 8/16/32/64 bits for the explicit types; `usize` and `isize` are 64 bits on the v0.1 x86-64 target. `bool` is 1 byte and has exactly the values `true` and `false`. Signed types use two's-complement representation.
*Verification:* compile-and-run programs per type that produce observable results at each width; a test asserting `usize`/`isize` are 64-bit (via `[T; N]` sizing or allocation arithmetic).

**FR-2 Unsupported types are rejected.** Float types, character types, string types, and any other type name outside FR-1, FR-3, and FR-4 MUST be rejected with a diagnostic. There is no implicit `bool`/integer conversion.
*Verification:* negative tests per rejected type name; negative test assigning an integer to a `bool`.

**FR-3 Raw pointer types.** The compiler MUST support `*T` and `*mut T` for every supported element type `T`, including nested pointers. `*T` and `*mut T` are distinct types. `*mut T` is accepted where `*T` is expected (never the reverse); conversion to and from integers requires `as` inside `unsafe`.
*Verification:* positive declaration and `unsafe` use of `*T`, `*mut T`, `**T`; negative test passing `*mut T` where `*T` is required without a cast.

**FR-4 Fixed-size arrays.** The compiler MUST support `[T; N]` where `N` is a constant integer expression known at compile time. Arrays are value types with copy semantics on binding and assignment. Arrays cannot be passed or returned by value (`E0216`); pass `*T` instead. Indexing uses zero-based `usize` indices. Slicing does not exist.
*Verification:* arrays of primitives and pointers; assignment copies; out-of-range constant index handling per OQ-1; negative test for non-constant `N`.

**FR-5 Explicit numeric casts.** The `as` operator MUST convert between numeric types, and (inside `unsafe`) between integers and pointers; pointer-to-pointer casts are also allowed. Narrowing truncates, and widening sign- or zero-extends according to the source signedness. No implicit widening, narrowing, or sign conversion is permitted anywhere, including in function arguments, returns, and operators.
*Verification:* round-trip and truncation tests per source/destination pair; negative tests for implicit conversions in calls, returns, and binary operators.

### 4.2 Functions and statements

**FR-6 Function declarations.** Functions use the form `fn name(p1: T1, p2: T2) -> R`. The return type is optional: omitting `->` declares a function that returns no value. There is no writable `void` type. Parameters are passed by value. Functions may call themselves and each other recursively. Overloading, default parameters, variadic user functions, closures, and nested function definitions are not supported.
*Verification:* positive tests for multi-parameter functions, mutual recursion, deep recursion within stack limits; negative tests for missing return type, duplicate parameter names, and overload attempts.

**FR-7 Entry point and exit status.** Compilation MUST produce an executable whose entry point runs the program's designated entry function and terminates the process through the kernel. The entry function is `fn main() -> i32` or `fn _start()`; `_start` takes precedence. For `main` the compiler synthesizes `_start`, and the returned value becomes the process exit status (low 8 bits, as on Linux). A `_start` body that falls through exits 0; a `_start` body ending in `exit` gets no synthesized exit.
*Verification:* integration test asserting the process exit status for several return values; ELF inspection confirming no libc `_start`.

**FR-8 Statements and scoping.** The compiler MUST support `let` (immutable binding), `let mut` (mutable binding), assignment to mutable places, expression statements, `if`/`else`, `while`, `loop`, `break`, `continue`, `return`, and `unsafe` blocks. Blocks introduce scopes. Assignment to a binding not declared `mut` MUST be rejected. `break` and `continue` outside a loop MUST be rejected; `return` outside a function is impossible by grammar.
*Verification:* positive tests per statement form, including nested blocks, shadowing per OQ-5, and loop control; negative tests per illegal form.

**FR-9 Conditions are boolean.** Conditions of `if` and `while` MUST have type `bool`. Integer truthiness does not exist.
*Verification:* negative tests using `i32`, `u8`, and pointer values as conditions.

### 4.3 Operators and semantics

**FR-10 Operator set and precedence.** The compiler MUST implement `+ - * / %`, `== != < <= > >=`, `&& || !`, `& | ^ << >>`, unary `-` and `!`, unary `&` (address-of) and `*` (dereference), `as` casts, indexing `a[i]`, and calls `f(x)`. Precedence and associativity MUST be fixed, documented, and covered by tests; ASSUMPTION: C-like precedence with `as` binding tighter than binary operators. `&&` and `||` short-circuit. Binary arithmetic and comparison operators require identical operand types; mixed types are rejected.
*Verification:* a precedence table test per adjacent pair of operators; short-circuit tests where the right operand traps if evaluated; negative mixed-type tests.

**FR-11 Wrapping integer arithmetic.** `+`, `-`, `*`, and unary `-` on every integer type MUST use two's-complement wrapping in all builds, including unoptimized builds. No overflow trap, panic, or undefined behavior exists. `usize`/`isize` follow their underlying 64-bit signedness.
*Verification:* boundary tests at each width (max + 1, min - 1, min * -1) with expected wrapped values; tests at `--opt 0` and `--opt 3`.

**FR-12 Division, remainder, and traps.** `/` and `%` on unsigned types are the corresponding machine operations. On signed types, division truncates toward zero and the remainder has the sign of the dividend (ASSUMPTION, see OQ-9). Division or remainder by zero MUST abort deterministically: the compiler emits an explicit guard that writes `corex: division by zero or signed overflow` to stderr and exits 101. `MIN / -1` and `MIN % -1` behave the same way. No hardware trap is relied upon.
*Verification:* positive division/remainder tests including negative operands; execution tests asserting exit status 101 and the abort message for `x / 0`, `x % 0`, and the minimum-value case.

**FR-13 Shift semantics.** Shifts MUST mask the shift amount by `bit_width - 1` before shifting, so every shift count is defined and none traps. `>>` is arithmetic for signed types and logical for unsigned types. Shifting by a value of a different integer type is rejected.
*Verification:* tests at counts `0`, `width - 1`, `width`, `width + 1`, and large counts, for signed and unsigned types.

**FR-14 Comparison and logical operators.** `==`/`!=` are defined for integers and for pointers of the same type (address comparison). `bool` and arrays are not comparable; use a boolean value directly in a condition instead of comparing it with `true`. Ordering comparisons are defined for integers only; pointer ordering is rejected (`E0210`). `&&`/`||` require `bool` operands and produce `bool`.
*Verification:* per-type comparison tests; negative test ordering `bool` values if unsupported.

### 4.4 Memory, unsafe, and builtins

**FR-15 Allocation and deallocation builtins.** `alloc(bytes: usize) -> *mut u8` MUST obtain memory from the kernel via `mmap`; `free(ptr: *mut u8, bytes: usize)` MUST release it via `munmap` using the same length. The compiler MUST NOT maintain allocation metadata, headers, free lists, or a heap. ASSUMPTION: returned memory is zero-filled because it comes from fresh anonymous mappings, and the usable region is at least `bytes` long. Calling `free` with a pointer not returned by `alloc`, with a different length, or twice is programmer error (undefined behavior); the compiler performs no validation.
*Verification:* run tests asserting the returned bytes are zero; run tests asserting `free` accepts the exact length; syscall-trace test asserting one `mmap` and one `munmap` per pair; IR inspection confirming no allocator shim.

**FR-16 Unsafe enforcement.** Dereferencing a raw pointer (`*p`), pointer arithmetic, integer/pointer casts, and calls to `alloc`, `free`, and `syscall` MUST be permitted only lexically inside an `unsafe` block. Using any of them outside `unsafe` MUST be a compile error. ASSUMPTION: taking an address (`&place`) is allowed in safe code and yields a raw pointer whose mutability follows the place; dereferencing it still requires `unsafe`.
*Verification:* one negative test per operation outside `unsafe`; positive tests inside `unsafe`; nested-block test confirming `unsafe` scope is lexical.

**FR-17 Syscall builtin.** `syscall(nr, a1, a2, a3, a4, a5, a6)` MUST be available inside `unsafe`, where all seven values are `usize` (cast pointers with `as usize`) and omitted trailing arguments default to 0. The return value is the kernel's result as `isize` (64-bit). The compiler MUST emit a direct `syscall` instruction and MUST NOT emulate, wrap, retry, or translate syscall numbers.
*Verification:* tests calling `write` and `exit`; a test omitting trailing arguments; a negative test outside `unsafe`; IR inspection for a direct `syscall` instruction and no libc calls.

### 4.5 Toolchain, CLI, diagnostics, and IR

**FR-18 CLI surface.** The `corex` binary MUST provide:
- `corex build <file.cx | -> [-o <out>] [--opt <0-3>]` - compile to an executable; default output name is the source stem (`a.out` for stdin); default optimization level 2.
- `corex run <file.cx | -> [--opt <0-3>] [-- <args>...]` - build to a temporary location and execute, propagating the program's exit status.
- `corex exec '<source>' [--opt <0-3>]` - run source given on the command line; `-` reads stdin.
- `corex repl [--opt <0-3>]` - interactive session; one-line chunks run on Enter.
- `corex emit-ir <file.cx | -> [--verify]` - write textual (pre-optimization) LLVM IR to stdout; `--verify` assembles it. `--opt` is rejected here because it would have no effect.
- `corex check <file.cx | ->` - parse and type-check only; produce no artifact; `--opt` is rejected.
`-` means stdin for every file argument.
Unknown commands, unknown flags, missing operands, and unreadable files MUST produce a diagnostic on stderr and a nonzero exit status.
*Verification:* CLI tests per command and flag; tests for unknown command, unknown flag, missing file, unwritable output.

**FR-19 Exit codes.** `0` MUST mean success. Documented codes: `1` compile error, `2` usage or I/O error, `3` external toolchain failure (missing `llvm-as`/`opt`/`llc`, IR rejected, link failure). `corex run` and `corex exec` propagate the program's exit status.
*Verification:* one test per code path asserting the exact exit status.

**FR-20 Diagnostics.** Every rejection MUST carry a stable identifier of the form `Exxxx` (implemented as `E0001`-`E0303`; the earlier `CXnnnn` assumption is superseded), a severity, a human-readable message, and a primary source span as `file:line:column` with 1-based counting. The parser recovers and stops after 100 syntax diagnostics; semantic analysis reports all errors and the CLI prints the count. Malformed input MUST NOT panic or abort (covered by a deterministic fuzz test). Diagnostics go to stderr; program output goes to stdout.
*Verification:* negative corpus asserting exact code and span line; malformed-input corpus asserting no panic and nonzero exit; multi-error test asserting ordering and the cap.

**FR-21 LLVM IR backend.** The compiler MUST emit LLVM 20 textual IR whose module is accepted by `llvm-as` from a pinned LLVM 20 toolchain. `--opt` maps to LLVM optimization levels 0-3; the compiler MUST NOT run custom optimization passes outside that mapping. `emit-ir` output MUST be byte-identical for identical input, flags, and toolchain (see `docs/NFR.md`, NFR-D1).
*Verification:* `llvm-as` on the IR of every accepted test program; twin-run byte comparison; `--opt` matrix test.

**FR-22 Freestanding artifact.** `corex build` MUST produce a statically linked x86-64 Linux ELF executable with no interpreter, no dynamic section, no libc dependency, and no compiler-inserted runtime initialization. Execution MUST begin at the compiler-generated entry sequence and reach user code without calling any external function other than the user's own functions; compiler-emitted internal helpers (syscall wrapper, `print`, `alloc`, `free`, abort path) are part of the module and execute the `syscall` instruction directly. ASSUMPTION: the artifact is a non-PIE static ELF (see OQ-7).
*Verification:* ELF header/section inspection in tests (`no PT_INTERP`, no `.dynamic`, no undefined symbols); execution on Linux x86-64.

**FR-23 Test deliverables.** The repository MUST contain: unit tests for lexer, parser, type checker, and code generation; integration tests that compile and execute programs and assert exit status and byte output; invalid-program tests that assert diagnostic code and span; and IR-verification tests using `llvm-as`. A single documented command MUST run all of them.
*Verification:* CI runs the documented command from a clean clone and fails on any test failure.

## 5. Non-functional requirements

Detail lives in `docs/NFR.md`; this table is the index. Every performance figure there is unvalidated until benchmarked.

| Area | NFR IDs | Headline obligation |
|---|---|---|
| Performance | NFR-P1..P7 | Compile-time and generated-code targets, explicitly unvalidated in v0.1 |
| Memory footprint | NFR-M1..M5 | Compiler peak RSS ceiling; zero runtime allocation not requested by the program |
| Determinism | NFR-D1..D4 | Byte-identical IR and ELF for identical inputs; deterministic diagnostic order |
| Diagnostic quality | NFR-G1..G9 | Stable codes, spans, no panics, documented exit codes, ordered output |
| Build reproducibility | NFR-R1..R6 | Pinned Rust toolchain, pinned LLVM 20, zero external crates, offline build |
| Safety and responsibility | NFR-S1..S8 | What the compiler guarantees and what the programmer must guarantee |
| Portability | NFR-PT1..PT4 | Linux x86-64 only in v0.1; target isolation for future ports |
| Testability | NFR-T1..T8 | Stage-level, integration, negative, and IR-verification tests; CI gates |
| Resource limits | NFR-L1..L5 | Input and compiler limits that produce errors, never crashes |

## 6. MVP scope

### 6.1 In scope

| Area | v0.1 content |
|---|---|
| Language | The types, statements, operators, casts, arrays, pointers, functions, and `unsafe` blocks in FR-1..FR-17 |
| Semantics | Wrapping arithmetic, masked shifts, division-by-zero trap, no implicit conversions, no runtime |
| Memory | `alloc`/`free` over `mmap`/`munmap`, raw pointers, no heap metadata |
| Kernel interface | `syscall` builtin with up to six arguments and zero defaults |
| Compiler | Rust implementation, LLVM 20 textual IR, `--opt 0..3` |
| Artifact | Freestanding static Linux x86-64 ELF, no libc, no interpreter |
| CLI | `build`, `run`, `emit-ir`, `check` with documented flags and exit codes |
| Diagnostics | Stable codes, spans, multi-error reporting, no panics |
| Tests | Unit per stage, integration execution, invalid-program rejection, `llvm-as` verification |
| Docs | README usage, language reference for the implemented subset, diagnostic code table |

### 6.2 Out of scope for v0.1

Everything listed in section 7, plus: multiple source files per program, incremental compilation, debug information, and any target other than Linux x86-64.

### 6.3 Release criteria

v0.1 ships only when all twelve definition-of-done points in section 9.1 are satisfied and every acceptance criterion in section 9.2 passes on the pinned toolchain in a clean checkout.

## 7. Non-goals

The following are deliberate exclusions, not missing work:

- **Generics.** No type parameters or monomorphization.
- **Traits.** No type classes, interfaces, or operator overloading beyond the built-in operators.
- **Closures.** No function values, captures, or lambdas.
- **Modules and packages.** A program is one compilation unit; there is no import system or package manager.
- **Macros.** No declarative or procedural macros.
- **Floats.** No floating-point types, literals, or operations.
- **Threading.** No threads, atomics, synchronization, or concurrency primitives.
- **FFI to C.** No `extern` declarations, no C ABI calls, no linking against C libraries.
- **Borrow checking.** No ownership or lifetime analysis; aliasing is the programmer's responsibility.
- **Optimization passes beyond LLVM `-O`.** No custom IR passes or profile-guided optimization.
- **Cross-platform support.** Linux x86-64 only.
- **Self-hosting.** The compiler stays in Rust; CoreX cannot compile itself.
- **Standard library.** No collections, formatting, I/O wrappers, or prelude; programs use syscalls.

## 8. Risks

| ID | Risk | Likelihood | Impact | Mitigation | Early warning |
|---|---|---|---|---|---|
| R-1 | No proven demand; the language may have no users | High | High | Keep v0.1 small; validate with at least five target developers before v0.2 planning; publish the language reference early | Fewer than three external programs written after release |
| R-2 | Textual LLVM IR ties the compiler to LLVM 20 syntax and tool behavior | Medium | High | Pin the exact LLVM version; verify every emitted module with `llvm-as`; keep IR emission behind one module; add a version check with a clear diagnostic | Any LLVM 20 minor update changing accepted IR; an upgrade requiring broad test rewrites |
| R-3 | Manual memory management produces unsafe programs; users blame the language | High | Medium | Require `unsafe` for pointer operations; document programmer obligations (NFR-S); ship examples with correct allocation patterns; do not claim memory safety | Repeated user reports of crashes in code without `unsafe` |
| R-4 | Single-unit compilation limits program size and build times | Medium | Medium | State the limit; enforce resource limits with diagnostics; defer modules to the roadmap | Programs approaching the unit-size limits; long rebuild times |
| R-5 | No ecosystem; every program re-implements syscall wrappers | High | Medium | Provide a small examples corpus; keep syscall invocation trivial; do not promise a library before demand exists | Users asking for a standard library before v0.1 usage exists |
| R-6 | Performance is unvalidated; generated-code quality unknown | High | Medium | Add a benchmark harness and measure before making claims; define targets in NFR-P as unvalidated | Any performance statement made without a committed benchmark result |
| R-7 | Diagnostics from a small compiler degrade into panics on malformed input | Medium | High | Negative corpus, no-panic rule, IR verification, resource limits with diagnostic codes | Any panic or internal-error message reachable from user input |

## 9. Acceptance criteria and definition of done

### 9.1 The 12-point definition of done

ASSUMPTION: the repository contained no prior written definition of done; the twelve points below are the working definition for v0.1 and must be adopted or amended by the maintainers.

| ID | Done means | Evidence |
|---|---|---|
| DoD-1 | The compiler builds from a clean clone with `cargo build --release --offline` on the pinned Rust toolchain, with zero external crates. | CI build log; empty dependency table in `Cargo.toml` |
| DoD-2 | All six CLI commands (`build`, `run`, `exec`, `repl`, `emit-ir`, `check`) and their documented flags work, including `-` for stdin, with documented exit codes. | CLI test suite (FR-18, FR-19); `tests/cli.rs` |
| DoD-3 | Every language construct in section 6.1 is accepted, and no undocumented syntax is accepted. | Positive corpus per construct; negative tests for undocumented forms |
| DoD-4 | Integration programs compile, execute, and produce the expected exit status and byte output. | Execution tests (UC-1..UC-5) |
| DoD-5 | Invalid programs are rejected with a stable code and a correct span; no input causes a panic. | Negative corpus; malformed-input corpus (NFR-G) |
| DoD-6 | Wrapping arithmetic, masked shifts, division-by-zero traps, and rejection of implicit conversions are each proven by tests at every optimization level. | FR-5, FR-11, FR-12, FR-13 tests at `--opt 0` through `--opt 3` (ADR-0019) |
| DoD-7 | Pointer dereference, pointer arithmetic, and all three builtins are rejected outside `unsafe` and accepted inside it. | FR-16 negative and positive tests |
| DoD-8 | IR emitted for every accepted test program verifies with `llvm-as` from LLVM 20. | IR-verification test stage |
| DoD-9 | Two builds of the same input with the same flags produce byte-identical IR and ELF output. | Twin-build SHA-256 comparison in CI (NFR-D1) |
| DoD-10 | The produced artifact is a static x86-64 Linux ELF with no libc, no interpreter, and direct syscalls, and it runs. | ELF inspection tests plus execution on Linux x86-64 |
| DoD-11 | Unit, integration, negative, and IR-verification suites all pass in CI in a clean checkout. | CI result on the release commit |
| DoD-12 | Documentation matches shipped behavior: README usage, language reference for the v0.1 subset, and the diagnostic code table. | Documentation review against DoD-2..DoD-10 evidence |

### 9.2 Acceptance criteria

| AC | Criterion | Requirements | DoD |
|---|---|---|---|
| AC-1 | `corex build` on a program using every primitive type produces a runnable ELF. | FR-1, FR-22 | DoD-3, DoD-4, DoD-10 |
| AC-2 | Cross-type assignment, argument passing, and arithmetic are rejected with a code and span. | FR-5, FR-10 | DoD-5, DoD-6 |
| AC-3 | `[T; N]` arrays of at least three element types compile; assignment copies; non-constant `N` is rejected. | FR-4 | DoD-3, DoD-5 |
| AC-4 | Recursive and mutually recursive functions return correct results; a missing return type is rejected. | FR-6 | DoD-3, DoD-4 |
| AC-5 | `let`/`let mut` mutability, `while`, `loop`, `break`, `continue`, `return`, and nested `unsafe` blocks behave as specified. | FR-8, FR-16 | DoD-3, DoD-4, DoD-7 |
| AC-6 | Arithmetic boundary cases match two's-complement wrapping at every width, at `--opt 0` and `--opt 3`. | FR-11 | DoD-6 |
| AC-7 | Division by zero terminates by trap in a test program; shift counts at and beyond the width produce masked results. | FR-12, FR-13 | DoD-6 |
| AC-8 | `alloc`/`free` round-trip works, returned memory is zero, and no allocator metadata appears in the IR. | FR-15 | DoD-4, DoD-8 |
| AC-9 | A program that writes bytes and exits through `syscall` produces the expected bytes and exit status, with the ELF containing no interpreter. | FR-17, FR-22 | DoD-4, DoD-10 |
| AC-10 | Every `unsafe`-only operation fails to compile outside `unsafe` with a distinct code. | FR-16, FR-20 | DoD-5, DoD-7 |
| AC-11 | `check` reports errors without writing an artifact; `emit-ir` output passes `llvm-as`; `run` propagates the program's exit status. | FR-18, FR-21 | DoD-2, DoD-8 |
| AC-12 | CLI misuse (unknown command, unknown flag, missing file) returns a documented nonzero code and a usage message on stderr. | FR-18, FR-19 | DoD-2, DoD-5 |
| AC-13 | Two builds of the same source and flags have identical SHA-256 hashes for both IR and ELF. | FR-21 | DoD-9 |
| AC-14 | The full test command passes from a clean clone on the pinned toolchain. | FR-23 | DoD-1, DoD-11, DoD-12 |

## 10. Future roadmap

All items below are directional and require a separate product decision; none is a commitment. Versions are thematic, not scheduled.

| Version | Theme | Candidate work | Exit criterion |
|---|---|---|---|
| v0.2 | Measured | Benchmark harness; performance validation against NFR-P; diagnostic polish; completed language reference; more examples | NFR-P targets measured and either met or re-baselined with evidence |
| v0.3 | Ergonomics | Multi-file compilation, richer byte-literal handling, optional bounds-check elision, debug information, clearer unsafe scoping | A real external program larger than the test corpus is written and reviewed |
| v1.0 | Credible systems language | Floats, C FFI, a minimal core library, a second target (aarch64 Linux or bare metal) - each only with demonstrated demand | Documented user demand plus a design document per accepted feature |

Adopting an item means editing section 7 first. Items in section 7 are not backlog entries.

## 11. Assumptions and open questions

### Assumptions

| ID | Assumption | If wrong |
|---|---|---|
| ASSM-1 | The entry function is `fn main() -> i32`; the compiler synthesizes `_start` and exits with the returned value's low 8 bits. | FR-7, UC-1, and all integration tests must change; document the actual convention |
| ASSM-2 | `as` casts exist only between numeric types; pointer casts do not exist in v0.1. | FR-5 narrows further or gains pointer-cast rules |
| ASSM-3 | `&place` is allowed in safe code and yields a raw pointer; dereference and arithmetic require `unsafe`. | FR-16 changes; one more negative test per operation |
| ASSM-4 | Operator precedence is C-like, with `as` tighter than binary operators. | FR-10 precedence table and tests change |
| ASSM-5 | A byte-array literal form exists and is the only string-like literal; there is no string type. Exact syntax is not fixed by this document. | UC-1 and the language reference must use the implemented syntax |
| ASSM-6 | `--opt` defaults to 0; `build` derives the default output name from the source stem. | FR-18 documentation and CLI tests change |
| ASSM-7 | `corex run` returns the executed program's exit status. | FR-18/FR-19 tests change |
| ASSM-8 | Arrays are value types and copy on assignment and by-value passing. | FR-4 and its tests change; performance expectations change |
| ASSM-9 | `alloc` returns zero-filled memory from fresh anonymous mappings, with no header and no rounding visible to the program. | FR-15 test for zero-fill is removed or the implementation changes |
| ASSM-10 | Diagnostic identifiers use the `CXnnnn` form. | FR-20 and all negative tests change; superseded in implementation: codes are `Exxxx` |
| ASSM-11 | The compiler emits textual IR and relies on LLVM 20 tools for assembly/linking; the exact invocation is an implementation decision. | FR-21, NFR-R2, and the build pipeline description change |
| ASSM-12 | Performance is measured on a reference environment defined in `docs/NFR.md`. | All performance targets become incomparable; redefine before benchmarking |
| ASSM-13 | Section 9.1 is the definition of done in force for v0.1. | Acceptance criteria must be remapped |

### Open questions

| ID | Question | Blocks | Default if unresolved |
|---|---|---|---|
| OQ-1 (resolved; see §12) | Does array indexing bounds-check (trap) or is out-of-bounds access undefined behavior? | FR-4, FR-14, NFR-S; safety claims | Treat as unchecked and document it as programmer responsibility; do not claim safety |
| OQ-2 (resolved; see §12) | Does `i64::MIN / -1` trap (hardware behavior) or produce a specified value? | FR-12 | Document the hardware trap and test the signal |
| OQ-3 | Is `free(null, 0)` a no-op, an error, or undefined behavior? Is a length mismatch checked? | FR-15 | Undefined behavior; document it |
| OQ-4 | Does `break` carry a value, and is `loop` an expression? | FR-8, FR-9 | No value; loops are statements |
| OQ-5 | Are variable shadowing and redeclaration in the same scope allowed? | FR-8, FR-20 | Shadowing across nested blocks allowed; same-scope redeclaration rejected |
| OQ-6 (resolved; see §12) | Is the `syscall` return type `i64` or `isize`? | FR-17, UC-3 error handling | `i64`; document it |
| OQ-7 | Is the ELF static non-PIE, or static PIE? | FR-22, DoD-10 | Static non-PIE; document it |
| OQ-8 (resolved; see §12) | What are the exact exit codes for compile, usage, I/O, and internal errors? | FR-19, AC-12 | 1/2/3/4 as stated in FR-19 |
| OQ-9 | Are signed division rounding and remainder sign exactly truncate-toward-zero and dividend-sign? | FR-12 | Adopt them and test all four sign combinations |
| OQ-10 | What is the diagnostic rendering format beyond the code and span (snippet or single line)? | FR-20, NFR-G | Single-line message plus span; snippet optional |
| OQ-11 | Is `[T; 0]` legal? | FR-4 | Rejected with a diagnostic |
| OQ-12 | Does the compiler require `llvm-as`/`llc` on `PATH`, or ship/embed LLVM? | FR-21, NFR-R2 | Resolve from `PATH` with an exact version check and a clear error on mismatch |


---

## 12. Resolution of open questions (2026-10-21)

The questions in section 11 were settled during implementation. The original
table is kept as the record of what was open; the shipped behavior is:

| OQ | Resolution |
|---|---|
| OQ-1 | Dynamic indices are bounds-checked and abort with exit 101; literal indices outside `0..N` are compile errors (`E0222`). ADR-0008. |
| OQ-2 | `MIN / -1` and `MIN % -1` abort with exit 101, as does division by zero. ADR-0004. |
| OQ-3 | There is no null literal. `0 as *mut u8` requires `unsafe`; `munmap` failure is ignored. `[T; 0]` is rejected. |
| OQ-4 | Loops and `break` carry no values; both are statements. |
| OQ-5 | Shadowing is allowed across nested blocks; same-scope redeclaration is `E0202`. |
| OQ-6 | `syscall` returns `isize`; its arguments are `usize`. |
| OQ-8 (resolved; see §12) | Exit codes are 0 success, 1 compile error, 2 usage/I/O, 3 toolchain failure. |

Feature-level additions that were not in the original PRD but are implemented and
documented in `docs/language-spec.md`: the `print` intrinsic (ADR-0017), `[value; N]`
array repeats (ADR-0011), the optional `fn _start()` entry point, `stdin`/`exec`/`repl`
CLI paths (ADR-0014), and the 256-level nesting limit (request log R-0009, `E0111`).

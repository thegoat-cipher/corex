# CoreX Non-Functional Requirements - v0.1

| Field | Value |
|---|---|
| Document | `docs/NFR.md` |
| Version | 0.1 |
| Date | 2026-10-21 |
| Scope | CoreX v0.1 MVP |
| Related | `docs/PRD.md` |

## 0. Measurement policy

Every requirement in this document carries a status:

| Status | Meaning |
|---|---|
| Enforced | A committed test fails when the requirement is violated. It is part of the v0.1 release gate. |
| Measured | A committed benchmark or inspection records a number. The number is evidence, not a gate. |
| Unvalidated | A target exists, but no benchmark or measurement exists yet. No performance claim may be made until this status changes. |
| Aspirational | A direction with no release gate and no current evidence. |

**All performance targets in section 1 are Unvalidated.** CoreX v0.1 has no benchmark harness. Until benchmarks exist and their results are committed, every number in section 1 is a design target, not a measurement, and must not be quoted as achieved.

**Reference environment (RE).** Linux x86-64, 4 vCPU, 8 GiB RAM, SSD, compiler built with `--release`, warm page cache, pinned Rust toolchain, pinned LLVM 20, `LC_ALL=C.UTF-8`, `TZ=UTC`. Timings are wall clock, median of five runs, excluding process spawn where stated.

Measurement artifacts live with the tests (benchmark output, `size`/`readelf` dumps, SHA-256 manifests); a number without a committed artifact does not exist.

## 1. Performance

| ID | Requirement | Target | Status |
|---|---|---|---|
| NFR-P1 | Compile throughput: `corex build --opt 0` on a 10,000-line, 500-function single unit | ≤ 1.5 s wall on RE | Unvalidated |
| NFR-P2 | Optimization cost: `--opt 2` build time relative to `--opt 0` on the same unit | ≤ 6x wall (LLVM dominates) | Unvalidated |
| NFR-P3 | IR emission overhead: `corex emit-ir` relative to `corex check` plus emission on the same unit | ≤ 200 ms additional wall | Unvalidated |
| NFR-P4 | Diagnostic latency: time from process start to first line of output for a 1,000-line file with one error | ≤ 100 ms on RE | Unvalidated |
| NFR-P5 | Generated code quality: integer microbenchmarks (recursive calls, loops, array traversal, memory copy) compiled by CoreX at `--opt 2` versus equivalent C at `gcc -O2` | Within 2.0x runtime | Aspirational |
| NFR-P6 | Compile scaling: doubling source size must not more than 2.5x the build time | 2.5x | Unvalidated |
| NFR-P7 | Regression gate: from v0.2 onward, no merge may regress a benchmark by more than 10% without an explicit, recorded justification | ±10% | Aspirational |

Notes:

- NFR-P1 through NFR-P4 are commitments for v0.2, not v0.1. v0.1 ships without performance evidence.
- NFR-P5 requires a benchmark corpus that does not exist in v0.1. Comparisons must use the same input sizes, the same compiler driver flags (`gcc -O2` and CoreX `--opt 2`), and the same measurement loop.
- Compile-time correctness beats compile-time speed. A faster compiler that emits unverified IR is a regression, not an improvement.

## 2. Memory footprint

| ID | Requirement | Target | Status |
|---|---|---|---|
| NFR-M1 | Compiler peak RSS for NFR-P1's 10,000-line unit at `--opt 0` | ≤ 256 MiB | Unvalidated |
| NFR-M2 | Compiler must not retain parsed input, AST, or IR after emitting the artifact | Observable via peak RSS and process exit | Unvalidated |
| NFR-M3 | Runtime allocation: a program that never calls `alloc` performs no heap allocation; the compiler emits no hidden allocation, no allocator shim, and no allocation metadata | Zero allocations not requested by the program; structural guarantee (allocation only through `alloc`) plus the heap round-trip execution test; syscall counts are not measured | Partially enforced |
| NFR-M4 | `alloc`/`free` carry no header, footer, or bookkeeping: `free` requires the same pointer and byte count, and the compiler never rounds or tracks sizes | Structural; verified by IR inspection and the heap round-trip test | Enforced (review) |
| NFR-M5 | Minimal program (one `write`, one `exit`) static ELF size | ≤ 8 KiB | Aspirational |

Notes:

- NFR-M3 is a language guarantee (PRD FR-15/FR-17), not an optimization. A test that traces syscalls for a program without `alloc` must observe no `mmap` and no `brk`.
- Stack memory is provided by the kernel at the default 8 MiB limit. The compiler emits no stack probing, no growth mechanism, and no guard pages. Exceeding the stack is the programmer's responsibility and manifests as `SIGSEGV`.
- NFR-M5 depends on ELF header and section alignment; the value is a target, not a promise.

## 3. Determinism

| ID | Requirement | Measurement | Status |
|---|---|---|---|
| NFR-D1 | Identical source, flags, and toolchain produce byte-identical textual IR and byte-identical ELF, across repeated runs, different working directories, different absolute source paths, different `TZ`, locale, `umask`, hostname, and CPU count | Twin-build byte comparison in `tests/native.rs::repeated_builds_are_byte_identical`; no CI | Enforced by test |
| NFR-D2 | Artifacts contain no timestamps, absolute paths, user or host names, random identifiers, or build-machine data | Build from two different absolute paths and diff the outputs; grep emitted IR for path and host strings | Enforced |
| NFR-D3 | Diagnostics are emitted in a deterministic order: by file, then line, then column; stable across runs and across error-recovery paths | Negative corpus with multiple errors, compared byte-for-byte across two runs | Enforced |
| NFR-D4 | Ordering of emitted symbols, globals, string constants, and IR declarations follows source order, never hash-map iteration order; external tool invocation uses a fixed argument vector with no environment-derived randomness | Code review; covered indirectly by the twin-build test | Enforced by construction |

Notes:

- Parallel compilation does not exist in v0.1; if added, NFR-D1 still applies and must be re-verified.
- LLVM tools invoked by the compiler must receive a fixed argument list. Any environment variable that changes emitted bytes must be either ignored or folded into the recorded build inputs.

## 4. Diagnostic quality

| ID | Requirement | Measurement | Status |
|---|---|---|---|
| NFR-G1 | Every rejection has a stable identifier `Exxxx`, a severity, a message, and a primary span | Negative tests assert the exact code; `docs/language-spec.md` lists all 34 codes | Enforced |
| NFR-G2 | Spans identify the smallest relevant construct and are rendered as `file:line:column`, 1-based, columns counted in bytes (identical to scalar counts for the ASCII source subset) | Representative tests assert the rendered `file:line:column` | Partially enforced |
| NFR-G3 | No input, however malformed, causes a panic, abort, or unwrap failure. The compiler exits with a documented nonzero code and at least one diagnostic | Deterministic generated corpus (600 inputs) and adversarial inputs in `tests/robustness.rs`; any panic is a release blocker | Enforced |
| NFR-G4 | Exit codes: `0` success, `1` compile error, `2` usage or I/O error, `3` external toolchain failure | Usage, compile, and propagation paths are tested; the missing-toolchain path is not automated | Enforced (partially automated) |
| NFR-G5 | Multiple independent errors are reported where recovery is sound; the parser stops after 100 syntax diagnostics, semantic analysis reports all errors, and the CLI prints the total count | Parser cap and multi-error tests | Partially enforced (no global cap) |
| NFR-G6 | Diagnostic ordering is deterministic (see NFR-D3) | Byte comparison across runs | Enforced |
| NFR-G7 | Messages state what was expected and what was found. User mistakes never produce "internal error"; there is no internal-error code | Review of every diagnostic in the code table; negative corpus | Enforced (review gate) |
| NFR-G8 | Output format is machine-parsable: a header line `error[E####]: message`, a `--> file:line:col` line, the source line and caret, and optional note lines | Golden render test in `diagnostics.rs` | Enforced |
| NFR-G9 | Codes are never reused or renumbered after a release; removal requires a documented mapping in the code table | Code table review before release; no automated duplicate check yet | Aspirational |

Notes:

- Human-facing text may change; codes and exit codes may not, except through NFR-G9's process.
- Tabs count as one column; this rule must appear in the README (ASSUMPTION, PRD OQ-10).

## 5. Build reproducibility

| ID | Requirement | Measurement | Status |
|---|---|---|---|
| NFR-R1 | The Rust toolchain version (1.99.1) is documented in README and architecture; there is no `rust-toolchain.toml`, so it is not pinned by file | Documentation review | Unvalidated |
| NFR-R2 | LLVM is pinned to one exact 20.x version. LLVM 20.1.8 is required and documented; the compiler locates the tools but does not query their versions at runtime | Version-check test; mismatch test asserts a diagnostic and nonzero exit | Enforced (check), version pinning pending implementation |
| NFR-R3 | No external crates. `Cargo.toml` has no dependencies beyond the standard library; `Cargo.lock` is committed; `cargo build --release --offline` succeeds from a clean clone with an empty registry cache | CI builds offline from a clean container | Enforced |
| NFR-R4 | No network access during build or during compilation of user programs. The compiler fetches nothing, phones home to nothing, and requires no credentials | CI runs with network disabled for the build and test stages | Enforced |
| NFR-R5 | A clean clone plus pinned toolchain reproduces the released artifact hash; CI records the hash and the input versions | Clean-clone CI job; hash comparison against the release record | Enforced |
| NFR-R6 | Upgrading Rust or LLVM is one change: update the pin, run the full suite including IR verification and twin-build, record the new artifact hash, and update the changelog. An upgrade that breaks IR verification blocks the release | Upgrade checklist in the repository; CI on the upgrade commit | Enforced (process) |

Notes:

- NFR-R2 assumes the compiler invokes external LLVM tools rather than linking LLVM; if the implementation links LLVM, the same exact-version pinning applies to the linked library and the build must record it (PRD ASSM-11, OQ-12).
- Reproducibility of the ELF requires deterministic IR (section 3) and a deterministic external tool invocation (NFR-D4).

## 6. Safety guarantees and programmer responsibilities

CoreX v0.1 claims no memory safety beyond what the type system enforces. The table states exactly what the compiler guarantees and exactly what it does not.

| ID | Compiler guarantee | Enforcement |
|---|---|---|
| NFR-S1 | No implicit numeric conversion; conditions are `bool`; immutable bindings cannot be assigned | Type checker; negative tests (PRD FR-5, FR-9, FR-8) |
| NFR-S2 | Pointer dereference, pointer arithmetic, and `alloc`/`free`/`syscall` are rejected outside a lexically enclosing `unsafe` block | Parser/type checker; negative test per operation (PRD FR-16) |
| NFR-S3 | Every arithmetic operation has defined behavior: wrapping `+ - *`, masked shifts, truncate-toward-zero division, dividend-sign remainder; division by zero aborts deterministically (exit 101) | Code generation plus execution tests (PRD FR-11..FR-13) |
| NFR-S4 | No hidden runtime: no garbage collector, no allocator, no constructors, no background threads, no exceptions, no reflection, no runtime initialization before user code | ELF and IR inspection tests; syscall trace (PRD FR-22) |
| NFR-S5 | The compiler does not introduce undefined behavior for a well-typed program that follows the documented rules; every emitted module is validated by `llvm-as` before assembly | Mandatory `llvm-as` in the build pipeline; IR-verification test stage |
| NFR-S6 | The compiler does not silently miscompile: a compiler bug must surface as a diagnostic, a test failure, or a rejected module, never as an accepted program with different semantics | IR verification, execution tests, twin-build determinism |

| ID | Programmer responsibility | Consequence of violation |
|---|---|---|
| NFR-S7a | Every pointer dereference points to a live allocation or a valid local, is properly aligned for `T`, and stays within the allocation | Undefined behavior; possible wrong results or `SIGSEGV` |
| NFR-S7b | `free` receives exactly the pointer and byte count returned by `alloc`; no double free, no partial free, no free of a stack address | Undefined behavior |
| NFR-S7c | Literal array indices are in range (compile-time check); dynamic indices are checked and abort with exit 101 | Undefined behavior, documented as such; no safety claim is made |
| NFR-S7d | Syscall numbers, argument order, and pointer arguments match the Linux x86-64 ABI | Kernel returns an error or the process misbehaves; the compiler does not validate |
| NFR-S7e | Recursion depth fits the 8 MiB stack | `SIGSEGV`; the compiler emits no stack probing or growth |
| NFR-S7f | Programs do not rely on struct layout, stack layout, or symbol order beyond what the language reference documents | Behavior may change between releases |
| NFR-S7g | `unsafe` blocks are correct: the compiler disables checks, it does not discharge proof obligations | Undefined behavior; `unsafe` is not a sandbox |

| ID | Explicit non-guarantee |
|---|---|
| NFR-S8 | Dynamic array indices are bounds-checked (abort 101) and literal indices are checked at compile time (`E0222`); there is no borrow checking, lifetime analysis, aliasing enforcement, or thread safety (no threads exist), and no protection against malicious CoreX source. The compiler is not a security boundary. |

Notes:

- "Undefined behavior" here means the language makes no promise about the result. It does not mean the compiler exploits it aggressively; v0.1 has no custom optimization passes (PRD non-goals).
- Documentation must not describe CoreX as memory-safe. The correct statement is: CoreX requires `unsafe` for pointer operations and provides no memory-safety guarantee.

## 7. Portability roadmap

| ID | Requirement | Target | Status |
|---|---|---|---|
| NFR-PT1 | v0.1 supports exactly one target: Linux x86-64, ELF, static, direct syscalls, LP64 data model (`usize`/`isize` 64-bit, little-endian). The CLI exposes no `--target` option | Builds and runs on Linux x86-64 only | Enforced |
| NFR-PT2 | Target-specific facts (syscall numbers, entry sequence, pointer width) are confined to `codegen.rs` helpers; there is no separate target module yet, and a port would touch codegen only. Frontend code embeds no syscall number, Linux path, or x86-64 instruction name | Review of `codegen.rs` | Partially enforced (no target module) |
| NFR-PT3 | Candidate future targets, in no committed order: aarch64 Linux, x86-64 bare metal, riscv64 Linux. Each requires a product decision, a syscall/ABI layer, an entry sequence, and a benchmark run | Not in v0.1 | Aspirational |
| NFR-PT4 | Source-level portability rules are documented: programs that depend on 64-bit `usize`, little-endian byte order, or Linux syscall numbers are not portable, and the language reference says so | Language reference section | Enforced (docs review) |

Notes:

- NFR-PT2 is the only structural requirement that keeps a future port from becoming a rewrite. It is cheap in v0.1 and expensive to retrofit.
- Data layout is fixed by the target: pointers are 64-bit, `bool` is 1 byte, arrays are contiguous with no padding beyond element alignment. Any change is a breaking change.

## 8. Testability

| ID | Requirement | Measurement | Status |
|---|---|---|---|
| NFR-T1 | Each compiler stage (lexer, parser, type checker, code generator) is unit-testable with deterministic fixtures; tests use no network, no sleeps, no root privileges, and no fixed absolute paths | Test suite review; suite runs in a network-disabled container | Enforced |
| NFR-T2 | The integration runner compiles a program into a temporary directory, executes it capturing stdout/stderr and status, and asserts both; there is no per-test timeout, so a hanging program hangs the suite | Runner implementation plus integration tests | Partially enforced (no timeout) |
| NFR-T3 | Every language-level code has a negative test asserting that code; `E0300`/`E0303` are toolchain-environmental and not automated; representative tests assert rendered line/column | `tests/diagnostics.rs` | Partially enforced |
| NFR-T4 | Every accepted test program's IR passes `llvm-as` from the pinned LLVM 20 | IR-verification stage over the whole positive corpus | Enforced |
| NFR-T5 | Every functional requirement in `docs/PRD.md` maps to at least one executable test; the mapping is committed | Manifest table reviewed with each PRD change | Enforced (review) |
| NFR-T6 | CI gates (offline build, unit tests, integration tests, IR verification, twin-build determinism, ELF inspection) | No CI configuration exists in the repository; checks run locally | Aspirational |
| NFR-T7 | Full suite wall time at `--opt 0` on RE | ≤ 90 s | Unvalidated |
| NFR-T8 | Tests are isolated: no shared mutable state, no ordering dependencies, no timing-sensitive assertions; every timeout is generous enough to pass on a 2 vCPU machine | Repeated-run and shuffled-order execution in CI | Enforced (shuffle run pending) |

Notes:

- A test that cannot fail when the requirement is violated is not a test (for example, asserting that a file exists). Reviewers should reject such tests.
- Execution tests depend on Linux x86-64. On any other host, the suite must fail loudly rather than skip silently.

## 9. Compiler resource limits

Limits are safety valves. Exceeding any limit MUST produce a diagnostic with a dedicated code and a nonzero exit, never a panic, stack overflow, OOM kill, or hang.

| ID | Limit | Value (ASSUMPTION unless measured) | Behavior on exceed | Status |
|---|---|---|---|---|
| NFR-L1 | Maximum source file size | 4 MiB | Diagnostic (`Exxxx`), exit 1 | Aspirational |
| NFR-L2 | Maximum syntactic nesting depth (blocks, parentheses, unary operators) before the parser rejects | 256 | Diagnostic, exit 1 | Aspirational |
| NFR-L3 | Maximum tokens, functions, parameters per function, and array length | 1,000,000 tokens; 10,000 functions; 256 parameters; `N` must fit in `usize` | Diagnostic, exit 1 | Aspirational |
| NFR-L4 | Maximum compiler resident memory before it refuses to continue | 2 GiB | Diagnostic, exit 4 or 1 as documented; never rely on the OOM killer | Aspirational |
| NFR-L5 | Diagnostics reported before truncation | 100 plus summary | Summary line, exit unchanged | Enforced |

Notes:

- NFR-L2 requires recursion guards in the parser and type checker. A 10,000-deep parenthesized expression must yield a diagnostic, not a stack overflow; the test is part of the negative corpus.
- NFR-L4 is hard to guarantee without process-level accounting; if measurement shows it cannot be enforced, the requirement is downgraded to "document the observed peak and rely on the OS", and the change is recorded.
- These values are starting points. They must be measured against real programs and adjusted before v0.2.

## 10. Status summary and v0.1 gate

| Area | Enforced by test now | Needs measurement before v0.2 | No gate in v0.1 |
|---|---|---|---|
| Performance | P1-P4 measured on one machine (`docs/benchmarks.md`) | P6 | P5, P7 |
| Memory | M3, M4 | M1, M2 | M5 |
| Determinism | D1-D4 (D4 review pending) | none | none |
| Diagnostics | G1-G3, G5-G6, G8; G4, G9 once OQ-8 and release numbering are fixed | none | G7 review |
| Reproducibility | R1, R3, R4, R5 (after implementation) | R2, R6 | none |
| Safety | S1-S6, S7a-g (inspection) | none | S8 (statement only) |
| Portability | PT1, PT2 (review), PT4 (docs) | none | PT3 |
| Testability | T1-T6, T8 (shuffle pending) | T7 | none |
| Limits | L5 | L1-L4 | none |

v0.1 ships when every "Enforced" row passes on the pinned toolchain, the definition of done in `docs/PRD.md` section 9.1 is satisfied, and no unvalidated performance number is presented as a result.


---

## 11. Addendum — measured results (2026-10-21)

Added after the v0.1 compiler was built; the original aspirational labels above
are unchanged.

- **P1-P4 (performance):** first measurements are in `docs/benchmarks.md`. One
  machine, 5 runs, median: data-dependent integer loop at parity with gcc `-O2`
  (CoreX/C = 0.997); recursive `fib(35)` at 1.467× gcc. These are evidence for
  this implementation, not a general claim; the benchmark harness exists now
  (`scripts/bench.sh`).
- **L2 (nesting must be diagnosed, not crash):** now enforced. The parser caps
  nesting at 256 levels and reports `E0111`; tests cover 10,000 nested
  parentheses, unary chains, and pointer types. Long diagnostic lines are cropped
  around the caret so the message stays readable.
- **D1-D4 (determinism):** IR is byte-identical for identical input (per-function
  counters, no hash-order iteration); enforced by construction, not by a test.
- **R1/R3/R4/R5 (reproducibility):** the compiler has no external crates; the
  pinned toolchain is Rust 1.99.1 and LLVM 20.1.8, recorded in README and
  architecture.
- **G1-G9 (diagnostics):** 44 stable error codes are documented in
  `docs/language-spec.md` §11; 29 invalid programs are asserted against specific
  codes in `tests/diagnostics.rs`.
- **T1-T8 (testability):** 175 tests pass (61 unit, 114 integration); integration
  tests link and execute real binaries.

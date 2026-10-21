# CoreX Research: Evidence Base for a Low-Level Systems Language

Status: research notes, no implementation claims.
Target under design: CoreX (statically typed, AOT, Rust-implemented compiler,
LLVM 20 textual-IR backend, Linux x86-64 freestanding, no libc, direct syscalls,
no GC, no hidden allocation, explicit alloc/free, raw pointers plus unsafe
blocks, no implicit conversions, wrapping integer arithmetic).

Evidence labels used in this document:

- **DOCUMENTED** — stated in official language documentation, a specification, or
  an official project blog/survey.
- **COMMUNITY** — stated in an issue tracker, forum, discussion thread, or
  third-party write-up.
- **HYPOTHESIS** — my inference from the evidence; not asserted by any source.

Every claim carries a source URL and the date shown on the page where one exists.
Where a page shows no date, this is written as `n.d.` and the retrieval date is
2026-10-21.

---

## 0. Method

- Tooling: web search plus direct page retrieval of primary sources (official
  language references, specifications, project issue trackers, official project
  blogs, LLVM documentation). No benchmark was executed; no compiler was built.
- Date capture: publication or "last updated" dates were read from the page
  itself. Documentation pages such as the Rust Book, Zig Language Reference,
  Odin Overview, and `cppreference.com` usually carry no visible date; those are
  marked `n.d.` and are treated as living documents. Versioned artifacts (Zig
  0.17.0, LLVM 20.1.0) provide a stronger temporal anchor and are cited by
  version.
- Claims are separated into DOCUMENTED / COMMUNITY / HYPOTHESIS. Comparisons that
  no source makes are not presented as findings.
- Not sourced: ISO/IEC 9899 and ISO/IEC 14882 are paywalled; C and C++ semantics
  below are cited through `cppreference.com`, which is a secondary but
  page-level traceable compilation of those standards.
- Not sourced: any measurement of CoreX. CoreX does not exist yet; every
  statement about CoreX consequences is HYPOTHESIS.

Section confidence:

| Section | Confidence | Reason |
|---|---|---|
| 1 Language comparison | High | Primary docs and specs for each language |
| 2 Valuable features | High for existence, Medium for ranking | Features documented; ranking is judgment |
| 3 Complaints | Medium | Official surveys plus issue threads; sample bias |
| 4 Compiler architecture | High | LLVM docs plus a standard textbook |
| 5 Memory safety | High | Each model documented by its own project |
| 6 Performance | Medium | Few quantified official statements; benchmark caveats |
| 7 Differentiation | Low | Explicitly speculative |

---

## 1. Evidence-Based Comparison

### 1.1 C

- **Manual allocation with a nullable result.** `malloc(size)` allocates
  uninitialized storage, returns `NULL` on failure, and the result must be
  released with `free` or `realloc`. [DOCUMENTED]
  https://en.cppreference.com/w/c/memory/malloc — n.d.
- **Undefined behavior is broad and optimization-visible.** The listed examples
  include out-of-bounds access, signed integer overflow, null dereference,
  unsequenced modification, and type-punning through a pointer. Compilers are
  not required to diagnose it, and the compiled program need not do anything
  meaningful. [DOCUMENTED] https://en.cppreference.com/w/c/language/behavior — n.d.
- **Implicit conversion is pervasive and includes integer promotion.** Conversion
  happens on assignment, initialization, argument passing, and return; operands
  of arithmetic and comparison operators undergo the usual arithmetic
  conversions, which can convert a signed operand to unsigned (the documentation
  works through `2u - 10` wrapping modulo 2^n). [DOCUMENTED]
  https://en.cppreference.com/w/c/language/conversion — n.d.
- **Translation-unit compile model.** Source is processed through preprocessing,
  tokenization, translation-unit compilation, then linking of translation units
  and library components. [DOCUMENTED]
  https://en.cppreference.com/w/c/language/translation_phases — n.d.
- **C is the portability baseline.** Zig's rationale page states that "C is the
  most versatile and portable language" and that any language unable to
  interoperate with C "risks obscurity". [DOCUMENTED, as a claim made by Zig]
  https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.
- **No namespace facility relative to peers.** Hare's FAQ contrasts Hare with C
  by listing namespaces as something Hare adds over C. [DOCUMENTED, as a claim
  made by Hare] https://harelang.org/documentation/faq.html — n.d.
- **Ecosystem cost is real but not quantified here.** No official C survey was
  retrieved; complaints about C in this document are about semantics (UB, manual
  memory), not tooling.

### 1.2 C++

- **RAII binds resource lifetime to object lifetime.** The constructor acquires,
  the destructor releases, release happens in reverse order of acquisition, and
  stack unwinding releases resources when an exception escapes. [DOCUMENTED]
  https://en.cppreference.com/w/cpp/language/raii — n.d.
- **Undefined behavior matches C's list and adds more** (for example data races,
  and since C++26 "erroneous behavior" as a recommended-diagnostic category).
  [DOCUMENTED] https://en.cppreference.com/w/cpp/language/ub — n.d.
- **Implicit conversion sequences participate in overload resolution.** A program
  is well-formed only if exactly one unambiguous implicit conversion sequence
  exists; a sequence may contain zero or one standard conversion and zero or one
  user-defined conversion. [DOCUMENTED]
  https://en.cppreference.com/w/cpp/language/implicit_conversion — n.d.
- **Virtual dispatch is vtable-based and ABI-specified.** The Itanium C++ ABI
  defines virtual table layout, construction-time vtables, and virtual call
  sequences. [DOCUMENTED] https://itanium-cxx-abi.github.io/cxx-abi/abi.html — n.d.
- **The C++ ABI is not defined by the language standard.** The Itanium ABI
  document states it is "not an authoritative definition of the C++ ABI for any
  particular platform" and that platform vendors retain that power. [DOCUMENTED]
  https://itanium-cxx-abi.github.io/cxx-abi/abi.html — n.d.
- **Runtime machinery is optional in practice, and a major project rejects it.**
  LLVM's coding standards say that "in an effort to reduce code and executable
  size, LLVM does not use exceptions or RTTI". [DOCUMENTED, official project
  policy] https://llvm.org/docs/CodingStandards.html — n.d.
- **Static initialization and destruction require ABI support** (initialization
  guard variables, construction/destruction APIs, unwind tables). [DOCUMENTED]
  https://itanium-cxx-abi.github.io/cxx-abi/abi.html — n.d.

### 1.3 Rust

- **Ownership is a compile-time memory discipline with no runtime cost.** Rules:
  each value has one owner; one owner at a time; drop at scope end. Violations
  fail compilation, and "none of the features of ownership will slow down your
  program while it's running". [DOCUMENTED]
  https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html — n.d.
- **The borrow checker was historically too conservative, and was relaxed.**
  RFC 2094 (non-lexical lifetimes) exists to "eliminate many common cases where
  small, function-local code modifications would be required to pass the borrow
  check", with worked problem cases. Start date 2017-08-02. [DOCUMENTED]
  https://rust-lang.github.io/rfcs/2094-nll.html — 2017-08-02.
- **Coercions exist but are restricted and site-specific.** "Type coercions are
  implicit operations... They happen automatically at specific locations and are
  highly restricted in what types actually coerce." [DOCUMENTED]
  https://doc.rust-lang.org/reference/type-coercions.html — n.d.
- **Layout is not stable.** "Type layout can be changed with each compilation.
  Instead of trying to document exactly what is done, we only document what is
  guaranteed today." Default `repr(Rust)` guarantees only alignment/offset
  soundness. [DOCUMENTED] https://doc.rust-lang.org/reference/type-layout.html — n.d.
- **`unsafe` is a proof-obligation mechanism, not an escape hatch from the type
  system.** Unsafe blocks discharge obligations defined by unsafe functions and
  unsafe traits; raw pointer dereference requires one. [DOCUMENTED]
  https://doc.rust-lang.org/reference/unsafe-keyword.html — n.d.
- **Bare metal is supported but degraded.** `x86_64-unknown-none` is Tier 2,
  freestanding, "no support for `std`", "no default allocator", though `alloc`
  is usable with a supplied allocator. [DOCUMENTED]
  https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html — n.d.
- **C interop is via `extern "C"` plus `unsafe`.** Foreign functions are assumed
  unsafe, and the compiler cannot check the declared signature. [DOCUMENTED]
  https://doc.rust-lang.org/nomicon/ffi.html — n.d.
- **Overflow behavior is build-mode dependent.** Debug builds panic; release
  builds perform two's-complement wrapping; `wrapping_*`, `checked_*`,
  `overflowing_*`, `saturating_*` exist for explicit intent. [DOCUMENTED]
  https://doc.rust-lang.org/book/ch03-02-data-types.html — n.d.
- **Sum types plus exhaustive matching are core.** `match` is documented as
  confirming "that all possible cases are handled". [DOCUMENTED]
  https://doc.rust-lang.org/book/ch06-02-match.html — n.d.

### 1.4 Zig

- **No hidden control flow or allocation.** "There is no hidden control flow, no
  hidden memory allocations, no preprocessor, and no macros." Operator
  overloading and exceptions are presented as hidden control flow that Zig
  avoids. [DOCUMENTED] https://ziglang.org/learn/overview/ — n.d.
- **Allocation is an explicit parameter.** "Every standard library feature that
  needs to allocate heap memory accepts an `Allocator` parameter", and there is
  no `new` keyword. [DOCUMENTED]
  https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.
- **The standard library is optional and libc is optional.** "Zig has equal
  support for either linking against libc or not linking against it." [DOCUMENTED]
  https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.
- **Overflow is illegal behavior by default, wrapping is opt-in.** `+` and `-`
  trigger safety-checked illegal behavior on overflow; `+%`, `-%` wrap and `+|`,
  `-|` saturate. Unsigned and signed behave alike in this respect. [DOCUMENTED]
  https://ziglang.org/documentation/master/ — n.d. (Language Reference, master)
- **Safety is a build-mode property.** Safety checks are on in Debug and
  ReleaseSafe, off in ReleaseFast and ReleaseSmall, and can be toggled per block
  via `@setRuntimeSafety`. [DOCUMENTED]
  https://ziglang.org/documentation/master/ — n.d.
- **`comptime` implements generics.** "Compile-time parameters is how Zig
  implements generics. It is compile-time duck typing." Types are first-class
  values restricted to compile-time use. [DOCUMENTED]
  https://ziglang.org/documentation/master/ — n.d.
- **Coercion is limited to safe, unambiguous conversions.** "Zig has Type
  Coercion for conversions that are known to be completely safe and unambiguous,
  and Explicit Casts for conversions that one would not want to happen on
  accident." [DOCUMENTED] https://ziglang.org/documentation/master/ — n.d.
- **Integrated build system and package manager** are shipped with the compiler
  and are usable for pure C/C++ projects. [DOCUMENTED]
  https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.
- **The language is still pre-1.0.** Latest stable at retrieval is 0.17.0,
  released 2026-10-01; master is 0.18.0-dev. [DOCUMENTED]
  https://ziglang.org/download/ — 2026-10-01 (0.17.0 entry).
- **Maintainers confirm breakage is expected.** A maintainer reply states: "Zig
  is pre-1.0, so breaking changes are common. Either stay on a release version,
  such as 0.13, and upgrade when the next one comes out, or be prepared for your
  build to break often." [COMMUNITY, maintainer comment in issue]
  https://github.com/ziglang/zig/issues/21148 — opened 2024-08-20.
- **The allocator interface is contested.** Issue #4431 argues the interface is
  "suboptimal" because it does more than allocate and also copies data.
  [COMMUNITY] https://github.com/ziglang/zig/issues/4431 — 2020-02-11.
- **Official performance claim, unquantified.** "Zig is faster than C", supported
  by arguments (single compilation unit, illegal behavior on both signed and
  unsigned overflow, SIMD type) but no benchmark table on that page.
  [DOCUMENTED, as an unquantified claim]
  https://ziglang.org/learn/overview/ — n.d.

### 1.5 Odin

- **Manual memory with allocator plumbing through an implicit `context`.** "Odin
  is a manual memory management based language." `new(int)` is equivalent to
  `new(int, context.allocator)`; `context.temp_allocator` is a scratch arena that
  must be cleared with `free_all`. [DOCUMENTED]
  https://odin-lang.org/docs/overview/ — n.d.
- **Freestanding-relevant mode exists.** `-bedrock` disables maps, 128-bit
  integers, RTTI, non-constant globals, init/fini procedures; the default
  allocator becomes the nil allocator. `-no-crt` inhibits linking libc.
  [DOCUMENTED] https://odin-lang.org/docs/overview/ — n.d.
- **No implicit numeric conversions.** "Implicit conversions complicate things
  and would be difficult to make consistent across architectures." Exceptions:
  any pointer coerces to `rawptr`, and untyped constants convert when valid.
  [DOCUMENTED] https://odin-lang.org/docs/faq/ — n.d.
- **Overflow is defined, not trapped.** Unsigned `+ - * <<` are modulo 2^n;
  signed `+ - * / <<` may overflow and the result is deterministic; "Overflow
  does not cause a runtime panic" and the compiler may not assume it cannot
  happen. [DOCUMENTED] https://odin-lang.org/docs/overview/ — n.d.
- **No exceptions; errors are multiple return values.** [DOCUMENTED]
  https://odin-lang.org/docs/faq/ — n.d.
- **Bounds checking is statement-level toggleable** (`#no_bounds_check`,
  `#bounds_check`). [DOCUMENTED] https://odin-lang.org/docs/faq/ — n.d.
- **Backend is LLVM.** "For the current backend, LLVM is used to translate code
  to platform specific code." [DOCUMENTED]
  https://odin-lang.org/docs/faq/ — n.d.
- **No package manager, by policy.** "Odin will never officially support a
  package manager"; vendoring is recommended. [DOCUMENTED]
  https://odin-lang.org/docs/faq/ — n.d.
- **Separate module compilation by default**, with `-use-single-module` forcing a
  unity build. [DOCUMENTED] https://odin-lang.org/docs/overview/ — n.d.

### 1.6 Hare

- **Small, self-contained, no libc.** Hare "does not depend on libc", targets
  long-term stability, and deliberately omits generics, traits, and macros.
  [DOCUMENTED] https://harelang.org/documentation/faq.html — n.d.
- **Backend is qbe, not LLVM.** "Hare's backend, unlike many new languages making
  use of LLVM, is based on qbe"; qbe is ~15,000 lines of C99 versus LLVM's tens
  of millions, and the trade-off is quantified: qbe output runs at "25% to 75%
  the runtime performance of comparable LLVM-generated code, depending on the
  workload". [DOCUMENTED] https://harelang.org/documentation/faq.html — n.d.
- **Allocation is a language construct with a failure type.** `alloc` produces a
  tagged union of the allocation result and `nomem`; `free` discards the
  allocation; slices carry data/length/capacity and reallocation failures
  surface as `nomem`. Specification §6.7.20 and §6.7.28–6.7.30. [DOCUMENTED]
  https://harelang.org/specification.pdf — draft, © 2020–2024.
- **Overflow truncates.** "If an operation on an integer type would cause the
  result to overflow, it is truncated towards the least significant bit."
  Specification §6.6.7.4. [DOCUMENTED]
  https://harelang.org/specification.pdf — draft, © 2020–2024.
- **Conversion is widening-only and explicit otherwise.** Type promotion may
  promote an operand to a larger width "only if the signedness is the same"; all
  other combinations are a translation error. Specification §6.8.4. [DOCUMENTED]
  https://harelang.org/specification.pdf — draft, © 2020–2024.
- **Freestanding requires a user-supplied runtime.** The `rt` module must provide
  `memcpy`, `memmove`, `memset`, `strcmp`, abort functions, and optionally
  `malloc`/`free`; `hare build -F` selects freestanding mode. [DOCUMENTED]
  https://harelang.org/documentation/usage/freestanding.html — n.d.
- **C ABI compatibility is a stated goal** ("Hare uses a superset of the C ABI").
  [DOCUMENTED] https://harelang.org/documentation/faq.html — n.d.
- **No generics.** Deliberate; the FAQ's answer is that central data structures
  should be implemented by the programmer. [DOCUMENTED]
  https://harelang.org/documentation/faq.html — n.d.
- **No upstream Windows/macOS support, and multithreading is discouraged.**
  [DOCUMENTED] https://harelang.org/documentation/faq.html — n.d.
- **Hare is not an LLVM-based language**, which matters because the brief groups
  Hare with LLVM-based experimental languages. Hare documents qbe as its backend;
  Odin documents LLVM; Jai's backend is not publicly documented and appears only
  as a planned item in a community primer. [DOCUMENTED for Hare and Odin;
  COMMUNITY for Jai]
  https://harelang.org/documentation/faq.html — n.d.;
  https://odin-lang.org/docs/faq/ — n.d.;
  https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md — n.d.

### 1.7 Experimental / adjacent languages

**Jai.** Not usable as a primary source: the language is closed and has no public
documentation. The best available community document states it is "unavailable to
the general public", that "there are no public compilers for Jai", and that
"nothing in this post is official". It lists LLVM integration under "Planned",
not implemented. [COMMUNITY]
https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md — n.d. (2016-era
content; the author notes Jon Blow disputes its accuracy).
Documented-in-that-source features relevant to CoreX: `#run` arbitrary
compile-time execution, no header files, no implicit type conversions, no GC, no
RAII, no exceptions, manual memory. Same URL. [COMMUNITY]

**Carbon.** An explicitly experimental C++ successor with published goals:
performance-critical software, C++ interoperability and migration, practical
safety with compile-time checks where possible, and "fast and scalable
development" including separate compilation. It declares a stable language/library
ABI a non-goal. [DOCUMENTED] https://docs.carbon-lang.dev/docs/project/goals.html
— n.d.

**LLVM itself.** "LLVM is a Static Single Assignment (SSA) based representation
that provides type safety, low-level operations, flexibility, and the capability
of representing 'all' high-level languages cleanly." It exists as in-memory IR,
on-disk bitcode, and human-readable assembly. [DOCUMENTED]
https://llvm.org/docs/LangRef.html — n.d.

### 1.8 Cross-language matrix

| Property | C | C++ | Rust | Zig | Odin | Hare |
|---|---|---|---|---|---|---|
| Memory reclamation | manual `malloc`/`free` [DOCUMENTED] | manual + RAII conventions [DOCUMENTED] | ownership/drop, compile-checked [DOCUMENTED] | manual via `Allocator` parameter [DOCUMENTED] | manual via `context.allocator` [DOCUMENTED] | manual `alloc`/`free` expressions [DOCUMENTED] |
| Runtime required | not asserted here (C standard not retrieved; see §8) | startup + possibly unwind/RTTI/static-init runtime [DOCUMENTED] | `core`; `std` optional; panic handler required in `no_std` [DOCUMENTED] | none required; libc optional [DOCUMENTED] | `base:runtime`; libc optional via `-no-crt` [DOCUMENTED] | `rt` module mandatory [DOCUMENTED] |
| Backend named by the project | not language-defined | not language-defined | not stated in the sources cited here | LLVM for optimized builds plus custom backends [DOCUMENTED] | LLVM [DOCUMENTED] | qbe [DOCUMENTED] |
| Integer overflow | signed = UB [DOCUMENTED] | signed = UB [DOCUMENTED] | panic in debug, wrap in release [DOCUMENTED] | illegal behavior; `+%` wraps [DOCUMENTED] | defined wrap [DOCUMENTED] | truncation [DOCUMENTED] |
| Implicit numeric conversion | yes: assignment, argument, and usual arithmetic conversions (note 1) | yes: standard and user-defined conversion sequences (note 2) | no numeric conversion; restricted coercions [DOCUMENTED] | coercion only where "completely safe and unambiguous", otherwise explicit cast (note 3) | no, except `rawptr` and untyped constants [DOCUMENTED] | widening with matching signedness only [DOCUMENTED] |
| Bounds checks | no | no (library only) | yes in safe code | yes in safe build modes [DOCUMENTED] | toggleable [DOCUMENTED] | yes (abort on failure) [DOCUMENTED] |

Notes:

Matrix cells summarize §1.1–1.6 and notes 1–3; per-claim URLs and dates are in
those subsections.

1. C conversion contexts and usual arithmetic conversions:
   https://en.cppreference.com/w/c/language/conversion — n.d. [DOCUMENTED]
2. C++ implicit conversion sequences (standard plus user-defined):
   https://en.cppreference.com/w/cpp/language/implicit_conversion — n.d. [DOCUMENTED]
3. Zig: "Zig has Type Coercion for conversions that are known to be completely
   safe and unambiguous, and Explicit Casts for conversions that one would not
   want to happen on accident." https://ziglang.org/documentation/master/ — n.d.
   [DOCUMENTED]

Cells marked "no" without a citation are the absence of a documented check or
conversion, not a positive documented claim; treat them as HYPOTHESIS-level
negatives. "Bounds checks: no" for C and C++ means the language does not require
them, not that implementations never provide library-level checks.

---

## 2. Top 10 Most Valuable Low-Level Language Features

Ranking criterion: how much a feature reduces a documented failure mode or
documented friction, weighted by how many of the surveyed languages adopted it.
The ranking itself is judgment; the existence and semantics of each feature are
documented.

1. **Explicit allocator parameters (Zig).** Every allocating standard-library
   API takes an `Allocator`; there is no `new`. This makes "does this call
   allocate?" answerable from the signature and keeps the standard library
   usable on freestanding targets. [DOCUMENTED]
   https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.
2. **Deterministic cleanup at scope exit: `defer` and RAII.**
   `defer` executes unconditionally at scope exit (Zig `defer`, Hare
   defer-expressions, Odin `defer`); RAII ties release to object lifetime and
   runs in reverse order. [DOCUMENTED]
   https://ziglang.org/documentation/master/ — n.d.;
   https://en.cppreference.com/w/cpp/language/raii — n.d.;
   https://harelang.org/specification.pdf §6.7.49 — draft, © 2020–2024.
3. **Sum types with exhaustive matching.** Rust `match` is checked for
   exhaustiveness; Odin has tagged unions with `switch`/type switches; Hare's
   spec defines tagged union types. This removes an entire class of "forgot a
   case" bugs without runtime dispatch. [DOCUMENTED]
   https://doc.rust-lang.org/book/ch06-02-match.html — n.d.;
   https://odin-lang.org/docs/overview/ — n.d.;
   https://harelang.org/specification.pdf §6.6.15 — draft.
4. **Compile-time execution and reflection.** Zig `comptime` makes types
   first-class compile-time values and implements generics; Jai's `#run` is
   documented by its community primer as arbitrary compile-time execution.
   [DOCUMENTED for Zig; COMMUNITY for Jai]
   https://ziglang.org/documentation/master/ — n.d.;
   https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md — n.d.
5. **Slices that carry length.** Zig slices, Odin slices, and Hare slices
   (data/length/capacity) make the length part of the value rather than a
   separate parameter. Hare's spec makes capacity explicit and defines
   reallocation behavior. [DOCUMENTED]
   https://ziglang.org/documentation/master/ — n.d.;
   https://odin-lang.org/docs/overview/ — n.d.;
   https://harelang.org/specification.pdf §6.6.16 — draft.
6. **Errors as values, without exceptions.** Zig error sets with `try`; Odin
   multiple return values with `or_return`; Hare error types with propagation
   expressions. Removes unwinding from the runtime contract.
   [DOCUMENTED] https://ziglang.org/documentation/master/ — n.d.;
   https://odin-lang.org/docs/overview/ — n.d.;
   https://harelang.org/specification.pdf §6.7.31 — draft.
7. **No hidden control flow and no hidden allocation as an explicit design
   invariant.** Zig states both as language properties, which makes auditing a
   call graph for allocation and re-entrancy tractable. [DOCUMENTED]
   https://ziglang.org/learn/overview/ — n.d.
8. **C ABI interoperability as a first-class goal.** Zig targets "straightforward
   conformance to the C ABI"; Hare uses "a superset of the C ABI"; Rust uses
   `extern "C"` with `unsafe`. This is the adoption gate for a new systems
   language. [DOCUMENTED] https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.;
   https://harelang.org/documentation/faq.html — n.d.;
   https://doc.rust-lang.org/nomicon/ffi.html — n.d.
9. **Freestanding/no-std as a supported configuration, not a fork.** Zig has
   "first-class support for no standard library"; Rust ships
   `x86_64-unknown-none`; Hare documents freestanding builds with a custom `rt`;
   Odin has `-bedrock` and `-no-crt`. [DOCUMENTED]
   https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.;
   https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html — n.d.;
   https://harelang.org/documentation/usage/freestanding.html — n.d.;
   https://odin-lang.org/docs/overview/ — n.d.
10. **Integrated build system and package manager.** Zig ships build and package
    management usable even for C/C++ projects, removing the autotools/CMake
    dependency for native libraries. [DOCUMENTED]
    https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.

Honorable mentions with equally good evidence: nullable-pointer elimination via
optionals (Zig), `#no_bounds_check`-style local safety control (Odin), distinct
types (Odin), and packet-style explicit struct layout controls. These are not
ranked because their value is domain-dependent.

---

## 3. Top 10 Recurring Developer Complaints

Ranking criterion: frequency and severity as evidenced by official surveys or
public issue threads. This ranking is judgment over the cited evidence.

1. **Rust compilation is slow.** The 2024 State of Rust survey reports that
   "slow compilation was at the top of the list" of productivity limiters.
   [DOCUMENTED, official survey] 
   https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/ — 2025-02-13.
   The Compiler Performance Working Group repeats that long compile times are
   "frequently being cited as one of the biggest challenges". [DOCUMENTED]
   https://blog.rust-lang.org/2025/06/16/rust-compiler-performance-survey-2025/ — 2025-06-16.
2. **Rust is perceived as too complex.** Same survey: "one of the top answers
   remained the worry that Rust will become too complex"; ~31% of non-users cite
   perceived difficulty. [DOCUMENTED]
   https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/ — 2025-02-13.
3. **Rust async is hard.** Same survey lists "struggles with async programming
   (an all-time favourite)" among open-answer productivity problems, and async
   closures as a most-wanted stabilization. [DOCUMENTED]
   https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/ — 2025-02-13.
4. **Borrow-checker friction.** RFC 2094 documents three concrete patterns that
   did not compile and motivated non-lexical lifetimes; the RFC's summary says
   the change eliminates "many common cases where small, function-local code
   modifications would be required to pass the borrow check". [DOCUMENTED]
   https://rust-lang.github.io/rfcs/2094-nll.html — 2017-08-02.
5. **Zig breaks user code between releases.** A maintainer response in a public
   issue: "Zig is pre-1.0, so breaking changes are common... be prepared for your
   build to break often." The reporter calls the specific builtin removal
   "unnecessarily hostile toward early adopters". [COMMUNITY]
   https://github.com/ziglang/zig/issues/21148 — opened 2024-08-20.
6. **Zig's allocator interface is awkward.** Issue #4431: "the current Allocator
   interface is suboptimal. Mainly because it does more than just allocate
   memory, it will also copy data for you." [COMMUNITY]
   https://github.com/ziglang/zig/issues/4431 — 2020-02-11.
7. **Memory unsafety dominates serious security bugs in C/C++.** Chromium
   reports "around 70% of our high severity security bugs are memory unsafety
   problems (that is, mistakes with C/C++ pointers). Half of those are
   use-after-free bugs", based on 912 high/critical bugs since 2015.
   [DOCUMENTED, first-party project data]
   https://www.chromium.org/Home/chromium-security/memory-safety/ — n.d.
8. **Undefined behavior produces optimizer surprises in C/C++.** LLVM's
   three-part series documents real cases where UB-based reasoning changes
   program output; `cppreference` shows the same with `x + 1 > x` compiling to
   `return 1`. [DOCUMENTED]
   http://blog.llvm.org/2011/05/what-every-c-programmer-should-know.html — 2011-05-14;
   https://en.cppreference.com/w/cpp/language/ub — n.d.
9. **C++ runtime features cost code size, so projects remove them.** LLVM:
   "In an effort to reduce code and executable size, LLVM does not use
   exceptions or RTTI." This is documented institutional evidence that the
   feature cost is real enough to reject wholesale. [DOCUMENTED]
   https://llvm.org/docs/CodingStandards.html — n.d.
10. **Ecosystem/tooling gaps in deliberately minimal languages.**
    Odin: dependency handling is manual and users ask how to do it; the official
    answer is that a package manager will never exist. [DOCUMENTED policy +
    COMMUNITY question]
    https://github.com/odin-lang/Odin/discussions/1479 — 2022-02-08;
    https://forum.odin-lang.org/t/dealing-with-dependencies/1676 — n.d.;
    https://odin-lang.org/docs/faq/ — n.d.
    Hare: no generics means hash tables and similar structures must be
    reimplemented per type; the FAQ states this directly and a project blog
    reiterates "Hare does not support generics, and our approach to data
    structures is much like C: DIY". [DOCUMENTED]
    https://harelang.org/documentation/faq.html — n.d.;
    https://harelang.org/blog/2021-03-26-high-level-data-structures — 2021-03-26.

Additional documented trade-offs that did not make the top 10: Hare discourages
multithreading ("Probably not") and does not support Windows/macOS upstream
(https://harelang.org/documentation/faq.html — n.d.); Odin will never support
8-bit or 16-bit targets (https://odin-lang.org/docs/faq/ — n.d.).

---

## 4. Common Compiler Architecture Patterns

### 4.1 Front end: lexer, parser, AST, then IR

LLVM's Kaleidoscope tutorial builds a language in explicit stages: lexer,
recursive-descent parser and AST, then LLVM IR generation, then JIT/optimizer
integration, then control flow and SSA construction, then object-file emission,
then debug info. [DOCUMENTED]
https://llvm.org/docs/tutorial/MyFirstLanguageFrontend/index.html — n.d.

Implication for CoreX: the tutorial's stage boundaries are the accepted
decomposition; a new language does not need a novel front-end architecture.
[HYPOTHESIS]

### 4.2 SSA-based IR as the optimization substrate

"LLVM is a Static Single Assignment (SSA) based representation"; the manual
distinguishes syntactically valid IR from *well-formed* IR (for example,
definitions must dominate uses) and specifies a verifier pass that runs after
parsing and before bitcode output. [DOCUMENTED]
https://llvm.org/docs/LangRef.html — n.d.

Implication: emitting textual LLVM IR means CoreX inherits dominance checking,
type checking, and a large optimization pipeline instead of writing them.
[HYPOTHESIS]

### 4.3 Layered pass management with cached analyses

The new pass manager organizes IR units as Module → (CGSCC →) Function → Loop,
caches analysis results, invalidates them via `PreservedAnalyses`, and exposes
`PassBuilder` hooks so front ends can inject passes at pipeline start.
[DOCUMENTED] https://llvm.org/docs/NewPassManager.html — n.d.

Implication: CoreX should not hand-roll optimization passes; it should emit IR
and, if needed, register a small number of custom passes through `PassBuilder`.
[HYPOTHESIS]

### 4.4 Lowering, ABI, and object emission

Kaleidoscope chapter 8 covers compiling IR to object files; the Itanium C++ ABI
defines data layout, calling sequences, mangling, and unwind tables as a layer
on top of the platform C ABI. [DOCUMENTED]
https://llvm.org/docs/tutorial/MyFirstLanguageFrontend/index.html — n.d.;
https://itanium-cxx-abi.github.io/cxx-abi/abi.html — n.d.

Implication: CoreX's freestanding Linux target needs an explicit decision about
the C ABI for `extern` functions and about whether any unwinding tables are
emitted at all. [HYPOTHESIS]

### 4.5 Textbook pipeline

Cooper and Torczon's *Engineering a Compiler* (3rd ed., 2022) is the standard
text for the front-end/middle-end/back-end decomposition and for SSA construction
and destruction. [DOCUMENTED, bibliographic]
https://shop.elsevier.com/books/engineering-a-compiler/cooper/978-0-12-815412-0
— published 2022-08-20.

### 4.6 Architecture patterns observed across the surveyed languages

| Pattern | Instances | Evidence |
|---|---|---|
| AST interpreter or bytecode for compile-time execution | Jai `#run` (community), Zig `comptime` | JaiPrimer n.d.; Zig Reference n.d. |
| Single-pass or few-pass front end for fast builds | Hare (small spec, qbe), Odin (fast parse goal) | Hare FAQ n.d.; Odin FAQ n.d. |
| LLVM as the only backend | Odin, Rust (rustc), Carbon | Odin FAQ n.d.; rustc target docs n.d.; Carbon goals n.d. |
| Custom backend to keep toolchain auditable | Hare (qbe), Zig (custom backends plus LLVM) | Hare FAQ n.d.; Zig rationale n.d. |
| Whole-program or unity-build option | Zig ("all Zig code lives in one compilation unit"), Odin `-use-single-module` | Zig overview n.d.; Odin overview n.d. |

The last row is an optimization/compile-time trade-off, not a correctness
requirement: Zig argues one compilation unit enables cross-module optimization
(https://ziglang.org/learn/overview/ — n.d.), while Odin defaults to separate
module objects (https://odin-lang.org/docs/overview/ — n.d.).

---

## 5. Memory-Safety Approaches Compared

### 5.1 C: manual, unchecked

`malloc` returns uninitialized storage or `NULL`; the caller must free exactly
once. Out-of-bounds access, use-after-free, double-free, and signed overflow are
undefined behavior. [DOCUMENTED]
https://en.cppreference.com/w/c/memory/malloc — n.d.;
https://en.cppreference.com/w/c/language/behavior — n.d.
Failure mode: the compiler cannot distinguish a valid pointer from a dangling
one, so bugs are found (if at all) at runtime, and the optimizer may assume they
do not occur.

### 5.2 C++: manual plus library conventions

RAII and smart pointers (`unique_ptr`, `shared_ptr`) make release deterministic
*when used*, and destructors run during stack unwinding. These are library and
coding conventions, not language-enforced ownership; raw pointers and manual
`new`/`delete` remain legal, and UB categories remain. [DOCUMENTED]
https://en.cppreference.com/w/cpp/language/raii — n.d.;
https://en.cppreference.com/w/cpp/language/ub — n.d.
Failure mode: safety depends on discipline and on which abstractions a codebase
permits; Chromium's data shows the residual cost at scale (70% of high-severity
security bugs are memory-safety bugs). [DOCUMENTED]
https://www.chromium.org/Home/chromium-security/memory-safety/ — n.d.

### 5.3 Rust: compile-time ownership and borrowing

Ownership rules are checked at compile time; violations do not compile. Borrows
are tracked by inferred lifetimes, and RFC 2094 moved the analysis from lexical
scopes to the control-flow graph to remove false positives. [DOCUMENTED]
https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html — n.d.;
https://rust-lang.github.io/rfcs/2094-nll.html — 2017-08-02.
Escape hatch: `unsafe` blocks, where the programmer discharges proof obligations
the compiler cannot check; raw pointer dereference requires one. [DOCUMENTED]
https://doc.rust-lang.org/reference/unsafe-keyword.html — n.d.
Cost: the type system is "a conservative approximation of the dynamic safety
requirements", so some data structures (for example a doubly linked list with
back-pointers) need `unsafe` or reference counting. [DOCUMENTED]
https://doc.rust-lang.org/reference/unsafe-keyword.html — n.d.

### 5.4 Zig: explicit allocators, no borrow checker

The language performs no allocation; every allocating API takes an `Allocator`.
Allocators can be arena, debug (detects use-after-free, double-free, leaks), or
application-specific. Safety is a build-mode property, and safety-checked illegal
behavior traps only while checks are enabled. [DOCUMENTED]
https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.;
https://ziglang.org/documentation/master/ — n.d.
Failure mode: lifetimes are still the programmer's responsibility; the language
makes allocation sites visible and makes some classes of bug detectable in test
builds, but has no static use-after-free proof.

### 5.5 Odin: manual with context allocators

Manual management, with `context.allocator` supplying the default for `new`,
dynamic arrays, and maps; `context.temp_allocator` is a per-frame scratch arena
cleared with `free_all`. Bounds checks exist and can be disabled per statement.
[DOCUMENTED] https://odin-lang.org/docs/overview/ — n.d.;
https://odin-lang.org/docs/faq/ — n.d.
Failure mode: same class as C/Zig (programmer-managed lifetime), with more
centralized allocator control and default bounds checks.

### 5.6 Hare: manual with language-level allocation expressions

`alloc` returns a tagged union that includes `nomem`, so allocation failure is a
typed value the programmer must handle; `free` is an expression; slices carry
capacity and define reallocation behavior. [DOCUMENTED]
https://harelang.org/specification.pdf §6.7.20, §6.7.28–6.7.30 — draft, © 2020–2024.
Failure mode: no static lifetime proof; the design instead makes failure and
reallocation explicit in the type system.

### 5.7 Summary table

| Model | Compile-time proof | Runtime checks | Escape hatch | Documented cost |
|---|---|---|---|---|
| C manual | none | none | N/A (everything is unchecked) | UB reaches production |
| C++ RAII | none for ownership; destructor ordering guaranteed | bounds not checked by language | raw pointers, `new`/`delete` | code size for EH/RTTI; discipline-dependent |
| Rust ownership | yes for safe code | bounds checks, overflow checks in debug | `unsafe` blocks | borrow-checker restrictions; conservative type system |
| Zig allocators | none for lifetime | safety-checked illegal behavior in safe modes | `@setRuntimeSafety(false)`, `@ptrFromInt` | checks removed in fast/small builds |
| Odin allocators | none for lifetime | bounds checks, toggleable | `#no_bounds_check` | manual lifetime tracking |
| Hare alloc/free | none for lifetime | bounds/abort paths in runtime | casts, `rawptr`-style escape via `opaque` pointers | manual lifetime tracking |

The "documented cost" column is a summary of cited statements, not a measurement.
Rust bounds checking is evidenced by the Rust Book's statement that optimizations
include "eliminating bounds checking on array access":
https://doc.rust-lang.org/book/ch13-04-performance.html — n.d.

---

## 6. Performance Trade-Offs

Only official statements and published benchmark material are used. No
cross-language measurement was run for this document.

### 6.1 Quantified official statement

Hare: qbe-generated code achieves "25% to 75% the runtime performance of
comparable LLVM-generated code, depending on the workload". This is the only
quantified backend comparison found in an official language FAQ. [DOCUMENTED]
https://harelang.org/documentation/faq.html — n.d.
Implication for CoreX: choosing LLVM 20 buys the top of that range; the Hare
number is evidence for the *cost of avoiding* LLVM, not a CoreX measurement.
[HYPOTHESIS]

### 6.2 Official qualitative claims

- Zig: "Zig is faster than C", justified by one compilation unit, illegal
  behavior on both signed and unsigned overflow, a SIMD vector type, and a
  standard library with hash maps and array lists. No numbers on the page.
  [DOCUMENTED, unquantified] https://ziglang.org/learn/overview/ — n.d.
- Rust: iterators and closures are "zero-cost abstractions"; the book's own
  micro-benchmark reports 19,620,300 ns/iter for a `for` loop versus 19,234,900
  ns/iter for the iterator version on one text. [DOCUMENTED]
  https://doc.rust-lang.org/book/ch13-04-performance.html — n.d.
- Rust: ownership "will not slow down your program while it's running".
  [DOCUMENTED] https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html — n.d.
- C/C++: UB lets the optimizer remove checks, which is presented by LLVM as a
  performance feature with correctness hazards. [DOCUMENTED]
  http://blog.llvm.org/2011/05/what-every-c-programmer-should-know.html — 2011-05-14.

### 6.3 Benchmark caveats

The Computer Language Benchmarks Game describes its own measurements as
"micro benchmarks... easy to use, easy to measure, but far from realistic", and
notes the confusion between a language and a language implementation when
labels like "Java" are used. [DOCUMENTED]
https://benchmarksgame-team.pages.debian.net/benchmarksgame/ — n.d.
Therefore no table of "C vs Rust vs Zig" numbers is reproduced here; any such
table would be implementation- and workload-specific.

### 6.4 Structural trade-offs that are documented

| Decision | Documented consequence | Source |
|---|---|---|
| No GC | no collector pauses, but manual lifetime management | Zig rationale; Odin FAQ |
| No hidden allocation | allocator must be threaded through APIs | Zig rationale |
| Exceptions | code size and unwind tables; rejected by LLVM | LLVM Coding Standards |
| RTTI | code size; rejected by LLVM | LLVM Coding Standards |
| Bounds checks | safety in safe modes, removed in ReleaseFast/Small | Zig Reference |
| Overflow trapping | safety, but blocks some optimizations; Zig argues the opposite for *illegal* overflow | Zig overview |
| Borrow checking | no runtime cost, compile-time restriction | Rust Book ch4 |
| Stable ABI absent | layout may change per compilation; FFI must pin `repr(C)` | Rust Reference layout |
| qbe instead of LLVM | small auditable toolchain, 25–75% of LLVM runtime performance | Hare FAQ |

### 6.5 What cannot be claimed

No official benchmark compares Odin, Hare, Zig, Rust, C, and C++ on the same
workload. No official statement quantifies the cost of Rust's bounds checks. No
official statement quantifies startup or binary-size differences for
freestanding targets across these languages. These are gaps, not findings.

---

## 7. Opportunities for CoreX Differentiation

All items in this section are **HYPOTHESIS**. They are inferences from the
evidence above, not claims made by any cited source.

1. **Allocation visibility stronger than Zig's, without Zig's allocator
   interface complexity.** Zig documents that allocator plumbing is verbose
   enough to generate a design issue (#4431). CoreX could keep allocation
   explicit at the *call site* (a distinct `alloc`/`free` form with a size and
   alignment) while keeping the allocator out of every generic signature.
   Risk: loses allocator substitutability that arenas depend on.
2. **Wrapping arithmetic as the defined default, with no UB.** C/C++ leave signed
   overflow undefined; Rust makes it build-mode dependent; Zig traps; Odin and
   Hare define wrapping/truncation. A language that *defines* wrapping and never
   traps would be closest to Odin/Hare semantics while fitting the "no hidden
   control flow" goal. Risk: forecloses overflow-based optimization, which Zig
   explicitly relies on.
3. **No implicit conversions at all, including widening.** Odin already forbids
   implicit numeric conversions; Hare permits widening only. CoreX's stated "no
   implicit conversions" is stricter than every surveyed language. Risk: numeric
   code becomes noisy; Odin's FAQ admits explicit conversion "can annoy the
   writer".
4. **Freestanding Linux x86-64 as the primary target, not a tier.** Rust's
   `x86_64-unknown-none` is Tier 2 with no `std`; Hare requires a user-supplied
   `rt`; Odin requires a runtime package. A language whose default runtime is a
   syscall layer and a panic path could make "hello world with no libc" the
   normal case. Risk: no ecosystem, no crates/packages to reuse.
5. **Textual LLVM IR as a stable, inspectable compiler output.** LLVM supports
   in-memory, bitcode, and textual forms equivalently. Emitting textual IR makes
   the compiler's output diffable and testable without a disassembler, and
   inherits LLVM's verifier. Risk: textual IR is not a stable interface across
   LLVM major versions; LLVM 20 removed recursive types and intrinsics, showing
   IR churn is real.
6. **Panic-free core, with failure as a value.** Hare's `nomem` and Zig's error
   sets both make failure typed; CoreX could make allocation failure a return
   value and keep panics out of the freestanding runtime entirely. Risk:
   ergonomics; every call site must handle failure or propagate it.
7. **A small, specified subset with a written specification early.** Hare's
   specification is a draft but exists, and its FAQ treats "one person can read
   and understand the entire compiler" as a feature. Rust's survey shows
   complexity fear as a top concern. A short normative spec plus a
   single-crate compiler is a defensible differentiator. Risk: a small spec
   limits adoption for large codebases.
8. **Explicit unsafe boundary for raw pointers, with no safe/unsafe split
   elsewhere.** Rust's `unsafe` is a proof-obligation system covering functions,
   traits, impls, and extern blocks. CoreX's brief says "raw pointers plus unsafe
   blocks"; scoping `unsafe` to pointer operations only is simpler and easier to
   audit, but loses the ability to express invariant-bearing abstractions.
9. **Direct syscalls as the only OS interface.** No surveyed language documents
   this as its primary model; Rust, Zig, and Odin all assume a libc option.
   A syscall-only standard layer would make the runtime surface auditable. Risk:
   portability and the need to reimplement libc-adjacent functionality.
10. **Deliberate omission of generics or a restricted form.** Hare omits generics
    and documents the cost (DIY hash tables); Zig implements generics through
    `comptime`; Odin has explicit parametric polymorphism. CoreX has not
    specified generics. Choosing "no generics, monomorphic containers" would
    maximize simplicity and match Hare's evidence; choosing `comptime`-style
    generics would match Zig's. This is a decision point, not a recommendation.

---

## 8. Claims That Could Not Be Sourced

These are stated as gaps so they are not mistaken for findings.

1. **Jai's current implementation and backend.** No official documentation is
   public. The community primer lists "LLVM integration" under *Planned* and
   states that nothing in it is official. Any statement that Jai "uses LLVM
   today" is unsupported by the retrieved sources.
2. **Hare's release status and 1.0 date.** The FAQ says the language will freeze
   at 1.0; no date or version number is given. The specification is marked
   DRAFT.
3. **C's freestanding startup rules from the standard itself.** ISO/IEC 9899 is
   paywalled; `cppreference` was used for UB and translation phases, and the
   freestanding comparison relies on Hare's documentation of what a freestanding
   environment must provide.
4. **ISO wording for C/C++ conversion rules.** Both standards are paywalled;
   `cppreference` pages for C and C++ implicit conversions were used as the
   traceable secondary source. Exact standard paragraph numbers are therefore
   not cited.
5. **Quantified performance for Odin, Zig, Rust, and C on identical workloads.**
   No official cross-language measurement was found. The Benchmarks Game
   explicitly warns against treating its numbers as realistic.
6. **The evidentiary basis for Zig's "faster than C" claim.** The page asserts
   it without a benchmark table or citation.
7. **The cost of Rust bounds checks and of `unsafe` auditing.** No official
   quantification was found; the Rust Reference only states that the type system
   is conservative and that some structures need `unsafe`.
8. **Chromium's memory-safety page date.** The page reports data "since 2015"
   and 912 bugs; no publication date is displayed.
9. **Any CoreX behavior.** No implementation exists; §7 is explicitly
   hypothesis.

---

## 9. Source Index

Primary documentation and specifications:

- Rust Book, ownership: https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html (n.d.)
- Rust Book, data types and overflow: https://doc.rust-lang.org/book/ch03-02-data-types.html (n.d.)
- Rust Book, `match`: https://doc.rust-lang.org/book/ch06-02-match.html (n.d.)
- Rust Book, iterator performance: https://doc.rust-lang.org/book/ch13-04-performance.html (n.d.)
- Rust Reference, coercions: https://doc.rust-lang.org/reference/type-coercions.html (n.d.)
- Rust Reference, type layout: https://doc.rust-lang.org/reference/type-layout.html (n.d.)
- Rust Reference, `unsafe`: https://doc.rust-lang.org/reference/unsafe-keyword.html (n.d.)
- Rustonomicon, introduction: https://doc.rust-lang.org/nomicon/intro.html (n.d.)
- Rustonomicon, FFI: https://doc.rust-lang.org/nomicon/ffi.html (n.d.)
- rustc book, `x86_64-unknown-none`: https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html (n.d.)
- RFC 2094, non-lexical lifetimes: https://rust-lang.github.io/rfcs/2094-nll.html (2017-08-02)
- Zig rationale: https://ziglang.org/learn/why_zig_rust_d_cpp/ (n.d.)
- Zig overview: https://ziglang.org/learn/overview/ (n.d.)
- Zig Language Reference (master): https://ziglang.org/documentation/master/ (n.d.)
- Zig downloads (0.17.0, 2026-10-01): https://ziglang.org/download/
- Odin Overview: https://odin-lang.org/docs/overview/ (n.d.)
- Odin FAQ: https://odin-lang.org/docs/faq/ (n.d.)
- Hare FAQ: https://harelang.org/documentation/faq.html (n.d.)
- Hare specification (draft): https://harelang.org/specification.pdf (© 2020–2024)
- Hare freestanding programs: https://harelang.org/documentation/usage/freestanding.html (n.d.)
- Hare blog, high-level data structures: https://harelang.org/blog/2021-03-26-high-level-data-structures (2021-03-26)
- cppreference, C undefined behavior: https://en.cppreference.com/w/c/language/behavior (n.d.)
- cppreference, C implicit conversions: https://en.cppreference.com/w/c/language/conversion (n.d.)
- cppreference, C++ undefined behavior: https://en.cppreference.com/w/cpp/language/ub (n.d.)
- cppreference, C++ implicit conversions: https://en.cppreference.com/w/cpp/language/implicit_conversion (n.d.)
- cppreference, RAII: https://en.cppreference.com/w/cpp/language/raii (n.d.)
- cppreference, `malloc`: https://en.cppreference.com/w/c/memory/malloc (n.d.)
- cppreference, translation phases: https://en.cppreference.com/w/c/language/translation_phases (n.d.)
- Itanium C++ ABI: https://itanium-cxx-abi.github.io/cxx-abi/abi.html (n.d.)
- LLVM Language Reference: https://llvm.org/docs/LangRef.html (n.d.)
- LLVM Kaleidoscope tutorial: https://llvm.org/docs/tutorial/MyFirstLanguageFrontend/index.html (n.d.)
- LLVM new pass manager: https://llvm.org/docs/NewPassManager.html (n.d.)
- LLVM coding standards: https://llvm.org/docs/CodingStandards.html (n.d.)
- LLVM 20.1.0 release notes: https://releases.llvm.org/20.1.0/docs/ReleaseNotes.html (LLVM 20.1.0)

Surveys, issues, and project data:

- 2024 State of Rust survey: https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/ (2025-02-13)
- Rust compiler performance survey 2025: https://blog.rust-lang.org/2025/06/16/rust-compiler-performance-survey-2025/ (2025-06-16)
- Zig issue #4431: https://github.com/ziglang/zig/issues/4431 (2020-02-11)
- Zig issue #21148: https://github.com/ziglang/zig/issues/21148 (2024-08-20)
- Odin discussion #1479: https://github.com/odin-lang/Odin/discussions/1479 (2022-02-08)
- Odin forum, dependencies: https://forum.odin-lang.org/t/dealing-with-dependencies/1676 (n.d.)
- LLVM blog, undefined behavior #2: http://blog.llvm.org/2011/05/what-every-c-programmer-should-know.html (2011-05-14)
- Chromium memory safety: https://www.chromium.org/Home/chromium-security/memory-safety/ (n.d.)
- Benchmarks Game: https://benchmarksgame-team.pages.debian.net/benchmarksgame/ (n.d.)

Books and adjacent projects:

- Cooper and Torczon, *Engineering a Compiler*, 3rd ed.:
  https://shop.elsevier.com/books/engineering-a-compiler/cooper/978-0-12-815412-0
  (2022-08-20)
- Carbon goals: https://docs.carbon-lang.dev/docs/project/goals.html (n.d.)
- JaiPrimer (community, unofficial):
  https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md (n.d.)

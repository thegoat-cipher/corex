# Competitor Analysis: C, C++, Rust, Zig, Odin, Hare

Companion to `docs/research.md`. Same evidence labels: **DOCUMENTED** (official
docs/spec), **COMMUNITY** (issue/forum/third-party), **HYPOTHESIS** (inference).
Dates are as shown on the page; `n.d.` means no date displayed, retrieved
2026-10-21. CoreX conclusions are HYPOTHESIS by definition: no CoreX
implementation exists.

Purpose: decide what CoreX borrows and what it rejects, with the cost of each
choice made explicit.

---

## C

| Dimension | Assessment | Evidence |
|---|---|---|
| Memory model | Manual. `malloc` returns uninitialized storage or `NULL`; the caller must `free`. No ownership tracking, no lifetime checking, no bounds information in pointers. | https://en.cppreference.com/w/c/memory/malloc — n.d. [DOCUMENTED] |
| Runtime requirements | Effectively none in a freestanding build beyond startup code; a hosted build requires a C library. No exceptions, no RTTI, no static-init machinery in the language. | https://en.cppreference.com/w/c/language/translation_phases — n.d. [DOCUMENTED for the compile/link model; HYPOTHESIS for the runtime characterization] |
| Compile model | Preprocess, tokenize, compile each translation unit, then link translation units and libraries. Headers are textual inclusion; no module system in the language's historical model. | https://en.cppreference.com/w/c/language/translation_phases — n.d. [DOCUMENTED] |
| Safety guarantees | None at the language level. Out-of-bounds access, signed overflow, null dereference, unsequenced modification, and type-punning are undefined behavior; compilers need not diagnose them. | https://en.cppreference.com/w/c/language/behavior — n.d. [DOCUMENTED] |
| ABI/FFI story | C is the interoperability baseline: "C is the most versatile and portable language. Any language that does not have the ability to interact with C code risks obscurity." Struct layout and calling convention are platform C ABI. | https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED, as Zig's claim] |
| Toolchain | Compiler plus system linker; no bundled build system or package manager in the language. | Implied by the translation/link phases, same URL. [DOCUMENTED for compile model; HYPOTHESIS for "no bundled tooling"] |
| Notable weaknesses | Memory unsafety dominates serious security bugs in large C/C++ codebases: Chromium reports ~70% of high-severity security bugs are memory-safety problems, half of them use-after-free. Undefined behavior also enables optimizer surprises. | https://www.chromium.org/Home/chromium-security/memory-safety/ — n.d.; https://en.cppreference.com/w/cpp/language/ub — n.d. [DOCUMENTED] |

---

## C++

| Dimension | Assessment | Evidence |
|---|---|---|
| Memory model | Manual plus RAII conventions. Constructors acquire, destructors release in reverse order, stack unwinding releases on exception. Smart pointers are library types, not language enforcement; raw `new`/`delete` remain legal. | https://en.cppreference.com/w/cpp/language/raii — n.d. [DOCUMENTED] |
| Runtime requirements | CRT plus, depending on features used: unwind tables and an exception runtime, RTTI data, static-initialization guards, and construction/destruction APIs. All are ABI-defined. | https://itanium-cxx-abi.github.io/cxx-abi/abi.html — n.d. [DOCUMENTED] |
| Compile model | Same translation-unit and header model as C, with templates instantiated per translation unit and vtables emitted per class. Separate compilation is per TU; LTO is an option, not the model. | https://en.cppreference.com/w/c/language/translation_phases — n.d. [DOCUMENTED for TU model; per-TU template instantiation is HYPOTHESIS-level detail] |
| Safety guarantees | Same UB categories as C plus data races; RAII prevents leaks only when used consistently. No compile-time ownership or bounds checking. | https://en.cppreference.com/w/cpp/language/ub — n.d. [DOCUMENTED] |
| ABI/FFI story | No language-defined ABI. The Itanium ABI explicitly says it is "not an authoritative definition of the C++ ABI for any particular platform" and that platform vendors decide. It layers on the platform C ABI and covers mangling, vtables, RTTI, guards, and unwinding. | https://itanium-cxx-abi.github.io/cxx-abi/abi.html — n.d. [DOCUMENTED] |
| Toolchain | Compiler, linker, and an external build system (CMake/Meson/Make); no standard package manager. Static/shared library split and ABI mismatch are recurring operational costs. | [HYPOTHESIS] from the ABI and TU evidence above; no single source retrieved |
| Notable weaknesses | Feature cost is real enough that a major project rejects the runtime features outright: "In an effort to reduce code and executable size, LLVM does not use exceptions or RTTI." Complexity and ABI instability are structural. | https://llvm.org/docs/CodingStandards.html — n.d. [DOCUMENTED]; ABI: same Itanium URL [DOCUMENTED] |

---

## Rust

| Dimension | Assessment | Evidence |
|---|---|---|
| Memory model | Compile-time ownership: one owner per value, move on assignment, drop at scope end, borrows checked against inferred lifetimes. No GC. Violations are compile errors. | https://doc.rust-lang.org/book/ch04-01-what-is-ownership.html — n.d. [DOCUMENTED] |
| Runtime requirements | `core` is always required. `std` is optional; `x86_64-unknown-none` has "no support for `std`" and "no default allocator", though `alloc` works with a supplied allocator. A `no_std` binary needs a panic handler. | https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html — n.d. [DOCUMENTED] |
| Compile model | Crate-based compilation with monomorphized generics; Cargo drives builds. Layout is not stable: "Type layout can be changed with each compilation." | https://doc.rust-lang.org/reference/type-layout.html — n.d. [DOCUMENTED]; Cargo is HYPOTHESIS-level detail |
| Safety guarantees | Safe code is memory- and thread-safe by construction; raw pointer dereference and foreign calls require `unsafe`, which is a proof-obligation mechanism, not an unchecked mode. Bounds checks in safe indexing; overflow panics in debug and wraps in release. | https://doc.rust-lang.org/reference/unsafe-keyword.html — n.d.; https://doc.rust-lang.org/book/ch03-02-data-types.html — n.d. [DOCUMENTED] |
| ABI/FFI story | No stable layout by default; FFI pins layout with `repr(C)` and calls through `extern "C"`, which the compiler cannot type-check against the foreign definition. Foreign functions are assumed unsafe. | https://doc.rust-lang.org/reference/type-layout.html — n.d.; https://doc.rust-lang.org/nomicon/ffi.html — n.d. [DOCUMENTED] |
| Toolchain | `rustc` plus Cargo, rustfmt, clippy, rust-analyzer; cross-compilation via `rustup target add`. | https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html — n.d. [DOCUMENTED] |
| Notable weaknesses | Slow compilation is the top self-reported productivity limiter; complexity is a top worry; async is a recurring pain point; the borrow checker rejects some valid patterns (the reason RFC 2094 exists). | https://blog.rust-lang.org/2025/02/13/2024-State-Of-Rust-Survey-results/ — 2025-02-13; https://rust-lang.github.io/rfcs/2094-nll.html — 2017-08-02 [DOCUMENTED] |

---

## Zig

| Dimension | Assessment | Evidence |
|---|---|---|
| Memory model | Manual, with allocation made explicit: "There is no `new` keyword or any other language feature that uses a heap allocator"; every allocating std API takes an `Allocator`. No borrow checker, no GC, no hidden allocation. | https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED] |
| Runtime requirements | No libc required: "equal support for either linking against libc or not linking against it", and "first-class support for no standard library". The standard library is compiled in only where used. | https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED] |
| Compile model | Compiler plus bundled build system and package manager; "all Zig code lives in one compilation unit, optimized together"; LLVM for release builds and custom backends for compile speed. | https://ziglang.org/learn/overview/ — n.d.; https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED] |
| Safety guarantees | Safety-checked illegal behavior in Debug/ReleaseSafe (integer overflow, bounds, null unwrap), disabled in ReleaseFast/ReleaseSmall and togglable per block via `@setRuntimeSafety`. Unadorned pointers cannot be null; optionals are explicit. | https://ziglang.org/documentation/master/ — n.d.; https://ziglang.org/learn/overview/ — n.d. [DOCUMENTED] |
| ABI/FFI story | Targets "straightforward conformance to the C ABI for external functions"; ships libc source and `zig cc` with GCC/Clang-compatible flags. | https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED] |
| Toolchain | Single self-contained archive, statically compiled, no system configuration, cross-compiles out of the box, includes `zig cc` and a Windows resource compiler. | https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED] |
| Notable weaknesses | Pre-1.0 and breaking: latest stable is 0.17.0 (2026-10-01); maintainers state "breaking changes are common" and advise pinning a release. The allocator interface is contested as doing more than allocation. Official "faster than C" claim is unquantified. | https://ziglang.org/download/ — 2026-10-01; https://github.com/ziglang/zig/issues/21148 — 2024-08-20; https://github.com/ziglang/zig/issues/4431 — 2020-02-11; https://ziglang.org/learn/overview/ — n.d. [DOCUMENTED/COMMUNITY] |

---

## Odin

| Dimension | Assessment | Evidence |
|---|---|---|
| Memory model | Manual: "Odin is a manual memory management based language." Allocations default to `context.allocator`; `context.temp_allocator` is a scratch arena cleared with `free_all`. Dynamic arrays and maps each carry an allocator. | https://odin-lang.org/docs/overview/ — n.d. [DOCUMENTED] |
| Runtime requirements | A runtime package (`base:runtime`) supplies compiler-required declarations and context defaults. libc is used by default but `-no-crt` inhibits it; `-bedrock` disables maps, RTTI, init/fini, and non-constant globals. | https://odin-lang.org/docs/overview/ — n.d. [DOCUMENTED] |
| Compile model | Each package compiles to an object file, then links; `-use-single-module` forces a unity build. LLVM is the backend. | https://odin-lang.org/docs/overview/ — n.d.; https://odin-lang.org/docs/faq/ — n.d. [DOCUMENTED] |
| Safety guarantees | Bounds checks by default, toggleable per statement (`#bounds_check`, `#no_bounds_check`). No implicit numeric conversions. Overflow is defined and never panics. No borrow checker, so lifetime safety is manual. | https://odin-lang.org/docs/faq/ — n.d.; https://odin-lang.org/docs/overview/ — n.d. [DOCUMENTED] |
| ABI/FFI story | Foreign system with explicit calling conventions, C variadic support, `cstring`, and `rawptr` coercion for any pointer. | https://odin-lang.org/docs/overview/ — n.d. [DOCUMENTED] |
| Toolchain | Single `odin` driver with `build`, `run`, `test`; targets selected by flags; no package manager by policy ("Odin will never officially support a package manager"). | https://odin-lang.org/docs/faq/ — n.d.; https://odin-lang.org/docs/overview/ — n.d. [DOCUMENTED] |
| Notable weaknesses | No official dependency management, so users vendor manually and ask how to handle transitive dependencies. No exceptions (deliberate). No 8-bit or 16-bit targets. Feature set is broader than a minimal systems language. | https://github.com/odin-lang/Odin/discussions/1479 — 2022-02-08; https://forum.odin-lang.org/t/dealing-with-dependencies/1676 — n.d.; https://odin-lang.org/docs/faq/ — n.d. [DOCUMENTED/COMMUNITY] |

---

## Hare

| Dimension | Assessment | Evidence |
|---|---|---|
| Memory model | Manual with language-level `alloc`/`free`. `alloc` returns a tagged union of the allocation and `nomem`; slices carry data/length/capacity and reallocation failure is `nomem`. | https://harelang.org/specification.pdf §6.7.20, §6.7.28–6.7.30 — draft, © 2020–2024 [DOCUMENTED] |
| Runtime requirements | A runtime module `rt` is mandatory; it must provide `memcpy`, `memmove`, `memset`, `strcmp`, abort entry points, and optionally `malloc`/`free`. Hare itself does not depend on libc. | https://harelang.org/documentation/usage/freestanding.html — n.d.; https://harelang.org/documentation/faq.html — n.d. [DOCUMENTED] |
| Compile model | Module/translation-unit model; `hare build` compiles and links, with `-F` selecting freestanding. Backend is qbe, with binutils or lld as linker. | https://harelang.org/specification.pdf §5.2–5.3 — draft; https://harelang.org/documentation/usage/freestanding.html — n.d.; https://harelang.org/documentation/faq.html — n.d. [DOCUMENTED] |
| Safety guarantees | Bounds and type-assertion failures abort through runtime error paths; overflow truncates by definition; only widening with matching signedness is implicit, everything else requires a cast. No borrow checker. | https://harelang.org/specification.pdf §6.6.7.4, §6.8.4 — draft; https://harelang.org/documentation/usage/freestanding.html — n.d. [DOCUMENTED] |
| ABI/FFI story | "Hare uses a superset of the C ABI and you can directly call C functions from Hare and vice-versa." Calling convention is fixed; entry-point translation can be done in assembly or C. | https://harelang.org/documentation/faq.html — n.d.; https://harelang.org/documentation/usage/freestanding.html — n.d. [DOCUMENTED] |
| Toolchain | `hare` build driver plus `harec`; small enough that "one person can read and understand the entire compiler" is a stated goal. No package manager and no language server yet. | https://harelang.org/documentation/faq.html — n.d. [DOCUMENTED] |
| Notable weaknesses | No generics (hash tables must be written per type); no package manager; multithreading is discouraged upstream; no official Windows/macOS support; qbe output runs at "25% to 75%" of comparable LLVM output. | https://harelang.org/documentation/faq.html — n.d.; https://harelang.org/blog/2021-03-26-high-level-data-structures — 2021-03-26 [DOCUMENTED] |

---

## What CoreX Borrows vs Rejects

CoreX constraints used as the decision baseline: statically typed, AOT, Rust
compiler, LLVM 20 textual IR, Linux x86-64 freestanding, no libc, direct
syscalls, no GC, no hidden allocation, explicit alloc/free, raw pointers plus
unsafe blocks, no implicit conversions, wrapping arithmetic.

| Decision | Borrow from | Reason | Cost / risk |
|---|---|---|---|
| Explicit allocation call sites, no hidden allocation | Zig | Zig documents that every allocating API takes an `Allocator` and that there is no `new`; this makes "does this call allocate?" answerable from the signature and keeps std usable on freestanding targets. https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED] | Allocator identity must be threaded through code or fixed globally; Zig's interface is contested (issue #4431), so copying its exact shape is not obviously right. |
| `alloc`/`free` as visible, paired operations | C, Hare | C's `malloc`/`free` pairing is the baseline everyone knows; Hare turns allocation into a typed expression with `nomem`, which CoreX can mirror without a GC or a runtime. https://harelang.org/specification.pdf §6.7.20 — draft [DOCUMENTED] | Manual lifetime bugs remain possible; no static proof, so an unsafe boundary is mandatory. |
| Unsafe blocks scoped to raw pointer operations | Rust (narrowed) | Rust's `unsafe` is a proof-obligation system spanning functions, traits, impls, and extern blocks; CoreX's brief restricts it to raw pointers, which is simpler to audit. https://doc.rust-lang.org/reference/unsafe-keyword.html — n.d. [DOCUMENTED for Rust; HYPOTHESIS for the narrowing] | Loses the ability to express invariant-bearing safe abstractions; every pointer dereference site becomes a trust boundary. |
| No implicit conversions, including widening | Odin, Hare (stricter) | Odin forbids implicit numeric conversions except `rawptr` and untyped constants; Hare allows widening only with matching signedness. CoreX's "no implicit conversions" is stricter than both. https://odin-lang.org/docs/faq/ — n.d.; https://harelang.org/specification.pdf §6.8.4 — draft [DOCUMENTED] | Numeric and FFI-heavy code becomes verbose; Odin's FAQ concedes explicit conversion "can annoy the writer". |
| Wrapping integer arithmetic as defined behavior | Odin, Hare | Odin defines signed overflow deterministically and forbids the optimizer from assuming it cannot happen; Hare truncates to the least significant bits. This removes the C/C++ signed-overflow UB class. https://odin-lang.org/docs/overview/ — n.d.; https://harelang.org/specification.pdf §6.6.7.4 — draft [DOCUMENTED] | Gives up overflow-based optimization that Zig deliberately uses (`@setRuntimeSafety`), and differs from Rust's debug-panic default. |
| LLVM 20 textual IR as the only backend | Odin, Rust, Carbon (LLVM); Zig (LLVM for release) | LLVM is SSA-based, typed, verified, and documented in three equivalent forms; using it inherits a mature optimizer and verifier. https://llvm.org/docs/LangRef.html — n.d.; https://odin-lang.org/docs/faq/ — n.d. [DOCUMENTED] | Textual IR is not a stable API across LLVM majors; LLVM 20 already removed recursive types and intrinsics. Also abandons Hare's auditable-small-toolchain property (qbe is ~15k lines). |
| Freestanding Linux x86-64 as the default target | Hare (freestanding `rt`), Rust (`x86_64-unknown-none`) | Hare requires only a small `rt` (memcpy/memset/abort, optional malloc); Rust proves a no-`std`, no-default-allocator target is viable. https://harelang.org/documentation/usage/freestanding.html — n.d.; https://doc.rust-lang.org/rustc/platform-support/x86_64-unknown-none.html — n.d. [DOCUMENTED] | No ecosystem: no libc, no crates, no C++ stdlib; every utility must be built or bound. |
| **Reject:** garbage collection | Rejected (vs Go-like models not in scope) | No surveyed systems language in this set uses GC; CoreX requires deterministic frees and no hidden allocation. Zig and Odin both document manual management. https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d.; https://odin-lang.org/docs/overview/ — n.d. [DOCUMENTED as their design] | None beyond programmer burden; the burden is the point. |
| **Reject:** borrow checker / lifetime inference | Rejected (vs Rust) | CoreX specifies raw pointers plus `unsafe` blocks, not ownership. Rust's own RFC 2094 documents that the checker rejected legitimate patterns and needed a redesign; the checker also forces reference counting for some structures. https://rust-lang.github.io/rfcs/2094-nll.html — 2017-08-02; https://doc.rust-lang.org/reference/unsafe-keyword.html — n.d. [DOCUMENTED] | Memory safety is not statically proven; CoreX must compensate with explicit unsafe boundaries and, ideally, runtime-checked debug builds. |
| **Reject:** exceptions and unwinding | Rejected (vs C++) | Exceptions require unwind tables and runtime support; LLVM rejects them "to reduce code and executable size". A syscall-only freestanding runtime has no unwinder. https://llvm.org/docs/CodingStandards.html — n.d.; https://itanium-cxx-abi.github.io/cxx-abi/abi.html — n.d. [DOCUMENTED] | Error paths must be values or aborts; no destructor-on-unwind semantics, so cleanup discipline is manual. |
| **Reject:** RTTI and reflection in the core language | Rejected (vs C++/Odin) | RTTI is a code-size cost LLVM rejects; Odin's `-bedrock` disables RTTI for freestanding work. https://llvm.org/docs/CodingStandards.html — n.d.; https://odin-lang.org/docs/overview/ — n.d. [DOCUMENTED] | No dynamic dispatch or type introspection; generics/monomorphization must carry the load. |
| **Reject:** implicit numeric conversions and integer promotion rules | Rejected (vs C/C++) | C's conversion and promotion rules are documented as a source of confusion by Odin's FAQ, which forbids implicit numeric conversion. https://odin-lang.org/docs/faq/ — n.d. [DOCUMENTED as Odin's rationale] | More casts; FFI boundaries need explicit conversion helpers. |
| **Reject:** stable language-level ABI / stable struct layout | Rejected (vs C's de facto ABI) | Rust makes no layout guarantees by default and changes layout per compilation; Carbon lists a stable ABI as a non-goal because it impedes evolution. CoreX targets one platform, so it can define a fixed layout instead of inheriting C's. https://doc.rust-lang.org/reference/type-layout.html — n.d.; https://docs.carbon-lang.dev/docs/project/goals.html — n.d. [DOCUMENTED for Rust/Carbon; HYPOTHESIS for CoreX] | Breaking layout changes become a compatibility event; FFI still needs `repr(C)`-equivalent types. |
| **Partially borrow:** generics | Undecided: Zig `comptime` vs Odin parametric polymorphism vs none (Hare) | Zig implements generics through compile-time parameters and first-class types; Odin has explicit parametric polymorphism; Hare omits generics deliberately and documents the DIY cost. https://ziglang.org/documentation/master/ — n.d.; https://odin-lang.org/docs/overview/ — n.d.; https://harelang.org/documentation/faq.html — n.d. [DOCUMENTED] | Monomorphization increases compile time and binary size; omitting generics pushes container code onto users. This is the largest unresolved design decision. |
| **Partially borrow:** compile-time execution | Zig `comptime`, Jai `#run` (community) | Zig documents types as first-class compile-time values used to implement generics; Jai's community primer documents arbitrary compile-time execution via `#run`. https://ziglang.org/documentation/master/ — n.d.; https://github.com/BSVino/JaiPrimer/blob/master/JaiPrimer.md — n.d. [DOCUMENTED/COMMUNITY] | A compile-time interpreter adds compiler complexity and nondeterminism risk (I/O at compile time). |
| **Reject:** package manager in the compiler | Rejected (vs Zig), aligned with Odin/Hare | Odin and Hare both deliberately omit package managers; Zig bundles one. For a freestanding syscall-only target, dependency resolution is orthogonal to the language. https://odin-lang.org/docs/faq/ — n.d.; https://harelang.org/documentation/faq.html — n.d.; https://ziglang.org/learn/why_zig_rust_d_cpp/ — n.d. [DOCUMENTED] | Users vendor dependencies manually, which the Odin forum thread shows is friction. |
| **Reject:** hidden control flow (operator overloading, user-defined conversions, exceptions) | Rejected (vs C++/Rust) | Zig's stated design: no hidden control flow, no hidden allocations, no macros; operator overloading and exceptions are named as hidden control flow. https://ziglang.org/learn/overview/ — n.d. [DOCUMENTED] | Less expressive libraries; arithmetic and indexing cannot be customized. |

### Net position

CoreX's combination is not a copy of any one language:

- Semantics closest to **Odin/Hare**: manual memory, defined wrapping arithmetic,
  no implicit conversions, no GC, no hidden allocation.
- Toolchain closest to **Odin/Rust**: LLVM backend, Rust implementation.
- Target discipline closest to **Hare**: freestanding is the primary environment
  and the runtime is a small, explicit surface.
- Safety posture stricter than **Zig** on conversions, looser than **Rust** on
  lifetimes, and with a narrower `unsafe` surface than Rust.

The uncovered risk is memory safety: CoreX gives up Rust's static proof and Zig's
safety-checked default without adopting a replacement. That is a deliberate
trade, but `docs/research.md` §5 and §7 record it as the main open design
liability. [HYPOTHESIS]

# Third-party notices

This repository contains no vendored third-party source code. The compiler is
written for this project and has zero crate dependencies (`cargo tree --depth 1`
lists only `corex`).

## Build tools (not redistributed)

Building or running the compiler requires external tools that are **not**
included in this repository and are not covered by its license:

| Tool | Typical license | Role |
|---|---|---|
| Rust toolchain (`rustc`, `cargo`) | MIT OR Apache-2.0 | Builds the compiler |
| LLVM 20 (`llvm-as`, `opt`, `llc`) | Apache-2.0 WITH LLVM-exception | Assembles, verifies, optimizes, and lowers the emitted IR |
| C compiler driver (`cc`) and GNU binutils | GPL-family (toolchain) | Links the object file into a static ELF |
| `stty` (util-linux) | GPL-2.0-or-later | Terminal raw mode for the REPL line editor |

The compiler invokes LLVM through its command-line tools. It does not link
`libLLVM`, and the executables it produces are freestanding: they contain no
LLVM code, no Rust runtime, and no C library.

## Compiled programs

Programs you compile with CoreX are your own work. The compiler's license does
not apply to their output, and no runtime library is linked that would impose
terms on them.

## Documentation and research

`docs/research.md` and `docs/competitor-analysis.md` summarize public
documentation, issue trackers, and articles, with source URLs and dates. They
paraphrase; quoted names, product names, and trademarks belong to their
respective owners. References are nominative and do not imply endorsement.

The audit report in `docs/audit-2026-10-21.md` was produced for this project.

## Benchmarks

`bench/fib.c` and `bench/mix.c` are reference implementations written for this
project and are covered by the project license. The harness compares against
the system GCC; GCC is not redistributed.

## Character encoding

CoreX source is UTF-8. The compiler itself ships no character-data tables.

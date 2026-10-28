# CoreX Self-Hosting — Master Status

Last updated: 2026-10-28.

## 1. Objective

A CoreX compiler written in CoreX that compiles its own source and produces a
functional native Linux x86-64 executable, with a reproducible multi-stage
bootstrap.

## 2. Current stage

**Stage 3 reproducibility verified (fixed point: Stage 2 == Stage 3 == Stage 4).**

| Stage | Compiler | Output | Status |
|---|---|---|---|
| 0 | Rust `corex 0.3.0` | `build/stage1/corexc` | COMPLETED |
| 1 | Stage 0 | `build/stage2/corexc` | COMPLETED |
| 2 | Stage 1 | `build/stage3/corexc` | COMPLETED |
| 3 | Stage 2 | `build/stage4/corexc` | COMPLETED |

Hashes (final run, `--opt 0`):

```text
stage1 7b2f8837093d0dd5b6a4e4bce01c2e5dce2d908b7dfdd843fd6e2354df989601
stage2 644c22c5bd3772b8a71365798731c52387b7c9349385030b62a573489244c7a8
stage3 644c22c5bd3772b8a71365798731c52387b7c9349385030b62a573489244c7a8
stage4 644c22c5bd3772b8a71365798731c52387b7c9349385030b62a573489244c7a8
```

Stage 2/3/4 are byte-identical. Stage 1 is Rust-compiled and differs by design
(backend/ABI), but passes the same behavioral corpus.

## 3. Completed milestones

- Baseline audit: Rust compiler 205/205 tests, release binary, toolchain.
- CoreX-native runtime: arena, growable buffer, strings/paths, files, env,
  fork/exec/wait.
- Lexer, AST, parser, type model, semantic analysis, LLVM IR backend, driver.
- Stage 1, Stage 2, Stage 3, Stage 4 builds.
- Reproducibility gate: Stage 2 == Stage 3 == Stage 4.
- Behavioral corpus: 79 tests per stage (16 positive × 4 opt levels + 15
  negative), passed by Stage 1, Stage 2, and Stage 3.
- Slice support in the bootstrap subset: `[T]` values, `&array`/`&mut array`
  coercion, params/returns/locals/struct fields, indexing, range slicing on
  arrays/slices/`str`, `len`, `data_ptr`, `[u8]` print.
- Runtime bounds checks for element indexing and slice ranges (exit 101),
  frontend rejection of constant out-of-range array indices, and spec §3.3
  `str` ↔ `[u8]` compatibility.
- `vec<T>` support: `vec_new`, `vec_push`, `vec_free`, `vec_slice`, `len`,
  checked indexing, struct/slice/str/nested elements, params/returns/fields,
  arrays of vecs, shallow header copies.
- CI step for self-hosting.

## 4. Active tasks

None. See `tasks.md`; SH-001…SH-018 are COMPLETED.

## 5. Blockers

None.

## 6. Architectural decisions

See `ADRs.md` (ADR-SH-001…014). Key choices: single unit via `import`, subset
compiler instead of language changes, process-lifetime arena, textual LLVM IR,
first-class aggregate ABI, explicit else blocks, masked shifts, no `Expr.index`
reuse, str-compatible slices with runtime bounds checks, 24-byte `vec<T>` with
spec growth and checked indexing, Stage2/3/4 hash equality, Rust Stage 0
unchanged.

## 7. Validation

- `selfhost/bootstrap.sh` → Stage 1–4 built; hashes checked; corpus run.
- `tests/selfhosting/run.sh <compiler>` → 79 passed for each stage.
- `cargo test` in `compiler/` → 205 passed (reference suite unchanged).
- `file build/stage2/corexc` → static ELF x86-64; `ldd` → not dynamic.
- Explicit `emit-ir` + `llvm-as` → valid module.
- Struct sizes match between Rust-built and self-built binaries (17 structs).

## 8. Next implementation priorities

The complete remaining list is `tasks.md` — Remaining Backlog (SH-019…SH-046).
Order:

1. **SH-019…SH-021:** `f32`/`f64` types, arithmetic/casts/ABI, and `print`.
2. **SH-022…SH-024:** division/remainder aborts, static aggregate globals,
   `str_to_cstr`/`read_file`/`write_file`.
3. **SH-025, SH-038, SH-039:** diagnostics/fuzz parity, differential
   token/AST/IR dumps, and the full 205-test behavior suite against `corexc`.
4. **SH-030…SH-034:** modules/`corex.toml`, macros/cfg, C ABI/hosted,
   threads/atomics/TLS/asm, then build `lib/corex`.
5. **SH-035…SH-037, SH-040…SH-046:** CLI/tooling parity, debug/sanitizers,
   memory fuzzing, performance, remote CI, and the final AC-1…AC-9 gates.

`compiler/` remains the full-language Stage 0 oracle; do not delete it.

## 9. Known limits

The bootstrap subset excludes floats, modules, function pointers, unions,
bitfields, threads/TLS, inline asm, C `extern`, macros, `for`, `do while`,
labels/goto. Slices and `vec<T>` are supported; element indexing and slice
ranges are bounds-checked at runtime (exit 101). Vec constants and globals are
rejected, and vec headers copy shallowly.
See `bootstrap-architecture.md` §4 and `memory-audit.md` §5.

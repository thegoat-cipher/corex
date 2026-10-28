# Self-Hosting Agent Guide

This document defines ownership, conventions, and integration rules for the
CoreX bootstrap compiler. It is intentionally short; the architecture and ADRs
hold the rationale.

## 1. Roles and module ownership

| Role | Owns | Read-only inputs |
|---|---|---|
| Repository auditor | `repository-audit.md`, `gap-analysis.md` | entire repo |
| Bootstrap architect | `bootstrap-architecture.md`, stage model | audit, compiler, runtime |
| Language analyst | `language-readiness.md` | spec, tests, `lib/corex` |
| Memory engineer | `runtime.cx`, `memory-audit.md` | arena/buffer/process paths |
| Lexer engineer | `lexer.cx` | tokens, spans, diagnostics |
| Parser engineer | `ast.cx`, `parser.cx` | grammar, spec |
| Semantic engineer | `types.cx`, `semantic.cx` | AST, spec, layout rules |
| Codegen engineer | `codegen.cx` | AST annotations, LLVM IR contract |
| Integration engineer | `corexc.cx`, `driver.cx`, `bootstrap.sh` | all modules |
| Validation engineer | `tests/selfhosting/`, `validation-report.md` | compiler artifacts |
| Documentation engineer | remaining `docs/selfhosting/*` | all evidence |

A change that crosses two owners needs one integration owner. The master
orchestrator owns architectural conflicts (ADR-level decisions).

## 2. Coordination rules

- The bootstrap is one translation unit. Concurrent edits to `selfhost/*.cx`
  from multiple agents are not safe in a shared checkout.
- Independent read-only work (audit, language probes) may use subagents.
  Implementation workstreams are serialized unless each agent has its own
  isolated worktree/branch and a defined merge point.
- Every interface change (types, builtins, token kinds, AST fields) must update
  all producers/consumers in the same change:
  `types.cx` → `ast.cx` → `parser.cx` → `semantic.cx` → `codegen.cx`.
- Never modify `compiler/` to make the bootstrap pass. The Rust compiler is the
  oracle. If the bootstrap and reference disagree, the bootstrap is wrong unless
  an ADR documents a deliberate subset difference.

## 3. Coding conventions

- CoreX only; no Rust in `selfhost/`.
- No new runtime dependencies. LLVM tools and `cc` are build-time tools.
- Prefer explicit context pointers (`*mut Sem`, `*mut Fn`, `*mut Cg`) over
  globals. The only process-lifetime state is the arena passed through `main`.
- Tagged structs, not payload enums. All fields present; initialize every field
  a reader can observe.
- Use `switch` for tag dispatch, `while`/`loop` for control flow, and explicit
  return codes for errors. No exceptions, `Result`, or `?`.
- Keep functions small enough to reason about; the semantic/codegen functions
  are the exception and must be covered by corpus tests.
- Use `rt_s`/`rt_i`/`rt_c` to build strings; never hand-roll formatting in
  multiple places.
- Avoid speculative abstraction. A helper belongs in `runtime.cx` only if at
  least two compiler components use it.

## 4. Testing expectations

Before declaring a component done:

1. `corex check selfhost/corexc.cx` (Rust Stage 0) succeeds.
2. `selfhost/bootstrap.sh` reaches Stage 4 with Stage 2 == Stage 3 == Stage 4.
3. `tests/selfhosting/run.sh` passes for Stage 1, Stage 2, and Stage 3.
4. `cargo test` in `compiler/` still passes (205).
5. Any new construct has a positive case at `--opt 0,1,2,3` and, if rejection is
   possible, a negative case.
6. Any IR/codegen change re-runs the corpus; a `--opt` difference is a bug.

Never claim a stage passed without the actual hash/corpus output.

## 5. Integration procedure

1. Add/modify the smallest component.
2. Update the downstream annotations/consumers in the same commit.
3. Build Stage 1 with Rust: `compiler/target/release/corex build
   selfhost/corexc.cx -o build/stage1/corexc --opt 0`.
4. Self-compile Stage 2/3/4 and compare hashes.
5. Run the corpus on all three stages.
6. Update `tasks.md` evidence and, if a decision changed, append an ADR.
7. Keep `docs/selfhosting/request-log.md` append-only.

## 6. Escalation

- Type/layout disagreement: stop and compare `sizeof`/`alignof` between Rust-
  and Stage 1-built binaries before changing code.
- Stage 1 works but Stage 2 crashes: suspect a construct used by the compiler
  but not by the corpus; isolate with `check`/`emit-ir` on individual files and
  add the smallest failing case to `tests/selfhosting/cases/negative` or a
  temporary probe (never commit the probe).
- Reproducibility mismatch: diff IR (`emit-ir`) first; check shift masking,
  temporary/label counters, string IDs, struct order, and `--opt` consistency.
- Any proposed language change: stop and write an ADR; do not change the
  language to make the bootstrap easier (ADR-SH-002).

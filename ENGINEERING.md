# Engineering Roles and Conventions

This file defines who owns what, how work is verified, and how documentation is
maintained. It describes roles, not aspirational process: read the "Execution
reality" section first.

---

## 1. Execution reality (2026-10-21)

- The **Research** and **Product Requirements** workstreams ran as two
  independent work packages in separate checkouts. Their deliverables were
  reviewed and merged into this tree.
- Every other workstream ran **sequentially** in the main checkout. Reason: the
  compiler stages share the AST, `Type`, and semantic tables inside one crate;
  splitting them across parallel checkouts would have produced conflicting edits
  to the same files.
- No work is claimed without a command output or a test result recorded in
  `docs/tasks.md`.

---

## 2. Responsibility matrix

| # | Role | Owns | Inputs | Outputs | Must not |
|---|---|---|---|---|---|
| 1 | Research | `docs/research.md`, `docs/competitor-analysis.md` | Public docs, issue trackers, forums | Evidence-labelled findings with URLs and dates | Invent findings or dates |
| 2 | Product Requirements | `docs/PRD.md`, `docs/NFR.md` | Research, constraints from the directive | Vision, FRs, non-goals, risks, measurable NFRs | Promise features outside MVP without labeling |
| 3 | Systems Architecture | `docs/architecture.md`, `docs/ADRs.md` | PRD, implementation reality | Module boundaries, pipeline, decisions | Edit code |
| 4 | Language Specification | `docs/language-spec.md`, `docs/grammar.ebnf` | Architecture, implementation | Normative syntax and semantics, precedence, diagnostics catalog | Document unimplemented syntax as implemented |
| 5 | Memory Management | `docs/memory-model.md` | Language spec | Allocation model, invariants, guarantee matrix, UB list | Claim memory safety for raw pointer code |
| 6 | Lexer and Parser | `compiler/src/token.rs`, `lexer.rs`, `ast.rs`, `parser.rs` | Grammar and spec | Tokens with spans, AST, recovery, unit tests | Emit diagnostics without spans |
| 7 | Semantic Analysis | `compiler/src/semantic.rs`, `diagnostics.rs` | AST, spec | Typed analysis tables, rejections with codes | Weaken a documented rule for convenience |
| 8 | LLVM Backend | `compiler/src/codegen.rs` | Analysis + AST | LLVM 20 IR that the verifier accepts | Emit IR "that usually works"; verify it |
| 9 | Testing and Quality | `compiler/tests/*`, `scripts/bench.sh` | All stages | Unit and integration tests, benchmarks, defect reports | Mark a feature complete from source alone |
| 10 | Documentation | `README.md`, `ENGINEERING.md`, `master.md`, `docs/tasks.md`, `docs/request-log.md` | Everything above | Synchronized status, evidence, append-only history | Rewrite ADR or request-log history |

The **maintainer** coordinates the workstreams, integrates changes, resolves
conflicts, runs the toolchain, and is accountable for the final verification.

---

## 3. Module ownership

| Path | Owner | Rule |
|---|---|---|
| `compiler/src/token.rs`, `diagnostics.rs` | Lexer/Parser | Token or diagnostic format changes must update `docs/language-spec.md` §11 |
| `compiler/src/lexer.rs` | Lexer/Parser | New keywords also update `token.rs` and `grammar.ebnf` |
| `compiler/src/ast.rs` | Language Specification + Parser | Adding a node requires parser, semantic, codegen, and tests in one change |
| `compiler/src/parser.rs` | Lexer/Parser | Grammar changes are always paired with `docs/grammar.ebnf` |
| `compiler/src/semantic.rs` | Semantic Analysis | Every rejection gets a stable code and a case in `tests/diagnostics.rs` |
| `compiler/src/codegen.rs` | LLVM Backend | Every new construct needs an IR test and a native test when observable |
| `compiler/src/driver.rs`, `main.rs` | Maintainer | User-visible commands and exit codes are documented in README |
| `docs/ADRs.md`, `docs/request-log.md` | Maintainer | APPEND ONLY |
| `docs/tasks.md`, `master.md` | Maintainer | Living status documents |

No workstream edits a file outside its ownership without stating why in the
change description.

---

## 4. Development conventions

1. **Boring design first.** No abstraction without a second caller. No trait
   indirection in the compiler pipeline; data flows through concrete structs.
2. **One direction of information.** Later stages may read earlier outputs, never
   the reverse. The AST is immutable after parsing; semantic results live in side
   tables.
3. **Explicit over implicit.** Types are written; conversions use `as`; unsafe
   operations are in `unsafe`. This applies to the compiler's internals too:
   no hidden globals, no ambient state.
4. **No external crates** in the compiler. If a dependency is unavoidable, it
   requires an ADR.
5. **Errors are data.** Stages return `Result<_, Vec<Diagnostic>>`; the driver is
   the only place that prints or exits.
6. **Preserve user intent.** Do not change language semantics to simplify code;
   change the code.
7. **Append-only history.** ADRs and the request log are never rewritten.
   Superseding decisions get a new entry that references the old one.
8. **Deterministic output.** Same input → byte-identical IR. No hash-iteration
   order in emitted code, no timestamps in the module.

---

## 5. Testing expectations

Every change must pass `cargo test` (122 tests at v0.1). Additionally:

| Change | Required new coverage |
|---|---|
| Token, keyword, literal | lexer unit test with exact token stream |
| Grammar or precedence | parser unit test asserting tree shape |
| Semantic rule | rejection case in `tests/diagnostics.rs` with the exact code |
| New expression or statement | IR test (verifier accepts) + native test if observable |
| Runtime helper or abort path | native test asserting message and exit status |
| Optimization or codegen pipeline | native test at `--opt 3` plus `--opt 0` sanity |
| CLI behavior | manual run recorded in `docs/tasks.md`; exit codes unchanged |

A feature is COMPLETED only when its acceptance criteria pass and the evidence is
recorded. Source presence is not evidence.

Tests must not weaken assertions to make a build green. When a test fails,
determine whether the test or the code is wrong, and state the conclusion in the
request log.

---

## 6. Documentation rules

1. `docs/ADRs.md` and `docs/request-log.md`: append only. Corrections are new
   entries.
2. `docs/language-spec.md` and `docs/grammar.ebnf` are normative. If they and the
   compiler disagree, that is a defect in one of them; fix both in the same
   change.
3. `docs/tasks.md` records status and evidence, not intentions. Backlog items are
   explicitly `NOT STARTED`.
4. `master.md` is the current-state summary; it must match `docs/tasks.md`.
5. Performance numbers live only in `docs/benchmarks.md`, with environment,
   method, and caveats. No performance claim without a measured number.
6. Mark assumptions as assumptions. The PRD does this with `ASSUMPTION`; the
   research document uses `HYPOTHESIS`.

---

## 7. Collaboration procedure

1. The maintainer fixes the scope and interface before parallel work starts.
2. A workstream reports blockers instead of silently narrowing scope.
3. Integration is owned by the maintainer: it runs the toolchain, resolves
   conflicts, and is the only party that declares a milestone complete.
4. When two workstreams disagree (for example, research says "Hare uses LLVM" and
   the source says "qbe"), the primary source wins and the correction is logged.
5. Never fabricate tool output. If a command was not run, it is not evidence.

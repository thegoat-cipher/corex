# Self-Hosting Request Log

Append-only. Entries record requests/decisions that changed the work, not every
command. Timestamps are UTC on the working date.

| ID | Timestamp | Request | Origin | Responsible | Related tasks | Resolution |
|---|---|---|---|---|---|---|
| REQ-SH-001 | 2026-10-28T14:56Z | Achieve verified CoreX compiler self-hosting on Linux x86-64; audit before modifying; multi-stage bootstrap; preserve existing compiler | User master prompt | Master orchestrator + agents 01–11 | SH-001…SH-015 | Accepted; executed |
| REQ-SH-002 | 2026-10-28T15:03Z | Do not restart the project; reuse lexer/parser/spec/backend/tests/build infrastructure | User master prompt | Master orchestrator | SH-001, SH-004…SH-009 | Honored: Rust compiler untouched; new selfhost/ subtree |
| REQ-SH-003 | 2026-10-28T15:06Z | Run existing tests and establish the baseline | User master prompt §17 | Agent 01 / orchestrator | SH-001 | `cargo test`: 205 passed, 0 failed |
| REQ-SH-004 | 2026-10-28T15:07Z | Audit repository and language readiness in parallel | User master prompt §1 | Agents 01, 03 | SH-001, SH-002 | `repository-audit.md`, `gap-analysis.md`, `language-readiness.md` |
| REQ-SH-005 | 2026-10-28T15:20Z | Implement the smallest critical-path component first | User master prompt §17 step 8 | Master orchestrator | SH-004 | Arena/string/file/process runtime (`runtime.cx`) |
| REQ-SH-006 | 2026-10-28T15:47Z | Preserve LLVM backend and freestanding output | User master prompt §7 | Master orchestrator | SH-008, SH-009 | Textual IR + `llvm-as`/`llc`/`cc`; static ELF |
| REQ-SH-007 | 2026-10-28T16:10Z | Fix `else` block statement loss found during self-compile | Orchestrator (bug) | Master orchestrator | SH-006, SH-014 | Explicit `S::Block` for else; ADR-SH-006 |
| REQ-SH-008 | 2026-10-28T16:25Z | Fix `+=`/`-=` emitting invalid IR | Orchestrator (bug) | Master orchestrator | SH-008 | Compound-op mapping; ADR-SH-007 context |
| REQ-SH-009 | 2026-10-28T16:34Z | Fix nested struct-field call misread as `print` | Orchestrator (bug) | Master orchestrator | SH-007, SH-008 | Removed `Expr.index` field reuse; ADR-SH-008 |
| REQ-SH-010 | 2026-10-28T16:45Z | Fix shift-count semantics at `--opt 1..3` | Orchestrator (test failure) | Master orchestrator | SH-008, SH-014 | Mask shift counts; ADR-SH-007 |
| REQ-SH-011 | 2026-10-28T16:55Z | Verify Stage 2/3 byte identity and corpus | User master prompt §3, §8 | Master orchestrator | SH-012…SH-014 | First fixed point `2d66ed14…` for Stages 2/3/4; 42 tests × 3 (superseded by REQ-SH-015 after weak helpers) |
| REQ-SH-012 | 2026-10-28T17:05Z | Extract `selfhost/types.cx` and re-verify | Orchestrator (deliverable alignment) | Master orchestrator | SH-007, SH-013 | Type model extracted; bootstrap re-run green |
| REQ-SH-013 | 2026-10-28T17:12Z | Extend CI with self-hosting gates | User master prompt §13 | Master orchestrator | SH-015 | CI step runs `bootstrap.sh` and asserts hashes/corpus |
| REQ-SH-014 | 2026-10-28T17:20Z | Produce final documentation and status | User master prompt §16 | Master orchestrator | SH-015 | Files in `docs/selfhosting/` |

| REQ-SH-015 | 2026-10-28T17:35Z | Add weak memory helpers for `--opt 1..3` self-compile and record final hashes | Orchestrator (link failure) | Master orchestrator | SH-008, SH-013, SH-015 | ADR-SH-011; final Stage 2/3/4 `11080572…`, Stage 1 `0d75b20a…`; corpus green |

| REQ-SH-016 | 2026-10-28T18:05Z | Add slice support (`[T]`, `&array`, ranges, slice params/returns, `[u8]` print) and re-verify the fixed point | User "make another agent for the next steps" | Agent 12 (Bootstrap subset) | SH-016 | ADR-SH-012; Stage 2/3/4 `ff6677ca…`; 49 tests × Stage 1/2/3; Rust suite 205 |
| REQ-SH-017 | 2026-10-28T18:40Z | Fix the remaining slice-related limitations: runtime element bounds checks and spec §3.3 `str`/`[u8]` compatibility | User "help to fix any limitations" | Agent 12 (Bootstrap subset) | SH-017 | ADR-SH-013; Stage 2/3/4 `c417b392…`; 62 tests × Stage 1/2/3; Rust suite 205 |
| REQ-SH-018 | 2026-10-28T19:05Z | Implement the next milestone: `vec<T>` (`vec_new`/`vec_push`/`vec_free`/`vec_slice`, indexing, `len`) and re-verify the fixed point | User "next milestone" | Agent 12 (Bootstrap subset) | SH-018 | ADR-SH-014; Stage 2/3/4 `644c22c5…`; 79 tests × Stage 1/2/3; Rust suite 205 |

## Open items

None blocking. Non-blocking follow-ups are recorded in `master.md` §7.

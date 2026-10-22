# Handoff — CoreX C-class roadmap (v0.3)

Date: 2026-10-22
Branch: `room/corex-c-class-roadmap-deepseek-hjce`
Base: `main` @ `2e4bfb2`
Worktree: `~/.gui-cloudroom/plugins/environment-git-worktree/host-data/worktrees/thr_jw54wyhjce-1/first-app`

## State

All P0 and P1 roadmap items are implemented, tested, documented, and committed.
The working tree was clean at handoff time except for this file.

Commits on the branch (oldest → newest):

| Commit | Contents |
|---|---|
| `d6e4248` | Function pointers, C control flow, unions/bitfields/FAM, layout introspection, compound literals, `extern "C"`/`cstr`/varargs/`main(argc,argv,envp)`, C aggregate ABI, extra-object linking |
| `72f4547` | File modules + generated interfaces + `module$item` mangling, separate compilation, archives/shared objects, `corex.toml`, `#[cfg]`, freestanding stdlib (`lib/corex`) |
| `f5387a3` | Formatter, linter, docs generator, test runner, LSP (`corex-lsp`), DWARF 5, sanitizers, volatile/atomics, threads/futex/TLS, inline asm, macros, hosted mode, `dlopen` |
| `ff8e7c9` | v0.3 docs: spec, grammar, architecture, memory model, ADRs 0025–0032, PRD, tasks, README, installation, resources, benchmarks |
| `165d7f9` | `master.md` refresh + `examples/dispatch.cx` |

## Verification (all run)

- `cargo test` → **193 passed, 0 failed** (68 unit, 125 integration).
- Every `examples/*.cx` run at `--opt 0,1,2,3` → identical stdout and exit status.
- `tests/ffi.rs` compiles a C helper with `cc` and links it both directions
  (including aggregate and variadic calls).
- `tests/modules.rs` builds a manifest executable, a static archive, and a shared
  library loaded with `dlopen`/`dlsym`.
- `tests/tooling.rs` inspects DWARF with `readelf` and drives the LSP over a
  framed stdio session.
- `tests/concurrency.rs` runs two threads incrementing a futex-mutex-protected
  counter (10,000 increments) and checks TLS round-trips.
- Benchmarks (`scripts/bench.sh`, `docs/benchmarks.md`): median CoreX/C ratios
  `mix` 1.017, `array` 1.056, `struct` 0.964, `f64` 0.947, `fib` 1.636.
  Compiler throughput on 4,001 lines: 22 ms frontend, 798 ms full `--opt 2`.

## Blocker: branch not pushed

`git push -u origin room/corex-c-class-roadmap-deepseek-hjce` fails in this
environment:

```
error: unable to read askpass response from '/usr/bin/false'
fatal: could not read Username for 'https://github.com': terminal prompts disabled
```

Tried: `git fetch origin` (works, no divergence), `gh` (not installed),
`ssh-add -l` (no identities), `ssh -T git@github.com` (publickey denied).
There is no credential helper, `~/.git-credentials`, or `GH_TOKEN` available.

To finish: authenticate once (host user's git credential helper, `gh auth login`,
or an SSH key + SSH remote), then:

```bash
cd <repo>
git pull --rebase origin main
git push -u origin room/corex-c-class-roadmap-deepseek-hjce
```

No rebase was needed at handoff time (`HEAD..origin/main` was empty).

## Known gaps (documented, not silent)

1. No thread join; `std_thread` has spawn/yield/exit and `free_stack`.
2. TLS is the explicit `std_tls` table keyed by `gettid`, not compiler
   `thread_local`.
3. Macros are expression macros, unit-local and non-hygienic.
4. `volatile` applies to globals; pointers use `volatile_load`/`volatile_store`.
5. `fib` is ~1.6× slower than GCC; arithmetic/pointer/struct/FP kernels are at
   parity. See `docs/benchmarks.md`.
6. Package management and non-x86-64 targets remain out of scope (P2).

## Token usage

The harness budget helper reports `spent = 3055` (internal units; if those are
thousands of tokens this is ~3.05M, above the 850k threshold, which is why this
file exists). `total`/`remaining` are unbounded in this environment.

## Quick resume

```bash
cd compiler && PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo test
cd .. && ./scripts/bench.sh 5
```

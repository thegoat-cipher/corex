# Validation Report — CoreX Self-Hosting

Date: 2026-10-28
Compiler under test: integrated `selfhost/` (469 functions, 15,065 lines).

## 1. Result summary

| Gate | Result | Evidence |
|---|---|---|
| Stage 0 builds Stage 1 | PASS | Stage 1 `ccffc972…`; runs the corpus |
| Stage 1 builds Stage 2 | PASS | Stage 2 `d270cd67…` |
| Stage 2 builds Stage 3 | PASS | Stage 3 identical to Stage 2 |
| Stage 3 builds Stage 4 | PASS | Stage 4 identical |
| Stage 2/3 compile user programs | PASS | 241 checks per stage; outputs byte-identical |
| Compiler behavior across stages | PASS | same corpus on Stage 1/2/3 |
| Artifact reproducibility | PASS | `sha256(stage2) == sha256(stage3) == sha256(stage4)` |
| Optimized reproducibility | PASS | `repro-opt.sh --corpus`, `--opt 1..3` |
| Rust suite unaffected | PASS | 205/205 `cargo test` |

## 2. Behavioural corpus

`selfhost/bootstrap.sh` runs 241 checks against Stage 1, Stage 2, and Stage 3.
The corpus covers arithmetic, aggregates, pointers, slices, `vec<T>`, floats,
static initializers, modules, macros, function pointers, C FFI, and runtime
bounds/check aborts. All expected stdout and exit statuses are byte-for-byte
oracle output.

Additional gates:

| Gate | Result |
|---|---|
| CLI parity | 62/62 |
| Differential token/AST/IR dumps | 73/73 |
| Fuzz/ASan inputs | 326/326 |
| Rust reference tests | 205/205 |
| `emit-ir --verify` / `llvm-as` | all builds pass |

Known IR signature divergences are in
`tests/selfhosting/dump-ir-expected.txt`; they are ABI differences (not
miscompilation) recorded from the differential oracle.

## 3. Frontend and IR

- `compiler/target/release/corex check selfhost/corexc.cx` → `ok: 469
  function(s), entry main`; the bootstrap compiler produces the same result.
- Every build invokes `llvm-as`; malformed IR aborts the build.
- `llc -filetype=obj -relocation-model=static` and `cc -nostdlib -static
  -no-pie` produce a static ELF.
- The module emits weak freestanding `memset`/`memcpy`/`memmove`/`memcmp` so
  LLVM loop-idiom synthesis links without libc.

## 4. Native execution

The 36 positive cases run at `--opt 0,1,2,3`. Runtime abort cases (array,
slice, `str`, `vec` bounds; division by zero; signed `MIN/-1`) exit 101 with
the expected message on Stage 1 and Stage 2. The 52 negative cases are
rejected with stable diagnostic codes; `check` reports them without crashing.

## 5. Bootstrap validation

```text
stage1 ccffc9721da61cd5cf880a1616e01f7c762fba53939e7618c29b299321447032
stage2 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
stage3 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
stage4 d270cd6707df85fd01f5c2cb746e76900f1b5f6a9aaa64c5c667286a15711e20
reproducible: stage2 == stage3 == stage4
```

`file build/stage2/corexc` → static x86-64 ELF; `ldd` → not a dynamic
executable. Repeated Stage 2 builds are byte-identical.

Optimized levels are deterministic and self-reproducing:

| Level | SHA-256 |
|---|---|
| `--opt 1` | `a5a097167d9f1e83f217384ef134792cc2bc7eff569eb9fd0a6e6b868808519b` |
| `--opt 2` | `e076f652dbbd03ec7357bb18e20b71d26e194c9dd14507673ee6e9813a7e1aac` |
| `--opt 3` | `76445b562f9bf982b410af38b41992722ae112df4999c4bd1d4549564b992b36` |

## 6. Acceptance criteria

| ID | Status | Note |
|---|---|---|
| AC-1 corpus parity | PARTIAL | 241-check corpus + examples matrix; full example/lib matrix not re-run |
| AC-2 negative parity | PARTIAL | Selected negative corpus with stable codes; full oracle parity not measured |
| AC-3 IR validity | PASS | Every build is assembled |
| AC-4 full 205-test behavior suite | BLOCKED | Depends on SH-033 and remaining tooling/LSP gaps |
| AC-5 modules/FFI | PASS (scope) | Module projects, archives/shared objects, C both directions |
| AC-6 determinism | PASS | Twin builds byte-identical |
| AC-7 fixed point | PASS | Stage 2 == Stage 3 == Stage 4 |
| AC-8 performance | PASS | 695 ms self-compile at `--opt 0`; frontend within budget |
| AC-9 no regression | PASS | Rust `corex` unchanged; 205/205 |

## 7. Known limitations

- Threads, atomics, `volatile`, TLS, and inline asm are not implemented
  (SH-033).
- `lib/corex` builds 10/16 modules at P1 time; SH-034 remains blocked.
- LSP/line-editor parity is not ported (SH-037).
- `-g` is rejected because the bootstrap emits no DWARF.
- Aggregate/`bool` calling conventions differ from Rust `corex`, so objects
  from the two compilers are not link-compatible for aggregate parameters.
- Module builds are whole-program; no generated interface files.
- Vec globals/constants are rejected and vec headers copy shallowly.
- REPL prints no prompts.

See `master.md`, `tasks.md`, and the worklogs for details.

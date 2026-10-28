# CoreX language readiness for self-hosting

**Question:** can CoreX, as implemented by the release compiler in this
checkout, express a compiler that compiles itself?

**Verdict: yes, with two caveats.** Every language mechanism a compiler
frontend/backend/driver needs was verified against the real release binary. All
known gaps except one are real but have mechanical encodings; **`slices by
value` is not a gap** (verified working). Three implementation defects were
found that a self-hosted compiler must either see fixed or route around:
a **function-pointer-typed scalar `static` miscompiles** (SIGSEGV), and
**`case` labels and array-repeat counts reject named `const`s** even though the
spec language allows them. No language change is required for self-hosting.

Baseline for every claim below:

| Item | Value |
|---|---|
| Repo commit | `5f0b4bc0b8111d00e23965e5d236985c1dc840b2` |
| Compiler | `compiler/target/release/corex`, `corex 0.3.0 (LLVM 20 backend)`, sha256 prefix `c18726c7b0994fd6cdb050749f033746` |
| Build | `cd compiler && PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo build --release` → success |
| Toolchain | LLVM 20.1.8, `cc` (Ubuntu 15.2.0), Linux x86-64 |
| Existing suite | `cargo test --release` → **205 passed, 0 failed** (68 unit + 137 integration) |
| Probe corpus | `/tmp/ag03-corex/` (outside the repo); scripts `FINAL.sh`, `collect.sh` |

Probe commands use two shorthands. Every verdict below was produced by exactly
these:

```
CX=/home/vib/project/mobile/first-app/compiler/target/release/corex
run <file>  =  "$CX" run /tmp/ag03-corex/<file> --opt 0
chk <file>  =  "$CX" check /tmp/ag03-corex/<file>
```

`-I /home/vib/project/mobile/first-app/lib/corex` is added when a probe uses
`mod std_*`.

---

## 1. Capability matrix

Each row: requirement, verdict, probe source, command, observed result.
`rc` is the process exit status; `out` is stdout with one line per `,`.

| Requirement | Verdict | Probe | Command | Observed |
|---|---|---|---|---|
| Fixed-width integers, `bool`, `char` | **Works** | `probes/p01_primitives.cx` | `run probes/p01_primitives.cx` | rc=0; `i8` −128 … `u64` 18446744073709551615, `bool` true/false, `'A'` → 65, `0x4121` → 16673, `0o377` → 255, `i8` wrap 127+1 → −128 |
| Pointers, deref, pointer arithmetic | **Works** | `probes/p02_pointers.cx` | `run probes/p02_pointers.cx` | rc=0; `11,22,33,65,1` — `alloc(64)`, `*p`, `*(p+1)`, byte pointer, `usize` round-trip |
| Pointer stride uses pointee size | **Works** | `bound/w14_struct_ptr_arith.cx` | `run bound/w14_struct_ptr_arith.cx` | rc=0; `12,true,77` — `sizeof(Tok)==12` is the `*Tok` stride |
| Arrays `[T; N]`, copy semantics | **Works** | `probes/p03_arrays.cx` | `run probes/p03_arrays.cx` | rc=0; `5,30,3,20,50,1,2,150,7` — copy-on-bind confirmed (`b[0]=99` leaves `a[0]==10`) |
| Slices `[T]`, range indexing, `len` | **Works** | `probes/p03_arrays.cx`, `probes/p14_slices.cx` | `run probes/p14_slices.cx` | rc=0; `9,8,8,4,5,2,8,2` — `&array`, `&mut array`, `s[1..3]`, `s[2..]`, alias writes |
| Slices passed/returned **by value** | **Works** (claimed gap is false) | `probes/g08_slice_by_value.cx` | `run probes/g08_slice_by_value.cx` | rc=0; `3,10,20` — `fn passthru(s: [i32]) -> [i32]` and `fn first_of(s: [i32])` both compiled and ran |
| `str` and `cstr` | **Works** | `probes/p04_strings.cx` | `run probes/p04_strings.cx` | rc=0; `hello,5,ell,3,abc,HELLO,0` — literal, `len`, slice, `c"abc"`, `cstr_len`, `cstr_to_str`, `str_to_cstr` |
| Structs: literals, fields, copies, returns | **Works** | `probes/p05_structs.cx` | `run probes/p05_structs.cx` | rc=0; 16 lines `3,4,7,4,0,5,6,6,5,1,11,true,1,9,-3,true` — nested `Rect`, field mutation, by-value copy, `addp` return |
| Enums, explicit discriminants, compare, cast | **Works** | `probes/p05_structs.cx`, `bound/enum_disc.cx` | `run bound/enum_disc.cx` | rc=0; `-5,-4,100,101,0,4` — negative/offset discriminants, `sizeof(E)==4` |
| Functions, recursion, mutual recursion | **Works** | `probes/p06_functions.cx` | `run probes/p06_functions.cx -- alpha beta` | rc=0; `3628800,true,43,3,alpha,beta,true` — `fact(10)`, `is_even`↔`is_odd`, fn-ptr argument |
| `main(argc, argv, envp)` | **Works** | `probes/p06_functions.cx` | same | `argc==3`, `argv[1]="alpha"`, `argv[2]="beta"`, `envp != null` |
| `_start` entry | **Works** | `final/q05_start.cx` | `run final/q05_start.cx` | stdout `start-entry`, rc=7 (explicit `exit(7)`) |
| `if` / `else if` / `else` | **Works** | `probes/p07_control.cx` | `run probes/p07_control.cx` | rc=0; `12,10,3,3,2,1,2,9,1,42` |
| `while`, `loop`, `for`, `do while` | **Works** | `probes/p07_control.cx`, `bound/r10_for_continue.cx` | `run bound/r10_for_continue.cx` | rc=0; `8` — `for` + `continue`; `p07` covers `while true`+`break`, `do/while`, `for(let…)` |
| `switch` with fallthrough and `break` | **Works** | `probes/p07_control.cx`, `probes/p15_dispatch.cx` | `run probes/p15_dispatch.cx` | rc=0; `7,1,2,3,70,true`; `p07` prints `1,2,9` for fallthrough into `default` |
| `continue` inside `switch` reaches loop | **Works** | `forms/f05_switch_nest.cx`, `probes/p20_switch.cx` | `run forms/f05_switch_nest.cx` | rc=0; `112` — nested switch + `break`, `continue` targeted the loop |
| `return` from nested control flow | **Works** | `probes/p17_compiler_slice.cx`, `probes/p30_switch_nodefault.cx` | `run probes/p30_switch_nodefault.cx` | rc=0; `1,2,99` — returns inside switch arms; no exhaustiveness check |
| Casts (int width, sign, float, enum, ptr) | **Works** | `probes/p08_casts_layout.cx` | `run probes/p08_casts_layout.cx` | rc=0; `44,-1,255,65,3,-2.000000,6,1` — truncate, sign-extend, zero-extend, `f64→i32`, enum→i32, ptr↔int |
| `sizeof` / `alignof` | **Works** | `probes/p08_casts_layout.cx`, `forms/f03_sizes.cx`, `probes/p18_tables.cx` | `run forms/f03_sizes.cx` / `run probes/p18_tables.cx` | f03: `sizeof([u8;3])=3`, `sizeof([i32;3])=12`, `sizeof(S)=4`, `offsetof(S,b)=1`; p18: `sizeof(Aligned)=16`, `alignof(Aligned)=16`, `sizeof(*mut u8)=8`, `sizeof(bool)=1`, `alignof([u8;3])=1` |
| `offsetof`, incl. nested struct field | **Works** | `final/q06_nested_offsetof.cx` | `run final/q06_nested_offsetof.cx` | rc=0; `8,24,8,32` — `offsetof(Outer,inner)=8`, `offsetof(Outer,tail)=24`, `sizeof(Outer)=32` |
| C-style layout (natural, packed, align, union, bitfield, flex array) | **Works** | `probes/p08_casts_layout.cx`, `bound/flex.cx` | see §2 | matches expected C layout exactly |
| `static` globals, `static mut` | **Works** | `probes/p09_statics.cx` | `run probes/p09_statics.cx` | rc=0; 21 lines incl. `static str`, `[i32;4]`, `[Entry;2]` with `str` fields, `static mut` arithmetic |
| `const` (scalar/`str`/pointer; array lengths) | **Works** with limits | `probes/p09_statics.cx`, `forms/f03_sizes.cx`, `casef/const_arith.cx` | `run forms/f03_sizes.cx` / `run casef/const_arith.cx` | f03 rc=0 `3,12,4,1,3` — `const N` in `[u8; N]`, `sizeof([i32; N])`, `offsetof`; const_arith rc=0 `7,7` — `const B = A*3+1` used as `[u8; B]` |
| Static aggregate tables | **Works** | `probes/p18_tables.cx`, `bound/w26_2000_table.cx` | `run bound/w26_2000_table.cx` | rc=0; `2000,k1999` — 2000-entry `[str; 2000]` |
| Static function-pointer tables | **Works** | `probes/p09_statics.cx`, `extra2/x01_typedef_fn.cx` | `run extra2/x01_typedef_fn.cx -I …` | rc=0; `42,-21,24,16,10` — `[Entry2;2]` with fn ptrs, `sizeof`, `offsetof` |
| `import` textual inclusion, one flat namespace, idempotent | **Works** | `probes/p10_imports/main.cx` | `run probes/p10_imports/main.cx` | rc=0; `12,30,3,1` — transitive import, duplicate `import "lib.cx"` included once |
| Cyclic imports terminate | **Works** | `bound/cyc/main.cx` | `run bound/cyc/main.cx` | rc=0; `5,1` |
| `syscall` | **Works** | `probes/p11_builtins.cx` | `run probes/p11_builtins.cx` | rc=0; raw `write` printed `raw-syscall`, returned 12 |
| `alloc` / `free` | **Works** | `probes/p02_pointers.cx`, `forms/f06_bigio.cx` | `run forms/f06_bigio.cx -I …` | rc=0; 16 MiB `alloc`, last byte round-trip 42 |
| `print`, `len` | **Works** | `bound/print_types.cx` | `run bound/print_types.cx` | rc=0; `18446744073709551615,-5,1.250000,-2.500000,x,true,bytes,65` |
| `assert`, `panic` | **Works** | `probes/p11b_panic.cx`, `probes/p11c_assert.cx` | `run probes/p11b_panic.cx` | stdout `before`; stderr bytes `b o o m \n`; rc=101. Assert: stderr `b a d \n`, rc=101 |
| Bounds / divide-by-zero aborts | **Works** | `probes/p11d_bounds.cx`, `probes/p21_divzero.cx` | `run probes/p21_divzero.cx` | stdout `before`; stderr `corex: division by zero or signed overflow`; rc=101 |
| Struct literals (incl. partial `...`) | **Works** | `probes/p05_structs.cx`, `probes/p12_aggregates.cx` | `run probes/p12_aggregates.cx` | rc=0; `Pair2 { x: 4, ... }` → `4,0`; nested literals |
| Array literals (list, repeat, index designator) | **Works** | `probes/p12_aggregates.cx`, `bound/w28_big_literal_array.cx` | `run bound/w28_big_literal_array.cx` | rc=0; `2000,1` — 2000-element literal; repeat `[7;4]`; `[i32;4]{1:10,3:30}` → `0,10,0,30` |
| Local struct/array values, whole-value copy | **Works** | `probes/p12_aggregates.cx` | same | rc=0; `1,12,3,10,8,10,8,9,9,11,...` — struct/array assignment copies |
| Assignment to fields, elements, deref, `->` | **Works** | `probes/p12_aggregates.cx`, `final/q04_arrow.cx` | `run final/q04_arrow.cx` | rc=0; `10,2` — `q->x`, `(*q).x` |
| Compound assignment (`+= -= *= /= %= &= |= ^= <<= >>=`) | **Works** | `probes/p12_aggregates.cx` | same | rc=0; chain `20→25→22→44→11→4→16→8→8→8→11`, prints `11` |
| `vec<T>` grow/index/slice/free | **Works** | `probes/p13_vec.cx` | `run probes/p13_vec.cx` | rc=0; `3,10,3,3,65,2,0,0,1` — 3 pushes from cap 1, `vec_slice`, `vec_free` resets len |
| `vec<T>` as parameter/return/struct field | **Works** | `probes/p23_vec_param.cx`, `bound/z4_return_vec_struct.cx` | `run probes/p23_vec_param.cx` | rc=0; `2,7,1,1,1`; returned `Program{tokens:vec}` prints `5,4,src,5` |
| Function pointers as values, struct fields, args, tables | **Works** | `probes/p15_dispatch.cx`, `probes/p29_vtable.cx` | `run probes/p29_vtable.cx` | rc=0; `12,2,true` — fn ptr + `*mut u8` context, `vec<fn(i64)->i64>` |
| `switch` on enum; enum case labels | **Works** | `probes/p15_dispatch.cx`, `bound/w3_case_enum_arith.cx` | `run bound/w3_case_enum_arith.cx` | rc=0; `2` — `case K::A` / `case K::B` |
| File I/O builtins | **Works** | `probes/p16_fileio.cx` | `run probes/p16_fileio.cx` | rc=0; `0,5,104,111,0` — `write_file`→0, `read_file` bytes; missing file → empty vec |
| Compiler-shaped program (arena AST, tagged union, map, builder, diagnostics) | **Works** | `probes/p17_compiler_slice.cx` | `run probes/p17_compiler_slice.cx -I …` | rc=0; `2,Add(Local(0),Int(2)),2,2,0,error[E0201]: undefined name \`x\`` |
| Subprocess + pipe capture | **Works** | `probes/p19_process.cx`, `extra2/x03_spawn_argv.cx` | `run probes/p19_process.cx -I …` | rc=0; `true,0,3`; `x03` → `7,2,hi,0` (`run_argv` status, pipe bytes, `wait`) |
| `#[cfg]`, expression macros | **Works** | `extra/e01_cfg_macro.cx` | `run extra/e01_cfg_macro.cx` | rc=0; `1,42` |
| `extern "C"`, varargs, C ABI both directions | **Works** (`--hosted`) | `extra/e02_extern_c.cx` | `run extra/e02_extern_c.cx --hosted` | rc=0; `5,3,7-x,42` |
| Atomics, `volatile` globals, `asm` | **Works** | `extra/e03_atomics_asm.cx` | `run extra/e03_atomics_asm.cx` | rc=0; `1,1,0,5,true,9,9,true` |
| Structs by value across the internal ABI (copy-in/out) | **Works** | `extra/e04_struct_abi.cx`, `final/q09_struct_array_value.cx` | `run final/q09_struct_array_value.cx` | rc=0; `10,1,99,1,9,16,8` — callee mutation invisible to caller; struct-with-array by value |
| `main` with `argc/argv/envp` reaches `envp` lookup | **Works** | `perf/driver.cx`, `compiler/tests/driver_helpers.rs` | `run perf/driver.cx -I …` | rc=0; `true,true,fallback,44,0,true,true,0,0,0,false,0,true` |
| Modules: `mod`/`module`/`pub`, separate compilation | **Works** | `/tmp/ag03-corex/modtest/`, `bound/r9_module_const_len.cx` | `"$CX" build modtest/main.cx -o …` | build rc=0; run `1,5,4096`; cross-module `const` as array length → `3,helper` |
| End-to-end IR → `llc` → `cc` → ELF | **Works** | `e2e/minicc.cx` | `./e2e/minicc e2e/input.cx e2e/out.ll e2e/out.bin` | rc=0; 8984-byte ELF; running it exits **42**; `emit-ir --verify` prints `IR verified by llvm-as` |

---

## 2. Layout verification (`sizeof`-style)

`probes/p08_casts_layout.cx`, `probes/p18_tables.cx`, `extra2/x01_typedef_fn.cx`,
`final/q06_nested_offsetof.cx`, `final/q09_struct_array_value.cx`,
`bound/flex.cx`, `bound/u01_union_pun.cx`, `bound/z5_alignof.cx`:

| Type | Declared layout | `sizeof` | `alignof` | offsets |
|---|---|---:|---:|---|
| `Point { x:i32, y:i32 }` | natural | 8 | 4 | — |
| `Layout { a:u8, b:u32, c:u8 }` | natural padding | 12 | 4 | 0, 4, 8 |
| `packed struct Packed { a:u8, b:u32 }` | packed | 5 | 1 | — |
| `align(16) struct Aligned { a:u8 }` | raised align | 16 | 16 | — |
| `union Value { i:i32, b:u8 }` | C union | 4 | 4 | both 0 |
| `struct Flags { a:u8:3, b:u8:5 }` | bitfields | 1 | 1 | write 7/1 reads `71` |
| `struct Outer { head:u16, inner:Inner, tail:u32 }` | nested | 32 | — | `inner` 8, `tail` 24 |
| `struct WithSlice { data:[u8], tag:i32 }` | slice is 16 B | 24 | — | — |
| `struct Buf { data:[u8;8], len:usize }` | array field | 16 | — | `len` 8 |
| `struct Blob { length:u64, data:[u8;0] }` | flexible array member | 8 | — | trailing data addressed at +8 |
| `struct Emitter { name:str, fn_ptr:fn(i64)->i64, arity:u8 }` | mixed | 32 | — | `fn_ptr` 16 |
| `enum E` | i32 tag | 4 | 4 | — |

Bitfield write/read is correct (`p08` sets `a=7, b=1`, prints `71`);
union punning is bit-exact (`u01`: `0x0102030405060708` → bytes `8` and `1`).

---

## 3. Known-gap verification

| Claimed gap | Verdict | Probe | Evidence |
|---|---|---|---|
| Payload enums | **Confirmed gap** | `probes/g01_payload_enum.cx` | `chk` → rc=1, `error[E0100]: expected '}', found '(' (after enum variants)` |
| `match` | **Confirmed gap** | `probes/g02_match.cx` | `chk` → rc=1, `error[E0100]: expected ';', found identifier 'x' (after expression)` |
| Generics beyond `vec<T>` (function) | **Confirmed gap** | `probes/g03_generic_fn.cx` | `chk` → rc=1, `error[E0100]: expected '(', found '<' (after function name)` |
| Generics beyond `vec<T>` (struct) | **Confirmed gap** | `probes/g10_generic_struct.cx` | `chk` → rc=1, `E0100: expected '{', found '<' (after type name)` + `expected '=', found '<'` |
| Closures | **Confirmed gap** | `probes/g04_closure.cx` | `chk` → rc=1, `error[E0101]: expected expression, found '|'` |
| Traits | **Confirmed gap** | `probes/g05_trait.cx` | `chk` → rc=1, `E0100: expected an item (…), found identifier 'trait'` and `E0100: expected ':', found ')'` for `self` |
| Thread join | **Confirmed gap** | `probes/g06_thread_join.cx` | `run` rc=1, `error[E0201]: undefined item 'std_thread::join'`; `lib/corex/std_thread.cx` has spawn/yield/exit/free_stack only |
| `thread_local` | **Confirmed gap** | `probes/g07_thread_local.cx` | `chk` → rc=1, `E0100: expected an item (…), found identifier 'thread_local'` |
| Arrays as params/returns | **Confirmed gap** | `probes/g09_array_params.cx` | `chk` → rc=1, `error[E0216]: arrays cannot be passed by value; pass a pointer instead` and `…cannot be returned by value…` |
| Slices by value | **Not a gap** | `probes/g08_slice_by_value.cx` | rc=0, `3,10,20`; also `p14` (aliasing writes) and `p23` (`vec_slice` return) |

Encodings proven for each confirmed gap:

| Gap | Verified encoding | Evidence |
|---|---|---|
| Payload enums | `struct Node { tag: i32, payload: union {…}, … }` + `switch (tag)` | `probes/w01_tagged_union.cx` → `42,7`; `bound/w25_recursive_generic_free.cx` → `42,84,-1` (5-member payload union, recursive `*mut Expr`) |
| `match` | `switch` on the tag; no exhaustiveness checking, `default` required by convention | `p30` shows a missing arm falls out and returns `99` |
| Generics | One container per element type, or `vec<T>` plus `u64`-valued `std_map` indices | `probes/p13_vec.cx`, `extra2/x02_map_scale.cx` |
| Closures | explicit `*mut u8` context + `fn(*mut u8, …)` | `probes/p29_vtable.cx` → `12`; spec §12.1 example |
| Traits | function-pointer structs as vtables | `probes/p15_dispatch.cx` (`VTable` with three `fn(i64,i64)->i64` fields) |
| Thread join | none; use `fork`/`wait` for parallel units | `extra2/x03_spawn_argv.cx`, `probes/p19_process.cx` |
| `thread_local` | `std_tls` explicit table | `bound/u02_tls.cx` → `true,42,true`; `compiler/tests/concurrency.rs::explicit_thread_local_storage_round_trips` |
| Arrays by value | pass `*T` / `*mut T`, or the `[T]` slice (which *is* by value) | `p02`, `p14`, `g08` |

---

## 4. Defects and hard limits found

These are implementation facts or spec/implementation mismatches, not language
design requests.

### D1. Scalar `static` of function-pointer type miscompiles (blocking without workaround)

```corex
fn inc(x: i32) -> i32 { return x + 1; }
static FN: fn(i32) -> i32 = &inc;
fn main() -> i32 { let f: fn(i32) -> i32 = FN; print(f(1)); return 0; }
```

`run bug/a_scalar_static.cx` → **rc=139 (SIGSEGV)**, identical at `--opt 0,1,2,3`.
`emit-ir` shows the defect:

```llvm
@FN = constant ptr @inc, align 8
; in main:
store ptr @FN, ptr %slot0      ; address of the global, not its value
%t1 = load ptr, ptr %slot0
%t2 = call i32 (i32) %t1(i32 1) ; jumps to the data, not to @inc
```

A direct call `FN(41)` emits `call i32 (i32) @FN(...)`, same crash.
`static mut` behaves the same. A `static` of pointer type (`static P: *i32 = &G`)
loads correctly, so this is specific to function types.

**Workarounds verified:** put the function pointer in a one-element array
(`static FNA: [fn(i32)->i32; 1]` → `2`), in a struct field (→ `2`), or take
`&inc` directly at the use site. Direct calls through the array element and the
struct field also work and print `3`/`4`
(`bug/c_array_elem.cx`, `bug/d_struct_field.cx`, `bug/f_workarounds.cx`).
A self-hosted compiler should use **tables** for dispatch (which also matches
how the Rust compiler stores emitters) and never a lone scalar
function-pointer global.

### D2. `case` labels reject named `const`s (spec mismatch)

`docs/language-spec.md` §12.2 says case values are integer constant expressions
including enum variants. The implementation evaluates only literal expressions
and bare enum paths (`semantic.rs::eval_case_const` → `eval_int_const`).

| Label | Result |
|---|---|
| `case 7`, `case 0x07`, `case (7)`, `case 3 + 4`, `case 1 << 1`, `case 7 as i32`, `case -7` | accepted (`casef/*.cx`, `bound/w30_neg_lit.cx`) |
| `case K::A` (bare enum variant) | accepted (`bound/w3_case_enum_arith.cx`) |
| `case KW_FN` where `const KW_FN: i32 = 1` | `error[E0221]: case values must be integer constant expressions` |
| `case TWO + 1` where `const TWO: i32 = 2` | `E0221` |
| `case K::B + 6` | `E0221` |
| `case 'A'` / `case true` | `E0221` |
| `case 18446744073709551615` | `E0221` (case value is evaluated as `i64`) |

**Workaround:** use literals or bare enum variants in `case`; keep a
`const`-based enum or an `if`/`else` chain when names are required.

### D3. Array-repeat counts reject named `const`s (spec strictness)

`let a: [u8; N] = [0; N]` with `const N: usize = 4` →
`error[E0100]: expected repeat count, found identifier 'N'`
(`forms/f02_repeat_const.cx`). `[0; 4]` works; `[0; sizeof(S)]` is also rejected
(`final/q07_sizeof_arraylen.cx`). The type-side length `[u8; N]` **does** accept
a `const` (`forms/f03_sizes.cx`). **Workaround:** literal repeat counts, or
nested repeats `[[0; 1024]; 4]`, or `vec<T>`.

### D4. Value size cap of 1 MiB applies to `static`s too

`static mut A: [u8; 1048577]` → `error[E0223]: value of type '[u8; 1048577]' is
1048577 bytes; the stack allocation limit is 1048576 bytes`
(`bound/s_1m_plus.cx`). Exactly 1 MiB works (`bound/s_1m_exact.cx` →
`1048576,9`). `sizeof([u8; 2097152])` is legal (`bound/z1_sizeof_big.cx` →
`2097152`); only *values* are capped. **Workaround:** split tables across
statics, or allocate at runtime with `alloc`.

### D5. `-I` does not apply to `import`

`import "m04lib.cx"` with `-I bound/inc` → `error[E0232]: cannot import
'm04lib.cx': No such file or directory` (`bound/m04_import_path.cx`).
`import` resolves relative to the importing file, or absolute
(`bound/n05_import_abs.cx` → `77`). `mod` **does** use `-I`
(`modinc/main.cx` → `88`). Imports must therefore use relative or absolute
paths.

### D6. Other verified restrictions (all with accepted encodings)

| Restriction | Evidence | Encoding / note |
|---|---|---|
| No same-block redeclaration/shadowing (`E0202`); outer-block shadowing is fine | `final/q03_shadow.cx` rc=1 `E0202`; `forms/f04_more.cx` prints `first,second,inner,first` | distinct names; inner blocks |
| `let x: T;` without initializer rejected | `bound/r6_let_noinit.cx` → `E0100` | always initialize; zero value |
| Local `static` rejected | `bound/r7_local_static.cx` → `E0101` | module/global state or context struct |
| Labeled `break 'outer` absent (`'outer` lexes as char literal) | `final/q02_labeled_break.cx` → `E0006` | `goto` labels, flags, or helper functions |
| `str ==`, `str +`, struct `==`, `bool as int`, `int as enum`, `char` arithmetic | `p24` `E0210`, `q08` `E0210`, `p25` two × `E0210`, `bound/m01` `E0210`, `p26` `E0203` | `std_str::str_eq`, `std_builder`, field-wise compare, `if`/`else`, `switch` mapping, explicit casts |
| Enum value from integer | `bound/m02_int_enum_switch.cx` → `true,2,false,2` | `switch` mapping or an enum-value table (`bound/w24_table_of_switches.cx`) |
| `sizeof`/`sizeof`-derived `const` outside expressions | `bound/c1_const_sizeof_only.cx` → `E0226` `sizeof(S)` in `const`; `f11` → `E0100` in array length | compute the literal, or use `let n = sizeof(T)` |
| `static` initializers: no `c"…"`, no union/bitfield/packed fields | `bound/w13_cstr_static.cx`, `bound/cstr_tables.cx`, `extra2/x05_static_union.cx` → all `E0226` | build `cstr` vectors at runtime (`bound/w6_argv_build.cx` → `runtime,0`); `str` statics are fine |
| Expression nesting 64 (semantic), parser 128 / ≈62 parens | `bound/chain_200.cx` → `E0111`; chain 60 OK; `lim/paren_62` `E0111`; `lim/casts_64` `E0111`; 120 blocks OK, 125 `E0111` | keep generated expressions flat; statement-per-node |
| No chained assignment, `++`, ternary, tuples | `bound/n01..n04` → `E0100`/`E0101` | separate statements, `if` expressions as blocks, tagged structs |
| `case 'A'`, `case true` | `p20`, `forms/f04_more.cx` → `E0221` | `switch c as i32` |
| `vec<T>` growth invalidates element pointers | `final/q12_vec_realloc.cx` rc=139 after growth | arena + `vec<*mut Node>` (`final/q13_arena_stable.cx` → `0,999,499500`) |
| No exhaustiveness checking on enum `switch` | `p30` → `99` from missing arm | always emit `default` |

---

## 5. What a self-hosted compiler implementation needs

All of the following were exercised in `/tmp`, not inferred from docs.

### 5.1 Data structures

| Need | CoreX facility | Evidence |
|---|---|---|
| Arena for AST nodes and IR text | `alloc` bump allocator, or `std_alloc::Arena` | `probes/p17_compiler_slice.cx`, `perf/walk_arena.cx` |
| Growable node lists | `vec<T>` of structs and of `*mut Node` | `p13`, `p17`, `q13` |
| Stable node identity | `arena + vec<*mut Node>`; indices as `u32`/`usize` | `q13` |
| Symbol table | `std_map::Map` (`str` → `u64`) + `vec` of records, or hand-written open addressing | `x02` (2000 symbols), `probes/p17` |
| String interning / keys | borrowed `str` slices into the source or arena | `std_map` contract; `p17` |
| Tagged AST | `struct { tag: i32/enum, payload: union }` + `switch` | `w01`, `w25`, `p17` |
| Diagnostics | `std_builder::Builder` + `std_fmt` | `q14` → `v-42 0x0xff`; `err[203]: type mismatch` |
| Deterministic order | `std_map` is insertion-ordered; counter-based temp names | `lib/corex/std_map.cx`; `tests/collections.rs` |
| Optional values / errors | `union` + tag, sentinel return + out-params, `bool` + `*mut T` | `q11`; `bound/m02` |

**Memory rule:** per-node `std_alloc::malloc` is not viable. A 200k-node
AST-shaped list cost **800 MB RSS / 0.62 s** with one `malloc` per node
(`perf/walk.cx`) versus **6 MB RSS / 0.004 s** through an arena
(`perf/walk_arena.cx`). One arena per compilation unit, and never hold a
pointer into a `vec<T>` across `vec_push`.

### 5.2 Frontend

- Byte-indexed `str`/`[u8]` and `u8` arithmetic: `p01`, `perf/lex.cx`.
- `switch` + `goto` for dispatch and error recovery: `p07`, `f12`.
- Recursive descent with deep recursion: `recursion_100k.cx` computes
  `depth(100000)=100000`; no practical call-depth issue.
- 23,186-line single unit with 1,200 functions, 60 string tables, and 300
  generated structs: `check` **0.08 s / 44 MB**, `build --opt 0` **0.85 s /
  100 MB**, `--opt 2` **2.70 s**, `--opt 3` **2.88 s** (`scale/unit30k.cx`).
  Separately, a 120-arm `switch` and 300 local bindings compile and run
  (`capacity/big_switch.cx`, `capacity/many_locals.cx`).
- Lexer-shaped throughput: 167,823-byte source scanned with a keyword map in
  **0.95 ms** end-to-end including process start (~177 MB/s), `--opt 2`
  (`perf/lex.cx`).
- 15.6 MB of generated IR text through `std_builder` in **0.033 s / 30 MB**
  (`perf/bigbuild.cx`), written and read back byte-exact.
- 400k `std_map` operations over 4096 keys in **0.0037 s** (`perf/maps2.cx`).

### 5.3 Backend

- Emitting textual LLVM IR and driving the toolchain end-to-end:
  `e2e/minicc.cx` parsed `fn main() -> i32 { return 42; }`, emitted IR, ran
  `llc` and `cc` through `std_process::run_argv`, and produced an 8984-byte ELF
  that exits **42**. `emit-ir --verify` reports `IR verified by llvm-as`.
- Deterministic formatting: `std_fmt` (`push_i64`, `push_u64`, `push_hex`,
  `push_f64`) and `std_builder` are the only number-formatting paths needed.
- Same code path at every optimization level: `p01…p30` were run at `--opt 0`;
  `tests/opt_levels.rs` covers `0..3`, and the compiler's own `p12` IR verifies
  with `llvm-as`.

### 5.4 Driver

| Need | Facility | Evidence |
|---|---|---|
| Read source | `read_file(path) -> vec<u8>` (empty on failure) or `std_fs::read_file` (`-1` on failure, `0` for empty) | `p16`; `bound/z6_fs_exit.cx` → `-1,0,3` for missing/empty/3-byte file |
| Write `.ll`/`.o` | `write_file` builtin, `std_fs::write_file` | `p16`, `f06` |
| Tool discovery | `std_env::env_get(envp, …)`, `std_fs::exists` | `perf/driver.cx` (`PATH` found, fallback) |
| Spawn `llvm-as`/`opt`/`llc`/`cc` | `std_process::spawn_argv` / `run_argv` / `fork` + `exec` + `wait` | `x03` → status `7`; `p19` → pipe capture |
| Capture child output | `std_io::pipe`, `dup2`, `read_all` | `p19` → `true` for `child-ok` |
| Temp dirs | `std_process::getpid`, `std_time::monotonic_ns`, `std_fs::mkdir`/`rmdir` | `perf/driver.cx` (`chdir`/`getcwd` OK); `bound/z6_fs_exit.cx` → mkdir `0`, `is_dir=true`, rmdir `0`, exists `false` |
| Directory scan (module resolution) | `std_fs::dir_open` / `dir_next` / `dir_close` | `perf/driver.cx` → 44 entries |
| Stat / file checks | `std_fs::stat_path`, `is_file`, `file_size`, `exists` | same |
| Exit codes | `main -> i32`, `std_process::exit`, `_start` + `syscall(60)` | `q05` rc=7; `examples/loop_sum.cx` rc=51; `bound/z6_fs_exit.cx` `std_process::exit(9)` → rc=9 |

### 5.5 Concurrency

Not required for a first self-compilation. Threads spawn and share memory
(`compiler/tests/concurrency.rs::threads_share_memory_and_a_futex_mutex`) but
**cannot be joined**; parallel unit builds must use `fork`/`wait`, or stay
single-threaded.

---

## 6. Constructs a self-hosted compiler must avoid

| Do not use | Why | Use instead |
|---|---|---|
| `enum V { A(T) }` (payload enum) | `E0100` | `struct { tag, payload: union }` |
| `match` | `E0100` | `switch` on `tag` (always with `default`) |
| Generic functions/structs | `E0100` | one concrete container per type; `vec<T>` + `u64` indices |
| Closures / lambdas | `E0101` | `*mut u8` context + `fn(*mut u8, …)` |
| Traits / trait objects | `E0100` | struct of function pointers (vtable) |
| `thread_local` / `join` | `E0100` / `E0201` | explicit `std_tls`; `fork`+`wait` |
| Arrays as parameters/returns | `E0216` | `*T`/`*mut T`, or `[T]` (slices *are* by value) |
| Named `const` in `case` | `E0221` | literal / bare enum variant, or `if`/`else` |
| Named `const` in `[v; N]` | `E0100` | literal count, nested repeats, or heap `vec` |
| `sizeof(T)` in `const`/array length | `E0226` / `E0100` | literal constant, or `let` |
| One scalar `static` function pointer | **D1 miscompile → SIGSEGV** | `[fn(…); 1]`, struct field, or `&fn` at use |
| `static` > 1 MiB | `E0223` | split statics or `alloc` at runtime |
| `static` initializer with `c"…"` or a union/bitfield/packed field | `E0226` | `str` static, or build the table at runtime |
| `str ==`, `str +`, struct `==`, `bool as i32`, `i32 as Enum`, `char + i32` | `E0210` / `E0203` | `std_str::str_eq`, builder, field-wise compare, `if`, `switch`, casts |
| `let x: T;`, local `static`, labeled break, `++`, chained assignment, ternary, tuples | `E0100`/`E0101`/`E0006` | initialize, module globals, `goto`/flags, `+= 1`, separate statements, `if` blocks, tagged structs |
| Pointer into a `vec<T>` across `vec_push` | use-after-realloc | arena + `vec<*mut T>` |
| Deeply nested expressions (>64) | `E0111` | statement-per-node, temporaries |

---

## 7. Minimal language subset sufficient for self-compilation

A compiler that compiles itself needs the following closed subset. Every item
below was verified; nothing outside it is required by the Rust compiler's own
structure (18,166 lines; grep counts: 292 `match`, 91 `HashMap`, 37 `HashSet`,
100 `Option`, 90 `Result`, 367 `String`, 352 closure-looking sites — all with
mechanical encodings).

**Required — types.** `i8..i64`, `u8..u64`, `usize`, `isize`, `bool`, `char`,
`f32`, `f64`; `*T`, `*mut T`; `[T; N]`, `[T]`, `str`, `cstr`; `struct`, `union`,
`enum` (i32 tag), `typedef`; `vec<T>`; `fn(…) -> T`; `extern "C"`.

**Required — declarations.** `fn`; `static` / `static mut`; `const` (scalar,
`str`, pointer-to-global); `struct`/`union`/`enum`; `typedef`; `import` (flat,
textual); optional `mod`/`module`/`pub` for a multi-unit build.

**Required — statements and expressions.** `let`/`let mut` (initializer
required), assignment and all ten compound assignments; `if`/`else`;
`while`/`loop`/`for`/`do while`; `switch` with fallthrough, `break`,
`continue`, `return`; `unsafe`; blocks; `goto`/labels (error recovery);
calls, indexing, slicing `s[a..b]`, field access `.`/`->`, `as` casts;
`sizeof`/`alignof`/`offsetof`; struct/array literals (list, repeat, `...`,
index designators); unary/binary operators listed in spec §6.4; `&`/`&mut`.

**Required — builtins.** `syscall`, `alloc`, `free`, `print`, `len`, `assert`,
`panic`, `vec_new`, `vec_push`, `vec_free`, `vec_slice`, `read_file`,
`write_file`, `data_ptr`, `str_from_raw`, `cstr_len`, `cstr_to_str`,
`str_to_cstr`.

**Required — library.** `std_alloc` (arena/malloc/free), `std_mem`, `std_str`,
`std_io`, `std_fs`, `std_process`, `std_env`, `std_fmt`, `std_map`,
`std_builder`. Together these compiled and ran as one 15-module program
(`/tmp/ag03-corex/stdlib/all2.cx` → `true,true,true,1,1,true,true,32,1`).

**Optional, not needed for a first fixed point.** `f32`/`f64` beyond lexing and
printing literals; bitfields; `packed`/`align(N)`/`repr(C)` (only for C interop);
atomics, `volatile`, `asm` (only for the thread trampoline); modules/separate
compilation (single-unit `import` is enough for stage 1); `#[cfg]`; expression
macros; `extern` varargs; `_start` (use `main(argc, argv, envp)`).

**Explicitly not usable, therefore not in the subset.** payload enums, `match`,
generics beyond `vec<T>`, closures, traits, `thread_local`, thread join, arrays
by value, named `const` in `case`/repeat, `sizeof` in constant positions,
scalar `static` function pointers (D1), statics over 1 MiB.

**Size expectation.** The Rust source is 18,166 lines with 292 `match` sites and
91 `HashMap` uses. Expect **22k–32k lines** of CoreX: tagged unions and
`switch` trees replace `match`, `std_map` + index vectors replace typed maps,
`Builder` replaces `format!`, and explicit `*mut`/arena code replaces `Box`/
`Rc`/`RefCell`. That is within the single-unit scale proven in §5.2.

---

## 8. Language changes

**None are required.** Every gap in §3 has a verified encoding, and every
capability in §1 is present in the release compiler. Adding payload enums,
`match`, generics, closures, or traits would shrink the port but would change
CoreX's C-class identity; the encodings above are already proven to the point of
a compiler-shaped program (`p17`) and an end-to-end code generator (`e2e`).

Tradeoffs, stated explicitly:

- **Not fixing D1** (scalar `static` fn pointer) forces the compiler source to
  use array/struct-wrapped dispatch tables. That is an ergonomic and
  review-burden cost, not a blocker; a mis-written scalar global crashes only at
  run time, with no diagnostic.
- **Not fixing D2/D3** (named `const` in `case`/repeat) forces repeated literals
  in the compiler's dispatch and table code. Risk: literals drift from the
  `const` they mirror. Keep one copy in an enum or generate the table
  mechanically.
- **No thread join/TLS** caps parallel compilation at `fork`-per-unit. A
  single-threaded first bootstrap is fine; a stage-2 build that wants
  parallelism should use processes.
- **No arrays by value** forces pointer/slice APIs for every fixed-size buffer,
  which is the C convention anyway and is already how `lib/corex` is written.
- **1 MiB value cap** constrains static tables; the compiler's largest tables
  (token names, diagnostics) fit well below it, and large buffers already go
  through `std_builder`/arena.

If D1 and D2/D3 are fixed in the compiler, the subset in §7 does not change;
only the number of workarounds in the compiler source drops.

---

## 9. Reproduction

```console
$ cd /home/vib/project/mobile/first-app/compiler
$ PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo build --release
$ PATH="$HOME/.cargo/bin:$HOME/opt/llvm20/bin:$PATH" cargo test --release
# 205 passed; 0 failed

$ CX=/home/vib/project/mobile/first-app/compiler/target/release/corex
$ LIBS=/home/vib/project/mobile/first-app/lib/corex
$ /tmp/ag03-corex/FINAL.sh            # consolidated probe table
```

Every cited probe is a standalone `.cx` file under `/tmp/ag03-corex/`:

- `probes/p01…p30` — capability matrix and gaps.
- `probes/g01…g10` — known-gap checks (`g08` is the slice-by-value refutation).
- `probes/w01` — tagged-union encoding.
- `extra/e01…e05` — `#[cfg]`, macros, `extern "C"`/varargs (hosted), atomics/asm,
  struct ABI, string arrays.
- `extra2/x01…x06` — typedef'd function pointers and static vtables, 2000-symbol
  map, process capture, call-site forms, static-union rejection, 100k recursion.
- `final/q01…q14` — thread join, labeled break, shadowing, `->`, `_start`,
  nested `offsetof`, `sizeof` in length, string concat, struct-with-array,
  function-pointer aggregates, out-params, `vec` realloc hazard, arena stability,
  `std_fmt`/`std_builder`.
- `bound/*` — literal/const/scope restrictions, layout edges, enum casts,
  pointer comparisons, pointer stride, imports, 1 MiB boundary, cyclic imports,
  and the D1 repro (`bug/a_scalar_static.cx`).
- `forms/f01…f12` — case-label forms, repeat counts, sizes, misc forms, nested
  switches, 64 KiB buffers, vec element kinds, dispatch tables, `goto` recovery.
- `perf/*` — lexer throughput, AST walk, map scale, 15.6 MB builder, `fib(40)`
  versus C, 23k-line unit (`scale/unit30k.cx`), module build (`modtest/`).
- `e2e/minicc.cx` + `e2e/input.cx` — generated LLVM IR, `llc`, `cc`, ELF exit 42.

No compiler source, test, build script, or `selfhost/` file was modified. The
only write in the repository is this document.

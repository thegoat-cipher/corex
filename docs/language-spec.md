# CoreX Language Specification — v0.1

Status: **normative for the implementation in `compiler/`**. Every rule below is
enforced by the compiler or produced by its code generator. Anything not stated
here is not part of the language.

- Target: Linux x86-64, freestanding (no libc, no dynamic loader).
- Backend: LLVM 20 textual IR, lowered with `llc`, linked with `cc -nostdlib`.
- Grammar: [`grammar.ebnf`](grammar.ebnf) (normative, matches `parser.rs`).
- Memory rules: [`memory-model.md`](memory-model.md).
- Decision history: [`ADRs.md`](ADRs.md).

---

## 1. Design constraints

1. No garbage collection, no tracing, no background reclamation threads.
2. No hidden allocation: every byte comes from a stack slot or an explicit
   `alloc` call.
3. No mandatory runtime: the produced ELF has no startup code beyond the
   program's own `_start` (or the synthesized one), no C library, no exceptions,
   no reflection, no TLS, no init arrays.
4. Explicit over implicit: types are always written, conversions are always
   spelled with `as`, and every unsafe operation is inside `unsafe`.
5. Predictable semantics: integer overflow wraps, shifts are masked, division by
   zero and out-of-bounds indexing abort with a fixed exit code instead of
   invoking undefined behavior.

---

## 2. Lexical structure

- Source is UTF-8; code outside comments and string literals must be ASCII. Any
  other byte is `E0001`.
- Whitespace separates tokens and is otherwise insignificant.
- Comments: `// ...` to end of line, `/* ... */` (not nested). An unterminated
  block comment is `E0003`.
- Identifiers: `[A-Za-z_][A-Za-z0-9_]*`. Keywords and builtin names are reserved.
- Integer literals: decimal, `0x` hex, `0b` binary, `0o` octal. `_` may separate
  digits anywhere after the first digit. There are **no suffixes** (`42u8` is
  `E0004`); write `42 as u8` instead. A literal that does not fit `u64` is
  `E0004`.
- String literals: double-quoted byte strings. Escapes: `\n \t \r \0 \\ \" \'`
  and `\xHH`. Unknown escapes are `E0005`. A string literal has type
  `[u8; N]` where `N` is its byte length; there is **no NUL terminator** and no
  `str` type.
- Boolean literals: `true`, `false`.

---

## 3. Types

### 3.1 Primitive and composite types

| Type | Size (bytes) | Alignment | Range / meaning |
|---|---|---|---|
| `i8` | 1 | 1 | −128 … 127 |
| `i16` | 2 | 2 | −32768 … 32767 |
| `i32` | 4 | 4 | −2^31 … 2^31−1 |
| `i64` | 8 | 8 | −2^63 … 2^63−1 |
| `u8` | 1 | 1 | 0 … 255 |
| `u16` | 2 | 2 | 0 … 65535 |
| `u32` | 4 | 4 | 0 … 2^32−1 |
| `u64` | 8 | 8 | 0 … 2^64−1 |
| `usize` | 8 | 8 | unsigned pointer-sized (64-bit) |
| `isize` | 8 | 8 | signed pointer-sized (64-bit) |
| `bool` | 1 | 1 | `false` = 0, `true` = 1 |
| `*T` | 8 | 8 | read-only raw pointer to `T` |
| `*mut T` | 8 | 8 | writable raw pointer to `T` |
| `[T; N]` | `N * size(T)` | `align(T)` | fixed-size inline array, `N > 0` |

- Signed integers are two's complement.
- Arrays are inline values: `[u8; 16]` is 16 bytes of storage, not a pointer.
- `void` exists only as the absence of a return type; it cannot be written.
- A stack value larger than 1 MiB, or a type whose size overflows 64-bit
  arithmetic, is rejected with `E0223`.
- There are no floats, characters, structs, enums, unions, tuples, slices,
  references, function pointers, or trait objects in v0.1.

### 3.2 Mutability and pointer types

- `*T` is a read-only view; `*mut T` permits writes through it.
- Both are **raw** pointers: no lifetime, no borrow checking, no null guarantee,
  no aliasing restriction.
- `*mut T` is accepted where `*T` is expected (in `let` initializers, assignment,
  arguments, and `return`). The reverse is an `E0203` error.
- Pointer types are not implicitly convertible to integers or to `bool`.

### 3.3 Type compatibility

Two values are compatible when their types are identical, or when the expected
type is `*T` and the actual type is `*mut T`. **There is no other implicit
conversion.** Mixing `i32` and `u32`, or `usize` and `u64`, requires `as`.

---

## 4. Declarations and scoping

### 4.1 Bindings

```
let name: Type = expression;      // immutable
let mut name: Type = expression;  // mutable
```

- The type annotation is **required**; there is no inference for bindings.
- The initializer is **required**; there are no uninitialized stack bindings.
- `let` bindings cannot be assigned (`E0205`); `let mut` can.
- A binding is visible from its declaration to the end of the enclosing block.
- Redeclaring a name in the same block is `E0202`. Shadowing a name from an
  outer block is allowed.

### 4.2 Functions

```
fn name(param: Type, ...) -> ReturnType { ... }
fn name(param: Type, ...) { ... }          // returns nothing
```

- Functions must be declared before use at runtime but may be referenced in any
  order in the file: all signatures are collected before bodies are checked.
- Recursion and mutual recursion are allowed.
- No overloading; duplicate names are `E0202`.
- Arrays cannot be parameters or return types (`E0216`); pass `*T` instead.
- Parameters are immutable unless the function copies them into a `let mut`
  binding.
- Functions are the only items. There are no globals, constants, statics,
  modules, imports, `extern` declarations, or FFI.

### 4.3 Entry point

At least one of these must exist. If both are defined, `_start` is used:

- `fn _start()` — used verbatim as the ELF entry symbol. It may terminate with
  the `exit` syscall; if control reaches the end of the body, the compiler emits
  `exit(0)`.
- `fn main() -> i32` — the compiler synthesizes `_start`, which calls `main` and
  then issues `exit` with the returned value (the kernel keeps the low 8 bits).

`_start` takes precedence if both are defined. Any other signature is `E0217`;
no entry point is `E0212`.

---

## 5. Statements and control flow

- `if condition { } else if condition { } else { }` — `condition` must be `bool`
  (`E0214`). Blocks are not expressions; `if` produces no value.
- `while condition { }` — condition re-evaluated before each iteration.
- `loop { }` — infinite loop; without a reachable `break` it diverges, which
  satisfies the return analysis of a non-void function.
- `break;` / `continue;` — target the innermost enclosing loop only
  (`E0207` / `E0208` outside a loop). No labels.
- `return expression;` — expression required when the function returns a value
  (`E0206`), forbidden when it returns nothing (`E0219`).
- `unsafe { ... }` — a lexical region enabling unsafe operations (§8).
- `{ ... }` — a bare block statement introduces a new scope.
- `expression;` — evaluates and discards the value (used for calls).
- Parser recursion is capped at 256 levels; deeper input is rejected with
  `E0111`, so no input can overflow the compiler's stack. A parenthesized
  expression consumes two levels, so about 126 nested parentheses are accepted;
  unary chains and nested blocks allow about 253 levels, and pointer type chains
  255. The limit is an implementation bound, not a language rule.
- Assignment is a statement, not an expression: `a = b = c` does not parse.
  The left side must be a place (identifier, dereference, or index) and mutable
  (`E0205`).

### Return analysis

A function with a non-void return type must not fall off the end of its body.
A body "diverges" when every path ends in one of these:

- `return`;
- `loop { ... }` containing no `break` that belongs to it;
- `while true { ... }` containing no `break` that belongs to it;
- `if`/`else` where both branches diverge;
- an `unsafe` block or bare block whose body diverges;
- a call to `syscall` with the constant syscall number 60 (`exit`) or 231
  (`exit_group`) as the last statement — those never return.

Otherwise the compiler reports `E0206`. The analysis is conservative: an
`if true { ... }` without `else` is not treated as diverging, and an `exit`
syscall whose number is computed at runtime is not recognized.

---

## 6. Expressions

### 6.1 Places and values

Place expressions: identifier, `*p`, `a[i]`, and string literals (for indexing
and address-of). Anything else is an rvalue; taking its address or assigning to
it is `E0211`.

`&place` yields `*T`; `&mut place` yields `*mut T` and requires a mutable place
(`E0220`). Arrays are values; `&array` and `&array[0]` have the same address but
different types (`*[T; N]` versus `*T`).

### 6.2 Evaluation order

- Binary operands, call arguments, and array elements are evaluated strictly
  left to right.
- `&&` and `||` short-circuit: the right operand is evaluated only when needed.
- There are no hidden temporaries beyond stack slots for array values.

### 6.3 Operator precedence

From lowest to highest binding:

| Level | Operators | Associativity |
|---|---|---|
| 1 | `\|\|` | left |
| 2 | `&&` | left |
| 3 | `\|` | left |
| 4 | `^` | left |
| 5 | `&` | left |
| 6 | `==` `!=` | left |
| 7 | `<` `<=` `>` `>=` | left |
| 8 | `<<` `>>` | left |
| 9 | `+` `-` | left |
| 10 | `*` `/` `%` | left |
| 11 | `as Type` | left (postfix) |
| 12 | `-` `!` `&` `&mut` `*` (prefix) | right |
| 13 | `f(...)` `a[i]` | postfix |

### 6.4 Operators and semantics

| Operators | Operand types | Result | Notes |
|---|---|---|---|
| `+ - *` | same integer type | that type | wraps modulo 2^N |
| `+ -` | `*T`/`*mut T` on the left, `usize` on the right | same pointer type | pointer arithmetic, `unsafe`; `T` is the pointee |
| `/ %` | same integer type | that type | signed: truncates toward zero, remainder sign follows dividend; aborts on zero or `MIN / -1` |
| `<< >>` | same integer type | that type | shift count masked with `bits-1`; signed `>>` is arithmetic |
| `& \| ^` | same integer type | that type | bitwise |
| `!` | `bool` | `bool` | logical not |
| `!` | integer | same type | bitwise complement |
| unary `-` | integer | same type | wraps |
| `== !=` | identical integer or pointer types | `bool` | pointers compare addresses |
| `< <= > >=` | identical integer types | `bool` | signed or unsigned by type |
| `&& \|\|` | `bool` | `bool` | short-circuit |
| `as` | see §6.6 | target type | explicit conversion |
| `[]` | `[T; N]` base, `usize` index | `T` | bounds checked (§7.3) |

- Arithmetic and comparisons require both operands to have the **same** type;
  the only adaptation is an untyped integer literal adopting the other operand's
  type.
- Ordering comparisons on pointers are rejected (`E0210`); convert with
  `as usize`.

### 6.5 Literals

- An integer literal takes its type from context: the declared type of a `let`,
  the other operand of a binary expression, the parameter type in a call, or the
  element type of an array literal.
- A cast does not supply an expected type for its operand: in `5 as u64`, the
  literal is first resolved as `i32` and then converted. Use a typed binding when
  a literal must exceed `i32`, e.g. `let big: u64 = 18446744073709551615;`.
- With no context, an integer literal is `i32`.
- If the literal does not fit the resolved type, `E0215`. Negative literals are
  checked against the minimum value, so `let x: i8 = -128;` is valid.

### 6.6 Casts (`as`)

| From → To | Behavior |
|---|---|
| integer → integer, same width | bit-preserving reinterpretation |
| integer → narrower integer | truncation (low bits) |
| integer → wider integer | source signedness decides sign- or zero-extension |
| pointer → pointer | bit-preserving |
| integer → pointer | `unsafe`, no validity guarantee (`inttoptr`) |
| pointer → integer | `unsafe` (`ptrtoint`, 64-bit) |

- `bool` may not be cast (`E0210`); use a comparison.
- Casts between an array and anything else are rejected.
- Array literals: `[e1, e2, ...]` (elements must all be the same type; trailing
  comma allowed) and `[value; N]` with constant `N` in `1..=1024` (`E0221`
  above that). Both produce `[T; N]` values.

---

## 7. Integer semantics

All integer behavior is **defined**; nothing in this section is undefined.

### 7.1 Overflow

`+`, `-`, `*`, and unary `-` wrap modulo 2^N using two's complement. There are
no overflow traps, no checked arithmetic, and no saturation. With
`let x: i8 = 127;`, the expression `x + 1` yields `-128`.

### 7.2 Shifts

The shift count is masked with `N-1` (N = bit width): with `let one: u8 = 1;`,
`one << 8 == 1` and `one << 3 == 8`. Both operands must have the same type. Signed `>>` is
arithmetic; unsigned `>>` is logical.

### 7.3 Division and remainder

- `a / 0` and `a % 0` abort at runtime: message `corex: division by zero or
  signed overflow` on fd 2, exit status 101.
- Signed `MIN / -1` and `MIN % -1` abort identically.
- Signed division truncates toward zero; the remainder has the dividend's sign.

### 7.4 Array indexing

- The index expression must be `usize` (`E0213`).
- A literal index outside `0..N` is a compile error (`E0222`). Computed indices,
  even from constant expressions such as `1 + 1`, are checked at runtime.
- A dynamic index is bounds-checked at runtime; a violation prints
  `corex: index out of bounds` on fd 2 and exits 101.

---

## 8. Unsafe operations and builtins

`unsafe { ... }` enables exactly these operations; each is `E0209` elsewhere:

1. Dereferencing a pointer (read or write).
2. Pointer arithmetic (`p + n`, `p - n`).
3. Integer ↔ pointer casts.
4. Calling `alloc`, `free`, or `syscall`.

`unsafe` does not disable type checking. Nested `unsafe` blocks are allowed.
Address-of (`&`, `&mut`), pointer comparison, pointer-to-pointer casts, and
passing pointers by value are safe operations.

### 8.1 Builtins

Builtin names are reserved: declaring a function or binding with one of these
names is `E0202`.

```
syscall(number: usize, a1: usize, ..., a6: usize) -> isize
alloc(bytes: usize) -> *mut u8
free(pointer: *mut u8, bytes: usize) -> ()
print(value: integer | bool) -> ()
```

- `print` writes the decimal representation of its argument to stdout (fd 1)
  followed by a newline: signed integers with a leading `-` when negative,
  unsigned integers as magnitude, `true`/`false` for `bool`. It returns no
  value, needs no `unsafe`, performs no heap allocation (a 20-byte stack buffer
  holds the digits), and is type-checked: pointers, arrays, and `void` are
  rejected. It is intended for programs and for the REPL; there is no formatting
  language.

- `syscall` with number 60 (`exit`) or 231 (`exit_group`) never returns to the
  caller; a tail call to either counts as divergence for `E0206`.
- `syscall` takes 1 to 7 arguments (`E0204`); omitted trailing arguments are
  passed as 0. It executes the Linux `syscall` instruction with the number in
  `rax` and arguments in `rdi, rsi, rdx, r10, r8, r9`. It returns the raw kernel
  result in `rax` (negative values are `-errno`). The caller is responsible for
  syscall-specific semantics (for example, `write` returns the byte count).
- `alloc` calls `mmap(NULL, bytes, PROT_READ|PROT_WRITE,
  MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)`, returns a page-rounded, zero-filled
  mapping, and aborts with `corex: allocation failed` (exit 101) if the kernel
  returns a negative result. `alloc(0)` fails (mmap rejects zero length).
- `free` calls `munmap(pointer, bytes)`. The length must be the same value passed
  to `alloc`; a mismatch leaks or unmaps the wrong range. The return value is
  ignored.

There is no `malloc`, no `printf`, no standard library, and no panic mechanism.

---

## 9. Compilation model, ABI, and runtime

- Compilation unit: one `.cx` file. All functions are compiled together.
- Calling convention: System V AMD64. CoreX functions are ordinary C-ABI
  functions at the LLVM level; there are no variadic or register-only parameters.
- Locals live in the entry block's `alloca`s; arrays are copied by value.
  `alloca` never executes inside a loop.
- Code generation uses opaque pointers and emits no data layout, so `llc`
  applies the x86-64 default layout.
- Linking: `cc -nostdlib -static -no-pie` on an object produced by
  `llc -filetype=obj -relocation-model=static`. Result: statically linked ELF,
  executable, entry `_start`.
- No runtime initialization, no constructors, no TLS, no unwinding tables, no
  libc, no dynamic loader.
- `--opt 0..3`: `0` goes straight to `llc`; `1..3` runs
  `opt -passes=default<O{n}>` first, then `llc -O{n}`. There is no CoreX-level
  optimizer of its own.
- `_start` is compiled with 16-byte stack realignment (`alignstack=16`) because
  the kernel enters a process with `%rsp` 16-byte aligned, unlike a normal SysV
  callee.
- Runtime abort path: writes a message to fd 2 and exits 101.

---

## 10. Defined vs undefined behavior

### 10.1 Defined by the language

| Situation | Result |
|---|---|
| integer overflow | wraps modulo 2^N |
| shift count ≥ width | masked with `bits-1` |
| division/remainder by zero | abort, exit 101 |
| `MIN / -1`, `MIN % -1` | abort, exit 101 |
| dynamic index out of bounds | abort, exit 101 |
| constant index out of bounds | compile error `E0222` |
| `alloc` failure | abort, exit 101 |
| `print` output | decimal digits (`-` for negatives), `true`/`false`, then `\n`, on fd 1 |
| `main` return value | truncated to the low 8 bits by the kernel |
| `_start` fallthrough | exit status 0 |
| aliasing two pointers | allowed; no TBAA or `noalias` is emitted |

### 10.2 Programmer responsibility (not checked, not guaranteed)

- Dereferencing null, dangling, freed, or integer-fabricated pointers.
- Reading or writing through misaligned pointers (loads/stores declare natural
  alignment; a misaligned access is undefined).
- Pointer arithmetic outside the bounds of the pointed-to allocation (including
  more than one past the end).
- Double `free`, `free` with a wrong length, or use after `free`.
- `mmap` address reuse after `free`.
- Confusing `*T` and `*mut T` by casting; the language does not prevent writes
  through a pointer that was originally read-only.
- Data races (there is no threading support, but flags can be passed to `clone`).

---

## 11. Diagnostics

Format:

```
error[E0203]: type mismatch in assignment: expected `i32`, found `u8`
  --> file.cx:9:18
   |
 9 |     let x: i32 = y;
   |                  ^
   |
   = note: CoreX never converts integers implicitly; use `as` to convert
```

| Code | Meaning |
|---|---|
| `E0001` | unexpected character |
| `E0002` | unterminated string literal |
| `E0003` | unterminated block comment |
| `E0004` | invalid integer literal (missing digits, suffix, overflow) |
| `E0005` | invalid string escape |
| `E0100` | unexpected token / expected token |
| `E0101` | expected expression |
| `E0110` | empty array literal or zero repeat count |
| `E0111` | parser recursion depth exceeds 256 levels (≈126 nested parentheses) |
| `E0201` | undefined name or function |
| `E0202` | duplicate declaration or reserved name |
| `E0203` | type mismatch |
| `E0204` | wrong argument count (`print` with 0 or 2+ arguments) |
| `E0205` | assignment to an immutable place |
| `E0206` | missing return / missing return value |
| `E0207` | `break` outside a loop |
| `E0208` | `continue` outside a loop |
| `E0209` | unsafe operation outside `unsafe` |
| `E0210` | invalid operator, cast, or comparison for the type |
| `E0211` | expression is not a place |
| `E0212` | no entry point |
| `E0213` | index target not an array, or index not `usize` |
| `E0214` | condition is not `bool` |
| `E0215` | integer literal out of range for its type |
| `E0216` | array/void used where a scalar value is required |
| `E0217` | invalid entry point signature |
| `E0218` | function used as a value / call target not a function name |
| `E0219` | return value in a function returning nothing |
| `E0220` | `&mut` of an immutable place |
| `E0221` | array repeat count above 1024 |
| `E0222` | literal array index out of bounds |
| `E0223` | stack value larger than 1 MiB, or a type size that overflows 64 bits |
| `E0300` | IR verification or native toolchain failure |
| `E0303` | required LLVM/linker tool not found |

CLI exit codes: 0 success, 1 compile error, 2 usage/I/O error, 3 external tool
failure; `corex run` exits with the program's status (128+signal on Unix if the
program was killed).

---

## 12. Not in v0.1

Explicitly out of scope, so syntax is not reserved for them: structs, enums,
unions, tuples, floats, characters, slices, generic parameters, traits, closures,
function pointers, modules/imports, `extern`/FFI, globals and `static`,
compound assignment (`+=`), increment/decrement, labeled break, `match`, macros,
attributes, async, threads, `?`, debug info, and cross-compilation targets other
than Linux x86-64.

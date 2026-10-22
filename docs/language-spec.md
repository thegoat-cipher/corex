# CoreX Language Specification — v0.2

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
4. Explicit over implicit: bindings may infer their type from the initializer,
   but all other types are written, conversions are always spelled with `as`,
   and every unsafe operation is inside `unsafe`.
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
- Float literals: decimal digits with a fractional part and/or exponent
  (`1.5`, `2.0e-3`). A dot starts a fraction only when a digit follows it, so
  `1..3` is three tokens. A finite `f64` representation is required; overflow is
  `E0004`. There are no suffixes; write `1.5 as f32`. Integer syntax cannot be
  used as a float (`1f32` is `E0004`).
- Character literals: `'a'`, `'\n'`, `'\x41'`. Escapes match string escapes;
  the value is one byte. Type `char`.
- String literals: double-quoted byte strings. Escapes: `\n \t \r \0 \\ \" \'`
  and `\xHH`. Unknown escapes are `E0005`. A string literal has type `str`
  (a byte slice) by default. In a context that expects `[u8; N]`, a literal of
  exactly `N` bytes is accepted and produces the array type instead. There is no
  NUL terminator and no UTF-8 validation.
- Boolean literals: `true`, `false`.
- Compound assignment tokens: `+= -= *= /= %= &= |= ^= <<= >>=`.
- `..` is the slice-range delimiter; `.` is field access.

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
| `char` | 1 | 1 | byte-sized character, 0 … 255 |
| `f32` / `f64` | 4 / 8 | 4 / 8 | IEEE 754 binary32 / binary64 |
| `*T` | 8 | 8 | read-only raw pointer to `T` |
| `*mut T` | 8 | 8 | writable raw pointer to `T` |
| `[T; N]` | `N * size(T)` | `align(T)` | fixed-size inline array, `N > 0`; `N` is a constant integer expression |
| `[T]` | 16 | 8 | slice: `{ data: *T, len: usize }` by value |
| `str` | 16 | 8 | byte slice, layout identical to `[u8]` |
| `vec<T>` | 24 | 8 | dynamic array: `{ data: *mut T, len: usize, cap: usize }` |
| `struct Name { ... }` | natural | natural | named product type, by value |
| `enum Name { ... }` | 4 | 4 | named `i32` discriminant |

- Signed integers are two's complement.
- Arrays are inline values: `[u8; 16]` is 16 bytes of storage, not a pointer.
- An array length is a constant integer expression: an integer literal, a
  previously declared integer `const` name, or arithmetic over both
  (`[u8; N * 2]`). Only previously declared constants are visible, matching the
  forward-reference rule for constant expressions; anything else is `E0100`.
- `void` exists only as the absence of a return type; it cannot be written.
- A stack value larger than 1 MiB, or a type whose size overflows 64-bit
  arithmetic, is rejected with `E0223`.
- Structs use natural alignment: each field is placed at the next multiple of
  its alignment, and the total size is rounded up to the struct's alignment.
  A struct may contain itself only through a pointer; direct recursion is
  `E0224`.
- Slices and `str` are two-word values copied on binding, assignment, argument
  passing, and return. Copying a slice copies the pointer and length, not the
  referenced elements.
- Tuples, references, trait objects, and user-defined generic parameters do not
  exist; `vec<T>` is the only built-in generic type. Unions, bitfields, and
  function pointers are C-class extensions (§12).

### 3.2 Mutability and pointer types

- `*T` is a read-only view; `*mut T` permits writes through it.
- Both are **raw** pointers: no lifetime, no borrow checking, no null guarantee,
  no aliasing restriction.
- `*mut T` is accepted where `*T` is expected (in `let` initializers, assignment,
  arguments, and `return`). The reverse is an `E0203` error.
- Pointer types are not implicitly convertible to integers or to `bool`.

### 3.3 Type compatibility

Two values are compatible when their types are identical, when the expected type
is `*T` and the actual type is `*mut T`, or when one side is `str` and the other
is `[u8]`. A string literal is additionally accepted where `[u8; N]` of the same
byte length is expected. **There is no other implicit conversion.** Mixing
`i32` and `u32`, `usize` and `u64`, `f32` and `f64`, or an integer and a float
requires `as`.

---

## 4. Declarations and scoping

### 4.1 Bindings

```
let name: Type = expression;      // immutable
let mut name: Type = expression;  // mutable
let name = expression;            // type inferred from the initializer
let mut name = expression;        // inferred, mutable
```

- The type annotation is optional. Without it, the binding takes the type of the
  initializer; a `void` initializer is `E0225`.
- The initializer is **required**; there are no uninitialized stack bindings.
- `let` bindings cannot be assigned (`E0205`); `let mut` can.
- A binding is visible from its declaration to the end of the enclosing block.
- Redeclaring a name in the same block is `E0202`. Shadowing a name from an
  outer block, a parameter, or a global is allowed; locals win.
- Arrays, structs, slices, `str`, and `vec<T>` are copied on binding as described
  in §3.1; vector copies are shallow.

### 4.2 Compile-time constants

```
const NAME: Type = constant_expression;
```

- A `const` is substituted at compile time and has no storage, address, or
  lifetime; `&NAME` is `E0211`.
- Supported types are integers, `bool`, `char`, floats, enums, `str`, and
  pointers to globals. Aggregate constants (arrays, structs, slices, `vec`) are
  `E0231`.
- The initializer follows the global constant-expression rules and may read
  previously declared `const` values. Forward references are `E0226`.
- Integer `const` names, and constant expressions built from them, are accepted
  as array lengths in types (`[u8; N]`, `[u8; N * 2]`).
- Constants share the value namespace with functions and globals; duplicates are
  `E0202`. Builtin names are reserved.

### 4.3 Globals

```
static NAME: Type = constant_expression;
static mut NAME: Type = constant_expression;
```

- Globals live for the whole process and are addressable with `&NAME` /
  `&mut NAME`; the latter requires `static mut` (`E0220`).
- A `static` is immutable (`E0205` on assignment); `static mut` is mutable.
- The initializer is required and must be a constant expression: literals,
  unary `-`/`!`, arithmetic/comparison on constants, casts, `&GLOBAL`, and
  `&FUNCTION`. It may also read a previously declared immutable global.
  Address-of must name a global or a function. Calls, mutable-global reads, and
  forward references are `E0226`.
- Supported constant types are integers, `bool`, `char`, `f32`, `f64`, `str`,
  `[u8; N]` initialized from a string literal, pointers initialized from
  `&GLOBAL`, function pointers initialized from `&FUNCTION`, and enum variants.
- Aggregate constants are emitted as real LLVM globals with no runtime
  initializer. An array `static` accepts an array literal whose elements are
  constants, including nested arrays and struct literals; a struct `static`
  accepts a struct literal whose fields are constants. String elements use the
  ordinary `str` layout `{ ptr, i64 }`; function-pointer elements are emitted as
  `ptr @fn`. Struct literals require a plain C layout: unions, bitfields,
  packed/`align(N)` structs, and flexible array members are `E0226`. Element
  types that are themselves aggregates (arrays of arrays, arrays of structs,
  structs containing arrays/structs) are supported.
- Globals share one value namespace with functions; duplicates are `E0202`.
  Builtin names are reserved.

### 4.4 Enums and structs

```
enum Color { Red, Green = 5, Blue }
struct Point { x: i32, y: i32 }
```

- Enum variants have type `Color` and use `Color::Variant`. Discriminants are
  `i32`; the first is 0, each later one defaults to previous + 1, and an explicit
  discriminant must be an integer constant expression in `i32` range (`E0221`,
  `E0215`). Variant and type names are unique (`E0228`).
- An enum value is distinct from `i32`: arithmetic and implicit assignment are
  rejected (`E0203`, `E0210`). The same enum supports `== != < <= > >=`;
  `value as i32` yields the discriminant. `print` prints it as a signed integer.
- Struct fields are declared comma-separated and must be unique (`E0228`).
  Construction is `Point { x: 1, y: 2 }`; every field must be initialized
  exactly once (`E0229`, `E0228`), and unknown fields are `E0227`.
- `value.field` reads a field; it is a place when `value` is a place, so
  `p.x = 1` works when `p` (or the pointee for `(*p).x`) is mutable.
- Structs, including nested structs, are values: binding, assignment, argument
  passing, and return copy the whole object. Structs may be array elements.
- Struct type names share the type namespace with enums (`E0228`).

### 4.5 Imports

```
import "relative/or/absolute/path.cx";
```

- Imports are resolved recursively relative to the importing file; repeated
  imports of the same canonical file are included once, and cycles terminate.
- `import` is textual inclusion: all top-level items are merged into one **flat
  global namespace** with no per-file scopes, so top-level names must be unique
  across the program. (`module`/`mod` file modules are a separate feature with
  their own namespaces and mangled symbols; see §12.6.)
- Imports are compile-time file inclusion, not runtime loading. Diagnostics map
  source spans back to the originating file and line.
- `main`/`_start` must be defined in the root unit. A `main` in a `mod`
  file-module is an ordinary item with a mangled symbol (§12.6).

### 4.6 Functions

```
fn name(param: Type, ...) -> ReturnType { ... }
fn name(param: Type, ...) { ... }          // returns nothing
```

- Functions must be declared before use at runtime but may be referenced in any
  order in the file: all signatures are collected before bodies are checked.
- Recursion and mutual recursion are allowed.
- No overloading; duplicate names are `E0202`.
- Arrays cannot be parameters or return types (`E0216`); pass `*T` or a slice.
  Structs, slices, and `str` are allowed by value and are copied at the call
  boundary; aggregates larger than the ABI's register model are passed and
  returned indirectly by the compiler.
- Parameters are immutable unless the function copies them into a `let mut`
  binding.
- Top-level items are functions, `const` declarations, `static` globals, enums,
  structs/unions, `typedef`s, `extern "C"` declarations and definitions (§12.5),
  and `import`/`mod` declarations. `import` shares one flat namespace (§4.5);
  `mod` gives each file module its own mangled symbols (§12.6).
- Function pointers (`fn(T) -> U`) are first-class values; see §12.1.

### 4.7 Entry point

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
- `break;` — exits the innermost enclosing loop or `switch` (`E0207` when
  neither encloses it).
- `continue;` — jumps to the next iteration of the innermost enclosing loop
  (`E0208` outside a loop; inside a `switch` it targets the surrounding loop).
- `return expression;` — expression required when the function returns a value
  (`E0206`), forbidden when it returns nothing (`E0219`).
- `unsafe { ... }` — a lexical region enabling unsafe operations (§8).
- `{ ... }` — a bare block statement introduces a new scope.
- `expression;` — evaluates and discards the value (used for calls).
- Parser recursion is capped at 128 levels and semantic-expression nesting at
  64 levels; deeper input is rejected with `E0111`, so no input can overflow the
  compiler's stack. A parenthesized expression consumes two parser levels, so
  about 62 nested parentheses are accepted; chained unary operators and nested
  blocks consume one level each (about 125). Semantic analysis walks the
  expression tree recursively, so flat operator chains, cast chains, and nested
  calls are limited to 64 levels. The limits are implementation bounds, not
  language rules.
- Assignment is a statement, not an expression: `a = b = c` does not parse.
  The left side must be a place (identifier, dereference, index, or field) and
  mutable (`E0205`). Compound assignment (`+= -= *= /= %= &= |= ^= <<= >>=`) is
  a statement with the same place rules; float places accept `+= -= *= /= %=`,
  integer places accept all of them.

### Return analysis

A function with a non-void return type must not fall off the end of its body.
A body "diverges" when every path ends in one of these:

- `return`;
- `loop { ... }` containing no `break` that belongs to it;
- `while true { ... }` containing no `break` that belongs to it;
- `if`/`else` where both branches diverge;
- `switch` with a `default` arm where every arm body diverges and no arm
  contains a `break` that exits the switch (so no arm can fall out);
- an `unsafe` block or bare block whose body diverges, and a labeled statement
  whose statement diverges;
- a call to `syscall` with the constant syscall number 60 (`exit`) or 231
  (`exit_group`) as the last statement — those never return.

Otherwise the compiler reports `E0206`. The analysis is conservative: an
`if true { ... }` without `else` is not treated as diverging, a `switch`
without `default` never diverges, and an `exit` syscall whose number is
computed at runtime is not recognized. C fallthrough is unchanged: an arm that
can fall into the next one means the `switch` does not diverge.

---

## 6. Expressions

### 6.1 Places and values

Place expressions: identifier, `*p`, `a[i]`, `s.field`, and string literals
(for indexing and address-of). Anything else is an rvalue; taking its address or assigning to
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
| `== !=` | identical integer, char, enum, float, or pointer types | `bool` | pointers compare addresses; floats use ordered comparisons (`!=` is true for NaN) |
| `< <= > >=` | identical integer, char, enum, or float types | `bool` | signed or unsigned by type; floats ordered |
| `+ - * / %` | identical float type | that type | IEEE 754; no abort on division by zero |
| unary `-` | float | same type | negates |
| `&& \|\|` | `bool` | `bool` | short-circuit |
| `as` | see §6.6 | target type | explicit conversion |
| `[]` | array/slice/`str` base, `usize` index | element | bounds checked (§7.4) |
| `[a..b]` | array/slice/`str` base, `usize` bounds | `[T]` / `str` | sub-slice; omitted bounds default to 0/len; checked (§7.4) |
| `.field` | struct value/place | field type | field access |
| `len(x)` | array, slice, or `str` | `usize` | builtin (§8.1) |

- Arithmetic and comparisons require both operands to have the **same** type;
  the only adaptation is an untyped integer literal adopting the other operand's
  type.
- Ordering comparisons on pointers are rejected (`E0210`); convert with
  `as usize`.

### 6.5 Literals

- An integer literal takes its type from context: the declared type of a `let`,
  the other operand of a binary expression, the parameter type in a call, or the
  element type of an array literal. Float literals default to `f64` unless
  context expects `f32`; they never mix with integers implicitly.
- Character literals are always `char`; cast with `as i32` to inspect the byte.
- A string literal is `str` unless context expects `[u8; N]` with the same
  length.
- Array literals: `[e1, e2, ...]` (elements must all be the same type; trailing
  comma allowed) and `[value; N]` with constant `N` in `1..=1024` (`E0221`
  above that). Both produce `[T; N]` values. Arrays are copied by value.
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
| integer → float | signed source: `sitofp`; unsigned: `uitofp` |
| float → integer | truncating `fptosi`/`fptoui`; out-of-range is undefined at the LLVM level |
| `f32` ↔ `f64` | `fpext` / `fptrunc` |
| `char` ↔ integer | zero-extend / truncate to 8 bits |
| enum → integer | discriminant value (i32) |
| integer → enum | rejected (`E0210`) |

- `bool` may not be cast (`E0210`); use a comparison.
- Casts between an aggregate (array, struct, slice, `str`) and a scalar are
  rejected; enum→integer is the exception above.

### 6.7 Struct values

`Point { x: 1, y: 2 }` constructs a value in written field order. The struct name
is resolved from the type namespace. The expression's type is the named struct;
it can be used anywhere that type is expected, including as a function argument,
return value, array element, or nested struct field. Field reads use `.`, and
`p.x` is a place when `p` is a place.

### 6.8 Slices and strings

- `[T]` is a two-word value `{data, len}`; `str` is the byte specialization and
  is laid out exactly like `[u8]`.
- A string literal produces a `str`. Assigning it to `[u8; N]` requires exactly
  `N` bytes.
- A slice is produced by: a string literal; `&array` or `&mut array` in a context
  that expects `[T]` (the length is the array's); or a range index
  `s[start..end]` on an array, slice, or `str`. Either bound may be omitted:
  `s[a..]`, `s[..b]`, `s[..]`.
- `len(s)` returns the length. Indexing and range bounds are checked; a
  violation aborts with `corex: index out of bounds` or
  `corex: slice range out of bounds` (exit 101). Constant indices and constant
  array/string-literal bounds are also rejected at compile time (`E0222`).
- Elements are accessed with `s[i]`; writing requires a mutable slice binding or
  a mutable place. Slices are shallow copies and do not own their data.
- `str` can be printed with `print`; it writes its bytes plus a newline. Slices
  of types other than `u8` are not printable. Slices have no concatenation,
  equality, or implicit conversion to arrays.

---

### 6.9 Dynamic arrays (`vec<T>`)

- `vec<T>` is a 24-byte header `{data, len, cap}`. `vec_new(capacity)` allocates
  `capacity * size(T)` bytes; the element type must come from context
  (`let v: vec<T> = ...`, a parameter, or a return), otherwise `E0230`.
  `vec_new(0)` stores a null data pointer and zero capacity.
- `vec_push(&mut v, value)` appends; when full it grows to `max(4, cap * 2)`,
  allocates a new block, copies the live elements, frees the old block, and
  stores the value. `len(v)` and `v[i]` work like slices; indexing is bounds
  checked. `vec_free(&mut v)` frees the backing block and resets the header.
- `vec_slice(&v)` returns a `[T]` view of the live elements.
- Copying a `vec<T>` copies the 24-byte header and shares the backing block.
  There is no reference counting or move checking: two owners can double-free.
  Treat a vector as owned by one binding and pass `&mut`/`&` for access.
- `vec<T>` is not printable and has no equality operator.

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

### 7.4 Indexing and slicing

- The index/bound expression must be `usize` (`E0213`).
- A literal index outside `0..N` for an array, or a literal range outside a
  statically known length, is a compile error (`E0222`). Computed indices, even
  from constant expressions such as `1 + 1`, are checked at runtime.
- A dynamic index is bounds-checked at runtime; a violation prints
  `corex: index out of bounds` on fd 2 and exits 101.
- A range `s[a..b]` requires `a <= b <= len(s)`; a violation prints
  `corex: slice range out of bounds` and exits 101. The result starts at
  `s[a]` with length `b - a`. Omitted start is 0; omitted end is `len(s)`.
- `len` of an array is its compile-time `N`; for slices and `str` it reads the
  length word.

### 7.5 Floating-point semantics

- `f32` and `f64` are IEEE 754 binary32/binary64. Arithmetic uses `fadd`, `fsub`,
  `fmul`, `fdiv`, and `frem`; no fast-math flags are emitted.
- Float arithmetic never aborts: division by zero produces infinity or NaN;
  overflow produces infinity.
- Comparisons are ordered: `==`, `<`, `<=`, `>`, `>=` are false for NaN; `!=`
  is true for NaN.
- Integer→float and float→integer casts follow §6.6. Out-of-range float→integer
  conversion is not checked.
- `print` truncates finite floats toward zero to six fractional digits; it does
  not round (`print(3.14159265)` prints `3.141592`, `print(0.0000015)` prints
  `0.000001`). Values with magnitude ≥ 1e18 abort with
  `corex: float too large to print` (exit 101), and `nan`/`inf`/`-inf` are
  printed for non-finite values.
- There is no implicit integer/float mixing, no `%` on aggregates, and no
  decimal type.

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
print(value: integer | bool | char | float | enum | str | [u8]) -> ()
len(value: array | slice | str | vec<T>) -> usize
vec_new(capacity: usize) -> vec<T>                 // T from context
vec_push(v: *mut vec<T>, value: T) -> ()
vec_free(v: *mut vec<T>) -> ()
vec_slice(v: *vec<T>) -> [T]
read_file(path: str | [u8]) -> vec<u8>
write_file(path: str | [u8], data: str | [u8]) -> i32
panic(message: str | [u8]) -> ()
assert(condition: bool, message: str | [u8]) -> ()
```

- `print` writes its argument to stdout (fd 1) followed by a newline:
  signed integers with a leading `-` when negative, unsigned integers as
  magnitude, one raw byte for `char`, `true`/`false` for `bool`, enums as their
  signed `i32` discriminant, floats in fixed point with 6 fractional digits
  (`nan`/`inf`/`-inf` for non-finite values; values ≥ 1e18 abort with
  `corex: float too large to print`), and strings/`[u8]` slices as their raw
  bytes. It returns no value, needs no `unsafe`, performs no heap allocation,
  and is type-checked: pointers, other arrays, other slice element types, and
  `void` are rejected.

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
- `len` is safe for arrays, slices, `str`, and `vec<T>`; arrays produce the
  constant `N`, slices/`str` load the length word, and vectors load their `len`
  field. It takes exactly one argument (`E0204`) and rejects other types
  (`E0210`).
- `vec_new`/`vec_push`/`vec_free`/`vec_slice` are the dynamic-array API from
  §6.9. `vec_free` requires `&mut v` (`E0203` otherwise).
- `read_file(path)` reads a whole file into a caller-owned `vec<u8>`; on failure
  it returns an empty vector. `write_file(path, data)` truncates/creates the file
  and returns `0` on success or `-1` on failure. Paths are byte strings and are
  NUL-terminated internally; they are not UTF-8 validated. File I/O is safe (no
  `unsafe` required) and uses `open`/`read`/`write`/`lseek`/`close` syscalls.
- `panic(message)` writes the bytes plus a newline to fd 2 and exits 101. It
  never returns, so a tail `panic(...)` satisfies return analysis.
- `assert(condition, message)` does nothing when true; when false it panics via
  the same path. It returns no value and does not affect return analysis.
- CoreX has no exceptions, `Result`, `Option`, or `?`. Recoverable errors are
  represented with ordinary values (error codes, enums, bools); `panic`/`assert`
  are for unrecoverable failures.

There is no `malloc`, no `printf`, no standard library, and no panic mechanism.

---

## 9. Compilation model, ABI, and runtime

- Compilation unit: one `.cx` file. All functions are compiled together.
- Calling convention: System V AMD64 for scalars. Struct, slice, and `str`
  parameters are passed as a pointer to caller storage and copied by the callee;
  aggregate results use LLVM's hidden `sret` pointer. This is an internal CoreX
  ABI, not a stable C ABI. There are no variadic or register-only parameters.
- Locals live in the entry block's `alloca`s; arrays, structs, slices, and
  `str` are copied by value. `alloca` never executes inside a loop.
- Aggregate parameters (arrays, structs, slices, `str`, `vec<T>`) are passed as
  a pointer to caller storage and copied into the callee's slot at entry;
  aggregate returns use a hidden `ptr` result parameter (`sret`) written by the
  callee. Scalars use the System V registers.
- Dynamic-array growth and file I/O use compiler-generated helpers
  (`__corex_alloc`/`__corex_free`/`__corex_memcpy`, `__corex_cstr`,
  `__corex_read_file`, `__corex_write_file`, `__corex_panic`).
- Globals are emitted as LLVM globals; immutable globals use the `constant`
  linkage class, mutable ones `global`.
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
| slice range out of bounds | abort, exit 101 (`corex: slice range out of bounds`) |
| vector index out of bounds | abort, exit 101 |
| `read_file` failure | empty `vec<u8>` |
| `write_file` failure | returns `-1` |
| `panic` / failed `assert` | message + newline on fd 2, exit 101 |
| constant index/range out of bounds | compile error `E0222` |
| float overflow to infinity | IEEE 754 infinity (printed `inf`) |
| float `0.0/0.0` | NaN (printed `nan`) |
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
| `E0111` | nesting exceeds the implementation limit (parser: 128 levels, ≈62 nested parentheses; expressions: 64 levels) |
| `E0201` | undefined name, function, type, enum, or struct |
| `E0202` | duplicate declaration or reserved name |
| `E0203` | type mismatch |
| `E0204` | wrong argument count (`print`, `len`, or a user function) |
| `E0205` | assignment to an immutable place |
| `E0206` | missing return / missing return value |
| `E0207` | `break` outside a loop or `switch` |
| `E0208` | `continue` outside a loop |
| `E0209` | unsafe operation outside `unsafe` |
| `E0210` | invalid operator, cast, or comparison for the type |
| `E0211` | expression is not a place |
| `E0212` | no entry point |
| `E0213` | index/slice target or bound has the wrong type |
| `E0214` | condition is not `bool` |
| `E0215` | integer/float literal or enum discriminant out of range |
| `E0216` | array used by value, or `void` used where a value is required |
| `E0217` | invalid entry point signature |
| `E0218` | function used as a value / call target not a function name |
| `E0219` | return value in a function returning nothing |
| `E0220` | `&mut` of an immutable place |
| `E0221` | array repeat count above 1024, or non-constant enum discriminant |
| `E0222` | literal array index out of bounds |
| `E0223` | stack value larger than 1 MiB, or a type size that overflows 64 bits |
| `E0224` | struct contains itself by value / incomplete layout |
| `E0225` | cannot infer a binding type from `void` |
| `E0226` | global initializer is not a constant expression |
| `E0227` | unknown struct field / field access on a non-struct |
| `E0228` | duplicate type, enum variant, struct field, or field initializer |
| `E0229` | missing struct field in a literal |
| `E0230` | `vec_new` element type cannot be inferred |
| `E0231` | unsupported `const` value type |
| `E0232` | invalid import path or unreadable import |
| `E0300` | IR verification or native toolchain failure |
| `E0303` | required LLVM/linker tool not found |

CLI exit codes: 0 success, 1 compile error, 2 usage/I/O error, 3 external tool
failure; `corex run` exits with the program's status (128+signal on Unix if the
program was killed).

---

## 12. C-class extensions (v0.3)

This section is normative for the C-class features added in v0.3. Everything in
Parts 1–11 remains in force unless stated otherwise.

### 12.1 Function pointers

`fn(T1, T2) -> R` is a function type; it has pointer size and alignment. A
function name used as a value, or `&name`, produces a function pointer. Calling
an expression of function type performs an indirect call:

```corex
fn add(a: i32, b: i32) -> i32 { return a + b; }

fn apply(f: fn(i32, i32) -> i32, a: i32, b: i32) -> i32 { return f(a, b); }

fn main() -> i32 {
    let f: fn(i32, i32) -> i32 = &add;   // `add` works too
    let table: [fn(i32, i32) -> i32; 1] = [f];
    print(apply(&add, 20, 22));
    print(table[0](1, 2));
    let none: fn(i32, i32) -> i32 = null;
    if none == null { return 0; }
    return 1;
}
```

Function pointers compare with `==`/`!=`, convert to/from integers and other
pointers with `as` inside `unsafe`, and may be `null`. Function-pointer calls
use the CoreX internal convention for aggregates (see 12.5); scalar parameters
and results match System V AMD64.

### 12.2 Control flow

- `switch value { case CONST { ... } ... default { ... } }` evaluates `value`
  once, branches to the matching arm, and **falls through** into later arms
  unless `break` is executed. `case` values must be integer constant
  expressions (including enum variants) and are unique; one `default` is
  allowed. `break` inside a switch exits the switch; `continue` targets the
  enclosing loop. A `switch` counts as diverging for return analysis only when
  it has a `default` and every arm diverges without a `break`; fallthrough
  semantics are unchanged.
- `for (init; cond; step) { ... }` — any clause may be empty. `init` and `step`
  are statements (including `let` and assignments), `cond` is a `bool`
  expression.
- `do { ... } while cond;` runs the body at least once.
- `name: statement` defines a function-local label; `goto name;` branches to it.
  Labels are function-local and may be forward-referenced. A block interrupted
  by `goto`, `return`, `break`, or `continue` does not fall through; subsequent
  statements in that block are unreachable until the next label.

### 12.3 Aggregates: unions, bitfields, flexible arrays

```corex
union Value { i: i32, f: f32, b: u8 }          // field offsets are all 0
struct Flags { a: u8: 3, b: u8: 5, valid: u8: 1 } // C bitfields
struct Blob { len: u64, data: [u8; 0] }        // flexible array member
```

- A `union` has every field at offset 0; size is the largest member, rounded to
  the union's alignment. A union literal initializes exactly one field.
- A bitfield is written `name: IntegerType: WIDTH`, with `1 ≤ WIDTH ≤ bits`.
  Fields are packed least-significant-bit first into storage units of the
  declared integer type; a field that does not fit starts a new unit. Reads
  shift and mask; writes are read-modify-write. `offsetof` rejects bitfields.
- A zero-length array is allowed only as the last field of a struct; it occupies
  no bytes, contributes its element alignment, and models a trailing array.
  `sizeof` excludes it.

### 12.4 Layout and introspection

- `typedef Name = Type;` introduces an alias. Aliases participate in name
  resolution exactly like the aliased type; recursive aliases are rejected.
- `packed struct` / `packed union` removes inter-field padding; alignment is 1
  unless raised with `align(N)`. `align(N)` before `struct`/`union` raises the
  type's alignment and rounds its size up to `N`; `N` must be a power of two.
- `repr(C)` is accepted and is the default layout policy; it exists so the
  intent is explicit in headers and interfaces.
- `sizeof(T)`, `alignof(T)`, and `offsetof(T, field)` are compile-time
  `usize` values. `sizeof`/`alignof` take a type, not a value.
- Natural (non-packed) struct layout is the System V AMD64 C layout: each field
  at the next offset aligned to its alignment, struct alignment = maximum field
  alignment, size rounded up to that alignment. Unions follow C.

### 12.5 C ABI and FFI

- `extern "C" fn name(params) -> T;` declares an external C function.
  `extern "C" fn name(params) -> T { ... }` defines one with its exact C symbol
  (no mangling), so C can call it.
- `extern "C" fn name(fixed: T, ...)` declares a variadic C function. Calls may
  pass extra integer/pointer arguments. C default argument promotions apply:
  integers narrower than 32 bits promote to `i32` (sign/zero extended by
  signedness), `bool`/`char` to `i32`, and `f32` to `f64`.
- C scalar and aggregate values use System V AMD64: natural structs are built
  as LLVM literal structs and are passed/returned by value; LLVM applies the
  classification rules. CoreX-internal functions use a pointer/sret convention
  for aggregates. `extern "corex" fn` declares the internal convention with an
  exact symbol (used by generated module interfaces).
- `extern static name: T;` declares a global defined elsewhere.
- `cstr` is a NUL-terminated C string pointer, distinct from `str` (a
  pointer/length value). `c"text"` creates a NUL-terminated constant. Use:
  - `cstr_len(c) -> usize`
  - `cstr_to_str(c) -> str` (runs `strlen`)
  - `str_to_cstr(buf, cap, s) -> cstr` (writes NUL; aborts if `cap <= len(s)`;
    requires `unsafe`)
  - `c as str`, `c as *u8`; `str` cannot implicitly become `cstr` because that
    would require hidden allocation.
- `main` may be `fn main() -> i32` or
  `fn main(argc: i32, argv: **u8, envp: **u8) -> i32`. The second form is
  entered through a naked assembler `_start` that forwards the kernel stack;
  `argv[argc]` is null and `envp` follows the argv null terminator.
- Extra objects and libraries: `--link obj.o|lib.a`, `-L dir`, `-l name`,
  `--hosted`, `--emit shared|staticlib|obj`.

### 12.6 Modules and separate compilation

- `module name;` at the top of a file names its module. Items default to private
  to the module; `pub` exports them.
- `mod name;` declares a file-module dependency. The driver searches the
  importing file's directory, then each `-I` directory, for `name.cx` or
  `name/mod.cx`. The file must declare `module name;`.
- Cross-module references use `::`: `math::add`, `math::Point`,
  `math::Kind::Variant`, `math::CONST`.
- Symbols are mangled deterministically as `module$item`; `extern "C"` items
  keep their exact symbol. Entry points `main` and `_start` are never mangled in
  the root unit; in a module they are ordinary items (`a$main`), so a module
  cannot collide with or replace the program entry point.
- Each unit is compiled independently to an object. For every unit the compiler
  emits an interface (`module.cxi`, printable with `COREX_DUMP_INTERFACES=1`)
  containing public structs/unions/enums/typedefs/consts, `extern static`
  declarations, and `extern "corex" fn` declarations. Dependents are compiled
  against the concatenation of their transitive interfaces.
- A public function may not expose a private local type (`E0233`).
- `corex.toml` manifest (subset of TOML):

```toml
[package]
name = "app"

[build]
kind = "exe"          # exe | shared | staticlib
sources = ["src/main.cx"]
include_paths = ["src"]
libs = ["m"]
lib_paths = ["/usr/lib"]
opt = 2
features = ["fast"]
debug = true
sanitize = false
hosted = false
output = "build/app"
```

`corex build <directory>` builds the manifest. `kind = "staticlib"` produces
`lib<name>.a` with `llvm-ar rcs`; `kind = "shared"` links with `cc -shared`.
Object files for both are compiled position-independent (`llc
-relocation-model=pic`) so globals and string constants can be linked into a
shared object.
- Conditional compilation: `#[cfg(...)]` before an item. Conditions are
  `key = "value"`, `not(...)`, `all(...)`, `any(...)`. Keys are supplied with
  `--cfg name=value` and manifest `features` (as `feature=value`); the driver
  always defines `target_os=linux`, `target_arch=x86_64`, and
  `sanitize=true` under `--sanitize`.

### 12.7 Allocators and the standard library

The language never allocates implicitly. `alloc(bytes)` and
`free(ptr, bytes)` remain the raw mmap/munmap primitives (both require
`unsafe`). The standard library (`lib/corex`) adds:

| Module | Contents |
|---|---|
| `std_alloc` | `malloc`, `calloc`, `realloc`, `free` (C semantics, 16-byte alignment, size headers; size arithmetic that would overflow returns `null`), `aligned_alloc`, `Allocator` (context + function pointers), `c_allocator`, `Arena` (`arena_new`, `arena_alloc`, `arena_reset`, `arena_free`) |
| `std_mem` | `memcpy`, `memmove`, `memset`, `memzero`, `memcmp`, `memchr` |
| `std_str` | `strlen`, `strcmp`, `strncmp`, `strcpy`, `strncpy`, `strcat`, `strchr`, `str_eq`, `str_find`, `str_copy_cstr` |
| `std_io` | fd constants and flags, `open`, `read`, `write`, `seek`, `close`, `pipe`, `dup2`, `read_all`, `write_all`, `puts`, `eputs`, `getbyte`, `putbyte` |
| `std_fs` | `Stat`, `stat_path`, `lstat_path`, `file_size`, `exists`, `is_dir`, `is_file`, `mkdir`, `rmdir`, `unlink`, `rename`, `getcwd`, `chdir`, `read_file`, `write_file`, `Dir`/`dir_open`/`dir_next`/`dir_close` |
| `std_process` | `exit`, `getpid`, `fork`, `exec`, `wait`, `spawn`, `spawn3`, `spawn_argv`, `run`, `run_argv`, `env_count` |
| `std_env` | `env_get(envp, name, found)`, `env_has`, `env_get_or`: borrowed, allocation-free lookup over the `envp` vector from `main(argc, argv, envp)`; `found` distinguishes a missing variable from an empty value |
| `std_time` | `Timespec`, `clock_gettime`, `now_ns`, `monotonic_ns`, `sleep_ns`, `sleep_ms`, `unix_seconds` |
| `std_math` | integer abs/min/max, `clz64`, `ctz64`, `popcount64`, `is_pow2`, `next_pow2`, `fabs`, `sqrt`, `trunc`, `floor`, `ceil`, `round`, `powi` |
| `std_fmt` | typed `Fmt` builder: `push_str`, `push_cstr`, `push_i64`, `push_u64`, `push_hex`, `push_bin`, `push_bool`, `push_f64`, `finish`, `finish_cstr` |
| `std_map` | insertion-ordered string-keyed `Map` with `u64` values: `map_new`, `map_free`, `map_put`, `map_get` (with a found flag), `map_has`, `map_len`, `map_key_at`, `map_value_at`; `Set` with `set_new`, `set_free`, `set_insert`, `set_has`, `set_len`, `set_key_at`. Keys are borrowed `str` slices (never copied) |
| `std_builder` | growable byte `Builder`: `builder_new`, `builder_with_capacity`, `builder_free`, `builder_reserve`, `builder_push_byte`, `builder_push_str`, `builder_push_char`, `builder_push_i64`, `builder_push_u64`, `builder_push_hex`, `builder_push_f64`, `builder_len`, `builder_data`, `builder_failed`, `builder_finish`. Every push returns `bool`; allocation failure latches the error instead of aborting |
| `std_sync` | `Mutex`, `mutex_lock`/`try_lock`/`unlock`, `Condvar`, `condvar_wait`/`signal`/`broadcast`, `SpinLock` (futex syscalls 202) |
| `std_thread` | `Thread`, `spawn(entry, arg, stack_size)`, `gettid`, `yield_now`, `thread_exit` |
| `std_tls` | explicit thread-local blocks: `tls_alloc`, `tls_get`, `tls_release`, `tls_thread_id` |
| `std_dl` | hosted `dlopen`/`dlsym`/`dlclose`/`dlerror` wrappers: `open` (`RTLD_LAZY`), `open_now` (`RTLD_NOW`), `sym`, `close`, `error` |

The library is freestanding except `std_dl`, which requires `--hosted`.
`std_alloc` on `--sanitize` poisons freed payloads, keeps mappings so a second
`free` is diagnosed, and aborts (exit 101) via direct syscalls.

Two file-reading paths differ on failure: `std_fs::read_file(path, buf, cap)`
returns the byte count, `0` for an empty file, and `-1` on failure, so a driver
can tell an empty file from an error. The `read_file(path) -> vec<u8>` builtin
keeps its contract of returning an empty vector on failure and cannot
distinguish the two.

`std_map` keys and `std_builder::builder_finish` are borrowed slices, not
copies: the key storage must outlive the map entry, and builder bytes are valid
until the next push or `builder_free`.

### 12.8 Volatile, atomics, threads, TLS

- `volatile` qualifies a global declaration: `static mut volatile FLAG: u32 = 0;`.
  Every load and store of that global in generated IR is marked `volatile`.
  For arbitrary pointers use `volatile_load(ptr)` / `volatile_store(ptr, value)`
  (require `unsafe`).
- Atomic builtins (sequential consistency unless suffixed `_relaxed`):
  `atomic_load`, `atomic_load_relaxed`, `atomic_store`,
  `atomic_store_relaxed`, `atomic_fetch_add/sub/and/or/xor`, `atomic_xchg`,
  `atomic_cas(ptr, expected, new) -> bool`, `atomic_fence()`. The pointer's
  pointee must be an integer or pointer type; fetch operations return the
  previous value.
- `std_thread::spawn` uses `clone(2)` with a small inline-assembly trampoline.
  Threads share the address space and file table. Threads exit with `exit(2)`
  (syscall 60), which terminates only the calling thread. There is no `join`;
  synchronize with `std_sync` or sleep.
- TLS is explicit (`std_tls`): the runtime keeps a fixed 64-slot table keyed by
  `gettid`; the first allocation on a thread allocates a block. There is no
  language-level `thread_local` and no hidden destructor.

### 12.9 Inline assembly

`asm(template)` and `asm(template, constraints, operands...)` (require `unsafe`):

- `template` and `constraints` must be string literals. `constraints` is passed
  verbatim to LLVM (`"=r,r,~{memory}"`); if any output constraint starts with
  `=`, the expression has type `usize`, otherwise `void`.
- Operands are integers or pointers, numbered `$0`, `$1`, … in the template.
  A literal `$` in assembly must be written `$$` (LLVM syntax).
- `asm("", "~{memory}")` is a compiler barrier; `asm("pause", "")` is a CPU
  pause.

### 12.10 Declarative macros

`macro name(p1, p2) = expression;` defines a compile-time expression macro. A
call `name(args)` is replaced by the body with parameter identifiers substituted
by the argument expression trees; node ids are renumbered, so expansion cannot
alias semantic state. Expansion happens during parsing, before type checking;
macro arguments are type-checked after substitution. Macros are non-hygienic
(like C): a body identifier that is not a parameter is resolved at the call site,
parameters are duplicated if used twice, and recursion deeper than 64 is an
error. Macros are unit-local and are not exported by module interfaces.

### 12.11 Sanitizer/debug modes

- `-g` / `--debug` emits DWARF 5 line tables, `DW_TAG_subprogram` entries, and
  `DILocalVariable`/`llvm.dbg.declare` records for every named local.
  `readelf --debug-dump=decodedline` and `--debug-dump=info` show them.
- `--sanitize` enables checked signed `i8/i16/i32/i64` add/sub/mul
  (`llvm.*.with.overflow`, abort message `corex: integer overflow`), keeps
  existing bounds/division checks, and activates allocator double-free
  detection and payload poisoning via `#[cfg(sanitize = "true")]`.
  Unsigned arithmetic keeps its defined wrapping semantics.

### 12.12 Tooling

- `corex fmt <file> [-w|--check]` — indentation/spacing formatter that preserves
  line structure, comments, and literals; idempotent.
- `corex lint <file> [--deny-warnings]` — warnings `W0001` unused variable,
  `W0002` unreachable statement, `W0003` unused private item, `W0004` constant
  condition.
- `corex doc <file> [-o out.md]` — Markdown from `///` doc comments.
- `corex test <file>` — builds `fn test_*() -> bool` functions into a runner;
  prints `test name ... ok|FAILED`; exits 0 only if all pass.
- `corex-lsp` — LSP over stdio: diagnostics, completion, definition,
  references, rename, hover, document formatting.
- `corex emit-ir --verify` — prints IR and runs `llvm-as` as a verifier.
- `COREX_DUMP_INTERFACES=1` prints generated module interfaces during a build;
  `COREX_KEEP_TEMP=1` keeps the build directory; `COREX_DUMP_IR=1` includes the
  IR in a verifier failure note.

### 12.13 New diagnostics

| Code | Meaning |
|---|---|
| `E0233` | public interface exposes a private local type |
| `E0234` | manifest missing/unreadable/invalid |
| `E0235` | no tests found, or a `test_*` function does not return `bool` |
| `W0001` | unused variable |
| `W0002` | unreachable statement |
| `W0003` | unused private item |
| `W0004` | constant condition |

`E0224` additionally covers zero-length arrays outside a struct's last field,
flexible arrays in unions, and recursive typedefs. `E0209` covers all raw
pointer/atomic/asm operations outside `unsafe`.

---

## 13. Out of scope (current release)

Still out of scope: tuples, generic parameters beyond `vec<T>`, traits,
closures, exceptions, `async`, increment/decrement operators, labeled `break`,
`match`, operator overloading, destructors, a package registry, and targets
other than Linux x86-64. `str`/slices have no concatenation or comparison
operators; use `std_str::str_eq`. CoreX `volatile` applies to globals; use the
pointer builtins elsewhere. There is no language-level `thread_local`, no
thread join, and inline assembly uses LLVM's constraint syntax rather than a
CoreX-specific one.

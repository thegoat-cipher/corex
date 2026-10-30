# CoreX Syntax Tutorial

A compact, complete tour of the CoreX 0.4.0 syntax. Snippets are fragments
unless they define `main`. CoreX targets Linux
x86-64, compiles through LLVM 20, and produces freestanding ELF binaries with no
libc and no runtime.

This tutorial covers every syntax form the compiler accepts. The normative
details are in [language-spec.md](language-spec.md) and
[grammar.ebnf](grammar.ebnf).

## 1. Build and run

```corex
fn main() -> i32 {
    print("hello, corex");
    return 0;
}
```

```bash
cd compiler
cargo build --release
./target/release/corex run ../examples/hello.cx
./target/release/corex build ../examples/hello.cx -o hello --opt 2
```

Entry points:

```corex
fn main() -> i32 { return 0; }

// Or, for access to the process arguments:
// fn main(argc: i32, argv: **u8, envp: **u8) -> i32 { return argc; }

// Or, for full control of the ELF entry point:
// fn _start() { /* fallthrough exits 0 */ }
```

Use one `main` form, or `_start` for full control. If both exist, `_start`
wins. `main` returns an `i32`; the kernel keeps its low 8 bits.

## 2. Lexical rules

```corex
// line comment
/* block comment, not nested */
```

- Source is UTF-8; code outside comments and strings must be ASCII.
- Identifiers: `[A-Za-z_][A-Za-z0-9_]*`. Keywords and builtin names are reserved.
- Integer literals: decimal, `0x`, `0b`, `0o`; `_` may separate digits.
- There are **no literal suffixes**. Write `42 as u8`, not `42u8`.
- Floats: `1.5`, `2.0e-3`. No suffixes; cast `1.5 as f32`.
- Char: `'a'`, `'\n'`, `'\x41'`. Type `char`, one byte.
- String: `"bytes\n"`. Type `str`; escapes `\n \t \r \0 \\ \" \' \xHH`.
- C string: `c"text"`. Type `cstr`, NUL-terminated.
- `true`, `false`, `null`.
- Statements end with `;`; blocks use `{ ... }`.

## 3. Bindings and primitive types

```corex
let answer: i32 = 42;      // immutable
let mut total = 0;         // inferred i32, mutable
total += answer;

let n: i32 = 300 as i8 as i32;   // truncate then sign-extend
```

Every binding needs an initializer. `let` cannot be assigned; `let mut` can.
A name can shadow an outer binding, but not be redeclared in the same block.

| Type | Size | Meaning |
|---|---:|---|
| `i8 i16 i32 i64` | 1/2/4/8 | signed two's complement |
| `u8 u16 u32 u64` | 1/2/4/8 | unsigned |
| `usize isize` | 8 | pointer-sized |
| `bool` | 1 | `false`/`true` |
| `char` | 1 | byte |
| `f32 f64` | 4/8 | IEEE 754 |
| `*T`, `*mut T` | 8 | raw read-only / writable pointer |
| `[T; N]` | inline | fixed array |
| `[T]`, `str` | 16 | slice: pointer + length |
| `vec<T>` | 24 | dynamic array |
| `struct`, `union`, `enum` | varies | user types; enum is `i32` |
| `fn(T) -> R` | 8 | function pointer |
| `cstr` | 8 | NUL-terminated C string pointer |

There is **no implicit numeric conversion**. Mixing `i32`/`u32`,
`usize`/`u64`, `f32`/`f64`, or integer/float requires `as`. Untyped integer
literals adopt their context; otherwise they are `i32`.

## 4. Composite values

### Arrays and slices

```corex
let values: [i32; 4] = [1, 2, 3, 4];
let zeros: [u8; 16] = [0; 16];        // repeat count must be 1..=1024
let first: i32 = values[0];

let all: [i32] = &values;             // array -> slice
let middle: [i32] = all[1..3];        // sub-slice
print(len(all));                       // 4
```

Arrays are inline values and are copied on binding/assignment. Array length is
a literal or a previously declared integer `const` expression. Array literals
`[a, b]` and `[value; count]` produce `[T; N]`; the repeat count is an
integer literal in `1..=1024`. Arrays **cannot** be function
parameters or return types; pass `*T`, `[T]`, or `str`.

Index/bounds expressions must be `usize`. Literal out-of-bounds indices are
compile errors; dynamic violations abort with exit 101.

### Strings

```corex
let text: str = "CoreX";
let bytes: [u8; 5] = "CoreX";   // literal length must match exactly
print(text[1..4]);               // ore
print(len(text));                // 5
```

`str` is a byte slice, not UTF-8-validated and not NUL-terminated. Slices and
`str` copy the pointer and length, not the bytes. No concatenation or equality
operators; use `std_str`.

### Dynamic arrays

```corex
let mut v: vec<i32> = vec_new(4);
vec_push(&mut v, 10);
vec_push(&mut v, 20);
let view: [i32] = vec_slice(&v);
print(view[1]);                  // 20
vec_free(&mut v);
```

`vec_new(cap)` takes its element type from context. `vec_push`, `vec_free`, and
`vec_slice` are builtins. Indexing is bounds-checked. A vector copy shares its
backing block; there is no reference counting or move checking.

### Structs, enums, unions, and layout

```corex
struct Point { x: i32, y: i32 }
enum Color { Red, Green = 5, Blue }
union Value { i: u32, b: u8 }
struct Flags { a: u8: 3, b: u8: 5, valid: u8: 1 }
struct Blob { len: u64, data: [u8; 0] }

let mut p: Point = Point { x: 3, y: 4 };
p.x += 1;
let copy = p;                     // whole struct copied

let c: Color = Color::Blue;
print(c as i32);                  // 6

let u: Value = Value { i: 65 };
print(u.b);                       // 65: union fields share offset 0
```

- Struct fields are comma-separated and unique. Every field must be initialized
  exactly once. `Point { x: 9, ... }` is a partial literal; omitted fields are zero.
- `Color::Variant` is the enum value; enum arithmetic is rejected. The first
  discriminant is 0, later ones increment, and explicit values must fit `i32`.
- Union fields all start at offset 0; the size is the largest member.
- Bitfields use `name: IntegerType: WIDTH`; reads/writes mask the field. Signed
  bitfields sign-extend.
- A zero-length array is allowed only as the last struct field (flexible array);
  `sizeof` excludes it.

```corex
typedef Word = u64;
packed struct Header { tag: u8, value: u32 }
align(16) struct Aligned { x: u8 }
repr(C) struct CPoint { x: i32, y: i32 }

print(sizeof(CPoint));             // 8
print(alignof(CPoint));            // 4
print(offsetof(CPoint, y));        // 4
```

`packed` removes padding, `align(N)` raises alignment, and `repr(C)` is the
accepted default C layout. `sizeof`/`alignof` take a type; `offsetof` takes a
type and field.

```corex
let partial: Point = Point { x: 9, ... };
let sparse: [i32; 4] = [i32; 4] { 1: 10, 3: 30 };
```

The second form is an array compound literal; unspecified elements are zero.

### Pointers

```corex
let mut n: i32 = 7;
let r: *i32 = &n;
let m: *mut i32 = &mut n;

unsafe {
    *m = 8;              // dereference and write
    print(*r);           // 8
    let q: *u8 = m as *u8;
    let next: *u8 = q + 1;   // pointer arithmetic
}
```

`*T` is read-only, `*mut T` is writable, and `*mut T` is accepted where `*T` is
expected. Pointers are raw: no lifetime, null, aliasing, or bounds checking.
`&place`/`&mut place` are the safe address-of forms; `p->field` is sugar for
`(*p).field`. `null` is a valid pointer/function-pointer value.

Dereference, pointer arithmetic, integer/pointer casts, `alloc`/`free`,
`syscall`, atomics, volatile pointer operations, and inline asm require
`unsafe`. Address-of, pointer comparison, and passing pointers are safe.

## 5. Constants and globals

```corex
const N: usize = 4;
const SOURCE: str = "fn main() {}";
let table: [u8; N] = [1, 2, 3, 4];

static NAME: str = "corex";
static mut COUNTER: i32 = 0;
static TABLE: [i32; 3] = [1, 2, 3];
static mut volatile FLAG: u32 = 0;
```

- `const` is compile-time substituted; it has no address. Supported constant
  types are integers, `bool`, `char`, floats, enums, `str`, and pointers to
  globals. Arrays/structs are `E0231`; use `static`.
- `static` has real storage; `static mut` is writable. The initializer must be
  a constant expression and is required.
- `&NAME`/`&FUNCTION` are constant addresses. `&mut` requires `static mut`.
- Integer `const` names and constant arithmetic are valid array lengths.

## 6. Functions

```corex
fn add(a: i32, b: i32) -> i32 {
    return a + b;
}

fn log_value(v: i32) {          // no return type
    print(v);
}

fn sum(values: [i32]) -> i32 {  // slice, not array, by value
    let mut i: usize = 0;
    let mut total: i32 = 0;
    while i < len(values) {
        total += values[i];
        i += 1;
    }
    return total;
}
```

- Parameters are immutable; copy into `let mut` to modify.
- Functions may recurse and be mutually recursive.
- No overloading; duplicate names are errors.
- Arrays cannot be parameters/returns; use `*T` or `[T]`. Structs, slices, `str`,
  and `vec<T>` are allowed by value.
- A non-void function must return on every path, or diverge.

Function pointers:

```corex
fn inc(x: i32) -> i32 { return x + 1; }
fn dec(x: i32) -> i32 { return x - 1; }

fn apply(f: fn(i32) -> i32, x: i32) -> i32 {
    return f(x);
}

let f: fn(i32) -> i32 = &inc;
let table: [fn(i32) -> i32; 2] = [&inc, &dec];
print(apply(f, 4));       // 5
print(table[1](4));       // 3
let none: fn(i32) -> i32 = null;
if none == null { print("null"); }
```

A function name or `&name` produces a function pointer. Comparisons `==`/`!=`
work; null is allowed. Calls use the CoreX internal ABI for aggregates.

## 7. Statements and control flow

```corex
if x > 0 {
    print("positive");
} else if x == 0 {
    print("zero");
} else {
    print("negative");
}

while i < 10 {
    i += 1;
}

loop {
    i += 1;
    if i == 3 { break; }
}
```

- Conditions must be `bool`; blocks are statements, not expressions.
- `break;` exits the innermost loop or `switch`; `continue;` targets the
  innermost loop.
- `for (init; condition; step) { ... }` — each clause may be empty.
- `do { ... } while condition;` runs at least once.
- `return expression;` is required in a value-returning function and forbidden
  in a void function.
- A bare `{ ... }` creates a scope; `;` is an empty statement.
- There is no `++`, `--`, ternary `?:`, or assignment expression.

```corex
for (let mut i: i32 = 0; i < 5; i += 1) {
    print(i);
}

let mut n: i32 = 0;
do {
    n += 1;
} while n < 3;
```

### Switch, labels, and goto

```corex
switch code {
case 0 {
    print("zero");       // falls through into case 1
}
case 1: {
    print("one");
    break;               // exits the switch
}
default {
    print("other");
}
}
```

`switch` evaluates its value once and **falls through** until `break`. `case`
values are integer constant expressions (including enum variants) and must be
unique; one `default` is allowed. `continue` inside a switch targets the
enclosing loop.

```corex
let mut i: i32 = 0;

top:
if i < 3 {
    i += 1;
    goto top;
}

goto done;
print("not reached");
done:
print(i);
```

Labels are function-local and may be forward-referenced. A label applies to any
statement. `goto` jumps to it. Labeled `break`/`continue` do not exist.

A non-void function must not fall off the end. `loop`/`while true` without a
matching `break`, an `if`/`else` where both branches diverge, and a `switch`
with `default` where every arm diverges all satisfy return analysis. An `if`
without `else` does not.

## 8. Expressions and operators

Precedence, lowest to highest:

| Level | Operators |
|---:|---|
| 1 | `\|\|` |
| 2 | `&&` |
| 3 | `\|` |
| 4 | `^` |
| 5 | `&` |
| 6 | `== !=` |
| 7 | `< <= > >=` |
| 8 | `<< >>` |
| 9 | `+ -` |
| 10 | `* / %` |
| 11 | `as` |
| 12 | prefix `- ! & &mut *` |
| 13 | call `f()`, index `a[i]`, `[a..b]`, `.field`, `->field` |

Binary operands, arguments, and array elements evaluate left to right. `&&`
and `||` short-circuit.

- `+ - *` wrap modulo 2^N. Unary `-` wraps.
- Shift counts are masked with `bits - 1`; signed `>>` is arithmetic.
- Integer `/` and `%` truncate toward zero; division by zero and `MIN / -1`
  abort with exit 101.
- Float arithmetic is IEEE 754 and never aborts; `print` shows six fractional
  digits (truncated, not rounded).
- `== !=` work on equal scalar types and pointers. Ordered pointer comparisons
  are rejected.
- `& | ^` are bitwise; `!` is logical for `bool` and bitwise for integers.
- `as` is the only conversion. It truncates, extends, reinterprets bits, or
  converts int/float; `bool` cannot be cast, and integer-to-enum is rejected.
  Integer/pointer casts are `unsafe`.
- `&place` and `&mut place` require a place expression; `&mut` requires a
  mutable place.

`len(x)` accepts arrays, slices, `str`, and vectors. `s[a..b]` slices arrays,
slices, and strings; bounds may be omitted (`s[a..]`, `s[..b]`, `s[..]`). Index
and range violations abort with exit 101.

## 9. Builtins

| Builtin | Purpose |
|---|---|
| `print(v)` | newline-terminated output; integers, `bool`, `char`, floats, enums, `str`/`[u8]`, `cstr` |
| `len(x)` | array/slice/string/vector length |
| `syscall(nr, a1, ..., a6)` | raw Linux syscall; `unsafe`; returns `isize` |
| `alloc(bytes)` / `free(ptr, bytes)` | mmap/munmap; `unsafe` |
| `panic(msg)` / `assert(cond, msg)` | stderr message and exit 101 |
| `read_file(path)` / `write_file(path, data)` | whole-file I/O; `vec<u8>` / `0` or `-1` |
| `vec_new(cap)` / `vec_push` / `vec_free` / `vec_slice` | dynamic arrays |
| `data_ptr(slice_or_str)` | data pointer, `*T`/`*u8` |
| `str_from_raw(ptr, len)` | build a `str` from `*u8` + `usize` |
| `cstr_len(c)` / `cstr_to_str(c)` / `str_to_cstr(buf, cap, s)` | C string conversions |
| `sizeof(T)` / `alignof(T)` / `offsetof(T, field)` | compile-time layout |
| `atomic_*`, `volatile_load/store` | atomics and volatile access; `unsafe` |
| `asm(template, constraints, operands...)` | inline assembly; `unsafe` |

`str_to_cstr` aborts if `cap <= len(s)`. `syscall` arguments are `usize`;
missing trailing arguments are zero. `print` requires no `unsafe` and does not
allocate. `str_from_raw` is unchecked: the caller must provide a valid range.

The freestanding standard library in `lib/corex` adds `std_alloc`, `std_mem`,
`std_str`, `std_io`, `std_fs`, `std_process`, `std_env`, `std_time`,
`std_math`, `std_fmt`, `std_map`, `std_builder`, `std_sync`, `std_thread`,
`std_tls`, and hosted `std_dl`. Import them with `mod std_*;` and `-I lib/corex`.

## 10. Modules, projects, and configuration

`module` names a file; `mod` loads another file; `pub` exports an item.

```corex
// src/math.cx
module math;

pub fn add(a: i32, b: i32) -> i32 { return a + b; }
pub struct Vec2 { x: i32, y: i32 }
```

```corex
// src/main.cx
module app;
mod math;

fn main() -> i32 {
    print(math::add(20, 22));
    let v: math::Vec2 = math::Vec2 { x: 1, y: 2 };
    print(v.x);
    return 0;
}
```

Items are private by default. Use `math::item`, `math::Type`, and
`math::Enum::Variant`. Module symbols are mangled as `module$item`; root `main`
and `_start` are not mangled, but a module-level `main` is.

```toml
# corex.toml
[package]
name = "app"

[build]
kind = "exe"              # exe | shared | staticlib
sources = ["src/main.cx"]
include_paths = ["src"]
opt = 2
```

Build with `corex build .`. `kind = "shared"` / `"staticlib"` produces `.so` /
`.a`; `--emit obj` emits one object.

Legacy `import "file.cx";` is textual inclusion into one flat namespace; unlike
`mod`, it has no visibility or mangling.

```corex
#[cfg(feature = "fast")]
fn speed() -> i32 { return 1; }

#[cfg(all(not(feature = "fast"), target_os = "linux"))]
fn speed() -> i32 { return 2; }
```

Conditions are `key = "value"`, `not(...)`, `all(...)`, `any(...)`. Pass
`--cfg feature=fast` or list features in `corex.toml`. The driver always defines
`target_os=linux`, `target_arch=x86_64`, and `sanitize=true` under `--sanitize`.

## 11. C interop and low-level code

```corex
extern "C" fn puts(s: cstr) -> i32;
extern "C" fn c_add(a: i64, b: i64) -> i64 { return a + b; }
extern "C" fn printf(fmt: cstr, ...) -> i32;
extern static errno: i32;
```

An `extern "C"` declaration has no body; a definition exports the exact C
symbol. Variadic calls promote narrow integers and `f32` per C rules. Link with
`--link obj.o`, `-L dir`, `-l name`, and use `--hosted` for libc. `extern
"corex"` uses the internal aggregate ABI.

```corex
let c: cstr = c"hello";
print(c);
print(cstr_len(c));
let s: str = cstr_to_str(c);
let raw: str = c as str;

unsafe {
    let buf: *mut u8 = alloc(16);
    let out: cstr = str_to_cstr(buf, 16, "abc");
    print(out);
    free(buf, 16);
}
```

`cstr` is a pointer; `str` is pointer + length. `str_to_cstr` requires
`unsafe`.

```corex
static mut volatile FLAG: u32 = 0;

unsafe {
    atomic_store(&mut FLAG, 1);
    let old: u32 = atomic_fetch_add(&mut FLAG, 2);
    atomic_fence();
    let seen: u32 = volatile_load(&FLAG);
    volatile_store(&mut FLAG, seen);
}
```

Atomics are sequentially consistent unless suffixed `_relaxed`. `volatile`
applies to globals; use the pointer builtins for arbitrary memory. Threads,
futexes, and explicit TLS live in `std_thread`, `std_sync`, and `std_tls`; there
is no language-level `thread_local` or thread join.

```corex
macro twice(e) = e + e;
print(twice(21));                 // 42

unsafe {
    let v: usize = asm("movq $$1, $0", "=r");
    asm("nop", "");
    asm("", "~{memory}");
}
```

Macros expand at compile time; arguments are expression trees, but names are
not hygienic. `asm` constraints use LLVM syntax; `$0`, `$1`, … number operands,
and `$$` is a literal `$`.

## 12. Tooling and tests

```corex
/// Adds two integers.
fn add(a: i32, b: i32) -> i32 { return a + b; }

fn test_add() -> bool {
    return add(1, 2) == 3;
}
```

```bash
corex check file.cx
corex emit-ir file.cx --verify
corex fmt file.cx -w
corex lint file.cx --deny-warnings
corex doc file.cx -o api.md
corex test file.cx
corex-lsp
```

`///` comments feed `corex doc`. `corex test` runs every `fn test_*() -> bool`
and exits nonzero if any fail. The linter reports unused variables/items,
unreachable statements, and constant conditions.

## Rules that bite

- No implicit numeric conversions; use `as`.
- Arrays are not passed or returned by value; use slices/pointers.
- `switch` falls through unless every arm ends in `break`.
- `str` is not `cstr`, and slices have no equality or concatenation.
- `let` is immutable; `let mut` is required for assignment.
- A non-void function must return or provably diverge.
- `unsafe` is required for raw dereference, pointer arithmetic, syscalls, and
  allocator calls.
- There is no `++`, `--`, ternary, `?`, `Result`, exception, or assignment
  expression.

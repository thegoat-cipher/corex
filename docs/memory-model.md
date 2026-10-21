# CoreX Memory Model — v0.1

Status: **normative**. This document states exactly what the compiler enforces,
what the runtime checks, and what the programmer must get right. CoreX does not
claim that raw pointer code is memory-safe; it claims that a specific, small set
of operations is checked and that everything else is explicit.

Related: [`language-spec.md`](language-spec.md), [`ADRs.md`](ADRs.md) (ADR-0003).

---

## 1. Model summary

- **Manual, explicit allocation.** Memory exists in three places: read-only
  literals, stack slots for locals, and anonymous `mmap` regions requested with
  `alloc`.
- **No garbage collector, no reference counting, no background reclamation.**
  Nothing frees memory implicitly. A function return destroys its stack frame;
  heap regions live until `free` (or process exit).
- **No hidden allocation.** Assigning, returning, passing, and indexing values
  never allocates. Arrays are copied inline. A string literal is a read-only
  `[u8; N]` in the object file.
- **Type and mutability are checked; lifetime and validity are not.**
  - Compile time: every place has one type; writes require a mutable place; raw
    pointer dereference, pointer arithmetic, integer/pointer casts, `alloc`,
    `free`, and `syscall` require `unsafe`.
  - Runtime: dynamic array indices are bounds-checked; division by zero and
    failed allocation abort with exit 101.
  - Programmer: pointer validity, allocation lifetime, alignment, aliasing
    discipline, and matching `alloc`/`free` sizes.

CoreX deliberately does **not** implement ownership and borrowing (no borrow
checker, no lifetimes, no move semantics). `*T` and `*mut T` are raw addresses,
closer to C than to Rust. See ADR-0003 for the alternatives and why they were
rejected for v0.1.

---

## 2. Regions

### 2.1 Static (read-only)

String literals and runtime error messages are emitted as `private unnamed_addr
constant` globals. They are mapped read-only by the kernel (`.rodata`). Writing
through a pointer derived from a string literal via `as *mut u8` is undefined and
typically faults.

### 2.2 Stack

- Every local binding and array temporary gets one `alloca` in the function's
  entry block. Allocation therefore costs nothing at runtime inside loops.
- Lifetime: the enclosing function activation. A pointer to a local becomes
  dangling when the function returns.
- Size: fixed at compile time. No variable-length arrays. A single value larger
  than 1 MiB is rejected (`E0223`); the kernel's default 8 MiB stack applies, and
  exhausting it raises `SIGSEGV`.
- Initialization: every binding requires an initializer, and every array element
  is written before the value is used, so stack reads are always of initialized
  memory. There is no `uninit` construct.

### 2.3 Heap

```
alloc(bytes: usize) -> *mut u8      // mmap(NULL, bytes, PROT_READ|PROT_WRITE,
                                    //      MAP_PRIVATE|MAP_ANONYMOUS, -1, 0)
free(pointer: *mut u8, bytes: usize) // munmap(pointer, bytes)
```

- Requires `unsafe`.
- Granularity: pages (4096 bytes on x86-64). The kernel rounds `bytes` up; the
  returned address is page-aligned.
- Contents: freshly mapped anonymous pages are zero-filled. A second `mmap` may
  reuse the same virtual address after `munmap`; do not rely on zeroing across
  free/alloc cycles for security-sensitive data.
- Failure: a negative kernel result (including `alloc(0)`, which `mmap` rejects)
  prints `corex: allocation failed` on fd 2 and exits 101.
- `free` must receive the **same pointer** and the **same byte count** that were
  passed to `alloc`. The kernel rounds both to pages, so a slightly smaller or
  larger count within the same page is harmless; a count crossing a page boundary
  leaks or unmaps part of the region. The `munmap` result is ignored, so a double
  free or a bogus pointer fails silently.
- No `realloc`, no `calloc`, no alignment control beyond page alignment, no
  allocator metadata, no leak detector.

### 2.4 Printing (`print`)

`print` is a compiler-provided runtime helper, not a library call. It formats
into a 20-byte `alloca` in the helper's entry block and issues two `write`
syscalls: the digits, then a newline. It allocates nothing on the heap, requires no `unsafe`, and
cannot fail visibly: a failed `write` (for example a closed stdout) is ignored,
exactly like any other unchecked syscall result.

---

## 3. Operations and invariants

`unsafe` is required for every operation marked **unsafe** below. Using it
outside `unsafe` is `E0209`.

| Operation | Syntax | Compile-time requirement | Runtime requirement (programmer) | Failure mode |
|---|---|---|---|---|
| Declare pointer | `let p: *mut u8 = ...;` | type matches (or `*mut T` → `*T`) | none | `E0203` |
| Address of value | `&place` | operand is a place | object is alive while the pointer is used | `E0211` |
| Address of mutable value | `&mut place` | place is mutable | same as above | `E0220` |
| Read through pointer | `*p` (**unsafe**) | `p: *T` or `*mut T` | `p` points to a live, aligned, initialized `T` | segfault / garbage |
| Write through pointer | `*p = v` (**unsafe**) | `p: *mut T`, `v: T` | same, plus no concurrent readers | segfault / lost update |
| Pointer offset | `p + n`, `p - n` (**unsafe**) | `p` is a pointer, `n: usize` | result stays within the allocation (or one past the end) | undefined |
| Pointer difference | — | not provided | use `as usize` subtraction | — |
| Pointer comparison | `p == q`, `p != q` | same pointer type | none | — |
| Pointer-to-pointer cast | `p as *U` | both are pointers | alignment/type compatibility is your job | misaligned access |
| Integer to pointer | `n as *mut T` (**unsafe**) | `n` is an integer | address is mapped and aligned | segfault |
| Pointer to integer | `p as usize` (**unsafe**) | `p` is a pointer | none (address is exposed) | — |
| Index an array | `a[i]` | `a: [T; N]`, `i: usize` | none | compile error `E0222` (literal index) or abort 101 (computed index) |
| Copy an array | `let b: [T; N] = a;` | identical types | none (inline copy) | none |
| Heap allocation | `alloc(n)` (**unsafe**) | `n: usize` | choose a size your code will respect | abort 101 |
| Heap release | `free(p, n)` (**unsafe**) | `p: *mut u8`, `n: usize` | `p`/`n` came from one `alloc` | silent leak/partial unmap |
| Raw syscall | `syscall(nr, ...)` (**unsafe**) | 1–7 integer arguments | syscall-specific pointer validity | kernel error in return value |

### 3.1 Safety invariants by operation

**Dereference.** The pointer must have been produced by `&`/`&mut` of a live
object, by pointer arithmetic that stayed inside an allocation, by `alloc` (for
the whole requested range), or by an integer cast to a mapped address. It must be
aligned for `T` (natural alignment: `align_of(T)`), and the memory must hold a
valid `T`. CoreX does not insert null checks, alignment checks, or validity
checks.

**Pointer arithmetic.** `p + n` advances by `n * size_of(pointee)`. Computing a
pointer outside `[base, base + len]` (with a single one-past-the-end exception)
is undefined, and the generated `getelementptr inbounds` tells LLVM the same.
There is no overflow check on `n * size_of::<T>()`.

**Writes.** Writing through `*mut T` requires exclusive access by convention
only. CoreX has no threads in the language, but syscalls may expose memory to the
kernel; aliasing rules are not enforced.

**Arrays.** Literal indices are validated at compile time (`E0222`). Dynamic indices are compared against `N` with an unsigned
comparison and abort on failure (`corex: index out of bounds`, exit 101). Array
elements are stored inline; there is no bounds information in the value itself,
so a pointer into an array carries no length.

**Allocation sizes.** `alloc` returns a page-rounded region but no metadata; the
language cannot know how large the region is. `free` takes the length explicitly
for this reason. Passing the wrong length is a programmer error that may leak
memory or unmap unrelated pages.

---

## 4. Aliasing

Aliasing is **allowed**. Two pointers may refer to the same object; reads and
writes through either are well defined as far as the language and the generated
IR are concerned:

- No `noalias`, `restrict`, or TBAA metadata is emitted.
- Loads and stores use the declared alignment only.
- The compiler does not reorder or eliminate loads/stores across `unsafe` calls
  or inline-asm syscalls (`~{memory}` is attached to the `syscall` asm).

The consequence is that CoreX cannot offer the optimization benefits of strict
aliasing rules. That is a deliberate trade for predictability (ADR-0003).

---

## 5. What `unsafe` means here

`unsafe` is a **capability gate**, not a proof obligation checker. It unlocks
dereference, pointer arithmetic, integer/pointer casts, `alloc`, `free`, and
`syscall`. Inside it, all other checks (types, arity, mutability of locals,
return paths, bounds for constant indices) still apply. An `unsafe` block is
purely lexical and has no runtime representation.

It does **not** mean "anything goes": type errors inside `unsafe` are still
errors, and dynamic bounds/division checks still run.

---

## 6. Guarantee matrix

| Property | Compile time | Runtime | Programmer |
|---|---|---|---|
| Type correctness of every expression | yes | — | — |
| Writes go through a mutable place | yes | — | — |
| Unsafe operations are marked | yes | — | — |
| Non-void functions return on all paths | yes | — | — |
| Constant indices in bounds | yes | — | — |
| `usize` index type | yes | — | — |
| Pointer arithmetic stays inside an allocation | — | — | yes |
| Pointer is non-null, live, aligned | — | — | yes |
| Allocation freed exactly once with the right size | — | — | yes |
| No use after free | — | — | yes |
| Dynamic indices in bounds | — | abort 101 | — |
| Division by zero | — | abort 101 | — |
| Allocation failure | — | abort 101 | — |
| Integer overflow | defined (wraps) | — | — |

---

## 7. Undefined behavior

The following are undefined; the compiler makes no promise about the result:

1. Dereferencing a null, dangling, freed, or unmapped pointer.
2. Dereferencing a pointer whose address is not aligned for the pointee type.
3. Pointer arithmetic outside the bounds of the source allocation.
4. Reading memory that was never written through a path the compiler cannot see
   (for example, memory the kernel filled with device data) as a typed value.
5. Formatting or interpreting bytes as a type they are not (there are no unions
   in v0.1, so this requires pointer casts).
6. Double free, `free` with the wrong length, or use after `free`.
7. Writing through a pointer to read-only memory (string literals).
8. Executing an arbitrary `syscall` number with arguments the kernel interprets
   as pointers or lengths that your program does not own.

Everything in §10.1 of the language specification is defined.

---

## 8. Why not ownership and borrowing?

| Model | Safety | Cost | Why not for CoreX v0.1 |
|---|---|---|---|
| A. Explicit allocate/free with raw pointers | none beyond types | minimal; no analysis | **chosen** — smallest compiler, matches freestanding goal |
| B. Ownership + borrowing (Rust-like) | strong (no UAF, no races) | borrow checker, lifetimes, move semantics, region inference; large compiler and language surface | rejected for v0.1; would delay a working compiler and add runtime-invisible complexity |
| C. Hybrid (manual allocation behind checked handles) | partial | needs a type system for handles, escape analysis, and a policy for `free` | rejected: "partial safety" invites false confidence |

CoreX takes option A and compensates with:
- explicit `unsafe` so unsafe code is greppable and reviewable;
- defined behavior for arithmetic and indexing where C leaves it undefined;
- compile-time rejection of the easy mistakes (type confusion, immutable writes,
  constant out-of-bounds).

A future version may add option C as an opt-in layer (ADR-0003 records the
question as open), but v0.1 does not claim memory safety for pointer code.

---

## 9. Worked example

```corex
fn main() -> i32 {
    let mut buffer: [u8; 4] = [0; 4];   // 4 bytes on the stack, initialized

    unsafe {
        let first: *mut u8 = &mut buffer[0];   // mutable place -> *mut u8
        let second: *mut u8 = first + 1;       // pointer arithmetic, 1 byte
        *second = 7;                            // write through the pointer
    }

    return buffer[1] as i32;                    // reads back 7
}
```

What is checked: `buffer` is mutable, `&mut` needs a mutable place, the index is
a `usize` constant within bounds, `first + 1` is inside the array, the write
target is `*mut u8`, and `buffer[1]` is within bounds. What is not checked:
that `second` is still valid at the write (it is, by construction) or that no
other pointer aliases it.

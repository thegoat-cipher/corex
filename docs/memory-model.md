# CoreX Memory Model — v0.2

Status: **normative**. This document states exactly what the compiler enforces,
what the runtime checks, and what the programmer must get right. CoreX does not
claim that raw pointer code is memory-safe; it claims that a specific, small set
of operations is checked and that everything else is explicit.

Related: [`language-spec.md`](language-spec.md), [`ADRs.md`](ADRs.md) (ADR-0003).

---

## 1. Model summary

- **Manual, explicit allocation.** Memory exists in three places: read-only
  literals, stack slots for locals, and anonymous `mmap` regions requested with
  `alloc` (including vector growth and `read_file`).
- **No garbage collector, no reference counting, no background reclamation.**
  Nothing frees memory implicitly. A function return destroys its stack frame;
  heap regions live until `free` (or process exit).
- **No hidden allocation.** Assigning, returning, passing, and indexing values
  never allocates. Arrays and structs are copied inline. A string literal is
  read-only bytes in the object file plus a two-word `str` view (`{ptr, len}`)
  built on the stack; slices copy only those two words, never the elements.
  `vec<T>` is a three-word header whose backing block is explicitly allocated
  by `vec_new`/`vec_push` and released by `vec_free`/`process exit`.
  `const` declarations are compile-time substitutions and occupy no storage.
- **Type and mutability are checked; lifetime and validity are not.**
  - Compile time: every place has one type; writes require a mutable place; raw
    pointer dereference, pointer arithmetic, integer/pointer casts, `alloc`,
    `free`, and `syscall` require `unsafe`.
  - Runtime: dynamic array/slice indices and slice ranges are bounds-checked;
    integer division by zero and failed allocation abort with exit 101.
  - Programmer: pointer validity, allocation lifetime, alignment, aliasing
    discipline, and matching `alloc`/`free` sizes.

CoreX deliberately does **not** implement ownership and borrowing (no borrow
checker, no lifetimes, no move semantics). `*T` and `*mut T` are raw addresses,
closer to C than to Rust. See ADR-0003 for the alternatives and why they were
rejected for v0.1.

---

## 2. Regions

### 2.1 Static (read-only)

String literals, runtime error messages, and other private constants are emitted
as `private unnamed_addr constant` globals in `.rodata`. `static` globals are
emitted as LLVM `constant` or `global` definitions: immutable globals are
read-only, `static mut` globals live in writable data. Writing through a pointer
derived from a string literal via `as *mut u8` is undefined and typically faults.

A `str`/`[T]` value itself is a 16-byte `{data, len}` pair and can live on the
stack, in a struct, in an array, or in a global; the pair is copied, the
referenced elements are not. A slice may point into read-only memory, so writing
through a slice derived from a string literal is undefined even if the local
slice binding is mutable.

### 2.2 Stack

- Every local binding, array/struct temporary, slice view, and `vec<T>` header
  gets one `alloca` in the function's entry block. Allocation therefore costs
  nothing at runtime inside loops. A vector's element block is heap memory.
- Lifetime: the enclosing function activation. A pointer to a local becomes
  dangling when the function returns.
- Size: fixed at compile time. No variable-length arrays. Structs use natural
  alignment and padding; slices/`str` are always 16 bytes. A single value larger
  than 1 MiB is rejected (`E0223`); the kernel's default 8 MiB stack applies, and
  exhausting it raises `SIGSEGV`.
- Copies: binding, assignment, argument passing, and return copy arrays and
  structs byte-for-byte. Slices/`str` copy the pointer and length; the elements
  remain shared. There is no copy-on-write.
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

### 2.4 Dynamic arrays and files

- `vec_new(cap)` allocates `cap * size(T)` heap bytes (except `cap == 0`).
  `vec_push` grows to `max(4, cap * 2)`, copies live elements, frees the old
  block, and stores the new one. `vec_free` frees `cap * size(T)` bytes.
- A `vec<T>` copy is shallow: both headers point at the same block. Freeing both
  is a double free; the language has no move checking. Keep one owning binding.
- `read_file(path)` returns a `vec<u8>` whose block is allocated by the runtime;
  free it with `vec_free`. `write_file` allocates a temporary NUL-terminated path
  buffer and frees it before returning. Both use ordinary file syscalls.
- `read_file` failure returns an empty vector; `write_file` failure returns
  `-1`. `panic`/failed `assert` writes the message to fd 2 and exits 101.

### 2.5 Printing (`print`)

`print` is a compiler-provided runtime helper, not a library call. It formats
integers into a 20-byte `alloca` in the helper's entry block, commands into a
small buffer for `char`, and floats into integer/fraction parts; it then issues
`write` syscalls for the bytes and a newline. Strings and `[u8]` slices are
written directly from their data pointer and length. It allocates nothing on the heap, requires no `unsafe`, and
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
| Copy an array/struct | `let b: [T; N] = a;` | identical types | none (inline copy) | none |
| Copy a slice/`str` | `let t: [T] = s;` | identical slice type | none (two-word copy; elements shared) | none |
| Index/slice bounds | `a[i]`, `s[a..b]` | `usize` bounds | abort 101 if out of bounds | — |
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
| Slice ranges in bounds | — | abort 101 | — |
| Vector index bounds | — | abort 101 | — |
| Vector capacity arithmetic | best-effort | — | overflow is unchecked |
| `panic`/`assert` failure | — | exit 101 with message | — |
| File open/read/write failure | — | empty vector / `-1` | — |
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
7. Writing through a pointer to read-only memory (string literals), including a
   `str`/`[u8]` slice that points at a literal.
8. Freeing a vector backing block twice, including via a shallow vector copy.
9. Growing a vector until `cap * size(T)` overflows `usize`; the current
   implementation does not check this arithmetic.
10. Executing an arbitrary `syscall` number with arguments the kernel interprets
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

---

## 8. Allocators (v0.3)

The language still allocates nothing implicitly. Every byte is a stack slot or
comes from an explicit call.

### 8.1 Raw allocation

`alloc(bytes) -> *mut u8` and `free(ptr, bytes)` remain the primitives: a direct
`mmap`/`munmap` syscall pair, both gated by `unsafe`. They require the exact byte
count on free, which is why normal code should use the allocator API below.

### 8.2 C-compatible heap (`std_alloc`)

`malloc`/`calloc`/`realloc`/`free` have C semantics:

- `malloc(n)` returns 16-byte-aligned memory or `null`. The allocator stores a
  32-byte header immediately before the user pointer: mapping base, mapping
  length, payload size, and a magic word. The header is outside the user's
  region, so `free` needs no size argument.
- `free(null)` is a no-op. `free` of a pointer whose header magic does not match
  is ignored (and aborts under `--sanitize`).
- `realloc(p, n)` grows in place when the new size fits the existing payload,
  otherwise allocates, copies the old payload, and frees the old block.
- `calloc(n, size)` multiplies (wrapping, as in C) and zeroes.
- `aligned_alloc(size, align)` supports power-of-two alignments, including
  alignments above 16, by over-allocating and placing the header before the
  aligned pointer.

The heap is not thread-safe by itself. `malloc`/`free` may be used from multiple
threads only when the application serializes them; the language does not add a
hidden global lock.

### 8.3 Allocator interface

```corex
struct Allocator {
    context: *mut u8,
    alloc: fn(*mut u8, usize, usize) -> *mut u8,   // ctx, size, align
    realloc: fn(*mut u8, *mut u8, usize, usize) -> *mut u8, // ctx, ptr, size, align
    free: fn(*mut u8, *mut u8),                    // ctx, ptr
}
```

`c_allocator()` returns the malloc-backed implementation; `arena_allocator` binds
an arena. Passing an `Allocator` by value is a pointer + three function
pointers; accepting it by pointer avoids a copy.

### 8.4 Arena

`Arena` grows by allocating chunks with `malloc`. A chunk starts with
`[previous_chunk, capacity]`; allocations are bump-pointer and aligned on
request. `arena_reset` frees every chunk at once; individual `arena_alloc`
blocks cannot be freed separately, and `arena_realloc` returns `null` because it
cannot grow in place. This is the intended shape for compiler passes: allocate
freely, reset between phases.

### 8.5 Memory sanitizers

- Bounds: array/slice indexing and sub-slicing remain checked; violations print
  `corex: index out of bounds` / `corex: slice range out of bounds` and exit 101.
- Division: divide-by-zero and `MIN / -1` abort (already in v0.2).
- `--sanitize` adds checked signed `i8`/`i16`/`i32`/`i64` add/sub/mul through
  LLVM overflow intrinsics. On overflow the program prints
  `corex: integer overflow` to stderr and exits 101. Unsigned arithmetic keeps
  its defined wrapping semantics; shifts stay masked.
- Under `--sanitize`, `std_alloc::free` poisons the payload with `0xDE`, keeps
  the mapping (a deliberate leak) so a second `free` can be diagnosed, and aborts
  with `free of an invalid or already freed pointer`. Without the flag, double
  frees are ignored because the memory has already been unmapped.
- There is no compiler-instrumented use-after-free detector for arbitrary raw
  pointers: detecting every bad access would require shadow memory and a runtime,
  which contradicts the freestanding design. The allocator checks above are the
  practical subset.

## 9. Concurrency and TLS

- Atomics map to LLVM atomic instructions. The default is sequentially
  consistent; `_relaxed` variants use `monotonic`. `atomic_fence()` is a seq_cst
  fence. Atomic operations require `unsafe` because they take raw pointers.
- `volatile` on a global marks every load/store of that global `volatile`.
  `volatile_load`/`volatile_store` do the same for arbitrary pointers.
- Threads are created with `clone(2)` and a per-thread stack; they share the
  address space and file table. The runtime provides no scheduler, no join, and
  no automatic stack cleanup; `std_thread::free_stack` exists for a thread that
  has already exited.
- TLS is explicit: `std_tls` keeps a 64-slot table keyed by `gettid`, protected
  by a spin lock. `tls_alloc` allocates the block on first use; `tls_release`
  frees it. There is no hidden per-thread allocation and no destructor.
- Inline assembly (`asm`) is the escape hatch for instructions the compiler does
  not model; clobbers and memory barriers are specified in the constraint string.

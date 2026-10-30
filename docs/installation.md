# Installing CoreX

This guide installs the CoreX **0.4.0** compiler from source and verifies it by
compiling and running real freestanding ELF binaries. It also covers the few
environment variables that control tool discovery.

CoreX is tested with **Rust 1.99.1** and **LLVM 20.1.8** on **Linux x86-64**.
Other versions may work, but the emitted IR and the integration tests are pinned
to LLVM 20.

---

## 1. Supported platform

| Requirement | Supported |
|---|---|
| CPU / OS | Linux x86-64 (native or WSL2) |
| Kernel | Any modern x86-64 Linux with the standard syscall ABI |
| Runtime | None: generated ELFs are static, freestanding, and libc-free |
| Host tools | Rust + Cargo, LLVM 20 (`llvm-as`, `opt`, `llc`), and a linker driver |
| Not supported | macOS, Windows outside WSL2, ARM64, cross-compilation |

Targeting Linux x86-64 is a compiler rule, not a build flag: the backend emits
`x86_64-unknown-linux-gnu` IR and the runtime helpers issue Linux syscalls
directly.

---

## 2. Prerequisites

Install these before building the compiler:

| Tool | Purpose | Check |
|---|---|---|
| `rustc` / `cargo` | Build the compiler | `rustc --version` |
| `llvm-as` 20 | Assemble and verify textual IR | `llvm-as --version` |
| `opt` 20 | Run LLVM optimization passes for `--opt 1..3` | `opt --version` |
| `llc` 20 | Lower IR to an x86-64 object | `llc --version` |
| `cc` | Link the final static ELF | `cc --version` |
| `git` | Fetch the repository | `git --version` |

The compiler itself has **zero external Rust crates**, so no network access is
needed after the source and toolchain are present.

### Install Rust

Use [rustup](https://rustup.rs/) and install a current stable toolchain:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup toolchain install stable
rustc --version
cargo --version
```

Rust 1.99.1 is the tested version. Newer stable releases normally work.

### Install LLVM 20

#### Option A: official LLVM 20.1.8 tarball

This is the most reproducible option because it does not depend on distribution
packages:

```bash
curl -LO https://github.com/llvm/llvm-project/releases/download/llvmorg-20.1.8/LLVM-20.1.8-Linux-X64.tar.xz
tar -xf LLVM-20.1.8-Linux-X64.tar.xz
export COREX_LLVM_BIN="$PWD/LLVM-20.1.8-Linux-X64/bin"
```

Add the `export` line to your shell profile (for example `~/.bashrc`) to keep it
for future shells.

#### Option B: distribution packages

Install the LLVM 20 toolchain using your distribution's package manager, then
make sure `llvm-as`, `opt`, and `llc` are on `PATH`. If the tools are under a
versioned directory such as `/usr/lib/llvm-20/bin`, point CoreX at it:

```bash
export COREX_LLVM_BIN=/usr/lib/llvm-20/bin
```

Do not mix versions: `llvm-as`, `opt`, and `llc` should all report LLVM 20.
CoreX searches `COREX_LLVM_BIN`, then `PATH`, then numbered `/usr/lib/llvm-*/bin`
directories.

### Install a linker driver

Any GCC or Clang driver that supports static, non-PIE linking works:

```bash
cc --version
# or:
clang --version
```

If your linker driver has a non-default name:

```bash
export COREX_CC=clang
```

CoreX invokes it as:

```text
cc -nostdlib -static -no-pie -o <output> <object.o>
```

No libc is linked. A C toolchain is used only as a linker driver.

---

## 3. Build the compiler

```bash
git clone https://github.com/thegoat-cipher/corex.git
cd corex
export COREX_LLVM_BIN=/path/to/llvm-20/bin   # if not already on PATH
cd compiler
cargo build --release
```

The binaries are `compiler/target/release/corex` and
`compiler/target/release/corex-lsp`.

A cold build only compiles the compiler (and its zero external crates); it does
not need LLVM development headers or `libLLVM`.

### Smoke test

```bash
./target/release/corex --version
# corex 0.4.0 (LLVM 20 backend)

./target/release/corex run ../examples/hello.cx
# Hello, World!
```

Build a binary and inspect it:

```bash
./target/release/corex build ../examples/factorial.cx -o /tmp/factorial --opt 2
/tmp/factorial
# 3628800
file /tmp/factorial
# ELF 64-bit LSB executable, x86-64, statically linked (abridged)
```

`ldd /tmp/factorial` should report that it is not a dynamic executable.

---

## 4. Put `corex` on your `PATH`

Either copy the release binaries:

```bash
install -d "$HOME/.local/bin"
install -m 755 target/release/corex target/release/corex-lsp "$HOME/.local/bin/"
export PATH="$HOME/.local/bin:$PATH"
```

or let Cargo install it:

```bash
cargo install --path . --locked
# installs corex and corex-lsp to ~/.cargo/bin
```

For an isolated install root (no writes to `~/.cargo/bin`):

```bash
cargo install --path . --locked --root "$HOME/.local/corex"
# installs $HOME/.local/corex/bin/corex and corex-lsp
export PATH="$HOME/.local/corex/bin:$PATH"
```

Then run from any directory:

```bash
corex run /path/to/program.cx
```

---

## 5. Verify the installation

Run the checked-in examples from the repository root:

```bash
cd ..          # repository root, if you are still in compiler/
corex run examples/hello.cx
# Hello, World!

corex run examples/features.cx
# deterministic feature tour: enums, structs, floats, slices, strings

corex run examples/compiler_data.cx
# 9
# 0
# fn
# 4
# 1
```

Run the full test suite from `compiler/`:

```bash
cd compiler
cargo test
```

Expected result on a correctly installed toolchain: every `test result` line
reports `0 failed`, totaling **205 passed** (68 unit, 137 integration).

The integration tests invoke `llvm-as`, `llc`, and `cc` for real. If a tool is
missing, they fail rather than silently skipping.

---

## 6. Run the REPL

The REPL is an interactive, in-memory program builder. Start it from the
`compiler/` directory:

```bash
./target/release/corex repl
```

If `corex` is on your `PATH`:

```bash
corex repl
```

Example session:

```text
CoreX REPL — finish a block with an empty line. Type :help for commands.

corex> 1 + 2
3
[exit 0]
corex> fn fact(n: u64) -> u64 {
   ...>     if n <= 1 { return 1; }
   ...>     return n * fact(n - 1);
   ...> }
   ...>
corex> fact(10)
3628800
[exit 0]
corex> :quit
```

REPL basics:

- A complete one-liner such as `1 + 2` runs on Enter.
- A multi-line function or block finishes with a blank line.
- Top-level items accumulate: functions, structs, unions, typedefs, enums,
  consts, globals, and macros. Redefining an item replaces it.
- `fn main` or `fn _start` is executed after each subsequent definition update,
  so an old program may print again while you edit it. Use `:reset` before
  pasting an unrelated program.
- Bindings do not persist between submissions. Group related statements in
  `{ ... }` and submit them together with `:submit`.
- `:help` lists commands: `:run`, `:show`, `:history`, `:ir`, `:submit`,
  `:reset`, `:quit`.
- `Ctrl-D` or `:quit` exits.

Piping a source file is useful for a demo:

```bash
corex repl < ../examples/hello.cx
```

It prints REPL prompts as well as program output. For scripts and normal
execution, use plain file mode instead:

```bash
corex run ../examples/hello.cx
corex run ../examples/features.cx
```

Do not type shell commands such as `corex run ../examples/features.cx` at the `corex>`
prompt; that prompt expects CoreX code or a `:` command.

---

## 7. Environment variables

| Variable | Effect |
|---|---|
| `COREX_LLVM_BIN` | Directory containing `llvm-as`, `opt`, and `llc`. Takes precedence over `PATH`. |
| `COREX_CC` | Linker driver (default `cc`). Must support `-nostdlib -static -no-pie`. |
| `COREX_KEEP_TEMP` | If set, keep the temporary build directory (`/tmp/corex-build-*`) containing `.ll`, `.bc`, and `.o` files. |
| `COREX_DUMP_IR` | If set, include the generated IR in the diagnostic note when LLVM verification fails. |

Example:

```bash
COREX_LLVM_BIN=/opt/llvm-20/bin \
COREX_CC=clang \
corex build program.cx -o program --opt 2
```

---

## 8. Common failures

| Symptom | Cause | Fix |
|---|---|---|
| `E0303: llvm-as not found` | LLVM tools are not on `PATH` or under `COREX_LLVM_BIN`. | Export `COREX_LLVM_BIN`; check `llvm-as --version`. |
| `E0303: opt not found` only with `--opt 1..3` | `opt` is missing while `llvm-as`/`llc` exist. | Install the full LLVM 20 toolchain. |
| `E0300: generated LLVM IR failed verification` | Mismatched LLVM version or compiler defect. | Confirm all three tools report LLVM 20. Reproduce with `corex emit-ir file.cx --verify`; set `COREX_DUMP_IR=1` for details. |
| Linker exits with `unrecognized -no-pie` or cannot find `-lc` | Unsupported linker driver or a non-standard `cc`. | Set `COREX_CC` to GCC or Clang; CoreX must not link libc. |
| `E0217: main must be declared as fn main() -> i32` | Wrong entry signature. | Use `fn main() -> i32` or `fn _start()`. |
| `E0212: program has no entry point` | No entry function was defined. | Add `main` or `_start`. |
| `E0232: cannot import ...` | Import path is missing or relative to the wrong file. | Paths are resolved relative to the importing file; check the spelling and run `corex check`. |
| `read_file` returns an empty vector | File missing, unreadable, or empty. | `read_file` returns empty on failure; check the path with host tools. |
| REPL arrow keys/history do not work | stdin is not a TTY, or `stty` is unavailable. | Run `corex repl` interactively in a real terminal; plain line input still works. |
| `cargo test` fails because `cc` is missing | Integration tests link binaries. | Install GCC/Clang or run `cargo test --lib` for compiler-only unit tests. |

---

## 9. Updating and uninstalling

Update from the repository:

```bash
cd corex
git pull --rebase
cd compiler
cargo build --release
```

Uninstall:

```bash
rm -f "$HOME/.local/bin/corex" "$HOME/.local/bin/corex-lsp"  # manual install
cargo uninstall corex                    # if installed with cargo install
rm -rf "$HOME/.local/corex"              # if installed with --root
rm -rf compiler/target                   # build artifacts
```

No system services, global configuration, or runtime libraries are installed by
CoreX.

---

## 10. Next steps

- [`README.md`](../README.md) — quick start, CLI, language tour, limitations.
- [`docs/resources.md`](resources.md) — curated documentation, source map, tests,
  examples, and external references.
- [`docs/language-spec.md`](language-spec.md) — normative language reference.
- [`docs/architecture.md`](architecture.md) — compiler pipeline and code layout.
- [`examples/compiler_data.cx`](../examples/compiler_data.cx) — a tokenizer built
  from CoreX structs, enums, strings, slices, vectors, and `assert`.

---

## 11. v0.3 addendum: tooling and projects

The compiler now ships two binaries (`corex`, `corex-lsp`) and a freestanding
standard library. All tools use the same LLVM 20 lookup as the compiler:
`COREX_LLVM_BIN`, then `PATH`, then `/usr/lib/llvm-N/bin`.

### 11.1 Build and run the newer commands

```bash
cd compiler && cargo build --release

./target/release/corex fmt   ../examples/features.cx --check   # 0 if formatted
./target/release/corex fmt   ../examples/features.cx -w        # rewrite
./target/release/corex lint  ../examples/features.cx           # warnings, exit 0
./target/release/corex doc   ../examples/features.cx           # Markdown on stdout
cat > /tmp/corex-tests.cx <<'CX'
fn test_addition() -> bool { return 1 + 1 == 2; }
fn test_shift() -> bool { return (1 << 4) == 16; }
CX
./target/release/corex test  /tmp/corex-tests.cx               # per-test results
./target/release/corex-lsp                                     # stdio LSP server
```

`corex test` expects functions named `test_*` returning `bool`; it builds a
runner, prints `test name ... ok` or `... FAILED`, and exits 1 on any failure.

### 11.2 Compiling with the standard library

The library lives in `lib/corex`. Point the module search path at it:

```bash
./target/release/corex run app.cx -I ../lib/corex
```

`app.cx` uses `mod std_alloc; mod std_io; ...` and calls qualified functions.
A `corex.toml` project can list `include_paths = ["../lib/corex"]` instead of
passing `-I`.

### 11.3 Debug and sanitizer builds

```bash
./target/release/corex build app.cx -g -o app-dbg
readelf --debug-dump=decodedline app-dbg
readelf --debug-dump=info app-dbg | grep DW_TAG_subprogram

./target/release/corex build app.cx --sanitize -o app-san
```

`-g` adds DWARF 5 line tables, subprograms, and local variables. `--sanitize`
adds checked signed arithmetic and allocator double-free detection; overflow
aborts with exit 101 and `corex: integer overflow` on stderr.

### 11.4 Hosted builds and shared libraries

```bash
corex build lib.cx --emit shared -o libdemo.so
corex build app.cx --hosted -I ../lib/corex -o app     # dlopen/dlsym available
```

`--hosted` links the C runtime and lets it call `main`; freestanding builds
remain the default. `--emit staticlib` produces an archive with `llvm-ar`.

### 11.5 Environment additions

| Variable | Effect |
|---|---|
| `COREX_DUMP_INTERFACES=1` | Print generated module interfaces (`.cxi` source) to stderr |
| `COREX_DUMP_IR=1` | Include generated IR in an `llvm-as` failure note |
| `COREX_KEEP_TEMP=1` | Keep the build directory with `.ll`, `.bc`, `.o` files |

### 11.6 Troubleshooting additions

| Symptom | Cause / fix |
|---|---|
| `cannot find module 'std_alloc'` | Add `-I <repo>/lib/corex` (or `include_paths` in `corex.toml`) |
| `E0232` cycle | `mod` dependencies form a cycle; break it with a smaller shared module |
| `undefined reference to dlopen` | Build with `--hosted` (and `-l dl` on older glibc) |
| `E0233` | A public function/type mentions a private local type; mark the type `pub` |
| `E0235` | `corex test` found no `test_*` functions, or one does not return `bool` |
| Debug sections missing | Rebuild with `-g`; `readelf -S` must list `.debug_line` |

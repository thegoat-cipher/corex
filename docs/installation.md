# Installing CoreX

This guide installs the CoreX compiler from source and verifies it by compiling
and running real freestanding ELF binaries. It also covers the few environment
variables that control tool discovery.

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

The binary is `compiler/target/release/corex`.

A cold build only compiles the compiler (and its zero external crates); it does
not need LLVM development headers or `libLLVM`.

### Smoke test

```bash
./target/release/corex --version
./target/release/corex run ../examples/hello.cx
# Hello, World!
```

Build a binary and inspect it:

```bash
./target/release/corex build ../examples/factorial.cx -o /tmp/factorial --opt 2
/tmp/factorial
# 3628800
file /tmp/factorial
# ELF 64-bit LSB executable, x86-64, statically linked
```

`ldd /tmp/factorial` should report that it is not a dynamic executable.

---

## 4. Put `corex` on your `PATH`

Either copy the release binary:

```bash
install -d "$HOME/.local/bin"
install -m 755 target/release/corex "$HOME/.local/bin/corex"
export PATH="$HOME/.local/bin:$PATH"
```

or let Cargo install it:

```bash
cargo install --path . --locked
# installs to ~/.cargo/bin/corex
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

Expected result on a correctly installed toolchain:

```text
142 passed, 0 failed
```

The integration tests invoke `llvm-as`, `llc`, and `cc` for real. If a tool is
missing, they fail rather than silently skipping.

---

## 6. Environment variables

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

## 7. Common failures

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

## 8. Updating and uninstalling

Update from the repository:

```bash
cd corex
git pull --rebase
cd compiler
cargo build --release
```

Uninstall:

```bash
rm -f "$HOME/.local/bin/corex"          # manual install
cargo uninstall corex                    # if installed with cargo install
rm -rf compiler/target                   # build artifacts
```

No system services, global configuration, or runtime libraries are installed by
CoreX.

---

## 9. Next steps

- [`README.md`](../README.md) — quick start, CLI, language tour, limitations.
- [`docs/resources.md`](resources.md) — curated documentation, source map, tests,
  examples, and external references.
- [`docs/language-spec.md`](language-spec.md) — normative language reference.
- [`docs/architecture.md`](architecture.md) — compiler pipeline and code layout.
- [`examples/compiler_data.cx`](../examples/compiler_data.cx) — a tokenizer built
  from CoreX structs, enums, strings, slices, vectors, and `assert`.

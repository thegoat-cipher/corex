#!/usr/bin/env bash
# CoreX reproducible bootstrap.
#
# Stage 0: Rust `corex` (the reference compiler) compiles selfhost/corexc.cx.
# Stage 1: the CoreX source compiled by Stage 0.
# Stage 2: the CoreX source compiled by Stage 1.
# Stage 3: the CoreX source compiled by Stage 2.
#
# Stage 2 and Stage 3 must be byte-identical under the controlled toolchain.
# Usage: selfhost/bootstrap.sh [--jobs N]
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="${COREX_LLVM_BIN:-$HOME/opt/llvm20/bin}:$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
export CC="${COREX_CC:-cc}"

STAGE0="$ROOT/compiler/target/release/corex"
STAGE1="$ROOT/build/stage1/corexc"
STAGE2="$ROOT/build/stage2/corexc"
STAGE3="$ROOT/build/stage3/corexc"
STAGE4="$ROOT/build/stage4/corexc"
SRC="$ROOT/selfhost/corexc.cx"

mkdir -p "$ROOT/build/stage1" "$ROOT/build/stage2" "$ROOT/build/stage3" "$ROOT/build/stage4"

echo "== CoreX bootstrap =="
echo "root:      $ROOT"
echo "source:    $SRC"
echo "rustc:     $(rustc --version 2>/dev/null || echo missing)"
echo "llvm-as:   $(llvm-as --version | head -1)"
echo "llc:       $(llc --version | head -1)"
echo "cc:        $($CC --version | head -1)"

echo
echo "== Stage 0: Rust compiler =="
if [ ! -x "$STAGE0" ]; then
  (cd "$ROOT/compiler" && cargo build --release)
fi
"$STAGE0" --version

echo
echo "== Stage 1: CoreX source compiled by Stage 0 =="
time "$STAGE0" build "$SRC" -o "$STAGE1" --opt 0

echo
echo "== Stage 2: CoreX source compiled by Stage 1 =="
time "$STAGE1" build "$SRC" -o "$STAGE2" --opt 0

echo
echo "== Stage 3: CoreX source compiled by Stage 2 =="
time "$STAGE2" build "$SRC" -o "$STAGE3" --opt 0

echo
echo "== Stage 4: CoreX source compiled by Stage 3 =="
time "$STAGE3" build "$SRC" -o "$STAGE4" --opt 0

HASH1="$(sha256sum "$STAGE1" | cut -d' ' -f1)"
HASH2="$(sha256sum "$STAGE2" | cut -d' ' -f1)"
HASH3="$(sha256sum "$STAGE3" | cut -d' ' -f1)"
HASH4="$(sha256sum "$STAGE4" | cut -d' ' -f1)"

echo
echo "== Artifacts =="
printf 'stage1 %s %s\n' "$HASH1" "$STAGE1"
printf 'stage2 %s %s\n' "$HASH2" "$STAGE2"
printf 'stage3 %s %s\n' "$HASH3" "$STAGE3"
printf 'stage4 %s %s\n' "$HASH4" "$STAGE4"

if [ "$HASH2" != "$HASH3" ]; then
  echo "REPRODUCIBILITY FAILURE: stage2 and stage3 differ" >&2
  exit 1
fi
if [ "$HASH3" != "$HASH4" ]; then
  echo "REPRODUCIBILITY FAILURE: stage3 and stage4 differ" >&2
  exit 1
fi
echo "reproducible: stage2 == stage3 == stage4"

echo
echo "== Behavioural corpus =="
tests/selfhosting/run.sh "$STAGE1"
tests/selfhosting/run.sh "$STAGE2"
tests/selfhosting/run.sh "$STAGE3"

echo
echo "== CLI parity =="
tests/selfhosting/cli.sh "$STAGE1"
tests/selfhosting/cli.sh "$STAGE2"

echo
echo "== Differential dumps =="
tests/selfhosting/dump.sh "$STAGE2"

echo
echo "== Malformed input / ASan =="
tests/selfhosting/fuzz.sh "$STAGE2"

echo
echo "bootstrap ok"

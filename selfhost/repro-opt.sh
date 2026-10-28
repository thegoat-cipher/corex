#!/usr/bin/env bash
# Reproducibility gate for optimized self-builds (SH-042).
#
# `bootstrap.sh` pins the fixed point at --opt 0. This script checks the same
# property at --opt 1..3 and prints the hashes. The per-level hashes are
# expected to DIFFER from each other and from --opt 0: they are a function of
# LLVM's optimization pipeline, not of the compiler. What must hold is that
# each level is deterministic and reaches its own fixed point:
#
#   self[opt]   = stage2 (--opt 0) building the source at --opt opt
#   repeat      = stage2 building the source at --opt opt again
#   fixed       = self[opt] building the source at --opt opt
#
#   repeat == self[opt] == fixed        (determinism + self-fixpoint)
#   oracle == self[opt]                 (cross-check: stage1, built by the
#                                        Rust oracle, must emit the same code)
#
# Usage: selfhost/repro-opt.sh [--corpus]
# Requires build/stage1 and build/stage2; run selfhost/bootstrap.sh first.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

export PATH="${COREX_LLVM_BIN:-$HOME/opt/llvm20/bin}:$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin"
export CC="${COREX_CC:-cc}"

SRC="$ROOT/selfhost/corexc.cx"
STAGE1="$ROOT/build/stage1/corexc"
STAGE2="$ROOT/build/stage2/corexc"

corpus=0
for arg in "$@"; do
  case "$arg" in
    --corpus) corpus=1 ;;
    -h|--help) sed -n '2,20p' "$0"; exit 0 ;;
    *) echo "usage: selfhost/repro-opt.sh [--corpus]" >&2; exit 2 ;;
  esac
done

if [ ! -x "$STAGE2" ]; then
  echo "missing $STAGE2; run selfhost/bootstrap.sh first" >&2
  exit 2
fi

mkdir -p "$ROOT/build/opt1" "$ROOT/build/opt2" "$ROOT/build/opt3"

failures=0
fail() {
  echo "REPRODUCIBILITY FAILURE: $*" >&2
  failures=$((failures + 1))
}

hash_of() { sha256sum "$1" | cut -d' ' -f1; }

printf '%-6s %-64s %9s\n' level sha256 size
for opt in 1 2 3; do
  dir="$ROOT/build/opt$opt"
  self="$dir/self"
  repeat="$dir/repeat"
  fixed="$dir/fixed"

  "$STAGE2" build "$SRC" -o "$self" --opt "$opt"
  "$STAGE2" build "$SRC" -o "$repeat" --opt "$opt"
  "$self" build "$SRC" -o "$fixed" --opt "$opt"

  h_self="$(hash_of "$self")"
  h_repeat="$(hash_of "$repeat")"
  h_fixed="$(hash_of "$fixed")"

  printf 'opt%-3s %s %9s\n' "$opt" "$h_self" "$(stat -c%s "$self")"

  [ "$h_repeat" = "$h_self" ] || fail "opt$opt: repeated build differs ($h_repeat != $h_self)"
  [ "$h_fixed" = "$h_self" ] || fail "opt$opt: self-build is not a fixed point ($h_fixed != $h_self)"

  if [ -x "$STAGE1" ]; then
    oracle="$dir/oracle"
    "$STAGE1" build "$SRC" -o "$oracle" --opt "$opt"
    h_oracle="$(hash_of "$oracle")"
    [ "$h_oracle" = "$h_self" ] || fail "opt$opt: stage1 and stage2 disagree ($h_oracle != $h_self)"
  else
    echo "note: $STAGE1 missing, skipping the stage1/stage2 cross-check"
  fi

  if [ "$corpus" -eq 1 ]; then
    "$ROOT/tests/selfhosting/run.sh" "$self"
  fi
done

if [ "$failures" -ne 0 ]; then
  echo "repro-opt: $failures failure(s)" >&2
  exit 1
fi

echo "repro-opt: deterministic and self-reproducing at --opt 1..3"
echo "repro-opt: hashes differ per level by design; compare within a row, not across rows"

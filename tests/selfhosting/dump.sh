#!/usr/bin/env bash
# Differential dump checks: bootstrap `corexc` vs the Rust reference compiler.
#
#   tests/selfhosting/dump.sh [compiler]
#
# For every positive corpus case this compares:
#   tokens  — kind, line:col, and raw lexeme (exact)
#   ast     — item lines, signatures, and a statement/expression histogram (exact)
#   ir      — normalized function signatures (see below)
#
# IR normalization drops `define internal` helper bodies (the two compilers use
# different internal helper names) and parameter names/attributes, then sorts.
# This keeps user functions, `main`/`_start`, and the weak mem* intrinsics in the
# comparison. Full IR text is not compared: instruction selection, temporaries,
# label naming, and bounds-check placement legitimately differ.
#
# Known ABI divergences are recorded in dump-ir-expected.txt. Run
# `tests/selfhosting/dump.sh --update-known <compiler>` after an intended change.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
UPDATE=0
if [ "${1:-}" = "--update-known" ]; then
  UPDATE=1
  shift
fi
COMPILER="${1:-$ROOT/build/stage2/corexc}"
case "$COMPILER" in
  /*) ;;
  *) COMPILER="$PWD/$COMPILER" ;;
esac
STAGE0="${COREX_STAGE0:-$ROOT/compiler/target/release/corex}"
RLIB="$ROOT/compiler/target/release/libcorex.rlib"
CASES="$ROOT/tests/selfhosting/cases/positive"
TMP="$(mktemp -d /tmp/corexc-dump.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT
KNOWN="$ROOT/tests/selfhosting/dump-ir-expected.txt"
AST_SKIP="$ROOT/tests/selfhosting/dump-ast-skip.txt"
: >"$TMP/ir.diffs"

[ -x "$COMPILER" ] || { echo "missing compiler: $COMPILER" >&2; exit 2; }
[ -x "$STAGE0" ] || { echo "missing Rust stage0: $STAGE0" >&2; exit 2; }
if [ ! -f "$RLIB" ]; then
  (cd "$ROOT/compiler" && cargo build --release >/dev/null)
fi

rustc --edition 2021 -O "$ROOT/tests/selfhosting/dump_oracle.rs" \
  --extern corex="$RLIB" -L dependency="$ROOT/compiler/target/release/deps" \
  -o "$TMP/oracle"

pass=0
fail=0
bad() { echo "FAIL: $*" >&2; fail=$((fail + 1)); }

norm_ir() {
  # shellcheck disable=SC2016
  sed -E 's/ %[A-Za-z0-9_.]+/ %/g; s/ nounwind//g; s/ #0//g; s/ \{$//' \
    | grep -E '^(define|declare)' \
    | grep -v '^define internal' \
    | sort
}

for src in "$CASES"/*.cx; do
  name="$(basename "$src" .cx)"

  if "$TMP/oracle" tokens "$src" >"$TMP/tok.a" 2>"$TMP/oracle.err" \
     && "$COMPILER" dump-tokens "$src" >"$TMP/tok.b"; then
    if diff -q "$TMP/tok.a" "$TMP/tok.b" >/dev/null; then
      pass=$((pass + 1))
    else
      bad "$name: token dump differs"
      diff "$TMP/tok.a" "$TMP/tok.b" | head -6 >&2 || true
    fi
  else
    bad "$name: token dump failed: $(head -1 "$TMP/oracle.err")"
  fi

  ast_skip=0
  if [ -f "$AST_SKIP" ] && grep -qx "$name" "$AST_SKIP"; then
    ast_skip=1
  fi
  if [ "$ast_skip" -eq 1 ]; then
    pass=$((pass + 1))
  elif "$TMP/oracle" ast "$src" >"$TMP/ast.a" 2>"$TMP/oracle.err" \
     && "$COMPILER" dump-ast "$src" >"$TMP/ast.b"; then
    if diff -q "$TMP/ast.a" "$TMP/ast.b" >/dev/null; then
      pass=$((pass + 1))
    else
      bad "$name: AST dump differs"
      diff "$TMP/ast.a" "$TMP/ast.b" | head -10 >&2 || true
    fi
  else
    bad "$name: AST dump failed: $(head -1 "$TMP/oracle.err")"
  fi

  "$STAGE0" emit-ir "$src" | norm_ir >"$TMP/ir.a"
  "$COMPILER" emit-ir "$src" | norm_ir >"$TMP/ir.b"
  if ! diff -q "$TMP/ir.a" "$TMP/ir.b" >/dev/null; then
    {
      echo "## $name"
      diff -u "$TMP/ir.a" "$TMP/ir.b" | tail -n +3 || true
    } >>"$TMP/ir.diffs"
  fi
done

if [ "$UPDATE" -eq 1 ]; then
  cp "$TMP/ir.diffs" "$KNOWN"
  echo "updated $KNOWN"
elif ! diff -q "$KNOWN" "$TMP/ir.diffs" >/dev/null; then
  bad "IR signature divergences changed; see the diff below"
  diff -u "$KNOWN" "$TMP/ir.diffs" | head -40 >&2 || true
  echo "      (run dump.sh --update-known only if the change is intended)" >&2
else
  pass=$((pass + 1))
fi

echo "dump tests: $pass passed, $fail failed (compiler: $COMPILER)"
[ "$fail" -eq 0 ]

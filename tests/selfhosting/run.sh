#!/usr/bin/env bash
# Differential and behavioural tests for the CoreX bootstrap compiler.
#
#   tests/selfhosting/run.sh [compiler]
#
# Each positive case is checked, compiled, executed, and compared byte-for-byte
# against <case>.out and <case>.rc at --opt 0..3. Negative cases must be rejected
# by the frontend with a non-empty diagnostic.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMPILER="${1:-$ROOT/build/stage2/corexc}"
CASES="$ROOT/tests/selfhosting/cases"
TMP="$(mktemp -d /tmp/corexc-selfhost.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

if [ ! -x "$COMPILER" ]; then
  echo "missing compiler: $COMPILER" >&2
  exit 2
fi

pass=0
fail=0
report_fail() {
  echo "FAIL: $*" >&2
  fail=$((fail + 1))
}

for src in "$CASES"/positive/*.cx; do
  name="$(basename "$src" .cx)"
  expected_out="$CASES/positive/$name.out"
  expected_rc="$CASES/positive/$name.rc"
  if [ ! -f "$expected_out" ] || [ ! -f "$expected_rc" ]; then
    report_fail "$name: missing expected output"
    continue
  fi
  want_rc="$(cat "$expected_rc")"
  if ! "$COMPILER" check "$src" >"$TMP/check.out" 2>"$TMP/check.err"; then
    report_fail "$name: check rejected valid input: $(head -1 "$TMP/check.err")"
    continue
  fi
  for opt in 0 1 2 3; do
    bin="$TMP/$name.$opt"
    if ! "$COMPILER" build "$src" -o "$bin" --opt "$opt" >"$TMP/build.out" 2>"$TMP/build.err"; then
      report_fail "$name --opt $opt: build failed: $(head -1 "$TMP/build.err")"
      continue
    fi
    set +e
    "$bin" >"$TMP/run.out" 2>"$TMP/run.err"
    got_rc=$?
    set -e
    if [ "$got_rc" != "$want_rc" ]; then
      report_fail "$name --opt $opt: exit $got_rc, expected $want_rc"
      continue
    fi
    if ! diff -q "$expected_out" "$TMP/run.out" >/dev/null; then
      report_fail "$name --opt $opt: stdout differs"
      diff "$expected_out" "$TMP/run.out" | head -10 >&2 || true
      continue
    fi
    pass=$((pass + 1))
  done
done

for src in "$CASES"/negative/*.cx; do
  name="$(basename "$src" .cx)"
  set +e
  "$COMPILER" check "$src" >"$TMP/neg.out" 2>"$TMP/neg.err"
  rc=$?
  set -e
  if [ "$rc" -eq 0 ]; then
    report_fail "negative/$name: accepted invalid input"
  elif [ ! -s "$TMP/neg.err" ]; then
    report_fail "negative/$name: rejected without a diagnostic"
  elif [ -f "$CASES/negative/$name.code" ] && ! grep -qF "error[$(cat "$CASES/negative/$name.code")]" "$TMP/neg.err"; then
    report_fail "negative/$name: expected diagnostic code $(cat "$CASES/negative/$name.code")"
  else
    pass=$((pass + 1))
  fi
done

# Malformed inputs must be rejected with a diagnostic and a clean exit 1. A
# timeout or a signal (segfault/abort) is a failure, not a rejection.
for src in "$CASES"/malformed/*.cx; do
  [ -e "$src" ] || break
  name="$(basename "$src" .cx)"
  set +e
  timeout 20 "$COMPILER" check "$src" >"$TMP/mal.out" 2>"$TMP/mal.err"
  rc=$?
  set -e
  if [ "$rc" -eq 124 ]; then
    report_fail "malformed/$name: timed out"
  elif [ "$rc" -ne 1 ]; then
    report_fail "malformed/$name: exit $rc (expected a clean diagnostic, exit 1)"
  elif [ ! -s "$TMP/mal.err" ]; then
    report_fail "malformed/$name: rejected without a diagnostic"
  else
    pass=$((pass + 1))
  fi
done

# Multi-unit module projects: every directory under cases/modules is a
# `corex.toml` project. A `must_fail` marker means the project must be
# rejected by `check`; otherwise it is built and run at --opt 0..3.
for proj in "$CASES"/modules/*/; do
  name="$(basename "$proj")"
  if [ -f "$proj/must_fail" ]; then
    set +e
    "$COMPILER" check "$proj" >"$TMP/mod.out" 2>"$TMP/mod.err"
    rc=$?
    set -e
    if [ "$rc" -eq 0 ]; then
      report_fail "modules/$name: accepted an invalid project"
    elif [ ! -s "$TMP/mod.err" ]; then
      report_fail "modules/$name: rejected without a diagnostic"
    else
      pass=$((pass + 1))
    fi
    continue
  fi
  expected_out="$proj/expected.out"
  expected_rc="$proj/expected.rc"
  if [ ! -f "$expected_out" ] || [ ! -f "$expected_rc" ]; then
    report_fail "modules/$name: missing expected output"
    continue
  fi
  want_rc="$(cat "$expected_rc")"
  for opt in 0 1 2 3; do
    bin="$TMP/mod_$name.$opt"
    if ! "$COMPILER" build "$proj" -o "$bin" --opt "$opt" >"$TMP/build.out" 2>"$TMP/build.err"; then
      report_fail "modules/$name --opt $opt: build failed: $(head -1 "$TMP/build.err")"
      continue
    fi
    set +e
    "$bin" >"$TMP/run.out" 2>"$TMP/run.err"
    got_rc=$?
    set -e
    if [ "$got_rc" != "$want_rc" ]; then
      report_fail "modules/$name --opt $opt: exit $got_rc, expected $want_rc"
      continue
    fi
    if ! diff -q "$expected_out" "$TMP/run.out" >/dev/null; then
      report_fail "modules/$name --opt $opt: stdout differs"
      diff "$expected_out" "$TMP/run.out" | head -10 >&2 || true
      continue
    fi
    pass=$((pass + 1))
  done
done

# C interop: every directory under cases/ffi holds prog.cx, helper.c and
# expected output. The C helper is compiled with cc and linked with
# `--link`, exercising extern "C" declarations, variadics, aggregate passing
# and a C -> CoreX export in one program.
CC_BIN="${COREX_CC:-cc}"
for proj in "$CASES"/ffi/*/; do
  name="$(basename "$proj")"
  expected_out="$proj/expected.out"
  expected_rc="$proj/expected.rc"
  if [ ! -f "$expected_out" ] || [ ! -f "$expected_rc" ]; then
    report_fail "ffi/$name: missing expected output"
    continue
  fi
  if ! "$CC_BIN" -c -O2 -fno-builtin -fno-stack-protector -o "$TMP/helper.o" "$proj/helper.c" >"$TMP/cc.out" 2>"$TMP/cc.err"; then
    report_fail "ffi/$name: C helper failed to compile: $(head -1 "$TMP/cc.err")"
    continue
  fi
  want_rc="$(cat "$expected_rc")"
  for opt in 0 1 2 3; do
    bin="$TMP/ffi_$name.$opt"
    if ! "$COMPILER" build "$proj/prog.cx" -o "$bin" --opt "$opt" --link "$TMP/helper.o" >"$TMP/build.out" 2>"$TMP/build.err"; then
      report_fail "ffi/$name --opt $opt: build failed: $(head -1 "$TMP/build.err")"
      continue
    fi
    set +e
    "$bin" >"$TMP/run.out" 2>"$TMP/run.err"
    got_rc=$?
    set -e
    if [ "$got_rc" != "$want_rc" ]; then
      report_fail "ffi/$name --opt $opt: exit $got_rc, expected $want_rc"
      continue
    fi
    if ! diff -q "$expected_out" "$TMP/run.out" >/dev/null; then
      report_fail "ffi/$name --opt $opt: stdout differs"
      diff "$expected_out" "$TMP/run.out" | head -10 >&2 || true
      continue
    fi
    pass=$((pass + 1))
  done
done

echo "selfhosting tests: $pass passed, $fail failed (compiler: $COMPILER)"
[ "$fail" -eq 0 ]

#!/usr/bin/env bash
# CLI parity tests for the CoreX bootstrap compiler.
#
#   tests/selfhosting/cli.sh [compiler]
#
# Exercises commands and flags that the behavioural corpus does not cover:
# stdin input, `run`/`exec` status propagation, `emit-ir --verify`, `fmt`,
# `lint`, `doc`, the `test` harness, usage errors, and default output naming.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMPILER="${1:-$ROOT/build/stage2/corexc}"
case "$COMPILER" in
  /*) ;;
  *) COMPILER="$PWD/$COMPILER" ;;
esac
TMP="$(mktemp -d /tmp/corexc-cli.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

if [ ! -x "$COMPILER" ]; then
  echo "missing compiler: $COMPILER" >&2
  exit 2
fi

pass=0
fail=0
ok() { pass=$((pass + 1)); }
bad() { echo "FAIL: $*" >&2; fail=$((fail + 1)); }

expect_rc() {
  local want="$1" got="$2" what="$3"
  if [ "$got" = "$want" ]; then ok; else bad "$what: exit $got, expected $want"; fi
}

# --- run from stdin, status propagation ------------------------------------
set +e
out="$(printf 'fn main() -> i32 { return 7; }\n' | "$COMPILER" run -)"
rc=$?
set -e
expect_rc 7 "$rc" "run - status"
[ -z "$out" ] && ok || bad "run - printed unexpected output: $out"

set +e
out="$(printf 'fn _start() { unsafe { syscall(1, 1, &"stdin\\n" as usize, 6); syscall(60, 0, 0, 0, 0, 0, 0); } }\n' | "$COMPILER" run -)"
rc=$?
set -e
expect_rc 0 "$rc" "run - _start status"
[ "$out" = "stdin" ] && ok || bad "run - stdout: $out"

# --- exec ------------------------------------------------------------------
set +e
"$COMPILER" exec 'fn main() -> i32 { return 42; }' >/dev/null 2>&1
rc=$?
set -e
expect_rc 42 "$rc" "exec status"

set +e
"$COMPILER" exec 'fn main(argc: i32, argv: **u8, envp: **u8) -> i32 { return argc; }' -- a b >/dev/null 2>&1
rc=$?
set -e
expect_rc 3 "$rc" "exec -- program args (argc)"

# --- check reports frontend errors from stdin ------------------------------
set +e
err="$(printf 'fn main() -> i32 { return nope; }\n' | "$COMPILER" check - 2>&1 >/dev/null)"
rc=$?
set -e
expect_rc 1 "$rc" "check - error status"
case "$err" in
  *E0201*) ok ;;
  *) bad "check - stderr lacks E0201: $err" ;;
esac
case "$err" in
  *'<stdin>'*) ok ;;
  *) bad "check - stderr lacks <stdin>: $err" ;;
esac

# --- check success summary -------------------------------------------------
out="$("$COMPILER" check "$ROOT/examples/hello.cx")"
[ "$out" = "ok: 1 function(s), entry \`_start\`" ] && ok || bad "check summary: $out"

# --- emit-ir --verify ------------------------------------------------------
set +e
"$COMPILER" emit-ir "$ROOT/tests/selfhosting/cases/positive/enums.cx" --verify >"$TMP/ir.ll" 2>"$TMP/ir.err"
rc=$?
set -e
expect_rc 0 "$rc" "emit-ir --verify"
grep -q "IR verified by llvm-as" "$TMP/ir.err" && ok || bad "emit-ir --verify message"
grep -q "define " "$TMP/ir.ll" && ok || bad "emit-ir produced no definitions"

# --- ignored/rejected flags -----------------------------------------------
for spec in "check --opt 2" "emit-ir --opt 2"; do
  set +e
  err="$("$COMPILER" $spec "$ROOT/examples/hello.cx" 2>&1 >/dev/null)"
  rc=$?
  set -e
  expect_rc 2 "$rc" "$spec rejected"
  case "$err" in
    *'unknown option `--opt`'*) ok ;;
    *) bad "$spec: wrong message: $err" ;;
  esac
done

# --- fmt -------------------------------------------------------------------
printf 'fn f() {\nlet s = "a  b";\nif 1 == 1 {\nreturn 0;\n}\n}\n' >"$TMP/f.cx"
"$COMPILER" fmt "$TMP/f.cx" >"$TMP/f.fmt"
cat >"$TMP/f.want" <<'EOF'
fn f() {
    let s = "a  b";
    if 1 == 1 {
        return 0;
    }
}
EOF
diff -q "$TMP/f.want" "$TMP/f.fmt" >/dev/null && ok || bad "fmt output"
"$COMPILER" fmt "$TMP/f.want" >"$TMP/f2.fmt"
diff -q "$TMP/f.want" "$TMP/f2.fmt" >/dev/null && ok || bad "fmt is not idempotent"
set +e
"$COMPILER" fmt "$TMP/f.cx" --check >/dev/null 2>&1
rc=$?
set -e
expect_rc 1 "$rc" "fmt --check unformatted"
set +e
"$COMPILER" fmt "$TMP/f.want" --check >/dev/null 2>&1
rc=$?
set -e
expect_rc 0 "$rc" "fmt --check formatted"
"$COMPILER" fmt "$TMP/f.cx" -w
diff -q "$TMP/f.want" "$TMP/f.cx" >/dev/null && ok || bad "fmt -w"

# --- lint ------------------------------------------------------------------
cat >"$TMP/lint.cx" <<'EOF'
fn helper() -> i32 { return 1; }
fn main() -> i32 {
    let unused = 5;
    if true { print(1); }
    return 0;
    print(2);
}
EOF
set +e
err="$("$COMPILER" lint "$TMP/lint.cx" 2>&1 >/dev/null)"
rc=$?
set -e
expect_rc 0 "$rc" "lint warnings are not errors"
for code in W0001 W0002 W0003 W0004; do
  case "$err" in
    *"$code"*) ok ;;
    *) bad "lint missing $code: $err" ;;
  esac
done
set +e
"$COMPILER" lint "$TMP/lint.cx" --deny-warnings >/dev/null 2>&1
rc=$?
set -e
expect_rc 1 "$rc" "lint --deny-warnings"

# --- doc -------------------------------------------------------------------
cat >"$TMP/doc.cx" <<'EOF'
/// Adds values.
fn add(a: i32, b: i32) -> i32 { return a + b; }
EOF
"$COMPILER" doc "$TMP/doc.cx" >"$TMP/doc.md"
grep -q '### `fn add(a: i32, b: i32) -> i32`' "$TMP/doc.md" && ok || bad "doc signature"
grep -q "Adds values." "$TMP/doc.md" && ok || bad "doc comment body"
"$COMPILER" doc "$TMP/doc.cx" -o "$TMP/doc2.md"
diff -q "$TMP/doc.md" "$TMP/doc2.md" >/dev/null && ok || bad "doc -o"

# --- test harness ----------------------------------------------------------
cat >"$TMP/t.cx" <<'EOF'
fn test_pass() -> bool { return 1 + 1 == 2; }
fn test_fail() -> bool { return 1 + 1 == 3; }
EOF
set +e
out="$("$COMPILER" test "$TMP/t.cx" 2>&1)"
rc=$?
set -e
expect_rc 1 "$rc" "test failure status"
case "$out" in
  *"test test_pass ... ok"*) ok ;;
  *) bad "test pass line missing: $out" ;;
esac
case "$out" in
  *"test test_fail ... FAILED"*) ok ;;
  *) bad "test failure line missing: $out" ;;
esac
printf 'fn main() -> i32 { return 0; }\n' >"$TMP/notests.cx"
set +e
"$COMPILER" test "$TMP/notests.cx" >/dev/null 2>&1
rc=$?
set -e
expect_rc 1 "$rc" "test with no tests"

# --- build default output name --------------------------------------------
mkdir -p "$TMP/out"
( cd "$TMP/out" && "$COMPILER" build "$ROOT/tests/selfhosting/cases/positive/arith.cx" >/dev/null )
[ -x "$TMP/out/arith" ] && ok || bad "build default output is not ./arith"
set +e
"$TMP/out/arith" >/dev/null 2>&1
rc=$?
set -e
expect_rc "$(cat "$ROOT/tests/selfhosting/cases/positive/arith.rc")" "$rc" "default output runs"

# --- usage errors ----------------------------------------------------------
set +e
"$COMPILER" >/dev/null 2>&1
rc=$?
set -e
expect_rc 2 "$rc" "no command"
set +e
"$COMPILER" bogus x.cx >/dev/null 2>&1
rc=$?
set -e
expect_rc 2 "$rc" "unknown command"
set +e
"$COMPILER" fmt >/dev/null 2>&1
rc=$?
set -e
expect_rc 2 "$rc" "fmt without file"
set +e
"$COMPILER" check /nonexistent.cx >/dev/null 2>&1
rc=$?
set -e
expect_rc 2 "$rc" "missing input file"

echo "cli tests: $pass passed, $fail failed (compiler: $COMPILER)"
[ "$fail" -eq 0 ]

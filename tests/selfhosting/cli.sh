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

# --- link/emit flags (SH-036) ---------------------------------------------
cat >"$TMP/lib.cx" <<'CXEOF'
pub fn add(a: i32, b: i32) -> i32 { return a + b; }
CXEOF

# --emit obj requires an entry point (same rule as the Rust CLI).
printf 'fn main() -> i32 { return 0; }\n' >"$TMP/entry.cx"
"$COMPILER" build "$TMP/entry.cx" --emit obj -o "$TMP/entry.o" >/dev/null
if file "$TMP/entry.o" | grep -q "relocatable"; then ok; else bad "--emit obj is not relocatable"; fi
set +e
"$COMPILER" build "$TMP/lib.cx" --emit obj -o "$TMP/noentry.o" >/dev/null 2>&1
rc=$?
set -e
expect_rc 1 "$rc" "--emit obj without entry point"

# --emit staticlib: no entry point needed; archive holds one object.
"$COMPILER" build "$TMP/lib.cx" --emit staticlib -o "$TMP/libadd.a" >/dev/null
[ "$(llvm-ar t "$TMP/libadd.a" | wc -l)" = "1" ] && ok || bad "--emit staticlib member count"

# --emit shared: no entry point needed; exported symbol is callable from C.
"$COMPILER" build "$TMP/lib.cx" --emit shared -o "$TMP/libadd.so" >/dev/null
if file "$TMP/libadd.so" | grep -q "shared object"; then ok; else bad "--emit shared is not a shared object"; fi
if nm -D "$TMP/libadd.so" | grep -q " T add"; then ok; else bad "--emit shared does not export add"; fi

cat >"$TMP/host.c" <<'CEOF'
#include <stdio.h>
extern int add(int, int);
int main(void) { printf("%d\n", add(20, 22)); return 0; }
CEOF
if cc "$TMP/host.c" "$TMP/libadd.a" -o "$TMP/host" 2>/dev/null; then
  out="$("$TMP/host")"
  [ "$out" = "42" ] && ok || bad "staticlib link output: $out"
else
  bad "cc could not link against libadd.a"
fi
# The shared object loads and the symbol is callable through the dynamic loader.
if command -v python3 >/dev/null 2>&1; then
  out="$(cd "$TMP" && python3 -c 'import ctypes; print(ctypes.CDLL("./libadd.so").add(19, 23))' 2>/dev/null)"
  [ "$out" = "42" ] && ok || bad "shared object call returned: $out"
fi

# --link / -L / -l against a C object.
cat >"$TMP/twice.c" <<'CEOF'
int twice(int x) { return x * 2; }
CEOF
cc -c "$TMP/twice.c" -o "$TMP/twice.o"
cat >"$TMP/usetwice.cx" <<'CXEOF'
extern "C" fn twice(x: i32) -> i32;
fn main() -> i32 { return twice(21); }
CXEOF
"$COMPILER" build "$TMP/usetwice.cx" --link "$TMP/twice.o" -o "$TMP/usetwice" >/dev/null
set +e
"$TMP/usetwice" >/dev/null 2>&1
rc=$?
set -e
expect_rc 42 "$rc" "--link object"
ar rcs "$TMP/libtwice.a" "$TMP/twice.o"
"$COMPILER" build "$TMP/usetwice.cx" -L "$TMP" -l twice -o "$TMP/usetwice2" >/dev/null
set +e
"$TMP/usetwice2" >/dev/null 2>&1
rc=$?
set -e
expect_rc 42 "$rc" "-L/-l static library"

# --hosted links against the C runtime and keeps main as the entry point.
printf 'fn main() -> i32 { return 3; }\n' >"$TMP/hostmain.cx"
"$COMPILER" build "$TMP/hostmain.cx" --hosted -o "$TMP/hostmain" >/dev/null
file "$TMP/hostmain" | grep -q "dynamically linked" && ok || bad "--hosted is not dynamically linked"
set +e
"$TMP/hostmain" >/dev/null 2>&1
rc=$?
set -e
expect_rc 3 "$rc" "--hosted program"

# --sanitize traps on signed integer overflow; the default wraps.
cat >"$TMP/ovf.cx" <<'CXEOF'
fn main() -> i32 {
    let a: i32 = 2147483647;
    let b: i32 = a + 1;
    return b;
}
CXEOF
"$COMPILER" build "$TMP/ovf.cx" --sanitize -o "$TMP/ovf" >/dev/null
set +e
msg="$("$TMP/ovf" 2>&1)"
rc=$?
set -e
expect_rc 101 "$rc" "--sanitize overflow trap"
case "$msg" in
  *"integer overflow"*) ok ;;
  *) bad "--sanitize message: $msg" ;;
esac
"$COMPILER" build "$TMP/ovf.cx" -o "$TMP/ovf0" >/dev/null
set +e
"$TMP/ovf0" >/dev/null 2>&1
rc=$?
set -e
expect_rc 0 "$rc" "default wrapping arithmetic"

# -I searches include directories for `import`.
mkdir -p "$TMP/inc"
printf 'fn triple(x: i32) -> i32 { return x * 3; }\n' >"$TMP/inc/util.cx"
cat >"$TMP/useimp.cx" <<'CXEOF'
import "util.cx";
fn main() -> i32 { return triple(14); }
CXEOF
"$COMPILER" build "$TMP/useimp.cx" -I "$TMP/inc" -o "$TMP/useimp" >/dev/null
set +e
"$TMP/useimp" >/dev/null 2>&1
rc=$?
set -e
expect_rc 42 "$rc" "-I import search path"
set +e
"$COMPILER" check "$TMP/useimp.cx" >/dev/null 2>&1
rc=$?
set -e
expect_rc 1 "$rc" "import without -I fails"

# --cfg is accepted on link commands too (and gates `#[cfg]`).
"$COMPILER" build "$TMP/hostmain.cx" --cfg feature=fast -o "$TMP/cfgmain" >/dev/null
set +e
"$TMP/cfgmain" >/dev/null 2>&1
rc=$?
set -e
expect_rc 3 "$rc" "--cfg accepted"

# -g emits DWARF (a compile unit with subprogram DIEs and line tables).
"$COMPILER" build "$TMP/hostmain.cx" -g -o "$TMP/dbgmain" >/dev/null
file "$TMP/dbgmain" | grep -q ELF && ok || bad "-g build is not an ELF"
if command -v readelf >/dev/null 2>&1; then
  if readelf --debug-dump=info "$TMP/dbgmain" 2>/dev/null | grep -q DW_TAG_subprogram; then
    ok
  else
    bad "-g did not emit DW_TAG_subprogram"
  fi
fi

# Unknown --emit values are usage errors.
set +e
"$COMPILER" build "$TMP/hostmain.cx" --emit elf -o "$TMP/x" >/dev/null 2>&1
rc=$?
set -e
expect_rc 2 "$rc" "unknown --emit value"

# --shared link flag on a program with an entry point.
"$COMPILER" build "$TMP/hostmain.cx" --shared -o "$TMP/hshared.so" >/dev/null
file "$TMP/hshared.so" | grep -q "shared object" && ok || bad "--shared is not a shared object"

# --- build failures propagate a nonzero status ----------------------------
# A link failure (unwritable output path) must exit 3 and must not claim
# success. Regression for the CLI contract.
set +e
out="$("$COMPILER" build "$TMP/hostmain.cx" -o "$TMP/missing-dir/out" 2>"$TMP/badbuild.err")"
rc=$?
set -e
expect_rc 3 "$rc" "build link failure status"
case "$out" in
  *compiled*) bad "build failure printed 'compiled': $out" ;;
  *) ok ;;
esac
[ -e "$TMP/missing-dir/out" ] && bad "build failure left an output file" || ok

# A tool killed by a signal is a failure, not exit status 0.
cat >"$TMP/fakecc.sh" <<'CCEOF'
#!/bin/sh
kill -SEGV $$
CCEOF
chmod +x "$TMP/fakecc.sh"
set +e
out="$(COREX_CC="$TMP/fakecc.sh" "$COMPILER" build "$TMP/hostmain.cx" -o "$TMP/sigout" 2>"$TMP/sig.err")"
rc=$?
set -e
expect_rc 3 "$rc" "signal-killed tool status"
case "$out" in
  *compiled*) bad "signal-killed tool printed 'compiled': $out" ;;
  *) ok ;;
esac
[ -e "$TMP/sigout" ] && bad "signal-killed tool left an output file" || ok

# --- cfg parity: built-in target keys and frontend options ----------------
cat >"$TMP/cfgbranch.cx" <<'CXEOF'
#[cfg(target_os = "linux")]
fn main() -> i32 { return 1; }
#[cfg(not(target_os = "linux"))]
fn main() -> i32 { return 2; }
CXEOF
set +e
"$COMPILER" run "$TMP/cfgbranch.cx" >/dev/null 2>&1
rc=$?
set -e
expect_rc 1 "$rc" "built-in cfg target_os selects linux"

# `check` and `emit-ir` accept frontend options, including import paths.
"$COMPILER" check "$TMP/hostmain.cx" --cfg feature=fast >/dev/null
ok
"$COMPILER" check "$TMP/useimp.cx" -I "$TMP/inc" >/dev/null
ok
"$COMPILER" emit-ir "$TMP/hostmain.cx" --cfg feature=fast >/dev/null
ok
"$COMPILER" emit-ir "$TMP/useimp.cx" -I "$TMP/inc" >/dev/null
ok

# stdin parsing receives --cfg and --sanitize; build from stdin also works.
cat >"$TMP/stdincfg.cx" <<'CXEOF'
#[cfg(feature = "fast")]
fn main() -> i32 { return 1; }
#[cfg(not(feature = "fast"))]
fn main() -> i32 { return 2; }
CXEOF
"$COMPILER" build - --cfg feature=fast -o "$TMP/stdincfg" <"$TMP/stdincfg.cx" >/dev/null
set +e
"$TMP/stdincfg" >/dev/null 2>&1
rc=$?
set -e
expect_rc 1 "$rc" "stdin build honours --cfg"

cat >"$TMP/stdinsan.cx" <<'CXEOF'
#[cfg(sanitize = "true")]
fn main() -> i32 { return 4; }
#[cfg(not(sanitize = "true"))]
fn main() -> i32 { return 5; }
CXEOF
"$COMPILER" build - --sanitize -o "$TMP/stdinsan" <"$TMP/stdinsan.cx" >/dev/null
set +e
"$TMP/stdinsan" >/dev/null 2>&1
rc=$?
set -e
expect_rc 4 "$rc" "stdin build honours --sanitize cfg"

# --- labels inside unreachable regions ------------------------------------
# The label is not the first statement of its block/if/loop/switch arm, so
# the whole dead region must be scanned and emitted.
cat >"$TMP/deadlabel.cx" <<'CXEOF'
fn block_label() -> i32 {
    goto done;
    {
        let x: i32 = 1;
        done: return 7;
    }
}

fn if_label() -> i32 {
    goto done;
    if 1 == 1 {
        let x: i32 = 2;
        done: return 8;
    }
    return 0;
}

fn loop_label() -> i32 {
    goto done;
    while false {
        let x: i32 = 3;
        done: return 9;
    }
    return 0;
}

fn switch_label() -> i32 {
    goto done;
    switch 1 {
        case 1 {
            let x: i32 = 4;
            done: return 10;
        }
        default { return 0; }
    }
    return 0;
}

fn main() -> i32 {
    if block_label() != 7 { return 1; }
    if if_label() != 8 { return 2; }
    if loop_label() != 9 { return 3; }
    if switch_label() != 10 { return 4; }
    return 0;
}
CXEOF
set +e
"$COMPILER" emit-ir "$TMP/deadlabel.cx" --verify >"$TMP/deadlabel.ll" 2>"$TMP/deadlabel.err"
rc=$?
set -e
expect_rc 0 "$rc" "goto into unreachable label verifies"
grep -q "IR verified by llvm-as" "$TMP/deadlabel.err" && ok || bad "dead-label IR was not verified"
set +e
"$COMPILER" run "$TMP/deadlabel.cx" >/dev/null 2>&1
rc=$?
set -e
expect_rc 0 "$rc" "goto into unreachable label executes"

# Float `%=` lowers to frem; it needs libm, so exercise it hosted.
cat >"$TMP/fmod.cx" <<'CXEOF'
fn main() -> i32 {
    let mut f: f64 = 5.0;
    f %= 1.5;
    let mut g: f32 = 5.0;
    g %= 1.5;
    print(f);
    print(g);
    return 0;
}
CXEOF
out="$(set +e; "$COMPILER" run "$TMP/fmod.cx" --hosted -l m 2>"$TMP/fmod.err")"
rc=$?
expect_rc 0 "$rc" "float %= hosted"
[ "$out" = "$(printf '0.500000\n0.500000')" ] && ok || bad "float %= output: $out"

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

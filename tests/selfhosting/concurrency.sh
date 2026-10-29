#!/usr/bin/env bash
# Concurrency parity: atomics, volatile globals, inline asm, TLS, threads.
#
#   tests/selfhosting/concurrency.sh [compiler]
#
# Runs the behavioural coverage of compiler/tests/concurrency.rs against the
# bootstrap compiler and the Rust reference compiler. Every program is built
# with the bootstrap compiler at --opt 0..3 (the bootstrap additionally runs
# LLVM `opt`, which is how the early-clobber inline-asm bug was found) and at
# --opt 2 with the reference compiler, then compared byte-for-byte.
#
# Negative diagnostics are also checked on both compilers: unsafe gating,
# wrong operand types, aggregate pointees, and malformed `asm` calls.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMPILER="${1:-$ROOT/build/stage2/corexc}"
STAGE0="${COREX_STAGE0:-$ROOT/compiler/target/release/corex}"
LIB="$ROOT/lib/corex"
CASES="$ROOT/tests/selfhosting/cases"
TMP="$(mktemp -d /tmp/corexc-concurrency.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

for cc in "$COMPILER" "$STAGE0"; do
  case "$cc" in
    /*) ;;
    *) cc="$PWD/$cc" ;;
  esac
  [ -x "$cc" ] || { echo "missing compiler: $cc" >&2; exit 2; }
done
[ -d "$LIB" ] || { echo "missing library: $LIB" >&2; exit 2; }

pass=0
fail=0
bad() { echo "FAIL: $*" >&2; fail=$((fail + 1)); }

run_one() {
  local label="$1" bin="$2"
  set +e
  timeout 60 "$bin" >"$TMP/out" 2>"$TMP/err"
  local rc=$?
  set -e
  if [ "$rc" -ne 0 ]; then
    bad "$label: exit $rc (expected 0): $(head -1 "$TMP/err")"
  elif ! diff -q "$TMP/expected" "$TMP/out" >/dev/null; then
    bad "$label: stdout differs"
    diff "$TMP/expected" "$TMP/out" | head -10 >&2 || true
  else
    pass=$((pass + 1))
  fi
}

# --- atomics / volatile / inline asm ---------------------------------------
cp "$CASES/positive/concurrency.cx" "$TMP/atomics.cx"
cp "$CASES/positive/concurrency.out" "$TMP/expected"
for opt in 0 1 2 3; do
  if "$COMPILER" build "$TMP/atomics.cx" -o "$TMP/atomics_$opt" --opt "$opt" \
      -I "$LIB" >"$TMP/build.err" 2>&1; then
    run_one "atomics bootstrap --opt $opt" "$TMP/atomics_$opt"
  else
    bad "atomics bootstrap --opt $opt: build failed: $(head -1 "$TMP/build.err")"
  fi
done
if "$STAGE0" build "$TMP/atomics.cx" -o "$TMP/atomics_oracle" --opt 2 \
    -I "$LIB" >"$TMP/build.err" 2>&1; then
  run_one "atomics oracle --opt 2" "$TMP/atomics_oracle"
else
  bad "atomics oracle: build failed: $(head -1 "$TMP/build.err")"
fi

# --- explicit TLS ----------------------------------------------------------
cat >"$TMP/tls.cx" <<'EOF'
mod std_tls;
fn main() -> i32 {
    let a: *mut u8 = std_tls::tls_alloc(16);
    let b: *mut u8 = std_tls::tls_alloc(16);
    print(a != null);
    print(a == b);
    unsafe { *(a + 0) = 42; }
    let c: *mut u8 = std_tls::tls_get();
    unsafe { print(*(c + 0) as i32); }
    std_tls::tls_release(a);
    print(std_tls::tls_thread_id() > 0);
    return 0;
}
EOF
printf 'true\ntrue\n42\ntrue\n' >"$TMP/expected"
for opt in 0 1 2 3; do
  if "$COMPILER" build "$TMP/tls.cx" -o "$TMP/tls_$opt" --opt "$opt" \
      -I "$LIB" >"$TMP/build.err" 2>&1; then
    run_one "tls bootstrap --opt $opt" "$TMP/tls_$opt"
  else
    bad "tls bootstrap --opt $opt: build failed: $(head -1 "$TMP/build.err")"
  fi
done
if "$STAGE0" build "$TMP/tls.cx" -o "$TMP/tls_oracle" --opt 2 \
    -I "$LIB" >"$TMP/build.err" 2>&1; then
  run_one "tls oracle --opt 2" "$TMP/tls_oracle"
else
  bad "tls oracle: build failed: $(head -1 "$TMP/build.err")"
fi

# --- threads and the futex mutex ------------------------------------------
cat >"$TMP/threads.cx" <<'EOF'
mod std_thread;
mod std_sync;

static mut COUNTER: i64 = 0;

fn worker(arg: i64) -> i32 {
    let mut m: *mut std_sync::Mutex = null;
    unsafe { m = arg as *mut std_sync::Mutex; }
    let mut i: i64 = 0;
    while i < 5000 {
        unsafe {
            std_sync::mutex_lock(m);
            COUNTER += 1;
            std_sync::mutex_unlock(m);
        }
        i += 1;
    }
    return 0;
}

fn main() -> i32 {
    let mut m: std_sync::Mutex = std_sync::mutex_new();
    let mut mp: i64 = 0;
    unsafe { mp = &mut m as *mut std_sync::Mutex as i64; }
    let t1: std_thread::Thread = std_thread::spawn(&worker, mp, 65536);
    let t2: std_thread::Thread = std_thread::spawn(&worker, mp, 65536);
    print(t1.tid > 0);
    print(t2.tid > 0);
    while true {
        unsafe {
            if atomic_load(&COUNTER) == 10000 {
                break;
            }
        }
        std_thread::yield_now();
    }
    print(COUNTER);
    print(std_thread::gettid() > 0);
    return 0;
}
EOF
printf 'true\ntrue\n10000\ntrue\n' >"$TMP/expected"
for opt in 0 1 2 3; do
  if "$COMPILER" build "$TMP/threads.cx" -o "$TMP/threads_$opt" --opt "$opt" \
      -I "$LIB" >"$TMP/build.err" 2>&1; then
    run_one "threads bootstrap --opt $opt" "$TMP/threads_$opt"
  else
    bad "threads bootstrap --opt $opt: build failed: $(head -1 "$TMP/build.err")"
  fi
done
if "$STAGE0" build "$TMP/threads.cx" -o "$TMP/threads_oracle" --opt 2 \
    -I "$LIB" >"$TMP/build.err" 2>&1; then
  run_one "threads oracle --opt 2" "$TMP/threads_oracle"
else
  bad "threads oracle: build failed: $(head -1 "$TMP/build.err")"
fi

# --- negative diagnostics --------------------------------------------------
check_negative() {
  local name="$1" code="$2" cc label
  local src="$CASES/negative/$name.cx"
  [ -f "$src" ] || { bad "negative/$name: missing case"; return; }
  for cc in "$COMPILER" "$STAGE0"; do
    if [ "$cc" = "$COMPILER" ]; then label="bootstrap"; else label="oracle"; fi
    set +e
    "$cc" check "$src" >"$TMP/neg.out" 2>"$TMP/neg.err"
    local rc=$?
    set -e
    if [ "$rc" -ne 1 ]; then
      bad "negative/$name $label: exit $rc (expected 1)"
    elif ! grep -qF "error[$code]" "$TMP/neg.err"; then
      bad "negative/$name $label: expected error[$code], got $(head -1 "$TMP/neg.err")"
    else
      pass=$((pass + 1))
    fi
  done
}

check_negative atomic_unsafe E0209
check_negative asm_unsafe E0209
check_negative asm_arity E0204
check_negative asm_template E0203
check_negative asm_operand E0216
check_negative atomic_pointer E0203
check_negative atomic_operand_type E0203
check_negative atomic_aggregate E0203
check_negative atomic_arity E0204

echo "concurrency tests: $pass passed, $fail failed (compiler: $COMPILER)"
[ "$fail" -eq 0 ]

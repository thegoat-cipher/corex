#!/usr/bin/env bash
# Malformed-input robustness: no signals, no hangs, no sanitizer reports.
#
#   tests/selfhosting/fuzz.sh [compiler]
#
# Deterministic mutation fuzzing (fixed seed) over the positive corpus plus a
# checked-in malformed corpus. When `cc -fsanitize=address` is available the
# compiler is also rebuilt hosted+ASan and the same inputs run under it.
#
# The compiler allocates from a process-lifetime arena (mmap); there is no
# free path by design. Leak checking is therefore off, and "no UAF/overflow
# beyond the arena" means: no ASan report, no signal, exit code in {0,1,2}.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMPILER="${1:-$ROOT/build/stage2/corexc}"
case "$COMPILER" in
  /*) ;;
  *) COMPILER="$PWD/$COMPILER" ;;
esac
CASES="$ROOT/tests/selfhosting/cases"
TMP="$(mktemp -d /tmp/corexc-fuzz.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

[ -x "$COMPILER" ] || { echo "missing compiler: $COMPILER" >&2; exit 2; }

pass=0
fail=0
bad() { echo "FAIL: $*" >&2; fail=$((fail + 1)); }

# One input: must terminate with 0, 1, or 2, without a signal.
run_one() {
  local label="$1" src="$2"
  set +e
  timeout 10 "$COMPILER" check "$src" >"$TMP/out" 2>"$TMP/err"
  local rc=$?
  set -e
  if [ "$rc" -eq 124 ]; then
    bad "$label: timed out"
  elif [ "$rc" -ge 128 ]; then
    bad "$label: crashed (signal, rc=$rc): $(head -1 "$TMP/err")"
  elif [ "$rc" -ne 0 ] && [ "$rc" -ne 1 ] && [ "$rc" -ne 2 ]; then
    bad "$label: unexpected exit $rc"
  elif [ "$rc" -eq 1 ] && [ ! -s "$TMP/err" ]; then
    bad "$label: rejected without a diagnostic"
  else
    pass=$((pass + 1))
  fi
  if grep -q "AddressSanitizer\|runtime error" "$TMP/err"; then
    bad "$label: sanitizer report"
    head -8 "$TMP/err" >&2
  fi
}

# --- checked-in malformed corpus -------------------------------------------
for src in "$CASES"/fuzz/*.cx; do
  run_one "fuzz/$(basename "$src")" "$src"
done
printf 'fn main() -> i32 { let s: str = "a\0b"; return 0; }\n' >"$TMP/nul.cx"
run_one "nul-byte" "$TMP/nul.cx"
printf 'fn main() -> i32 { return 0; }\n\377\376\x01' >"$TMP/badbytes.cx"
run_one "non-utf8-tail" "$TMP/badbytes.cx"

# --- deterministic mutation fuzzing ----------------------------------------
if command -v python3 >/dev/null 2>&1; then
  python3 - "$CASES/positive" "$TMP/mut" <<'PY'
import os, random, sys

seeds_dir, out_dir = sys.argv[1], sys.argv[2]
os.makedirs(out_dir, exist_ok=True)
seeds = [os.path.join(seeds_dir, n) for n in sorted(os.listdir(seeds_dir)) if n.endswith(".cx")]
rng = random.Random(1234)
alphabet = b'(){}[];"\'/*\\\n\t@#.,:=+-<>&|^%!?0123456789abcXYZ'

def mutate(data: bytes) -> bytes:
    op = rng.randrange(6)
    if not data:
        return bytes(rng.choice(alphabet) for _ in range(rng.randrange(0, 40)))
    if op == 0:  # truncate
        return data[: rng.randrange(0, len(data) + 1)]
    if op == 1:  # flip one byte
        i = rng.randrange(len(data))
        return data[:i] + bytes([rng.randrange(256)]) + data[i + 1:]
    if op == 2:  # delete a slice
        a = rng.randrange(len(data)); b = min(len(data), a + rng.randrange(1, 40))
        return data[:a] + data[b:]
    if op == 3:  # duplicate a slice
        a = rng.randrange(len(data)); b = min(len(data), a + rng.randrange(1, 80))
        return data[:b] + data[a:b] + data[b:]
    if op == 4:  # insert random bytes
        i = rng.randrange(len(data) + 1)
        ins = bytes(rng.choice(alphabet) for _ in range(rng.randrange(1, 20)))
        return data[:i] + ins + data[i:]
    # splice two seeds
    other = open(rng.choice(seeds), "rb").read()
    if not other:
        return data
    a = rng.randrange(len(data)); b = rng.randrange(len(other))
    return data[:a] + other[b:]

count = 200
for n in range(count):
    seed = rng.choice(seeds)
    data = open(seed, "rb").read()
    # Mutate up to three times for deeper damage.
    for _ in range(rng.randrange(1, 4)):
        data = mutate(data)
    with open(os.path.join(out_dir, f"mut_{n:03}.cx"), "wb") as f:
        f.write(data)
print(count)
PY
  mutants=0
  for src in "$TMP"/mut/*.cx; do
    run_one "mut/$(basename "$src")" "$src"
    mutants=$((mutants + 1))
  done
  echo "mutation inputs: $mutants"
else
  echo "python3 not found: skipping mutation fuzzing" >&2
fi

# --- ASan build (optional) --------------------------------------------------
asan_ok=0
if command -v cc >/dev/null 2>&1; then
  if "$COMPILER" build "$ROOT/selfhost/corexc.cx" --hosted --emit obj -o "$TMP/hosted.o" >/dev/null 2>&1 \
     && cc -fsanitize=address -no-pie "$TMP/hosted.o" -o "$TMP/corexc_asan" >/dev/null 2>&1; then
    asan_ok=1
  fi
fi

if [ "$asan_ok" -eq 1 ]; then
  export ASAN_OPTIONS=detect_leaks=0:abort_on_error=1
  asan_run() {
    local label="$1"; shift
    set +e
    timeout 30 "$TMP/corexc_asan" "$@" >"$TMP/aout" 2>"$TMP/aerr"
    local rc=$?
    set -e
    if [ "$rc" -ge 128 ] || grep -q "AddressSanitizer\|runtime error" "$TMP/aerr"; then
      bad "$label: sanitizer failure (rc=$rc)"
      head -12 "$TMP/aerr" >&2
    else
      pass=$((pass + 1))
    fi
  }
  asan_run "asan:check-selfhost" check "$ROOT/selfhost/corexc.cx"
  asan_run "asan:emit-ir-selfhost" emit-ir "$ROOT/selfhost/corexc.cx"
  for src in "$CASES"/positive/*.cx; do
    asan_run "asan:check/$(basename "$src")" check "$src"
  done
  for src in "$CASES"/fuzz/*.cx; do
    asan_run "asan:fuzz/$(basename "$src")" check "$src"
  done
  n=0
  for src in "$TMP"/mut/*.cx; do
    [ "$n" -ge 60 ] && break
    asan_run "asan:mut/$(basename "$src")" check "$src"
    n=$((n + 1))
  done
  echo "ASan: enabled (${n} mutated inputs + corpus)"
else
  echo "ASan: unavailable (could not link with -fsanitize=address); skipped" >&2
fi

echo "fuzz tests: $pass passed, $fail failed (compiler: $COMPILER)"
[ "$fail" -eq 0 ]

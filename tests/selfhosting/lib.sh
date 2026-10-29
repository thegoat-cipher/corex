#!/usr/bin/env bash
# lib/corex build matrix: every module must compile with both compilers.
#
#   tests/selfhosting/lib.sh [compiler]
#
# Each module is built through a one-line program that names it with `mod`.
# `std_dl` needs the hosted C runtime and libdl, matching its documented
# requirement. The Rust reference compiler is the control: a module passes only
# when both compilers produce a binary.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMPILER="${1:-$ROOT/build/stage2/corexc}"
STAGE0="${COREX_STAGE0:-$ROOT/compiler/target/release/corex}"
LIB="$ROOT/lib/corex"
TMP="$(mktemp -d /tmp/corexc-lib.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

for cc in "$COMPILER" "$STAGE0"; do
  case "$cc" in
    /*) ;;
    *) cc="$PWD/$cc" ;;
  esac
  [ -x "$cc" ] || { echo "missing compiler: $cc" >&2; exit 2; }
done

modules="std_alloc std_builder std_dl std_env std_fmt std_fs std_io std_map
  std_math std_mem std_process std_str std_sync std_thread std_time std_tls"

pass=0
fail=0
bad() { echo "FAIL: $*" >&2; fail=$((fail + 1)); }

for m in $modules; do
  mkdir -p "$TMP/$m"
  printf 'mod %s;\nfn main() -> i32 { return 0; }\n' "$m" >"$TMP/$m/main.cx"
  extra=""
  case "$m" in
    std_dl) extra="--hosted -l dl" ;;
  esac
  ok=1
  for cc in "$COMPILER" "$STAGE0"; do
    if [ "$cc" = "$COMPILER" ]; then label="bootstrap"; else label="oracle"; fi
    # shellcheck disable=SC2086
    if ! "$cc" build "$TMP/$m/main.cx" -o "$TMP/$m/$label" --opt 0 \
        -I "$LIB" $extra >"$TMP/build.err" 2>&1; then
      bad "$m $label: $(head -1 "$TMP/build.err")"
      ok=0
    fi
  done
  if [ "$ok" -eq 1 ]; then
    pass=$((pass + 1))
  fi
done

echo "lib tests: $pass passed, $fail failed (compiler: $COMPILER)"
[ "$fail" -eq 0 ]

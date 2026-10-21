#!/usr/bin/env bash
# Microbenchmarks: CoreX vs C.
#
# Usage: scripts/bench.sh [runs]
#
# Kernels:
#   mix  - data-dependent 64-bit integer loop, N read from stdin (1e9 iterations)
#   fib  - recursive fib(35), call-heavy, not foldable
#
# Requires: compiler built (`cargo build --release` in compiler/), llvm-as, llc,
# opt, cc, and gcc. Prints best and median wall time in milliseconds.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUNS="${1:-5}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

COREX="$ROOT/compiler/target/release/corex"
if [[ ! -x "$COREX" ]]; then
    COREX="$ROOT/compiler/target/debug/corex"
fi
if [[ ! -x "$COREX" ]]; then
    echo "error: build the compiler first: (cd compiler && cargo build --release)" >&2
    exit 2
fi

echo "compiler: $("$COREX" --version)"
echo "rustc:    $(rustc --version)"
echo "llvm:     $(llc --version | head -1)"
echo "cc:       $(cc --version | head -1)"
echo "cpu:      $(grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//')"
echo "runs:     $RUNS"
echo

printf '1000000000\n' > "$WORK/n.txt"

"$COREX" build "$ROOT/bench/mix.cx" -o "$WORK/mix.cx.bin" --opt 2 >/dev/null
cc -O2 -o "$WORK/mix.c.bin" "$ROOT/bench/mix.c"

"$COREX" build "$ROOT/bench/fib.cx" -o "$WORK/fib.cx.bin" --opt 2 >/dev/null
cc -O2 -o "$WORK/fib.c.bin" "$ROOT/bench/fib.c"

measure() {
    local bin="$1" input="${2:-}"
    local times=()
    for _ in $(seq "$RUNS"); do
        local start end
        start=$(date +%s%N)
        if [[ -n "$input" ]]; then
            "$bin" < "$input" || true
        else
            "$bin" || true
        fi
        end=$(date +%s%N)
        times+=($(( (end - start) / 1000000 )))
    done
    printf '%s\n' "${times[@]}" | sort -n | awk '
        { a[NR]=$1 }
        END {
            best=a[1];
            median = (NR % 2) ? a[(NR+1)/2] : int((a[NR/2]+a[NR/2+1])/2);
            printf "%d %d\n", best, median
        }'
}

compare() {
    local name="$1" input="$2" cx_bin="$3" c_bin="$4"
    local cx_status=0 c_status=0
    if [[ -n "$input" ]]; then
        "$cx_bin" < "$input" || cx_status=$?
        "$c_bin" < "$input" || c_status=$?
    else
        "$cx_bin" || cx_status=$?
        "$c_bin" || c_status=$?
    fi
    if [[ "$cx_status" != "$c_status" ]]; then
        echo "error: $name result mismatch (CoreX=$cx_status, C=$c_status)" >&2
        exit 1
    fi

    read -r cx_best cx_median <<<"$(measure "$cx_bin" "$input")"
    read -r c_best c_median <<<"$(measure "$c_bin" "$input")"

    printf '%s (exit status %s, both agree)\n' "$name" "$cx_status"
    printf '  %-10s %10s %12s\n' "binary" "best (ms)" "median (ms)"
    printf '  %-10s %10s %12s\n' "CoreX -O2" "$cx_best" "$cx_median"
    printf '  %-10s %10s %12s\n' "C gcc -O2" "$c_best" "$c_median"
    awk -v a="$cx_median" -v b="$c_median" 'BEGIN {
        printf "\n  ratio (median, CoreX/C): %.3f\n\n", (b == 0) ? 0 : a / b
    }'
}

compare "mix" "$WORK/n.txt" "$WORK/mix.cx.bin" "$WORK/mix.c.bin"
compare "fib" "" "$WORK/fib.cx.bin" "$WORK/fib.c.bin"

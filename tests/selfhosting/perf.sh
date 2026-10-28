#!/usr/bin/env bash
# Performance baseline for the bootstrap compiler.
#
#   tests/selfhosting/perf.sh [compiler]
#
# Measures best-of-N wall time for a self-compile (`build --opt 0`), frontend
# throughput (`check`) on the compiler source and the corpus, and the peak RSS
# of a self-compile. Numbers are printed as a table; no hard pass/fail gate is
# applied except a coarse smoke bound that the self-compile finishes.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
COMPILER="${1:-$ROOT/build/stage2/corexc}"
case "$COMPILER" in
  /*) ;;
  *) COMPILER="$PWD/$COMPILER" ;;
esac
TMP="$(mktemp -d /tmp/corexc-perf.XXXXXX)"
trap 'rm -rf "$TMP"' EXIT

[ -x "$COMPILER" ] || { echo "missing compiler: $COMPILER" >&2; exit 2; }
SRC="$ROOT/selfhost/corexc.cx"
BYTES="$(wc -c <"$SRC" | tr -d ' ')"
LINES="$(wc -l <"$SRC" | tr -d ' ')"

# GNU time prints "<seconds> <maxrss KiB>"; fall back to bash SECONDS.
have_gnu_time=0
if /usr/bin/time -f "%e %M" true >/dev/null 2>&1; then
  have_gnu_time=1
fi

best_ms=999999
for _ in 1 2 3; do
  start_ns="$(date +%s%N)"
  "$COMPILER" build "$SRC" -o "$TMP/stage" --opt 0 >/dev/null
  end_ns="$(date +%s%N)"
  ms=$(( (end_ns - start_ns) / 1000000 ))
  if [ "$ms" -lt "$best_ms" ]; then best_ms="$ms"; fi
done
if [ "$best_ms" -gt 60000 ]; then
  echo "FAIL: self-compile took ${best_ms}ms" >&2
  exit 1
fi

best_check_ms=999999
for _ in 1 2 3 4 5; do
  start_ns="$(date +%s%N)"
  "$COMPILER" check "$SRC" >/dev/null
  end_ns="$(date +%s%N)"
  ms=$(( (end_ns - start_ns) / 1000000 ))
  if [ "$ms" -lt "$best_check_ms" ]; then best_check_ms="$ms"; fi
done

corpus_bytes=0
for f in "$ROOT"/tests/selfhosting/cases/positive/*.cx; do
  corpus_bytes=$((corpus_bytes + $(wc -c <"$f")))
done
start_ns="$(date +%s%N)"
for f in "$ROOT"/tests/selfhosting/cases/positive/*.cx; do
  "$COMPILER" check "$f" >/dev/null
done
end_ns="$(date +%s%N)"
corpus_ms=$(( (end_ns - start_ns) / 1000000 ))
[ "$corpus_ms" -eq 0 ] && corpus_ms=1

rss_kb=0
if [ "$have_gnu_time" -eq 1 ]; then
  rss_kb="$(/usr/bin/time -f "%M" "$COMPILER" build "$SRC" -o "$TMP/stage2" --opt 0 2>&1 >/dev/null | tail -1)"
fi

printf 'perf: %s\n' "$COMPILER"
printf 'source: %s bytes, %s lines\n' "$BYTES" "$LINES"
printf 'self-compile --opt 0 (best of 3): %d ms\n' "$best_ms"
printf 'frontend check compiler source (best of 5): %d ms\n' "$best_check_ms"
printf 'frontend corpus: %d files, %d bytes in %d ms' \
  "$(ls "$ROOT"/tests/selfhosting/cases/positive/*.cx | wc -l | tr -d ' ')" \
  "$corpus_bytes" "$corpus_ms"
awk -v b="$corpus_bytes" -v ms="$corpus_ms" \
  'BEGIN { printf " (%.2f MB/s)\n", (b / 1048576.0) / (ms / 1000.0) }'
printf 'compiler-source throughput: '
awk -v b="$BYTES" -v ms="$best_check_ms" \
  'BEGIN { printf "%.2f MB/s\n", (b / 1048576.0) / (ms / 1000.0) }'
if [ "$have_gnu_time" -eq 1 ]; then
  printf 'peak RSS self-compile: %d KiB\n' "${rss_kb:-0}"
else
  printf 'peak RSS: unavailable (no GNU time)\n'
fi
printf 'parallel units: not applicable (single translation unit; see worklog)\n'

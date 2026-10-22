#!/usr/bin/env bash
# Run every examples/*.cx at --opt 0,1,2,3 and require identical stdout/stderr
# and exit status at all four levels. --opt 0 is the reference run.
#
#   ./scripts/example-matrix.sh
#   COREX=compiler/target/release/corex ./scripts/example-matrix.sh
#
# The compiler binary must already be built. CI calls this after `cargo test`.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COREX="${COREX:-$ROOT/compiler/target/debug/corex}"

if [[ ! -x "$COREX" ]]; then
    echo "error: compiler not found at $COREX" >&2
    echo "       run: (cd compiler && cargo build)" >&2
    exit 2
fi

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

failures=0
examples=0

for file in "$ROOT"/examples/*.cx; do
    examples=$((examples + 1))
    name="$(basename "$file")"
    reference="$TMP/$name.opt0"
    reference_rc=0
    "$COREX" run "$file" --opt 0 >"$reference" 2>&1 || reference_rc=$?
    printf '%-20s opt=0 exit=%-3s %s bytes (reference)\n' \
        "$name" "$reference_rc" "$(wc -c <"$reference")"

    for opt in 1 2 3; do
        out="$TMP/$name.opt$opt"
        rc=0
        "$COREX" run "$file" --opt "$opt" >"$out" 2>&1 || rc=$?
        if [[ "$rc" -ne "$reference_rc" ]]; then
            printf 'FAIL %-20s opt=%s exit=%s, opt=0 exit=%s\n' \
                "$name" "$opt" "$rc" "$reference_rc"
            failures=$((failures + 1))
        elif ! cmp -s "$reference" "$out"; then
            printf 'FAIL %-20s opt=%s output differs from opt=0\n' "$name" "$opt"
            diff -u "$reference" "$out" || true
            failures=$((failures + 1))
        else
            printf 'ok   %-20s opt=%s exit=%s\n' "$name" "$opt" "$rc"
        fi
    done
done

if [[ "$examples" -eq 0 ]]; then
    echo "error: no examples/*.cx found under $ROOT" >&2
    exit 2
fi

if [[ "$failures" -ne 0 ]]; then
    echo "error: $failures example/opt-level mismatch(es)" >&2
    exit 1
fi

printf 'all %s examples match at --opt 0,1,2,3\n' "$examples"

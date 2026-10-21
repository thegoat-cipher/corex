#!/usr/bin/env bash
# Compile-and-run one CoreX file (or check / show IR).
#
#   ./scripts/try.sh examples/hello.cx           # build in a temp dir and run
#   ./scripts/try.sh examples/hello.cx --check   # frontend only: lex, parse, semantic
#   ./scripts/try.sh examples/hello.cx --ir      # print IR and verify it with llvm-as
#   ./scripts/try.sh examples/hello.cx --build   # keep a binary under build/

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
COREX="${COREX:-$ROOT/compiler/target/release/corex}"
if [[ ! -x "$COREX" ]]; then
    COREX="$ROOT/compiler/target/debug/corex"
fi
if [[ ! -x "$COREX" ]]; then
    echo "error: build the compiler first: (cd compiler && cargo build --release)" >&2
    exit 2
fi

if [[ $# -lt 1 ]]; then
    echo "usage: $0 <file.cx> [--check|--ir|--build] [program args...]" >&2
    exit 2
fi

FILE="$1"
shift
MODE="run"
if [[ $# -gt 0 && "$1" == --* ]]; then
    MODE="$1"
    shift
fi

case "$MODE" in
    --check) exec "$COREX" check "$FILE" ;;
    --ir) exec "$COREX" emit-ir "$FILE" --verify ;;
    --build)
        mkdir -p "$ROOT/build"
        OUT="$ROOT/build/$(basename "${FILE%.cx}")"
        exec "$COREX" build "$FILE" -o "$OUT" --opt 2
        ;;
    run)
        exec "$COREX" run "$FILE" -- "$@"
        ;;
    *)
        echo "error: unknown mode '$MODE'" >&2
        exit 2
        ;;
esac

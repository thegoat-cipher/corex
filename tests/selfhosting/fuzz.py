#!/usr/bin/env python3
"""Random malformed-input fuzzer for the bootstrap compiler.

    tests/selfhosting/fuzz.py [compiler] [iterations]

Mutates the positive corpus (byte flips, deletions, insertions, truncations)
and runs `check` on every result. A malformed input must either be accepted or
rejected with a diagnostic and exit code 1: a timeout, a signal (segfault,
abort), or a rejection without a diagnostic is a failure. Deterministic.
"""
import os
import random
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
COMPILER = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "build/stage2/corexc")
ITERATIONS = int(sys.argv[2]) if len(sys.argv) > 2 else 400
SEEDS = os.path.join(ROOT, "tests/selfhosting/cases/positive")
ALPHABET = b"{}()[];:,.+-*/%&|^!<>=~?'\"#@$_\\\n\r\t abcxyz0123456789"
DEEP = [
    b"fn main() -> i32 { let x: i32 = " + b"(" * 5000 + b"1" + b")" * 5000 + b"; return 0; }",
    b"fn main() -> i32 { let x: " + b"*" * 5000 + b"i32 = null; return 0; }",
    b"fn main() -> i32 { let x: i32 = " + b"-" * 5000 + b"1; return 0; }",
    b"fn main() -> i32 { " + b"{" * 5000 + b"}" * 5000 + b" return 0; }",
    b"fn main() -> i32 {" + b"{" * 5000,
]


def seeds():
    out = []
    for name in sorted(os.listdir(SEEDS)):
        if name.endswith(".cx"):
            with open(os.path.join(SEEDS, name), "rb") as fh:
                out.append(fh.read())
    assert out, "no positive corpus found"
    return out


def mutate(rng, data):
    buf = bytearray(data)
    for _ in range(rng.randint(1, 12)):
        if not buf:
            buf = bytearray(b"x")
        pos = rng.randrange(len(buf))
        choice = rng.random()
        if choice < 0.4:
            buf[pos] = rng.choice(ALPHABET)
        elif choice < 0.7:
            del buf[pos:pos + rng.randint(1, 8)]
        else:
            buf[pos:pos] = bytes(rng.choice(ALPHABET) for _ in range(rng.randint(1, 8)))
    return bytes(buf)


def check(tmp, index, data):
    path = os.path.join(tmp, "case%d.cx" % index)
    with open(path, "wb") as fh:
        fh.write(data)
    try:
        proc = subprocess.run([COMPILER, "check", path], capture_output=True, timeout=20)
    except subprocess.TimeoutExpired:
        return "timed out"
    if proc.returncode == 0:
        return None
    if proc.returncode != 1:
        return "exit %d" % proc.returncode
    if not proc.stderr:
        return "rejected without a diagnostic"
    return None


def main():
    if not os.access(COMPILER, os.X_OK):
        sys.exit("missing compiler: %s" % COMPILER)
    rng = random.Random(99)
    corpus = seeds()
    failures = 0
    with tempfile.TemporaryDirectory(prefix="corex-fuzz.") as tmp:
        inputs = [mutate(rng, corpus[rng.randrange(len(corpus))]) for _ in range(ITERATIONS)]
        inputs += [corpus[rng.randrange(len(corpus))][:rng.randrange(len(corpus[0]))] for _ in range(ITERATIONS // 4)]
        inputs += DEEP
        for index, data in enumerate(inputs):
            problem = check(tmp, index, data)
            if problem is not None:
                failures += 1
                print("FAIL case%d: %s" % (index, problem), file=sys.stderr)
        print("fuzz: %d inputs, %d failures (compiler: %s)" % (len(inputs), failures, COMPILER))
    sys.exit(1 if failures else 0)


if __name__ == "__main__":
    main()

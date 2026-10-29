#!/usr/bin/env python3
"""Run the Rust behaviour suite against an external `corexc` compiler.

The Rust integration tests call `driver::build`/`check`/`emit_ir` through
`compiler/tests/common/mod.rs`. Setting `COREX_TEST_COMPILER` routes those
calls to the external binary; this script sets it, runs each integration test
binary, and reports honest counts plus the reason every non-pass was blocked.

    tests/selfhosting/behavior.py [--compiler PATH] [--suite NAME]... [--json]

Exit status is 0 only when every non-blocked test passes. Known scope gaps are
listed in `BLOCKED` with an explicit reason; an unexpected failure is reported
as `failed`, never silently reclassified.
"""

import argparse
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TESTS = os.path.join(ROOT, "compiler", "tests")

# Whole suites that cannot exercise `corexc` at all.
BLOCKED_SUITES = {
    "concurrency": "SH-033: threads/atomics/TLS/inline asm are not implemented",
}

# Individual tests that depend on oracle-only APIs or documented contracts.
BLOCKED_TESTS = {
    (
        "tooling",
        "formatter_is_idempotent_and_semantics_preserving",
    ): "calls the in-process Rust formatter; `corexc fmt` is covered by cli.sh",
    (
        "tooling",
        "docs_are_generated_from_doc_comments",
    ): "calls the in-process Rust parser/docgen; `corexc doc` is covered by cli.sh",
    (
        "tooling",
        "test_runner_reports_pass_and_failure",
    ): "uses `BuildConfig::test_mode`; `corexc test` runs rather than links",
    (
        "tooling",
        "debug_info_has_line_tables_and_subprograms",
    ): "DWARF is rejected by contract in the bootstrap (SH-036)",
    (
        "tooling",
        "language_server_reports_diagnostics_and_edits",
    ): "launches the Rust `corex-lsp` binary; the CoreX server is covered by lsp.py",
    (
        "compiler_caps",
        "imports_expand_relative_to_the_importing_file",
    ): "inspects `driver::load_source` expansion; there is no CLI equivalent",
}


def suites():
    names = []
    for entry in sorted(os.listdir(TESTS)):
        if entry.endswith(".rs"):
            names.append(entry[:-3])
    return names


def blocked_reason(suite, test):
    if suite in BLOCKED_SUITES:
        return BLOCKED_SUITES[suite]
    return BLOCKED_TESTS.get((suite, test))


TEST_LINE = re.compile(r"^test ([^ ]+) \.\.\. (ok|FAILED|ignored)$")


def run_suite(suite, compiler, env):
    command = ["cargo", "test", "--test", suite, "--", "--test-threads=1"]
    proc = subprocess.run(
        command,
        cwd=os.path.join(ROOT, "compiler"),
        env=env,
        text=True,
        capture_output=True,
    )
    results = []
    for line in proc.stdout.splitlines():
        match = TEST_LINE.match(line.strip())
        if match:
            results.append((match.group(1), match.group(2)))
    if not results:
        results.append((suite, "FAILED"))
    return results, proc


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", default=os.path.join(ROOT, "build", "stage2", "corexc"))
    parser.add_argument("--suite", action="append", default=None)
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--show-failures", action="store_true")
    args = parser.parse_args()

    compiler = os.path.abspath(args.compiler)
    if not (os.path.isfile(compiler) and os.access(compiler, os.X_OK)):
        print("missing compiler: %s" % compiler, file=sys.stderr)
        return 2

    env = dict(os.environ)
    env["COREX_TEST_COMPILER"] = compiler

    selected = args.suite or suites()
    counts = {"passed": 0, "skipped": 0, "failed": 0, "blocked": 0}
    failures = []
    blocked = []
    for suite in selected:
        results, proc = run_suite(suite, compiler, env)
        for test, status in results:
            name = "%s::%s" % (suite, test)
            reason = blocked_reason(suite, test)
            if status == "ok":
                counts["passed"] += 1
            elif status == "ignored":
                counts["skipped"] += 1
            elif reason is not None:
                counts["blocked"] += 1
                blocked.append((name, reason))
            else:
                counts["failed"] += 1
                failures.append((name, proc))
        if not results and proc.returncode != 0:
            counts["failed"] += 1
            failures.append((suite, proc))

    if args.json:
        print(
            json.dumps(
                {
                    "compiler": compiler,
                    "counts": counts,
                    "failures": [name for name, _ in failures],
                    "blocked": [{"test": name, "reason": reason} for name, reason in blocked],
                },
                indent=2,
            )
        )
    else:
        for name, reason in blocked:
            print("blocked %-60s %s" % (name, reason))
        for name, proc in failures:
            print("FAILED  %s" % name)
            if args.show_failures:
                tail = proc.stdout.strip().splitlines()[-25:]
                for line in tail:
                    print("        " + line)
        total = sum(counts.values())
        print(
            "behaviour tests: %d passed, %d skipped, %d failed, %d blocked (%d total)"
            % (
                counts["passed"],
                counts["skipped"],
                counts["failed"],
                counts["blocked"],
                total,
            )
        )
    return 0 if counts["failed"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())

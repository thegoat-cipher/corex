#!/usr/bin/env python3
"""Run the Rust behaviour suite against an external `corexc` compiler.

The Rust integration tests call `driver::build`/`check`/`emit_ir` through
`compiler/tests/common/mod.rs`. Setting `COREX_TEST_COMPILER` routes those
calls to the external binary; this script sets it and runs the integration
tests one at a time so a single hung compiler cannot stall the run.

Every `cargo test` invocation is bounded by `--timeout` seconds (default 60).
On expiry the whole process group is killed, the test is recorded as
`timeout`, and the run continues. Counts are exact per test; a suite is never
reported as passed because its process died.

    tests/selfhosting/behavior.py [--compiler PATH] [--suite NAME]...
                                  [--timeout SECONDS] [--json] [--show-failures]

Exit status is 0 only when every non-blocked test passes. Known scope gaps are
listed in `BLOCKED_TESTS` with an explicit reason; an unexpected failure or a
timeout is reported as such, never silently reclassified.
"""

import argparse
import json
import os
import re
import signal
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TESTS = os.path.join(ROOT, "compiler", "tests")
CARGO_DIR = os.path.join(ROOT, "compiler")

# Whole suites that cannot exercise `corexc` at all.
BLOCKED_SUITES = {}

# The 68 `cargo test --lib` unit tests call Rust-internal modules directly
# (lexer/parser/semantic/codegen). No CLI surface can run them against an
# external compiler, so they are reported as blocked instead of being dropped
# from the 205-test total.
LIB_SUITE = "corex::lib"
LIB_REASON = (
    "unit tests call Rust-internal modules directly; no CLI equivalent exists"
)

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
    names = [LIB_SUITE]
    for entry in sorted(os.listdir(TESTS)):
        if entry.endswith(".rs"):
            names.append(entry[:-3])
    return names


def blocked_reason(suite, test):
    if suite in BLOCKED_SUITES:
        return BLOCKED_SUITES[suite]
    return BLOCKED_TESTS.get((suite, test))


TEST_LINE = re.compile(r"^test ([^ ]+) \.\.\. (ok|FAILED|ignored)$")
LIST_LINE = re.compile(r"^(.*): test$")


def run_cargo(args, env, timeout):
    """Run `cargo ...` bounded by `timeout`; kill its whole group on expiry.

    Returns `(returncode, stdout+stderr, timed_out)`.
    """
    proc = subprocess.Popen(
        ["cargo"] + args,
        cwd=CARGO_DIR,
        env=env,
        text=True,
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    try:
        output, _ = proc.communicate(timeout=timeout)
        return proc.returncode, output, False
    except subprocess.TimeoutExpired:
        # The test binary and every compiler it spawned share this process
        # group; SIGKILL it so a wedged child cannot outlive the harness.
        try:
            os.killpg(proc.pid, signal.SIGKILL)
        except ProcessLookupError:
            pass
        output, _ = proc.communicate()
        return proc.returncode, output, True


def cargo_test_args(suite, extra):
    if suite == LIB_SUITE:
        return ["test", "--lib", "--"] + extra
    return ["test", "--test", suite, "--"] + extra


def list_tests(suite, env, timeout):
    """Return `(tests, error)`; `error` is a string when the suite is unusable."""
    rc, output, timed_out = run_cargo(cargo_test_args(suite, ["--list"]), env, timeout)
    if timed_out:
        return [], "listing timed out after %ss" % timeout
    if rc != 0:
        tail = "\n".join(output.strip().splitlines()[-8:])
        return [], "cargo test --list failed (exit %s): %s" % (rc, tail)
    names = []
    for line in output.splitlines():
        match = LIST_LINE.match(line.strip())
        if match:
            names.append(match.group(1))
    if not names:
        return [], "no tests listed"
    return names, None


def run_test(suite, test, env, timeout):
    """Return `(status, output)` where status is ok/FAILED/ignored/timeout."""
    rc, output, timed_out = run_cargo(
        cargo_test_args(suite, ["--exact", test, "--test-threads=1"]),
        env,
        timeout,
    )
    if timed_out:
        return "timeout", output
    for line in output.splitlines():
        match = TEST_LINE.match(line.strip())
        if match and match.group(1) == test:
            return match.group(2), output
    # No result line: the harness died, the binary aborted, or the filter
    # matched nothing. Any of those is a failure for a name from `--list`.
    return "FAILED", output


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--compiler", default=os.path.join(ROOT, "build", "stage2", "corexc"))
    parser.add_argument("--suite", action="append", default=None)
    parser.add_argument("--timeout", type=float, default=60.0,
                        help="seconds per cargo invocation (default: 60)")
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
    counts = {"passed": 0, "skipped": 0, "failed": 0, "blocked": 0, "timeout": 0}
    failures = []
    blocked = []
    timeouts = []
    suite_errors = []
    for suite in selected:
        tests, error = list_tests(suite, env, args.timeout)
        if error is not None:
            # A suite that cannot even be enumerated is one blocked/failed
            # unit, reported with the reason instead of guessed per-test rows.
            if suite in BLOCKED_SUITES:
                counts["blocked"] += 1
                blocked.append((suite, BLOCKED_SUITES[suite]))
            elif "timed out" in error:
                counts["timeout"] += 1
                timeouts.append((suite, error))
            else:
                counts["failed"] += 1
                suite_errors.append((suite, error))
            continue
        for test in tests:
            name = "%s::%s" % (suite, test)
            if suite == LIB_SUITE:
                counts["blocked"] += 1
                blocked.append((name, LIB_REASON))
                continue
            status, output = run_test(suite, test, env, args.timeout)
            reason = blocked_reason(suite, test)
            if status == "ok":
                counts["passed"] += 1
            elif status == "ignored":
                counts["skipped"] += 1
            elif status == "timeout":
                counts["timeout"] += 1
                timeouts.append((name, "no result after %ss; process group killed" % args.timeout))
            elif reason is not None:
                counts["blocked"] += 1
                blocked.append((name, reason))
            else:
                counts["failed"] += 1
                failures.append((name, output))

    if args.json:
        print(
            json.dumps(
                {
                    "compiler": compiler,
                    "timeout_seconds": args.timeout,
                    "counts": counts,
                    "failures": [name for name, _ in failures],
                    "suite_errors": [{"suite": name, "error": error} for name, error in suite_errors],
                    "blocked": [{"test": name, "reason": reason} for name, reason in blocked],
                    "timeouts": [{"test": name, "reason": reason} for name, reason in timeouts],
                },
                indent=2,
            )
        )
    else:
        for name, reason in blocked:
            print("blocked %-72s %s" % (name, reason))
        for name, reason in timeouts:
            print("TIMEOUT %s: %s" % (name, reason))
        for name, error in suite_errors:
            print("FAILED  %s" % name)
            print("        " + error.replace("\n", "\n        "))
        for name, output in failures:
            print("FAILED  %s" % name)
            if args.show_failures:
                tail = output.strip().splitlines()[-25:]
                for line in tail:
                    print("        " + line)
        total = sum(counts.values())
        print(
            "behaviour tests: %d passed, %d skipped, %d failed, %d blocked, "
            "%d timeout (%d total)"
            % (
                counts["passed"],
                counts["skipped"],
                counts["failed"],
                counts["blocked"],
                counts["timeout"],
                total,
            )
        )
    return 0 if counts["failed"] == 0 and counts["timeout"] == 0 else 1


if __name__ == "__main__":
    sys.exit(main())

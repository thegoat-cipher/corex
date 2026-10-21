//! CLI-level tests: source from stdin, inline `exec`, and the interactive REPL.
//!
//! These drive the actual `corex` binary (not the library), so they cover
//! argument parsing, stdin handling, exit-code propagation, and REPL session
//! behavior end to end.

use std::io::Write;
use std::process::{Command, Output, Stdio};

fn corex() -> Command {
    Command::new(env!("CARGO_BIN_EXE_corex"))
}

fn with_stdin(args: &[&str], input: &str) -> Output {
    let mut child = corex()
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn corex");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input.as_bytes())
        .expect("write stdin");
    child.wait_with_output().expect("wait for corex")
}

fn code(out: &Output) -> i32 {
    out.status.code().unwrap_or(-1)
}

#[test]
fn exec_runs_inline_source_and_forwards_status() {
    let out = corex()
        .args(["exec", "fn main() -> i32 { return 42; }"])
        .output()
        .expect("run corex exec");
    assert_eq!(code(&out), 42, "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let out = corex()
        .args([
            "exec",
            "fn _start() { unsafe { syscall(1, 1, &\"direct\\n\" as usize, 7); syscall(60, 0, 0, 0, 0, 0, 0); } }",
        ])
        .output()
        .expect("run corex exec");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "direct\n");
    assert_eq!(code(&out), 0);
}

#[test]
fn run_accepts_source_on_stdin() {
    let out = with_stdin(&["run", "-"], "fn main() -> i32 { return 3; }\n");
    assert_eq!(code(&out), 3, "stderr: {}", String::from_utf8_lossy(&out.stderr));

    let out = with_stdin(
        &["run", "-"],
        "fn _start() { unsafe { syscall(1, 1, &\"stdin\\n\" as usize, 6); syscall(60, 0, 0, 0, 0, 0, 0); } }",
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout), "stdin\n");
    assert_eq!(code(&out), 0);
}

#[test]
fn check_reports_errors_from_stdin() {
    let out = with_stdin(&["check", "-"], "fn main() -> i32 { return nope; }\n");
    assert_eq!(code(&out), 1);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("E0201"), "{stderr}");
    assert!(stderr.contains("<stdin>"), "{stderr}");
}

#[test]
fn repl_accumulates_definitions_and_runs_on_blank_line() {
    let session = "\
fn greet() {
    unsafe { syscall(1, 1, &\"hi from repl\\n\" as usize, 14); }
}

fn main() -> i32 {
    greet();
    return 5;
}

:quit
";
    let out = with_stdin(&["repl"], session);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[exit 5]"), "stdout: {stdout}");
    assert!(stdout.contains("hi from repl"), "stdout: {stdout}");
    assert_eq!(code(&out), 0);
}

#[test]
fn repl_redefinition_replaces_previous_function() {
    let session = "\
fn main() -> i32 { return 1; }

fn main() -> i32 { return 2; }

:quit
";
    let out = with_stdin(&["repl"], session);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("[exit 1]"), "stdout: {stdout}");
    assert!(stdout.contains("[exit 2]"), "stdout: {stdout}");
    let after_last = stdout.rsplit("[exit 2]").next().unwrap_or("");
    assert!(!after_last.contains("[exit 1]"), "stdout: {stdout}");
}

#[test]
fn repl_runs_statement_blocks_via_a_temporary_main() {
    let session = "\
unsafe { syscall(1, 1, &\"stmt mode\\n\" as usize, 10); }

:quit
";
    let out = with_stdin(&["repl"], session);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("stmt mode"), "stdout: {stdout}");
    assert!(stdout.contains("[exit 0]"), "stdout: {stdout}");
}

#[test]
fn repl_accepts_a_pasted_program_with_internal_blank_lines() {
    // Regression: a blank line inside a function body used to submit a
    // half-typed function ("expected `}`"), which also broke pasting examples.
    let session = "\
fn _start() {
    let message: [u8; 14] = \"Hello, World!\\n\";

    unsafe {
        syscall(1, 1, &message as usize, 14);
        syscall(60, 0, 0, 0, 0, 0, 0);
    }
}

:quit
";
    let out = with_stdin(&["repl"], session);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("Hello, World!"), "stdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.contains("[exit 0]"), "stdout: {stdout}");
    assert!(!stderr.contains("E0100"), "stderr: {stderr}");
}

#[test]
fn exec_accepts_a_program_terminating_with_the_exit_syscall() {
    // Regression: E0206 fired for a `main` whose last statement exits the
    // process, because divergence analysis only understood `return` and `loop`.
    let out = corex()
        .args([
            "exec",
            "fn main() -> i32 { unsafe { syscall(1, 1, &\"bye\\n\" as usize, 4); syscall(60, 3, 0, 0, 0, 0, 0); } }",
        ])
        .output()
        .expect("run corex exec");
    assert_eq!(String::from_utf8_lossy(&out.stdout), "bye\n");
    assert_eq!(code(&out), 3, "stderr: {}", String::from_utf8_lossy(&out.stderr));
}

#[test]
fn repl_evaluates_an_expression_and_prints_the_value() {
    let out = with_stdin(&["repl"], "1 + 2\n:quit\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("3\n[exit 0]"), "stdout: {stdout}");
    assert_eq!(code(&out), 0);
}

#[test]
fn repl_statement_semicolon_suppresses_the_value() {
    let out = with_stdin(&["repl"], "1 + 2;\n:quit\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(!stdout.contains("3\n[exit"), "stdout: {stdout}");
}

#[test]
fn repl_explains_missing_cross_submission_bindings() {
    let out = with_stdin(&["repl"], "print(x);\n:quit\n");
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stderr.contains("E0201"), "stderr: {stderr}");
    assert!(stdout.contains("do not persist"), "stdout: {stdout}");
}

#[test]
fn repl_installs_a_definition_submitted_on_a_blank_line() {
    // Regression: a helper-only definition followed by a blank line was
    // swallowed as if it were a blank line inside a pasted file, so the helper
    // was never installed and later calls failed to resolve it.
    let session = "\
fn twice(n: i32) -> i32 {
    return n * 2;
}

twice(21)
:quit
";
    let out = with_stdin(&["repl"], session);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stdout.contains("42\n[exit 0]"), "stdout: {stdout}\nstderr: {stderr}");
}

#[test]
fn repl_ir_command_verifies_the_module() {
    let out = with_stdin(&["repl"], "fn main() -> i32 { return 0; }\n:ir\n:quit\n");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("IR verified by llvm-as"),
        "stdout: {stdout}"
    );
}

#[test]
fn emit_ir_and_check_reject_opt_instead_of_ignoring_it() {
    // `--opt` only affects code generation; accepting it for commands that emit
    // pre-optimization IR or stop before codegen would silently do nothing.
    for command in ["emit-ir", "check"] {
        let out = corex()
            .args([command, "../examples/hello.cx", "--opt", "2"])
            .output()
            .expect("run corex");
        assert_eq!(code(&out), 2, "{command} should reject --opt");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(stderr.contains("unknown option `--opt`"), "stderr: {stderr}");
    }
}

//! CLI-level tests: source from stdin, inline `exec`, and the interactive REPL.
//!
//! These drive the actual `corex` binary (not the library), so they cover
//! argument parsing, stdin handling, exit-code propagation, and REPL session
//! behavior end to end.

use std::io::Write;
use std::process::{Command, Output, Stdio};

mod common;

fn corex() -> Command {
    match common::compiler() {
        Some(bin) => Command::new(bin),
        None => Command::new(env!("CARGO_BIN_EXE_corex")),
    }
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

#[test]
fn repl_accepts_typedef_struct_union_and_function_pointer_items() {
    let session = "\
typedef Word = u64;
struct Token { kind: i32, start: usize }
union Number { i: i64, f: f64 }
fn inc(x: i32) -> i32 { return x + 1; }
fn apply(f: fn(i32) -> i32, x: i32) -> i32 { return f(x); }
fn main() -> i32 {
    let mut tokens: vec<Token> = vec_new(1);
    vec_push(&mut tokens, Token { kind: 7, start: 0 });
    print(apply(&inc, 41));
    print(tokens[0].kind);
    print(sizeof(Token));
    vec_free(&mut tokens);
    let n: Number = Number { i: 42 };
    print(n.i);
    return 0;
}
:quit
";
    let out = with_stdin(&["repl"], session);
    let stdout = String::from_utf8_lossy(&out.stdout);
    for expected in ["42\n", "7\n", "16\n", "42\n"] {
        assert!(stdout.contains(expected), "missing {expected:?} in:\n{stdout}");
    }
    assert!(stdout.contains("[exit 0]"), "stdout: {stdout}");
}

/// A build directory must not survive any failing path: analysis errors,
/// tool failures, and link failures all returned before the old cleanup.
#[test]
fn failed_builds_remove_their_temp_directories() {
    let tmp = std::env::temp_dir().join(format!("corex-cli-tmp-fail-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create tmp");
    let out_path = tmp.join("app");

    // Analysis error.
    let src = tmp.join("bad.cx");
    std::fs::write(&src, "fn main() -> i32 { return missing; }\n").expect("write");
    let out = corex()
        .env("TMPDIR", &tmp)
        .args(["build", src.to_str().unwrap(), "-o", out_path.to_str().unwrap()])
        .output()
        .expect("run corex");
    assert!(!out.status.success());
    assert_no_build_dirs(&tmp, "analysis error");

    // Link error: the objects were already written to the work directory.
    let link_bad = tmp.join("link-bad.cx");
    std::fs::write(
        &link_bad,
        "extern \"C\" fn missing_symbol() -> i32;\nfn main() -> i32 { return missing_symbol(); }\n",
    )
    .expect("write");
    let out = corex()
        .env("TMPDIR", &tmp)
        .args(["build", link_bad.to_str().unwrap(), "-o", out_path.to_str().unwrap()])
        .output()
        .expect("run corex");
    assert!(!out.status.success());
    assert_no_build_dirs(&tmp, "link error");
    let _ = std::fs::remove_dir_all(&tmp);
}

fn assert_no_build_dirs(tmp: &std::path::Path, context: &str) {
    let leftovers: Vec<String> = std::fs::read_dir(tmp)
        .expect("read tmp")
        .map(|e| e.expect("entry").file_name().to_string_lossy().to_string())
        .filter(|name| name.starts_with("corex-"))
        .collect();
    assert!(
        leftovers.is_empty(),
        "{context} leaked temp directories: {leftovers:?}"
    );
}

/// Scratch directories carry a random name (no pid or timestamp that a local
/// attacker can predict), are created 0700, and are only kept when asked.
#[test]
fn temp_directories_are_private_and_unpredictable() {
    let tmp = std::env::temp_dir().join(format!("corex-cli-tmp-keep-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).expect("create tmp");
    let src = tmp.join("ok.cx");
    std::fs::write(&src, "fn main() -> i32 { return 0; }\n").expect("write");
    let out = corex()
        .env("TMPDIR", &tmp)
        .env("COREX_KEEP_TEMP", "1")
        .args([
            "build",
            src.to_str().unwrap(),
            "-o",
            tmp.join("app").to_str().unwrap(),
        ])
        .output()
        .expect("run corex");
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let build_dirs: Vec<std::path::PathBuf> = std::fs::read_dir(&tmp)
        .expect("read tmp")
        .filter_map(|e| {
            let path = e.expect("entry").path();
            let name = path.file_name()?.to_string_lossy().to_string();
            name.starts_with("corex-build-").then_some(path)
        })
        .collect();
    assert_eq!(build_dirs.len(), 1, "expected one kept build directory");
    let name = build_dirs[0]
        .file_name()
        .expect("name")
        .to_string_lossy()
        .to_string();
    assert!(
        !name.contains(&std::process::id().to_string()),
        "temp name leaks the pid: {name}"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&build_dirs[0])
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "temp directory mode is {mode:o}");
    }
    let _ = std::fs::remove_dir_all(&tmp);
}

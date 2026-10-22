//! Tooling coverage: formatter, linter, docs, test runner, DWARF, sanitizers,
//! and the language server.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

use corex::diagnostics::SourceFile;
use corex::driver::{self, BuildConfig, LinkInputs};
use corex::lsp::parse_json;

fn build(source: &str, config: &BuildConfig) -> (driver::Artifacts, driver::Artifacts, String) {
    let work = driver::temp_dir("corex-tooling");
    std::fs::create_dir_all(&work).expect("create dir");
    let src = work.join("program.cx");
    std::fs::write(&src, source).expect("write source");
    let file = SourceFile::new(src.display().to_string(), source);
    let exe = work.join("program");
    let artifacts = driver::build_with_config(&file, &exe, 2, false, &LinkInputs::default(), config)
        .unwrap_or_else(|d| panic!("compile failed:\n{}", file.render_all(&d)));
    // Keep the work dir path so callers can clean it up.
    let marker = driver::Artifacts {
        ir: work.display().to_string(),
        executable: work.clone(),
    };
    (artifacts, marker, String::new())
}

fn run(exe: &Path) -> (String, String, i32) {
    let out = Command::new(exe).output().expect("run");
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn formatter_is_idempotent_and_semantics_preserving() {
    let source = "fn main() -> i32 {\nlet mut x : i32 = 1;\nif x == 1 {\nx += 41;\n}\nprint( x );\nreturn 0;\n}\n";
    let formatted = corex::format::format_source(source);
    assert_eq!(corex::format::format_source(&formatted), formatted);
    assert!(formatted.contains("    let mut x : i32 = 1;"));
    assert!(formatted.lines().any(|line| line == "    print(x);"));
    let (artifacts, marker, _) = build(&formatted, &BuildConfig::default());
    assert_eq!(run(&artifacts.executable).0, "42\n");
    let _ = std::fs::remove_dir_all(&marker.executable);
}

#[test]
fn linter_reports_unused_and_unreachable() {
    let source = r#"
fn helper() -> i32 { return 1; }
fn main() -> i32 {
    let unused = 5;
    if true { print(1); }
    return 0;
    print(2);
}
"#;
    let diags = driver::lint_source(source).expect("parse");
    let codes: Vec<&str> = diags.iter().map(|d| d.code).collect();
    assert!(codes.contains(&"W0001"), "{codes:?}");
    assert!(codes.contains(&"W0002"), "{codes:?}");
    assert!(codes.contains(&"W0003"), "{codes:?}");
    assert!(codes.contains(&"W0004"), "{codes:?}");
    assert!(diags.iter().all(|d| d.is_warning()));
}

#[test]
fn docs_are_generated_from_doc_comments() {
    let source = "/// Adds values.\nfn add(a: i32, b: i32) -> i32 { return a + b; }\n";
    let file = SourceFile::new("x.cx", source);
    let program = corex::parser::parse(&corex::lexer::lex(source).unwrap()).unwrap();
    let markdown = corex::docgen::generate(&file, &program);
    assert!(markdown.contains("### `fn add(a: i32, b: i32) -> i32`"));
    assert!(markdown.contains("Adds values."));
}

#[test]
fn test_runner_reports_pass_and_failure() {
    let source = r#"
fn test_pass() -> bool { return 1 + 1 == 2; }
fn test_fail() -> bool { return 1 + 1 == 3; }
"#;
    let config = BuildConfig {
        require_entry: false,
        test_mode: true,
        ..Default::default()
    };
    let (artifacts, marker, _) = build(source, &config);
    let (stdout, _stderr, code) = run(&artifacts.executable);
    assert!(stdout.contains("test test_pass ... ok"), "{stdout}");
    assert!(stdout.contains("test test_fail ... FAILED"), "{stdout}");
    assert_eq!(code, 1);
    let _ = std::fs::remove_dir_all(&marker.executable);
}

#[test]
fn debug_info_has_line_tables_and_subprograms() {
    let source = "fn add(a: i32, b: i32) -> i32 {\n    return a + b;\n}\nfn main() -> i32 {\n    let x = add(40, 2);\n    print(x);\n    return 0;\n}\n";
    let work = driver::temp_dir("corex-debug");
    std::fs::create_dir_all(&work).expect("create dir");
    let src = work.join("debug.cx");
    std::fs::write(&src, source).expect("write");
    let file = SourceFile::new(src.display().to_string(), source);
    let exe = work.join("program");
    let config = BuildConfig {
        debug_info: true,
        ..Default::default()
    };
    driver::build_with_config(&file, &exe, 2, false, &LinkInputs::default(), &config)
        .expect("debug build");
    let readelf = Command::new("readelf")
        .args(["--debug-dump=decodedline"])
        .arg(&exe)
        .output();
    if let Ok(out) = readelf {
        assert!(out.status.success(), "readelf failed");
        let text = String::from_utf8_lossy(&out.stdout);
        assert!(text.contains("debug.cx"), "{text}");
        assert!(text.contains(" 5 ") || text.contains(" 6 "), "{text}");
        let info = Command::new("readelf")
            .args(["--debug-dump=info"])
            .arg(&exe)
            .output()
            .expect("readelf info");
        let info = String::from_utf8_lossy(&info.stdout);
        assert!(info.contains("DW_TAG_subprogram"), "{info}");
        assert!(info.contains("add"), "{info}");
    }
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn sanitizer_aborts_on_signed_overflow_and_double_free() {
    let config = BuildConfig {
        sanitize: true,
        ..Default::default()
    };
    let overflow = "fn main() -> i32 { let mut x: i8 = 127; x += 1; print(x); return 0; }\n";
    let (artifacts, marker, _) = build(overflow, &config);
    let (_stdout, stderr, code) = run(&artifacts.executable);
    assert_eq!(code, 101);
    assert!(stderr.contains("integer overflow"), "{stderr}");
    let _ = std::fs::remove_dir_all(&marker.executable);

    let source = r#"
mod std_alloc;
fn main() -> i32 {
    unsafe {
        let p: *mut u8 = std_alloc::malloc(16);
        std_alloc::free(p);
        std_alloc::free(p);
    }
    return 0;
}
"#;
    let work = driver::temp_dir("corex-san");
    std::fs::create_dir_all(&work).expect("create dir");
    let src = work.join("program.cx");
    std::fs::write(&src, source).expect("write");
    let file = SourceFile::new(src.display().to_string(), source);
    let exe = work.join("program");
    let lib = Path::new(env!("CARGO_MANIFEST_DIR")).join("../lib/corex");
    let config = BuildConfig {
        sanitize: true,
        include_paths: vec![lib],
        ..Default::default()
    };
    driver::build_with_config(&file, &exe, 2, false, &LinkInputs::default(), &config)
        .expect("sanitized build");
    let (_stdout, stderr, code) = run(&exe);
    assert_eq!(code, 101);
    assert!(stderr.contains("already freed"), "{stderr}");
    let _ = std::fs::remove_dir_all(&work);
}

fn send(stdin: &mut std::process::ChildStdin, value: corex::lsp::Json) {
    let text = value.dump();
    write!(stdin, "Content-Length: {}\r\n\r\n{}", text.len(), text).expect("write frame");
    stdin.flush().expect("flush");
}

fn recv(stdout: &mut BufReader<std::process::ChildStdout>) -> corex::lsp::Json {
    let mut length = 0usize;
    loop {
        let mut line = String::new();
        stdout.read_line(&mut line).expect("read header");
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse().expect("length");
        }
    }
    let mut buffer = vec![0u8; length];
    stdout.read_exact(&mut buffer).expect("read body");
    parse_json(&String::from_utf8_lossy(&buffer)).expect("json")
}

#[test]
fn language_server_reports_diagnostics_and_edits() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_corex-lsp"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("start lsp");
    let mut stdin = child.stdin.take().unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());

    send(
        &mut stdin,
        parse_json(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"capabilities":{}}}"#).unwrap(),
    );
    let init = recv(&mut stdout);
    assert!(init.get("result").is_some());

    let source = "fn main() -> i32 {\n  let x = missing;\n  return 0;\n}\n";
    let open = format!(
        r#"{{"jsonrpc":"2.0","method":"textDocument/didOpen","params":{{"textDocument":{{"uri":"file:///tmp/t.cx","languageId":"corex","version":1,"text":{}}}}}}}"#,
        corex::lsp::Json::Str(source.to_string()).dump()
    );
    send(&mut stdin, parse_json(&open).unwrap());
    let diags = recv(&mut stdout);
    let list = diags
        .get("params")
        .and_then(|p| p.get("diagnostics"))
        .and_then(|d| d.as_array())
        .expect("diagnostics array");
    assert!(list.iter().any(|d| d
        .get("message")
        .and_then(|m| m.as_str())
        .unwrap()
        .contains("undefined name")));

    send(
        &mut stdin,
        parse_json(r#"{"jsonrpc":"2.0","id":2,"method":"textDocument/formatting","params":{"textDocument":{"uri":"file:///tmp/t.cx"},"options":{"tabSize":4,"insertSpaces":true}}}"#).unwrap(),
    );
    let format = recv(&mut stdout);
    let edits = format.get("result").and_then(|r| r.as_array()).unwrap();
    assert!(edits[0]
        .get("newText")
        .and_then(|t| t.as_str())
        .unwrap()
        .contains("    let x"));

    send(
        &mut stdin,
        parse_json(r#"{"jsonrpc":"2.0","id":3,"method":"shutdown"}"#).unwrap(),
    );
    let _ = recv(&mut stdout);
    send(
        &mut stdin,
        parse_json(r#"{"jsonrpc":"2.0","method":"exit"}"#).unwrap(),
    );
    let _ = child.wait();
}

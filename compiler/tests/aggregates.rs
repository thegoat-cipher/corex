//! Static aggregate constants and `const` array lengths (P0-1).
//!
//! A CoreX compiler needs keyword tables, token descriptions, and dispatch
//! tables as real LLVM globals, not runtime `init_tables()` work. These tests
//! compile, link, execute, and verify the IR at every optimization level.

use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver;

struct RunResult {
    stdout: String,
    stderr: String,
    code: i32,
}

fn build_and_run_opt(source: &str, opt: u8) -> RunResult {
    let work = driver::temp_dir("corex-aggregates");
    std::fs::create_dir_all(&work).expect("create test dir");
    let src_path = work.join("program.cx");
    std::fs::write(&src_path, source).expect("write source");
    let file = SourceFile::new(src_path.display().to_string(), source);
    let exe = work.join("program");
    let artifacts = match driver::build(&file, &exe, opt, false) {
        Ok(a) => a,
        Err(diags) => panic!("compilation failed at --opt {opt}:\n{}", file.render_all(&diags)),
    };
    let out = Command::new(&artifacts.executable)
        .output()
        .expect("execute compiled program");
    let result = RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

/// Emit IR for `source` and run `llvm-as` over it, returning the IR text.
fn verified_ir(source: &str) -> String {
    let file = SourceFile::new("aggregates.cx", source);
    let ir = match driver::emit_ir(&file) {
        Ok(ir) => ir,
        Err(diags) => panic!("IR emission failed:\n{}", file.render_all(&diags)),
    };
    let llvm_as = driver::llvm_tool("llvm-as")
        .expect("llvm-as not found; set COREX_LLVM_BIN or PATH");
    let dir = driver::temp_dir("corex-aggregates-ir");
    std::fs::create_dir_all(&dir).expect("create ir dir");
    let path = dir.join("module.ll");
    std::fs::write(&path, &ir).expect("write ir");
    let out = Command::new(llvm_as)
        .arg(&path)
        .arg("-o")
        .arg(dir.join("module.bc"))
        .output()
        .expect("run llvm-as");
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "llvm-as rejected the IR:\n{}\n--- IR ---\n{ir}",
        String::from_utf8_lossy(&out.stderr)
    );
    ir
}

fn expect_error(source: &str, code: &str) {
    let file = SourceFile::new("aggregates.cx", source);
    match driver::check(&file) {
        Ok(_) => panic!("expected {code}, but analysis succeeded:\n{source}"),
        Err(diags) => {
            let codes: Vec<&str> = diags.iter().map(|d| d.code).collect();
            assert!(
                codes.contains(&code),
                "expected {code}, got {codes:?} for:\n{source}"
            );
        }
    }
}

const TABLES: &str = r#"
const N: usize = 2;
static NAMES: [str; 2] = ["fn", "let"];
fn add(a: i64, b: i64) -> i64 { return a + b; }
static OPS: [fn(i64, i64) -> i64; 1] = [&add];
struct Entry { name: str, code: i32 }
static TABLE: [Entry; 2] = [Entry { name: "a", code: 1 }, Entry { name: "b", code: 2 }];
static SHORT: [u8; N] = [7, 8];
static NESTED: [[u8; 2]; 2] = [[1, 2], [3, 4]];
static FLAGS: [bool; 2] = [true, false];
static LETTERS: [char; 2] = ['x', 'y'];
static RATIOS: [f64; 2] = [0.5, 2.5];
static GREETING: str = "hi";
static GREETED: [Entry; 1] = [Entry { name: GREETING, code: 3 }];

fn main() -> i32 {
    print(NAMES[0]);
    print(NAMES[1]);
    print(OPS[0](40, 2));
    print(TABLE[0].name);
    print(TABLE[1].code);
    print(SHORT[1]);
    print(NESTED[0][1]);
    print(NESTED[1][0]);
    print(FLAGS[0]);
    print(FLAGS[1]);
    print(LETTERS[1]);
    print(RATIOS[0]);
    print(RATIOS[1]);
    print(GREETED[0].name);
    print(GREETED[0].code);
    return 0;
}
"#;

const TABLES_OUTPUT: &str = "fn\nlet\n42\na\n2\n8\n2\n3\ntrue\nfalse\ny\n0.500000\n2.500000\nhi\n3\n";

#[test]
fn static_aggregate_tables_run_at_every_optimization_level() {
    for opt in 0..=3 {
        let r = build_and_run_opt(TABLES, opt);
        assert_eq!(r.stdout, TABLES_OUTPUT, "stdout differs at --opt {opt}");
        assert_eq!(r.code, 0, "--opt {opt} stderr: {}", r.stderr);
    }
}

#[test]
fn static_aggregates_are_real_llvm_globals() {
    let ir = verified_ir(TABLES);
    assert!(
        ir.contains("@NAMES = constant [2 x { ptr, i64 }]")
            && ir.contains("@OPS = constant [1 x ptr] [ptr @add]"),
        "expected structural globals in IR:\n{ir}"
    );
    assert!(
        ir.contains("@TABLE = constant [2 x { { ptr, i64 }, i32 }]"),
        "expected a struct-array global in IR:\n{ir}"
    );
    assert!(
        ir.contains("@NESTED = constant [2 x [2 x i8]]"),
        "expected a nested array global in IR:\n{ir}"
    );
    // The initializer must be static: no constructor/init function is emitted.
    assert!(
        !ir.contains("@init_tables") && !ir.contains("llvm.global_ctors"),
        "static tables must not need runtime initialization:\n{ir}"
    );
}

#[test]
fn const_expressions_are_array_lengths() {
    let source = r#"
const A: usize = 2;
const B: usize = A * 3;
static X: [u8; B] = [1, 2, 3, 4, 5, 6];
static Y: [u8; 2 + 2] = [1, 2, 3, 4];
fn main() -> i32 {
    return X[5] as i32 + Y[3] as i32;
}
"#;
    for opt in 0..=3 {
        let r = build_and_run_opt(source, opt);
        assert_eq!(r.code, 10, "stderr at --opt {opt}: {}", r.stderr);
    }
    let ir = verified_ir(source);
    assert!(ir.contains("@X = constant [6 x i8]"), "{ir}");
    assert!(ir.contains("@Y = constant [4 x i8]"), "{ir}");
}

#[test]
fn string_literal_array_globals_keep_their_special_case() {
    let source = r#"
static NAME: [u8; 5] = "hello";
static PICK: str = "lit";
fn main() -> i32 {
    print(str_from_raw(&NAME as *u8, 5));
    print(PICK);
    return 0;
}
"#;
    let r = build_and_run_opt(source, 2);
    assert_eq!(r.stdout, "hello\nlit\n", "stderr: {}", r.stderr);
    let ir = verified_ir(source);
    assert!(ir.contains("@NAME = constant [5 x i8] c\"hello\""), "{ir}");
}

#[test]
fn non_constant_aggregate_initializers_are_rejected() {
    expect_error(
        "static mut X: i32 = 1;\nstatic A: [i32; 2] = [X, 2];\nfn main() -> i32 { return 0; }",
        "E0226",
    );
    expect_error(
        "fn foo() -> i32 { return 1; }\nstatic B: [i32; 2] = [foo(), 2];\nfn main() -> i32 { return 0; }",
        "E0226",
    );
    // Unknown names are not constants; a call is not a constant field value.
    expect_error(
        "struct S { a: i32 }\nstatic S1: S = S { a: missing };\nfn main() -> i32 { return 0; }",
        "E0201",
    );
    // Non-constant array lengths keep the existing parse-level code.
    expect_error("static A: [u8; Q] = [1];\nfn main() -> i32 { return 0; }", "E0100");
}

/// Calls through a function pointer must use the same lowered signature as the
/// definition: aggregates pass as `ptr` (slices as `{ptr,i64}` lowered to
/// `ptr`), and an aggregate return takes a leading `sret` pointer. Building the
/// call site from surface types produced IR the verifier rejected.
#[test]
fn function_pointers_lower_aggregates_through_the_internal_abi() {
    const SOURCE: &str = r#"
struct P { a: i64, b: i64 }

fn add_pair(p: P) -> i64 { return p.a + p.b; }
fn make_pair(a: i64, b: i64) -> P { return P { a: a, b: b }; }
fn first_two(xs: [i64]) -> i64 { return xs[0] + xs[1]; }
fn text_len(s: str) -> i64 { return len(s) as i64; }

fn main() -> i32 {
    let pair_sum: fn(P) -> i64 = &add_pair;
    print(pair_sum(P { a: 1, b: 2 }));

    let pair_make: fn(i64, i64) -> P = &make_pair;
    let p: P = pair_make(3, 4);
    print(p.a);
    print(p.b);

    let xs: [i64; 2] = [10, 20];
    let slice_sum: fn([i64]) -> i64 = &first_two;
    print(slice_sum(xs[..]));

    let len_of: fn(str) -> i64 = &text_len;
    print(len_of("hello"));
    return 0;
}
"#;

    for opt in 0..=3 {
        let r = build_and_run_opt(SOURCE, opt);
        assert_eq!(r.stdout, "3\n3\n4\n30\n5\n", "stdout differs at --opt {opt}");
        assert_eq!(r.code, 0, "--opt {opt} stderr: {}", r.stderr);
    }
}

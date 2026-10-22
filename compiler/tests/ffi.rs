//! C ABI and FFI coverage: extern declarations, exported CoreX functions,
//! System V AMD64 aggregate passing, variadic C calls, C strings, and
//! `main(argc, argv, envp)`.
//!
//! The C helper is compiled with the system `cc` and linked statically with
//! `-nostdlib`, so the tests exercise the same freestanding link path as the
//! compiler's default profile.

use std::path::Path;
use std::process::Command;

use corex::diagnostics::SourceFile;
use corex::driver::{self, LinkInputs};

struct RunResult {
    stdout: String,
    stderr: String,
    code: i32,
}

fn cc() -> String {
    std::env::var("COREX_CC").unwrap_or_else(|_| "cc".to_string())
}

fn compile_helper(dir: &Path, source: &str) -> std::path::PathBuf {
    let c_path = dir.join("helper.c");
    let obj = dir.join("helper.o");
    std::fs::write(&c_path, source).expect("write helper C source");
    let out = Command::new(cc())
        .args([
            "-c",
            "-O2",
            "-fno-builtin",
            "-fno-stack-protector",
            "-fno-asynchronous-unwind-tables",
            "-o",
        ])
        .arg(&obj)
        .arg(&c_path)
        .output()
        .expect("run C compiler");
    assert!(
        out.status.success(),
        "helper compilation failed:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    obj
}

fn build_and_run(
    dir: &Path,
    source: &str,
    inputs: &LinkInputs,
    args: &[&str],
) -> RunResult {
    let src_path = dir.join("program.cx");
    std::fs::write(&src_path, source).expect("write source");
    let file = SourceFile::new(src_path.display().to_string(), source);
    let exe = dir.join("program");
    let artifacts = match driver::build_with(&file, &exe, 2, false, inputs) {
        Ok(a) => a,
        Err(diags) => panic!("compilation failed:\n{}", file.render_all(&diags)),
    };
    let out = Command::new(&artifacts.executable)
        .args(args)
        .output()
        .expect("execute compiled program");
    RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

const HELPER_C: &str = r#"
typedef struct { long a; long b; } Pair;
typedef struct { double x; double y; } Vec2;

long c_add(long a, long b) { return a + b; }
long c_apply(long (*f)(long, long), long a, long b) { return f(a, b); }
Pair c_make_pair(long a, long b) { Pair p; p.a = a; p.b = b; return p; }
long c_pair_sum(Pair p) { return p.a + p.b; }
Vec2 c_scale(Vec2 v, double s) { Vec2 r; r.x = v.x * s; r.y = v.y * s; return r; }

long c_sum(int count, ...) {
    __builtin_va_list ap;
    __builtin_va_start(ap, count);
    long total = 0;
    for (int i = 0; i < count; i++) total += __builtin_va_arg(ap, long);
    __builtin_va_end(ap);
    return total;
}

double c_mix(int count, ...) {
    __builtin_va_list ap;
    __builtin_va_start(ap, count);
    double total = 0;
    for (int i = 0; i < count; i++) total += __builtin_va_arg(ap, double);
    __builtin_va_end(ap);
    return total;
}

long c_strlen(const char *s) {
    long n = 0;
    while (s[n]) n++;
    return n;
}

extern long cx_export(long);
long c_call_corex(long x) { return cx_export(x) + 1; }
"#;

#[test]
fn extern_c_calls_exports_aggregates_and_varargs() {
    let work = driver::temp_dir("corex-ffi");
    std::fs::create_dir_all(&work).expect("create test dir");
    let helper = compile_helper(&work, HELPER_C);
    let inputs = LinkInputs {
        objects: vec![helper],
        ..Default::default()
    };
    let r = build_and_run(
        &work,
        r#"
        struct Pair { a: i64, b: i64 }
        struct Vec2 { x: f64, y: f64 }

        extern "C" fn c_add(a: i64, b: i64) -> i64;
        extern "C" fn c_apply(f: fn(i64, i64) -> i64, a: i64, b: i64) -> i64;
        extern "C" fn c_make_pair(a: i64, b: i64) -> Pair;
        extern "C" fn c_pair_sum(p: Pair) -> i64;
        extern "C" fn c_scale(v: Vec2, s: f64) -> Vec2;
        extern "C" fn c_sum(count: i32, ...) -> i64;
        extern "C" fn c_mix(count: i32, ...) -> f64;
        extern "C" fn c_strlen(s: cstr) -> i64;
        extern "C" fn c_call_corex(x: i64) -> i64;

        fn add(a: i64, b: i64) -> i64 { return a + b; }

        extern "C" fn cx_export(x: i64) -> i64 { return x * 2; }

        fn main() -> i32 {
            print(c_add(20, 22));
            print(c_apply(&add, 3, 4));
            let p: Pair = c_make_pair(7, 8);
            print(p.a);
            print(p.b);
            print(c_pair_sum(p));
            let v: Vec2 = c_scale(Vec2 { x: 1.5, y: 2.5 }, 2.0);
            print(v.x);
            print(v.y);
            print(c_sum(3, 10 as i64, 20 as i64, 30 as i64));
            print(c_mix(2, 1.5 as f32, 2.5));
            print(c_strlen(c"hello"));
            print(c_call_corex(5));
            return 0;
        }
        "#,
        &inputs,
        &[],
    );
    assert_eq!(
        r.stdout,
        "42\n7\n7\n8\n15\n3.000000\n5.000000\n60\n4.000000\n5\n11\n"
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    let _ = std::fs::remove_dir_all(&work);
}

#[test]
fn main_receives_argc_argv_and_envp() {
    let work = driver::temp_dir("corex-args");
    std::fs::create_dir_all(&work).expect("create test dir");
    let r = build_and_run(
        &work,
        r#"
        fn main(argc: i32, argv: **u8, envp: **u8) -> i32 {
            print(argc);
            unsafe {
                let first: *u8 = *argv;
                print(*first as i32);
                let env: *u8 = *envp;
                let has_eq: bool = cstr_len(env as cstr) > 0;
                print(has_eq);
            }
            return 7;
        }
        "#,
        &LinkInputs::default(),
        &["alpha", "beta"],
    );
    assert!(r.stdout.starts_with("3\n"), "stdout: {:?}", r.stdout);
    assert!(r.stdout.contains("true"), "stdout: {:?}", r.stdout);
    assert_eq!(r.code, 7, "stderr: {}", r.stderr);
    let _ = std::fs::remove_dir_all(&work);
}

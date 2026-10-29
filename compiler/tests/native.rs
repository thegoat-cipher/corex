//! End-to-end tests: compile CoreX programs to native ELF binaries and execute
//! them, asserting observable stdout and exit statuses.
//!
//! These tests require `llvm-as`, `llc`, and a `cc` that can link a static
//! freestanding x86-64 ELF (see `COREX_LLVM_BIN` in the README).

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver;

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples")
}

struct RunResult {
    stdout: String,
    stderr: String,
    code: i32,
}

fn build_and_run(source: &str) -> RunResult {
    build_and_run_opt(source, 2)
}

fn build_and_run_opt(source: &str, opt: u8) -> RunResult {
    let work = driver::temp_dir("corex-test");
    std::fs::create_dir_all(&work).expect("create test dir");
    let src_path = work.join("program.cx");
    std::fs::write(&src_path, source).expect("write source");
    let file = SourceFile::new(src_path.display().to_string(), source);

    let exe = work.join("program");
    let artifacts = match driver::build(&file, &exe, opt, false) {
        Ok(a) => a,
        Err(diags) => {
            panic!(
                "compilation failed:\n{}",
                file.render_all(&diags)
            );
        }
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

fn run_example(name: &str) -> RunResult {
    let path = examples_dir().join(name);
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    build_and_run(&source)
}

#[test]
fn hello_world_uses_direct_write_syscall() {
    let r = run_example("hello.cx");
    assert_eq!(r.stdout, "Hello, World!\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn factorial_example_prints_3628800() {
    let r = run_example("factorial.cx");
    assert_eq!(r.stdout, "3628800\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn pointers_example_allocates_writes_reads_and_frees() {
    let r = run_example("pointers.cx");
    assert_eq!(r.stdout, "123456789\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn loop_sum_example_returns_51() {
    let r = run_example("loop_sum.cx");
    assert_eq!(r.stdout, "");
    assert_eq!(r.code, 51);
}

#[test]
fn recursion_and_mixed_width_arithmetic() {
    let r = build_and_run(
        r#"
        fn fib(n: u32) -> u32 {
            if n < 2 { return n; }
            return fib(n - 1) + fib(n - 2);
        }
        fn main() -> i32 {
            return fib(20) as i32;
        }
        "#,
    );
    // fib(20) = 6765, low byte of the exit status is 6765 % 256 = 109.
    assert_eq!(r.code, 6765 % 256);
}

#[test]
fn integer_arithmetic_wraps_deterministically() {
    let r = build_and_run(
        r#"
        fn print_u64(value: u64) {
            let mut digits: [u8; 20] = [0; 20];
            let mut index: usize = 20;
            let mut rest: u64 = value;
            loop {
                index = index - 1;
                digits[index] = 48 + (rest % 10) as u8;
                rest = rest / 10;
                if rest == 0 { break; }
            }
            unsafe { syscall(1, 1, &digits[index] as usize, 20 - index); }
        }
        fn fact(n: u64) -> u64 {
            if n <= 1 { return 1; }
            return n * fact(n - 1);
        }
        fn main() -> i32 {
            let wrapped: u8 = 255 + 1;
            let signed_wrap: i8 = 127 + 1;
            print_u64(wrapped as u64);
            unsafe { syscall(1, 1, &" " as usize, 1); }
            print_u64(signed_wrap as i64 as u64);
            unsafe { syscall(1, 1, &" " as usize, 1); }
            print_u64(fact(21));
            unsafe { syscall(1, 1, &"\n" as usize, 1); }
            return 0;
        }
        "#,
    );
    // 255 + 1 wraps to 0 in u8; 127 + 1 wraps to -128 in i8; 21! wraps in u64.
    assert_eq!(r.stdout, "0 18446744073709551488 14197454024290336768\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn shift_amounts_are_masked() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let one: u8 = 1;
            let a: u8 = one << 8;   // 8 & 7 == 0, so result is 1
            let b: u8 = one << 3;   // 8
            return (a + b) as i32;  // 9
        }
        "#,
    );
    assert_eq!(r.code, 9, "stderr: {}", r.stderr);
}

#[test]
fn division_by_zero_aborts_with_exit_101() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let divisor: i32 = 0;
            return 10 / divisor;
        }
        "#,
    );
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("division by zero"), "stderr: {}", r.stderr);
}

#[test]
fn dynamic_out_of_bounds_aborts_with_exit_101() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let items: [u8; 4] = [1, 2, 3, 4];
            let i: usize = 5;
            return items[i] as i32;
        }
        "#,
    );
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("index out of bounds"), "stderr: {}", r.stderr);
}

#[test]
fn heap_allocation_round_trip() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            unsafe {
                let p: *mut u8 = alloc(16);
                let q: *mut u64 = p as *mut u64;
                *q = 41;
                let value: u64 = *q;
                free(p, 16);
                return value as i32;
            }
        }
        "#,
    );
    assert_eq!(r.code, 41, "stderr: {}", r.stderr);
}

#[test]
fn pointer_arithmetic_walks_an_array() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let mut buf: [u8; 3] = [10, 20, 30];
            unsafe {
                let first: *mut u8 = &mut buf[0];
                let second: *mut u8 = first + 1;
                *second = 99;
            }
            return buf[1] as i32;
        }
        "#,
    );
    assert_eq!(r.code, 99, "stderr: {}", r.stderr);
}

#[test]
fn print_formats_integers_and_bools() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            print(0);
            print(-7);
            print(true);
            print(false);
            let small: i8 = -128;
            print(small);
            let byte: u8 = 255;
            print(byte);
            let big: u64 = 18446744073709551615;
            print(big);
            let lowest: i64 = -9223372036854775808;
            print(lowest);
            return 0;
        }
        "#,
    );
    assert_eq!(
        r.stdout,
        "0\n-7\ntrue\nfalse\n-128\n255\n18446744073709551615\n-9223372036854775808\n"
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn exit_code_is_truncated_to_the_low_byte() {
    let r = build_and_run("fn main() -> i32 { return 300; }");
    assert_eq!(r.code, 300 % 256);
}

#[test]
fn optimized_entry_point_preserves_stack_alignment() {
    // Regression: the kernel enters `_start` with %rsp 16-byte aligned, while
    // LLVM assumes the SysV callee state. Without `alignstack=16` on `_start`,
    // optimized code that writes through an array pointer (memset/local stores)
    // segfaulted.
    let r = build_and_run_opt(
        r#"
        fn main() -> i32 {
            let mut buf: [u8; 32] = [0; 32];
            let mut i: usize = 0;
            while i < 32 {
                buf[i] = 65 + (i % 26) as u8;
                i = i + 1;
            }
            unsafe {
                syscall(1, 1, &buf[0] as usize, 32);
                syscall(1, 1, &"\n" as usize, 1);
            }
            return 0;
        }
        "#,
        3,
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    assert_eq!(r.stdout, "ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEF\n");
}

#[test]
fn _start_fallthrough_exits_zero() {
    let r = build_and_run("fn _start() { }");
    assert_eq!(r.code, 0);
    assert_eq!(r.stdout, "");
}

#[test]
fn alloc_zero_bytes_aborts_with_exit_101() {
    let r = build_and_run(
        "fn main() -> i32 { unsafe { let p: *mut u8 = alloc(0); return 0; } }",
    );
    assert_eq!(r.code, 101);
    assert!(
        r.stderr.contains("allocation failed"),
        "stderr: {}",
        r.stderr
    );
}

#[test]
fn repeated_builds_are_byte_identical() {
    // PRD DoD-9 / NFR-D1: same source and flags must produce identical IR and
    // ELF bytes.
    let source = "fn main() -> i32 { print(7); return 0; }";
    let file = SourceFile::new("determinism.cx", source);

    let ir_a = driver::emit_ir(&file).expect("emit");
    let ir_b = driver::emit_ir(&file).expect("emit");
    assert_eq!(ir_a, ir_b, "IR differs between two emissions");

    let work = driver::temp_dir("corex-determinism");
    std::fs::create_dir_all(&work).expect("temp dir");
    let a = work.join("a");
    let b = work.join("b");
    driver::build(&file, &a, 2, false).expect("first build");
    driver::build(&file, &b, 2, false).expect("second build");
    let bytes_a = std::fs::read(&a).expect("read first");
    let bytes_b = std::fs::read(&b).expect("read second");
    let _ = std::fs::remove_dir_all(&work);
    assert_eq!(bytes_a, bytes_b, "ELF differs between two builds");
}

#[test]
fn _start_takes_precedence_over_main() {
    let r = build_and_run(
        r#"
        fn _start() {
            unsafe {
                syscall(1, 1, &"S\n" as usize, 2);
                syscall(60, 7, 0, 0, 0, 0, 0);
            }
        }
        fn main() -> i32 {
            return 3;
        }
        "#,
    );
    assert_eq!(r.stdout, "S\n");
    assert_eq!(r.code, 7);
}

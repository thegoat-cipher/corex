//! End-to-end coverage for the post-MVP language features: compound
//! assignment, inferred bindings, characters, globals, enums, floats, structs,
//! slices, strings, and `len`.

use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver;

struct RunResult {
    stdout: String,
    stderr: String,
    code: i32,
}

fn build_and_run(source: &str) -> RunResult {
    let work = driver::temp_dir("corex-features");
    std::fs::create_dir_all(&work).expect("create test dir");
    let src_path = work.join("program.cx");
    std::fs::write(&src_path, source).expect("write source");
    let file = SourceFile::new(src_path.display().to_string(), source);
    let exe = work.join("program");
    let artifacts = match driver::build(&file, &exe, 2, false) {
        Ok(a) => a,
        Err(diags) => panic!("compilation failed:\n{}", file.render_all(&diags)),
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

#[test]
fn compound_assignment_inference_and_characters() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let mut x: i32 = 10;
            x += 5; x -= 3; x *= 2; x /= 4; x %= 5;
            x <<= 2; x >>= 1; x &= 6; x |= 8; x ^= 3;
            print(x);
            let y = 41;
            print(y + 1);
            let c: char = 'A';
            print(c);
            let d = 'z';
            print(d);
            print(c as i32);
            print(66 as char);
            print(c == 'A');
            let mut m: u8 = 250;
            m += 10;
            print(m);
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "9\n42\nA\nz\n65\nB\ntrue\n4\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn globals_enums_and_constant_addresses() {
    let r = build_and_run(
        r#"
        enum Color { Red, Green = 5, Blue }
        static LIMIT: i32 = 10 + 2 * 3;
        static mut COUNTER: i32 = 0;
        static NAME: [u8; 5] = "hello";
        static mut PTR: *mut i32 = &mut COUNTER;

        fn main() -> i32 {
            print(LIMIT);
            COUNTER = 7;
            print(COUNTER);
            print(Color::Red);
            print(Color::Green);
            print(Color::Blue);
            let c: Color = Color::Blue;
            print(c == Color::Blue);
            print(c as i32);
            print(NAME[1] as i32);
            unsafe {
                print(*PTR);
                *PTR = 9;
            }
            print(COUNTER);
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "16\n7\n0\n5\n6\ntrue\n6\n101\n7\n9\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn floats_arithmetic_casts_and_printing() {
    let r = build_and_run(
        r#"
        static PI: f64 = 3.5 + 0.25;
        static HALF: f32 = 0.5;

        fn main() -> i32 {
            let a: f64 = 1.5;
            let b = 2.25;
            print(a + b);
            print((a + b) as i32);
            print((b * 2.0) as i32);
            print(a < b);
            print((-a) as i32);
            let mut f: f32 = 1.25;
            f *= 4.0;
            print(f);
            print(PI * 10.0);
            print(7 as f64 as i32);
            print(HALF + HALF);
            return 0;
        }
        "#,
    );
    assert_eq!(
        r.stdout,
        "3.750000\n3\n4\ntrue\n-1\n5.000000\n37.500000\n7\n1.000000\n"
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn struct_values_fields_params_returns_and_arrays() {
    let r = build_and_run(
        r#"
        struct Point { x: i32, y: i32 }
        struct Pair { a: Point, b: Point }

        fn make(x: i32, y: i32) -> Point {
            return Point { x: x, y: y };
        }
        fn sum(p: Point) -> i32 { return p.x + p.y; }
        fn swap(p: Point) -> Point {
            return Point { x: p.y, y: p.x };
        }

        fn main() -> i32 {
            let p: Point = Point { x: 3, y: 4 };
            print(p.x);
            print(sum(p));
            let mut q = p;
            q.x = 10;
            print(p.x);
            print(q.x);
            let r = swap(q);
            print(r.x);
            print(r.y);
            let nested: Pair = Pair { a: p, b: r };
            print(nested.a.x + nested.b.y);
            print(make(7, 8).x);
            let mut arr: [Point; 2] = [Point { x: 1, y: 2 }, Point { x: 3, y: 4 }];
            arr[1].x = 9;
            print(arr[1].x);
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "3\n7\n3\n10\n4\n10\n13\n7\n9\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn slices_strings_len_and_ranges() {
    let r = build_and_run(
        r#"
        static GREETING: str = "hi";
        static BYTES: [u8; 3] = "hey";

        fn first(s: [u8]) -> u8 { return s[0]; }
        fn echo(s: str) -> str { return s; }
        fn sum_slice(s: [i32]) -> i32 {
            let mut i: usize = 0;
            let mut total: i32 = 0;
            while i < len(s) {
                total += s[i];
                i += 1;
            }
            return total;
        }

        fn main() -> i32 {
            print("hello");
            print(len("hello"));
            print("hello"[1]);
            print("hello"[1..3]);
            print(first("abc"));
            print(echo("xyz"));
            print(GREETING);
            print(len(GREETING));
            print(BYTES[0]);

            let a: [i32; 4] = [1, 2, 3, 4];
            let s: [i32] = &a;
            print(len(s));
            print(s[2]);
            print(sum_slice(&a));
            print(sum_slice(s));
            print(s[1..3][0]);

            let t: [u8] = "hey";
            print(len(t));
            print(t[0]);
            print(t);
            return 0;
        }
        "#,
    );
    assert_eq!(
        r.stdout,
        "hello\n5\n101\nel\n97\nxyz\nhi\n2\n104\n4\n3\n10\n10\n2\n3\n104\nhey\n"
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn runtime_slice_bounds_abort() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let a: [i32; 3] = [1, 2, 3];
            let s: [i32] = &a;
            let i: usize = 3;
            print(s[i]);
            return 0;
        }
        "#,
    );
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("index out of bounds"), "stderr: {}", r.stderr);
}

#[test]
fn nested_generic_types_and_shift_operators() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let mut v: vec<vec<u8>> = vec_new(2);
            let mut a: vec<u8> = vec_new(2);
            vec_push(&mut a, 1);
            vec_push(&mut a, 2);
            vec_push(&mut v, a);
            let mut b: vec<u8> = vec_new(1);
            vec_push(&mut b, 9);
            vec_push(&mut v, b);
            print(v[0][1]);
            print(v[1][0]);
            vec_free(&mut v);

            let mut deep: vec<vec<vec<i32>>> = vec_new(1);
            let mut mid: vec<vec<i32>> = vec_new(1);
            let mut leaf: vec<i32> = vec_new(1);
            vec_push(&mut leaf, 7);
            vec_push(&mut mid, leaf);
            vec_push(&mut deep, mid);
            print(deep[0][0][0]);
            vec_free(&mut deep);

            // `>>` and `>>=` stay shift operators in expressions.
            let mut x: i32 = 64;
            let y: i32 = x >> 3;
            x >>= 2;
            print(y);
            print(x);
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "2\n9\n7\n8\n16\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn features_example_runs() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/features.cx");
    let source = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let r = build_and_run(&source);
    assert_eq!(
        r.stdout,
        "13\n4\n4\n6\ntrue\n3.000000\n66\n4\n10\npo\nore\n5\n7\n"
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

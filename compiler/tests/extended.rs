//! End-to-end coverage for C-class features: function pointers, switch/for/do,
//! labels and goto, unions, bitfields, packed/aligned layout, flexible array
//! members, `sizeof`/`alignof`/`offsetof`, compound literals, and C strings.

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
    let work = driver::temp_dir("corex-extended");
    std::fs::create_dir_all(&work).expect("create test dir");
    let src_path = work.join("program.cx");
    std::fs::write(&src_path, source).expect("write source");
    let file = SourceFile::new(src_path.display().to_string(), source);
    let exe = work.join("program");
    let artifacts = match driver::build(&file, &exe, opt, false) {
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

fn build_and_run(source: &str) -> RunResult {
    build_and_run_opt(source, 2)
}

#[test]
fn function_pointers_are_values_and_dispatch_indirectly() {
    let r = build_and_run(
        r#"
        fn inc(x: i32) -> i32 { return x + 1; }
        fn dbl(x: i32) -> i32 { return x * 2; }
        fn neg(x: i32) -> i32 { return -x; }

        fn apply(f: fn(i32) -> i32, x: i32) -> i32 { return f(x); }

        fn main() -> i32 {
            let a: fn(i32) -> i32 = &inc;
            let b: fn(i32) -> i32 = dbl;
            print(a(41));
            print(b(21));
            print(apply(&neg, 7));
            let table: [fn(i32) -> i32; 3] = [&inc, &dbl, &neg];
            let mut i: usize = 0;
            while i < 3 {
                print(table[i](5));
                i += 1;
            }
            let empty: fn(i32) -> i32 = null;
            if empty == null { print(1); }
            if a != null { print(2); }
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "42\n42\n-7\n6\n10\n-5\n1\n2\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn layout_introspection_unions_bitfields_and_compound_literals() {
    let r = build_and_run(
        r#"
        typedef Word = u64;
        struct Point { x: i32, y: i32 }
        union Value { i: i32, f: f32, b: u8 }
        packed struct Packed { tag: u8, value: u32 }
        align(16) struct Aligned { x: u8 }
        struct Bits { a: u8: 3, b: u8: 5, flag: u8: 1 }
        struct Flexible { len: u64, data: [u8; 0] }

        fn main() -> i32 {
            print(sizeof(Point));
            print(alignof(Point));
            print(offsetof(Point, y));
            print(sizeof(Packed));
            print(sizeof(Aligned));
            print(alignof(Aligned));
            print(sizeof(Bits));
            print(sizeof(Flexible));
            print(sizeof(Word));
            let p: Point = Point { y: 2, x: 1 };
            print(p.x);
            print(p.y);
            let v: Value = Value { i: 65 };
            print(v.i);
            print(v.b);
            let mut bits: Bits = Bits { a: 5, b: 17, flag: 1 };
            print(bits.a);
            print(bits.b);
            print(bits.flag);
            bits.a = 2;
            bits.flag = 0;
            print(bits.a);
            print(bits.b);
            print(bits.flag);
            let c: Point = Point { x: 9, ... };
            print(c.x);
            print(c.y);
            print(Aligned { x: 3 }.x);
            let arr: [i32; 4] = [i32; 4] { 1: 10, 3: 30 };
            print(arr[0]);
            print(arr[1]);
            print(arr[2]);
            print(arr[3]);
            return 0;
        }
        "#,
    );
    assert_eq!(
        r.stdout,
        "8\n4\n4\n5\n16\n16\n2\n8\n8\n1\n2\n65\n65\n5\n17\n1\n2\n17\n0\n9\n0\n3\n0\n10\n0\n30\n"
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn switch_fallthrough_for_do_while_and_goto() {
    let r = build_and_run(
        r#"
        fn classify(x: i32) -> i32 {
            let mut acc: i32 = 0;
            switch x {
                case 0 { acc = acc + 1; }
                case 1 { acc = acc + 10; break; }
                case 2 { acc = acc + 100; }
                default { acc = acc + 1000; }
            }
            return acc;
        }

        fn main() -> i32 {
            print(classify(0));
            print(classify(1));
            print(classify(2));
            print(classify(3));
            let mut sum: i32 = 0;
            for (let mut i: i32 = 0; i < 5; i += 1) {
                sum += i;
            }
            print(sum);
            let mut n: i32 = 0;
            do { n += 1; } while (n < 3);
            print(n);
            let mut i: i32 = 0;
            top:
            if i < 3 { i += 1; goto top; }
            print(i);
            goto skip;
            print(999);
            skip:
            print(42);
            let mut k: i32 = 0;
            for (;;) {
                k += 1;
                if k == 4 { break; }
            }
            print(k);
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "11\n10\n1100\n1000\n10\n3\n3\n42\n4\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn switch_works_at_every_optimization_level() {
    for opt in 0..=3 {
        let r = build_and_run_opt(
            r#"
            fn main() -> i32 {
                let mut total: i32 = 0;
                for (let mut i: i32 = 0; i < 6; i += 1) {
                    switch i {
                        case 0 { total += 1; }
                        case 1 { total += 10; break; }
                        case 2 { total += 100; }
                        case 3 { total += 1000; break; }
                        default { total += 10000; }
                    }
                }
                print(total);
                return 0;
            }
            "#,
            opt,
        );
        assert_eq!(r.stdout, "22121\n", "at --opt {opt}");
        assert_eq!(r.code, 0, "at --opt {opt}: {}", r.stderr);
    }
}

#[test]
fn switch_with_default_satisfies_return_analysis() {
    let source = r#"
        enum K { A, B }
        fn f(k: K) -> i32 {
            switch k {
            case K::A: { return 1; }
            default:  { return 2; }
            }
        }
        fn main() -> i32 { return f(K::B); }
    "#;
    for opt in 0..=3 {
        let r = build_and_run_opt(source, opt);
        assert_eq!(r.stdout, "", "at --opt {opt}");
        assert_eq!(r.code, 2, "at --opt {opt}: {}", r.stderr);
    }
}

#[test]
fn declarative_macros_expand_at_compile_time() {
    let r = build_and_run(
        r#"
        macro square(x) = x * x;
        macro add3(a, b, c) = a + b + c;
        macro twice(e) = e + e;

        fn main() -> i32 {
            print(square(5));
            print(add3(1, 2, 3));
            print(twice(square(3)));
            let p: i32 = 4;
            // The argument is an expression tree, so precedence is preserved.
            print(square(p + 1));
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "25
6
18
25
");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn c_strings_convert_in_both_directions() {
    let r = build_and_run(
        r#"
        fn main() -> i32 {
            let c: cstr = c"world";
            print(c);
            print(cstr_len(c));
            let s: str = c"hello" as str;
            print(s);
            let from_c: str = cstr_to_str(c);
            print(from_c);
            unsafe {
                let buf: *mut u8 = alloc(16);
                let out: cstr = str_to_cstr(buf, 16, "abc");
                print(out);
                print(cstr_len(out));
                free(buf, 16);
            }
            print(len("plain"));
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "world\n5\nhello\nworld\nabc\n3\n5\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

/// A signed bitfield is stored in its low `width` bits and must be
/// sign-extended on read; zero-extension turned `-1` in four bits into `15`.
#[test]
fn signed_bitfield_reads_sign_extend() {
    let r = build_and_run(
        r#"
        struct Signed { a: i32: 4, b: i32: 5, u: u32: 4 }
        fn main() -> i32 {
            let mut s: Signed = Signed { a: -1, b: -16, u: 15 };
            print(s.a);
            print(s.b);
            print(s.u);
            s.a = -8;
            s.b = 7;
            print(s.a);
            print(s.b);
            print(s.a + 8);
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "-1\n-16\n15\n-8\n7\n0\n", "stderr: {}", r.stderr);
    assert_eq!(r.code, 0);
}

/// Labels name a statement; they do not create a breakable scope. A labeled
/// `break` still exits the enclosing `while true` (so the function can fall
/// through and must be rejected), and a labeled `return` still diverges.
#[test]
fn labeled_break_and_labeled_return_follow_control_flow() {
    for source in [
        "fn main() -> i32 { while true { l: break; } }",
        "fn main() -> i32 { while true { if true { l: break; } } }",
    ] {
        let file = SourceFile::new("label-break.cx", source);
        match driver::check(&file) {
            Ok(_) => panic!("labeled break was not seen by the return analysis:\n{source}"),
            Err(diags) => assert!(
                diags.iter().any(|d| d.code == "E0206"),
                "expected E0206 for:\n{source}\ngot {diags:?}"
            ),
        }
    }

    // A labeled `continue` does not leave the loop, so the loop still diverges.
    let file = SourceFile::new(
        "label-continue.cx",
        "fn main() -> i32 { l: while true { continue; } }",
    );
    driver::check(&file)
        .unwrap_or_else(|diags| panic!("labeled continue rejected: {}", file.render_all(&diags)));

    for source in [
        "fn main() -> i32 { done: return 7; }",
        "fn main() -> i32 { done: { return 7; } }",
        "fn main() -> i32 { done: if true { return 7; } else { return 8; } }",
    ] {
        let r = build_and_run(source);
        assert_eq!(r.code, 7, "source:\n{source}\nstderr: {}", r.stderr);
    }
}

//! Coverage for the dynamic arrays, const values, file I/O, imports, and
//! error-handling builtins used to write compiler-style programs in CoreX.

use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver;

#[derive(Debug)]
struct Run {
    stdout: String,
    stderr: String,
    code: i32,
}

fn compile_and_run(file: &SourceFile) -> Result<Run, String> {
    let work = driver::temp_dir("corex-caps");
    std::fs::create_dir_all(&work).map_err(|e| e.to_string())?;
    let exe = work.join("program");
    match driver::build(file, &exe, 2, false) {
        Ok(artifacts) => {
            let out = Command::new(&artifacts.executable)
                .output()
                .map_err(|e| e.to_string())?;
            let result = Run {
                stdout: String::from_utf8_lossy(&out.stdout).to_string(),
                stderr: String::from_utf8_lossy(&out.stderr).to_string(),
                code: out.status.code().unwrap_or(-1),
            };
            let _ = std::fs::remove_dir_all(&work);
            Ok(result)
        }
        Err(diags) => {
            let rendered = file.render_all(&diags);
            let _ = std::fs::remove_dir_all(&work);
            Err(rendered)
        }
    }
}

fn run_source(source: &str) -> Run {
    let file = SourceFile::new("caps.cx", source);
    compile_and_run(&file).unwrap_or_else(|e| panic!("compilation failed:\n{e}"))
}

#[test]
fn dynamic_arrays_grow_index_and_free() {
    let r = run_source(
        r#"
        struct Point { x: i32, y: i32 }
        fn sum(s: [i32]) -> i32 {
            let mut i: usize = 0;
            let mut total: i32 = 0;
            while i < len(s) { total += s[i]; i += 1; }
            return total;
        }
        fn main() -> i32 {
            let mut v: vec<i32> = vec_new(0);
            vec_push(&mut v, 1);
            vec_push(&mut v, 2);
            vec_push(&mut v, 3);
            vec_push(&mut v, 4);
            vec_push(&mut v, 5);
            print(len(v));
            print(v[4]);
            let s: [i32] = vec_slice(&v);
            print(len(s));
            print(sum(s));
            vec_free(&mut v);
            print(len(v));

            let mut ps: vec<Point> = vec_new(1);
            vec_push(&mut ps, Point { x: 6, y: 7 });
            vec_push(&mut ps, Point { x: 8, y: 9 });
            print(ps[1].y);
            print(len(ps));
            vec_free(&mut ps);
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "5\n5\n5\n15\n0\n9\n2\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn compile_time_constants() {
    let r = run_source(
        r#"
        enum Color { Red = 3, Blue }
        const A: i32 = 5;
        const B: i32 = A * 2 + 1;
        const S: str = "const";
        const C: char = 'Q';
        const F: f64 = 1.25 * 2.0;
        const K: Color = Color::Blue;
        const P: *i32 = &G;
        static G: i32 = 41;
        static H: i32 = B;
        fn main() -> i32 {
            print(A); print(B); print(S); print(len(S));
            print(C as i32); print(F); print(K as i32); print(H);
            unsafe { print(*P); }
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "5\n11\nconst\n5\n81\n2.500000\n4\n11\n41\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn file_io_round_trip_and_missing_file() {
    let dir = driver::temp_dir("corex-file");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("data.txt");
    let source = format!(
        r#"
        fn main() -> i32 {{
            let rc: i32 = write_file("{p}", "hello file");
            print(rc);
            let mut v: vec<u8> = read_file("{p}");
            print(len(v));
            let s: [u8] = vec_slice(&v);
            print(s);
            vec_free(&mut v);
            let mut missing: vec<u8> = read_file("{missing}");
            print(len(missing));
            vec_free(&mut missing);
            return 0;
        }}
        "#,
        p = path.display(),
        missing = dir.join("does-not-exist").display()
    );
    let r = run_source(&source);
    assert_eq!(r.stdout, "0\n10\nhello file\n0\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    let written = std::fs::read_to_string(&path).unwrap();
    assert_eq!(written, "hello file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn imports_resolve_relative_files_and_diagnose_paths() {
    let dir = driver::temp_dir("corex-imports");
    std::fs::create_dir_all(&dir).unwrap();
    let util = dir.join("util.cx");
    let lib = dir.join("lib.cx");
    let main = dir.join("main.cx");
    std::fs::write(&util, "fn twice(x: i32) -> i32 { return x * 2; }\n").unwrap();
    std::fs::write(
        &lib,
        "import \"util.cx\";\nconst BASE: i32 = 40;\nstruct Point { x: i32 }\nfn add(a: i32, b: i32) -> i32 { return twice(a) + b; }\nfn mk() -> Point { return Point { x: BASE }; }\n",
    ).unwrap();
    let root = "import \"lib.cx\";\nconst N: i32 = 2;\nfn main() -> i32 { print(add(4, N)); let p = mk(); print(p.x); return 0; }\n";
    std::fs::write(&main, root).unwrap();

    let base = SourceFile::new(main.display().to_string(), root);
    let expanded = match driver::load_source(&base) {
        Ok(f) => f,
        Err(diags) => panic!("load failed:\n{}", base.render_all(&diags)),
    };
    let r = compile_and_run(&expanded).unwrap();
    assert_eq!(r.stdout, "10\n40\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);

    let bad = dir.join("bad.cx");
    std::fs::write(&bad, "fn broken() -> i32 {\n    return missing_name;\n}\n").unwrap();
    let bad_root = "import \"bad.cx\";\nfn main() -> i32 { return 0; }\n";
    let bad_file = SourceFile::new(main.display().to_string(), bad_root);
    let expanded_bad = driver::load_source(&bad_file).unwrap();
    let err = compile_and_run(&expanded_bad).unwrap_err();
    assert!(err.contains("bad.cx:2:12"), "diagnostic:\n{err}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn panic_and_assert_abort_with_messages() {
    let r = run_source(
        r#"
        fn require_positive(x: i32) -> i32 {
            assert(x > 0, "x must be positive");
            return x;
        }
        fn always_panics() -> i32 {
            panic("unreachable");
        }
        fn main() -> i32 {
            print(require_positive(6));
            print(require_positive(0));
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "6\n");
    assert_eq!(r.code, 101);
    assert!(r.stderr.contains("x must be positive"), "stderr: {}", r.stderr);
}

#[test]
fn panic_satisfies_return_analysis() {
    // `always_panics` never returns, so it needs no trailing return.
    let r = run_source("fn f() -> i32 { panic(\"stop\"); } fn main() -> i32 { return 0; }");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn compiler_data_example_tokenizes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/compiler_data.cx");
    let source = std::fs::read_to_string(&path).unwrap();
    let file = SourceFile::new(path.display().to_string(), source);
    let r = compile_and_run(&file).unwrap();
    assert_eq!(r.stdout, "9\n0\nfn\n4\n1\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

#[test]
fn recursive_structs_via_pointers() {
    let r = run_source(
        r#"
        struct Node { value: i32, next: *Node }
        fn null_ptr() -> *Node { unsafe { return 0 as *Node; } }
        fn main() -> i32 {
            let a = Node { value: 2, next: null_ptr() };
            let b = Node { value: 1, next: &a };
            unsafe { print((*b.next).value); }
            return 0;
        }
        "#,
    );
    assert_eq!(r.stdout, "2\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

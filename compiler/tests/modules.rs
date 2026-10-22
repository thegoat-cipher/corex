//! File modules, separate compilation, public interfaces, deterministic symbol
//! mangling, conditional compilation, and the `corex.toml` build driver.

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
use corex::driver::{self, BuildConfig, LinkInputs};

struct RunResult {
    stdout: String,
    stderr: String,
    code: i32,
}

fn run(exe: &Path, args: &[&str]) -> RunResult {
    let out = Command::new(exe).args(args).output().expect("run program");
    RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

fn write(dir: &Path, name: &str, text: &str) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("create directory");
    }
    std::fs::write(&path, text).expect("write source");
    path
}

fn build_root(
    root: &Path,
    out: &Path,
    config: &BuildConfig,
    inputs: &LinkInputs,
) -> Result<driver::Artifacts, Vec<corex::diagnostics::Diagnostic>> {
    let text = std::fs::read_to_string(root).expect("read root");
    let file = SourceFile::new(root.display().to_string(), text);
    driver::build_with_config(&file, out, 2, false, inputs, config)
}

#[test]
fn file_modules_export_public_items_and_hide_private_ones() {
    let dir = driver::temp_dir("corex-modules");
    std::fs::create_dir_all(&dir).expect("create dir");
    write(
        &dir,
        "math.cx",
        r#"
        module math;

        pub struct Vec2 { x: i32, y: i32 }
        pub enum Op { Add, Sub, Mul }
        pub const K: i32 = 5;

        pub fn add(a: i32, b: i32) -> i32 { return a + b; }
        pub fn make(x: i32, y: i32) -> Vec2 { return Vec2 { x: x, y: y }; }
        pub fn apply(op: Op, a: i32, b: i32) -> i32 {
            switch op {
                case Op::Add { return a + b; }
                case Op::Sub { return a - b; }
                case Op::Mul { return a * b; }
            }
            return 0;
        }
        fn secret() -> i32 { return 77; }
        pub fn revealed() -> i32 { return secret(); }
        "#,
    );
    let main = write(
        &dir,
        "main.cx",
        r#"
        module app;
        mod math;

        fn main() -> i32 {
            print(math::add(20, 22));
            let v: math::Vec2 = math::make(3, 4);
            print(v.x);
            print(v.y);
            print(math::K);
            print(math::apply(math::Op::Mul, 6, 7));
            print(math::revealed());
            return 0;
        }
        "#,
    );
    let exe = dir.join("program");
    build_root(&main, &exe, &BuildConfig::default(), &LinkInputs::default())
        .unwrap_or_else(|d| {
            let file = SourceFile::new(main.display().to_string(), std::fs::read_to_string(&main).unwrap());
            panic!("compile failed:\n{}", file.render_all(&d))
        });
    let r = run(&exe, &[]);
    assert_eq!(r.stdout, "42\n3\n4\n5\n42\n77\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);

    // The private function is mangled but is not declared by the interface, so
    // another module cannot call it.
    let bad = write(
        &dir,
        "bad.cx",
        r#"
        module bad;
        mod math;
        fn main() -> i32 { print(math::secret()); return 0; }
        "#,
    );
    let err = match build_root(
        &bad,
        &dir.join("bad"),
        &BuildConfig::default(),
        &LinkInputs::default(),
    ) {
        Err(diags) => diags,
        Ok(_) => panic!("private module item was accessible"),
    };
    assert!(
        err.iter().any(|d| d.message.contains("undefined item `math::secret`")),
        "{err:?}"
    );

    // `nm` proves the deterministic mangling scheme.
    let nm = Command::new("nm").arg(&exe).output();
    if let Ok(nm) = nm {
        if nm.status.success() {
            let symbols = String::from_utf8_lossy(&nm.stdout);
            assert!(symbols.contains("math$add"), "symbols:\n{symbols}");
            assert!(symbols.contains("app$main") || symbols.contains(" main"), "symbols:\n{symbols}");
            assert!(symbols.contains("math$secret"), "symbols:\n{symbols}");
        }
    }
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn cfg_attributes_select_items_and_target_keys() {
    let dir = driver::temp_dir("corex-cfg");
    std::fs::create_dir_all(&dir).expect("create dir");
    let root = write(
        &dir,
        "cfg.cx",
        r#"
        #[cfg(feature = "fast")]
        fn value() -> i32 { return 1; }

        #[cfg(all(not(feature = "fast"), target_os = "linux"))]
        fn value() -> i32 { return 2; }

        #[cfg(target_os = "windows")]
        fn value() -> i32 { return 3; }

        fn main() -> i32 { print(value()); return 0; }
        "#,
    );
    let exe = dir.join("program");
    build_root(&root, &exe, &BuildConfig::default(), &LinkInputs::default()).expect("default cfg");
    assert_eq!(run(&exe, &[]).stdout, "2\n");

    let config = BuildConfig {
        cfg: vec!["feature=fast".to_string()],
        ..Default::default()
    };
    build_root(&root, &exe, &config, &LinkInputs::default()).expect("feature cfg");
    assert_eq!(run(&exe, &[]).stdout, "1\n");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn shared_library_can_be_loaded_with_dlopen() {
    let dir = driver::temp_dir("corex-dlopen");
    std::fs::create_dir_all(&dir).expect("create dir");
    let lib_source = r#"
        module shlib;
        extern "C" fn sh_add(a: i32, b: i32) -> i32 { return a + b; }
        extern "C" fn sh_mul(a: i32, b: i32) -> i32 { return a * b; }
    "#;
    let lib_path = dir.join("shlib.cx");
    std::fs::write(&lib_path, lib_source).expect("write lib");
    let lib_file = SourceFile::new(lib_path.display().to_string(), lib_source);
    let so = dir.join("libsh.so");
    let lib_config = BuildConfig {
        require_entry: false,
        ..Default::default()
    };
    driver::build_project(
        std::slice::from_ref(&lib_file),
        &so,
        2,
        &LinkInputs::default(),
        &lib_config,
        driver::OutputKind::Shared,
    )
    .expect("shared library build");
    assert!(so.is_file());

    let program = r#"
        mod std_dl;
        fn main() -> i32 {
            unsafe {
                let h: *mut u8 = std_dl::open(c"SHLIB_PATH");
                print(h != null);
                if h == null { return 1; }
                let add: fn(i32, i32) -> i32 = std_dl::sym(h, c"sh_add") as fn(i32, i32) -> i32;
                let mul: fn(i32, i32) -> i32 = std_dl::sym(h, c"sh_mul") as fn(i32, i32) -> i32;
                print(add(20, 22));
                print(mul(6, 7));
                std_dl::close(h);
            }
            return 0;
        }
    "#;
    let path_literal = format!("c\"{}\"", so.display());
    let program = program.replace("c\"SHLIB_PATH\"", &path_literal);
    let src = dir.join("program.cx");
    std::fs::write(&src, &program).expect("write program");
    let file = SourceFile::new(src.display().to_string(), program);
    let exe = dir.join("program");
    let config = BuildConfig {
        hosted: true,
        include_paths: vec![Path::new(env!("CARGO_MANIFEST_DIR")).join("../lib/corex")],
        ..Default::default()
    };
    let hosted_inputs = LinkInputs {
        hosted: true,
        ..Default::default()
    };
    driver::build_with_config(&file, &exe, 2, false, &hosted_inputs, &config)
        .expect("hosted build");
    let r = run(&exe, &[]);
    assert_eq!(r.stdout, "true\n42\n42\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn manifest_builds_executable_and_static_library() {
    let dir = driver::temp_dir("corex-manifest");
    std::fs::create_dir_all(&dir).expect("create dir");
    write(
        &dir,
        "corex.toml",
        r#"
        [package]
        name = "demo"

        [build]
        kind = "exe"
        sources = ["src/main.cx"]
        include_paths = ["src"]
        opt = 2
        features = ["fast"]
        "#,
    );
    write(
        &dir,
        "src/math.cx",
        r#"
        module math;
        pub fn square(x: i32) -> i32 { return x * x; }
        pub const BASE: i32 = 40;
        "#,
    );
    write(
        &dir,
        "src/main.cx",
        r#"
        module app;
        mod math;
        fn main() -> i32 {
            print(math::square(6));
            print(math::BASE + 2);
            return 0;
        }
        "#,
    );
    let exe = driver::build_manifest(&dir, None, None, &[]).expect("manifest exe");
    let r = run(&exe, &[]);
    assert_eq!(r.stdout, "36\n42\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);

    // Static library target, then link it into a standalone CoreX program.
    let libdir = dir.join("libproj");
    std::fs::create_dir_all(&libdir).expect("create lib dir");
    write(
        &libdir,
        "corex.toml",
        r#"
        [package]
        name = "mathlib"

        [build]
        kind = "staticlib"
        sources = ["math.cx"]
        "#,
    );
    write(
        &libdir,
        "math.cx",
        r#"
        module math;
        pub fn double(x: i64) -> i64 { return x * 2; }
        "#,
    );
    let archive = driver::build_manifest(&libdir, None, None, &[]).expect("manifest staticlib");
    assert_eq!(archive.extension().and_then(|e| e.to_str()), Some("a"));
    let ar = Command::new("llvm-ar").arg("t").arg(&archive).output();
    if let Ok(ar) = ar {
        if ar.status.success() {
            assert!(String::from_utf8_lossy(&ar.stdout).contains(".o"));
        }
    }

    let consumer = write(
        &dir,
        "consumer.cx",
        r#"
        extern "corex" fn math$double(x: i64) -> i64;
        fn main() -> i32 {
            print(math$double(21));
            return 0;
        }
        "#,
    );
    let inputs = LinkInputs {
        objects: vec![archive],
        ..Default::default()
    };
    let consumer_exe = dir.join("consumer");
    build_root(&consumer, &consumer_exe, &BuildConfig::default(), &inputs)
        .expect("link static archive");
    assert_eq!(run(&consumer_exe, &[]).stdout, "42\n");
    let _ = std::fs::remove_dir_all(&dir);
}

//! IR-level tests: every example must produce LLVM 20 IR that the LLVM
//! verifier accepts, and the emitter must not regress to constructs that only
//! appear valid (for example constant expressions wrapping function-local
//! values).

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
use corex::driver;

fn examples() -> Vec<PathBuf> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples");
    let mut out: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("examples directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "cx").unwrap_or(false))
        .collect();
    out.sort();
    assert!(!out.is_empty(), "no examples found in {}", dir.display());
    out
}

fn emit(path: &Path) -> String {
    let source = std::fs::read_to_string(path).expect("read example");
    let file = SourceFile::new(path.display().to_string(), source);
    match driver::emit_ir(&file) {
        Ok(ir) => ir,
        Err(diags) => panic!("{} failed:\n{}", path.display(), file.render_all(&diags)),
    }
}

fn verify_ir(ir: &str) -> Result<(), String> {
    let llvm_as = driver::llvm_tool("llvm-as")
        .ok_or_else(|| "llvm-as not found; set COREX_LLVM_BIN".to_string())?;
    let dir = driver::temp_dir("corex-ir-test");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("module.ll");
    std::fs::write(&path, ir).map_err(|e| e.to_string())?;
    let out = Command::new(llvm_as)
        .arg(&path)
        .arg("-o")
        .arg(dir.join("module.bc"))
        .output()
        .map_err(|e| e.to_string())?;
    let _ = std::fs::remove_dir_all(&dir);
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).to_string())
    }
}

#[test]
fn every_example_emits_verifiable_ir() {
    for path in examples() {
        let ir = emit(&path);
        verify_ir(&ir).unwrap_or_else(|e| {
            panic!("IR verification failed for {}:\n{e}\n--- IR ---\n{ir}", path.display())
        });
    }
}

#[test]
fn ir_targets_linux_x86_64_without_libc_calls() {
    for path in examples() {
        let ir = emit(&path);
        assert!(
            ir.contains("target triple = \"x86_64-unknown-linux-gnu\""),
            "{}: missing target triple",
            path.display()
        );
        for forbidden in ["@printf", "@malloc", "@free\""] {
            assert!(
                !ir.contains(forbidden),
                "{}: IR references libc symbol {forbidden}",
                path.display()
            );
        }
        assert!(!ir.contains("undef"), "{}: IR contains undef", path.display());
    }
}

#[test]
fn no_constant_expression_wraps_function_local_values() {
    // Regression: `call void @f(i64 ptrtoint (ptr %local to i64))` is invalid
    // LLVM; pointer-to-integer conversions must be instructions.
    for path in examples() {
        let ir = emit(&path);
        assert!(
            !ir.contains("ptrtoint (ptr %"),
            "{}: function-local ptrtoint used inside a constant expression",
            path.display()
        );
    }
}

#[test]
fn factorial_ir_contains_expected_structure() {
    let ir = emit(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/factorial.cx"));
    assert!(ir.contains("define i64 @fact(i64 %p0)"));
    assert!(ir.contains("call i64 @fact(i64"));
    assert!(ir.contains("udiv i64") || ir.contains("sdiv i64"));
    assert!(ir.contains("@__corex_syscall"));
    assert!(ir.contains("call i64 asm sideeffect \"syscall\""));
}

#[test]
fn start_emits_exactly_one_exit_path() {
    // With a body that ends in `exit`, the synthesized fallthrough exit must be
    // omitted; without one, it must be present.
    let hello = emit(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/hello.cx"));
    assert_eq!(
        hello.matches("__corex_syscall(i64 60").count(),
        1,
        "hello IR:\n{hello}"
    );

    let cast_exit = SourceFile::new(
        "cast.cx",
        "fn _start() { unsafe { syscall(60 as usize, 0, 0, 0, 0, 0, 0); } }",
    );
    let ir = driver::emit_ir(&cast_exit).expect("emit");
    // The cast prints as `i64 %t…`, so count syscall call sites, not literals.
    assert_eq!(
        ir.matches("call i64 @__corex_syscall(i64").count(),
        1,
        "constant-cast exit IR:\n{ir}"
    );

    let empty = SourceFile::new("empty.cx", "fn _start() { }");
    let ir = driver::emit_ir(&empty).expect("emit");
    assert_eq!(
        ir.matches("__corex_syscall(i64 60").count(),
        1,
        "empty _start IR:\n{ir}"
    );
}

#[test]
fn hello_ir_uses_a_single_explicit_exit_path_and_raw_start() {
    let ir = emit(&Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/hello.cx"));
    assert!(ir.contains("define void @_start()"));
    assert!(ir.contains("i64 1, i64 1")); // write(fd=1, count=1) argument pair
    assert!(ir.contains("i64 60")); // exit syscall number
}

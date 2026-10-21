//! Compilation driver: the only module that touches the filesystem and
//! external tools.
//!
//! Pipeline: frontend (`lexer` -> `parser` -> `semantic`) -> `codegen` IR ->
//! `llvm-as` (assembles and runs the LLVM verifier) -> `llc` (native object) ->
//! `cc -nostdlib -static -no-pie` (ELF executable).
//!
//! Tool lookup order: `COREX_LLVM_BIN`, then `PATH`, then `/usr/lib/llvm-N/bin`.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::ast::Program;
use crate::codegen;
use crate::diagnostics::{Diagnostic, SourceFile, Span};
use crate::lexer;
use crate::parser;
use crate::semantic::{self, Analysis};

pub struct Frontend {
    pub program: Program,
    pub analysis: Analysis,
}

pub struct Artifacts {
    pub ir: String,
    pub executable: PathBuf,
}

/// Lex, parse, and analyze without touching external tools.
pub fn check(file: &SourceFile) -> Result<Frontend, Vec<Diagnostic>> {
    let tokens = lexer::lex(&file.text)?;
    let program = parser::parse(&tokens)?;
    let analysis = semantic::analyze(&program)?;
    Ok(Frontend { program, analysis })
}

pub fn module_name(file: &SourceFile) -> String {
    Path::new(&file.name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "module.cx".to_string())
}

pub fn emit_ir(file: &SourceFile) -> Result<String, Vec<Diagnostic>> {
    let fe = check(file)?;
    Ok(codegen::generate(
        &fe.program,
        &fe.analysis,
        &module_name(file),
    ))
}

/// Locate an LLVM tool by name.
pub fn llvm_tool(name: &str) -> Option<PathBuf> {
    if let Ok(dir) = std::env::var("COREX_LLVM_BIN") {
        let p = Path::new(&dir).join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            let p = dir.join(name);
            if p.is_file() {
                return Some(p);
            }
        }
        for version in (14..=25).rev() {
            for candidate in [format!("{name}-{version}")] {
                for dir in std::env::split_paths(&path) {
                    let p = dir.join(&candidate);
                    if p.is_file() {
                        return Some(p);
                    }
                }
            }
        }
    }
    for version in (14..=25).rev() {
        let p = PathBuf::from(format!("/usr/lib/llvm-{version}/bin/{name}"));
        if p.is_file() {
            return Some(p);
        }
    }
    None
}

pub fn temp_dir(prefix: &str) -> PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!("{prefix}-{}-{stamp}", std::process::id()))
}

fn tool_error(tool: &str, stderr: &str) -> Diagnostic {
    let detail = stderr.trim();
    let note = if detail.is_empty() {
        format!("`{tool}` produced no diagnostics")
    } else {
        // Keep the diagnostic readable; LLVM can emit long messages.
        let truncated: String = detail.chars().take(4000).collect();
        truncated
    };
    Diagnostic::error("E0300", Span::new(0, 0), format!("`{tool}` failed")).with_note(note)
}

/// Verify IR with `llvm-as` and lower it to an object with `llc`, then link a
/// freestanding static ELF executable.
pub fn build(
    file: &SourceFile,
    output: &Path,
    opt_level: u8,
    keep_ir: bool,
) -> Result<Artifacts, Vec<Diagnostic>> {
    let ir = emit_ir(file)?;

    let work = temp_dir("corex-build");
    std::fs::create_dir_all(&work).map_err(|e| {
        vec![Diagnostic::error(
            "E0300",
            Span::new(0, 0),
            format!("cannot create build directory {}: {e}", work.display()),
        )]
    })?;

    let ir_path = work.join("module.ll");
    std::fs::write(&ir_path, &ir).map_err(|e| {
        vec![Diagnostic::error(
            "E0300",
            Span::new(0, 0),
            format!("cannot write {}: {e}", ir_path.display()),
        )]
    })?;

    let llvm_as = llvm_tool("llvm-as").ok_or_else(|| {
        vec![Diagnostic::error(
            "E0303",
            Span::new(0, 0),
            "`llvm-as` not found (set COREX_LLVM_BIN or add LLVM 20 to PATH)",
        )]
    })?;
    let bc_path = work.join("module.bc");
    let out = Command::new(&llvm_as)
        .arg(&ir_path)
        .arg("-o")
        .arg(&bc_path)
        .output()
        .map_err(|e| vec![Diagnostic::error("E0303", Span::new(0, 0), format!("cannot run llvm-as: {e}"))])?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let mut note = stderr.trim().to_string();
        if std::env::var("COREX_DUMP_IR").is_ok() {
            note.push_str("\ngenerated IR was:\n");
            note.push_str(&numbered(&ir, 400));
        }
        return Err(vec![Diagnostic::error(
            "E0300",
            Span::new(0, 0),
            "generated LLVM IR failed verification (this is a compiler bug)",
        )
        .with_note(note)]);
    }

    // Run the LLVM IR optimizer for -O1..-O3. `llc` alone only runs the codegen
    // pipeline, which leaves allocas and loop-carried loads largely in place.
    let mut object_input = bc_path.clone();
    if opt_level > 0 {
        let opt = llvm_tool("opt").ok_or_else(|| {
            vec![Diagnostic::error(
                "E0303",
                Span::new(0, 0),
                "`opt` not found (set COREX_LLVM_BIN or add LLVM 20 to PATH)",
            )]
        })?;
        let opt_bc = work.join("module.opt.bc");
        let out = Command::new(&opt)
            .arg(format!("-passes=default<O{}>", opt_level.min(3)))
            .arg(&bc_path)
            .arg("-o")
            .arg(&opt_bc)
            .output()
            .map_err(|e| vec![Diagnostic::error("E0303", Span::new(0, 0), format!("cannot run opt: {e}"))])?;
        if !out.status.success() {
            return Err(vec![tool_error("opt", &String::from_utf8_lossy(&out.stderr))]);
        }
        object_input = opt_bc;
    }

    let llc = llvm_tool("llc").ok_or_else(|| {
        vec![Diagnostic::error(
            "E0303",
            Span::new(0, 0),
            "`llc` not found (set COREX_LLVM_BIN or add LLVM 20 to PATH)",
        )]
    })?;
    let obj_path = work.join("module.o");
    let out = Command::new(&llc)
        .arg("-filetype=obj")
        .arg(format!("-O{}", opt_level.min(3)))
        .arg("-relocation-model=static")
        .arg(&object_input)
        .arg("-o")
        .arg(&obj_path)
        .output()
        .map_err(|e| vec![Diagnostic::error("E0303", Span::new(0, 0), format!("cannot run llc: {e}"))])?;
    if !out.status.success() {
        return Err(vec![tool_error("llc", &String::from_utf8_lossy(&out.stderr))]);
    }

    let cc = std::env::var("COREX_CC").unwrap_or_else(|_| "cc".to_string());
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                vec![Diagnostic::error(
                    "E0300",
                    Span::new(0, 0),
                    format!("cannot create output directory {}: {e}", parent.display()),
                )]
            })?;
        }
    }
    let out = Command::new(&cc)
        .arg("-nostdlib")
        .arg("-static")
        .arg("-no-pie")
        .arg("-o")
        .arg(output)
        .arg(&obj_path)
        .output()
        .map_err(|e| {
            vec![Diagnostic::error(
                "E0303",
                Span::new(0, 0),
                format!("cannot run linker driver `{cc}`: {e}"),
            )]
        })?;
    if !out.status.success() {
        return Err(vec![tool_error(&cc, &String::from_utf8_lossy(&out.stderr))]);
    }

    if keep_ir {
        let kept = output.with_extension("ll");
        let _ = std::fs::write(&kept, &ir);
    }
    if std::env::var("COREX_KEEP_TEMP").is_err() {
        let _ = std::fs::remove_dir_all(&work);
    }

    Ok(Artifacts {
        ir,
        executable: output.to_path_buf(),
    })
}

fn numbered(text: &str, limit: usize) -> String {
    let mut out = String::new();
    for (i, line) in text.lines().take(limit).enumerate() {
        out.push_str(&format!("{:4} | {line}\n", i + 1));
    }
    out
}

/// Run a produced executable, inheriting stdio.
pub fn run(exe: &Path, args: &[String]) -> Result<ExitStatus, String> {
    Command::new(exe)
        .args(args)
        .status()
        .map_err(|e| format!("cannot execute {}: {e}", exe.display()))
}

/// Assemble `ir` with `llvm-as`, which also runs the LLVM verifier. Returns the
/// verifier output on failure.
pub fn verify_ir(ir: &str) -> Result<(), String> {
    let llvm_as = llvm_tool("llvm-as")
        .ok_or_else(|| "`llvm-as` not found (set COREX_LLVM_BIN)".to_string())?;
    let dir = temp_dir("corex-verify");
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

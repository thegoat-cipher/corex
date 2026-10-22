//! Compilation driver: the only module that touches the filesystem and
//! external tools.
//!
//! Pipeline: frontend (`lexer` -> `parser` -> `semantic`) -> `codegen` IR ->
//! `llvm-as` (assembles and runs the LLVM verifier) -> `llc` (native object) ->
//! `cc -nostdlib -static -no-pie` (ELF executable).
//!
//! Tool lookup order: `COREX_LLVM_BIN`, then `PATH`, then `/usr/lib/llvm-N/bin`.

use std::collections::HashSet;
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
    let expanded = load_source(file)?;
    let tokens = lexer::lex(&expanded.text)?;
    let program = parser::parse(&tokens)?;
    let analysis = semantic::analyze(&program)?;
    Ok(Frontend { program, analysis })
}

/// Expand `import "path";` declarations into one virtual source file. Imports
/// are resolved relative to the importing file, deduplicated by canonical path,
/// and concatenated into a single translation unit (flat global namespace).
pub fn load_source(file: &SourceFile) -> Result<SourceFile, Vec<Diagnostic>> {
    if file.expanded {
        return Ok(file.clone());
    }
    if file.name.starts_with('<') {
        return Ok(SourceFile::new(file.name.clone(), file.text.clone()));
    }

    struct Unit {
        display: String,
        text: String,
        path: PathBuf,
    }

    let root_path = std::fs::canonicalize(&file.name)
        .unwrap_or_else(|_| PathBuf::from(&file.name));
    let mut units = vec![Unit {
        display: file.name.clone(),
        text: file.text.clone(),
        path: root_path.clone(),
    }];
    let mut seen = HashSet::new();
    seen.insert(root_path);

    let mut index = 0;
    while index < units.len() {
        let imports = parse_imports(&units[index].text)?;
        let base = units[index]
            .path
            .parent()
            .map(|p| p.to_path_buf())
            .unwrap_or_else(|| PathBuf::from("."));
        for import in imports {
            let raw = Path::new(&import.path);
            let candidate = if raw.is_absolute() {
                raw.to_path_buf()
            } else {
                base.join(raw)
            };
            let canonical = std::fs::canonicalize(&candidate).map_err(|e| {
                vec![Diagnostic::error(
                    "E0232",
                    import.span,
                    format!("cannot import `{}`: {e}", import.path),
                )]
            })?;
            if !seen.insert(canonical.clone()) {
                continue;
            }
            let text = std::fs::read_to_string(&canonical).map_err(|e| {
                vec![Diagnostic::error(
                    "E0232",
                    import.span,
                    format!("cannot read import `{}`: {e}", canonical.display()),
                )]
            })?;
            units.push(Unit {
                display: canonical.display().to_string(),
                text,
                path: canonical,
            });
        }
        index += 1;
    }

    let mut combined = String::new();
    let mut segments = Vec::new();
    for unit in units {
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        segments.push((combined.len(), unit.display));
        combined.push_str(&unit.text);
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
    }
    Ok(SourceFile::with_segments(
        file.name.clone(),
        combined,
        segments,
    ))
}

fn parse_imports(text: &str) -> Result<Vec<crate::ast::ImportDecl>, Vec<Diagnostic>> {
    let tokens = lexer::lex(text)?;
    let program = parser::parse(&tokens)?;
    Ok(program.imports)
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

/// Extra inputs and linkage options for the link step.
#[derive(Clone, Debug, Default)]
pub struct LinkInputs {
    /// Object files and static archives to link alongside the module.
    pub objects: Vec<PathBuf>,
    /// Libraries passed as `-l<name>`.
    pub libs: Vec<String>,
    /// Directories passed as `-L<path>`.
    pub lib_paths: Vec<PathBuf>,
    /// Link against the hosted C runtime instead of `-nostdlib -static`.
    pub hosted: bool,
    /// Produce a shared object instead of an executable.
    pub shared: bool,
}

impl LinkInputs {
    fn link_args(&self, output: &Path) -> Vec<String> {
        let mut args: Vec<String> = Vec::new();
        if self.shared {
            args.push("-shared".to_string());
        } else if self.hosted {
            args.push("-no-pie".to_string());
        } else {
            args.extend([
                "-nostdlib".to_string(),
                "-static".to_string(),
                "-no-pie".to_string(),
            ]);
        }
        args.push("-o".to_string());
        args.push(output.display().to_string());
        args
    }
}

/// Verify IR with `llvm-as` and lower it to an object with `llc`, then link a
/// freestanding static ELF executable.
pub fn build(
    file: &SourceFile,
    output: &Path,
    opt_level: u8,
    keep_ir: bool,
) -> Result<Artifacts, Vec<Diagnostic>> {
    build_with(file, output, opt_level, keep_ir, &LinkInputs::default())
}

pub fn build_with(
    file: &SourceFile,
    output: &Path,
    opt_level: u8,
    keep_ir: bool,
    inputs: &LinkInputs,
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
    let mut command = Command::new(&cc);
    command.args(inputs.link_args(output));
    command.arg(&obj_path);
    for object in &inputs.objects {
        command.arg(object);
    }
    for path in &inputs.lib_paths {
        command.arg(format!("-L{}", path.display()));
    }
    for lib in &inputs.libs {
        command.arg(format!("-l{lib}"));
    }
    let out = command
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

//! Compilation driver: the only module that touches the filesystem and
//! external tools.
//!
//! Pipeline: frontend (`lexer` -> `parser` -> `semantic`) -> `codegen` IR ->
//! `llvm-as` (assembles and runs the LLVM verifier) -> `llc` (native object) ->
//! `cc -nostdlib -static -no-pie` (ELF executable).
//!
//! Tool lookup order: `COREX_LLVM_BIN`, then `PATH`, then `/usr/lib/llvm-N/bin`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus};

use crate::ast::{ConstValue, Program, Type};
use crate::codegen;
use crate::diagnostics::{Diagnostic, SourceFile, Span};
use crate::lexer;
use crate::mangle;
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
    // Import extraction runs before module interfaces exist; unresolved array
    // lengths are re-checked by the real compile below.
    let program = parser::parse_for_discovery(&tokens, &[])?;
    Ok(program.imports)
}

pub fn module_name(file: &SourceFile) -> String {
    Path::new(&file.name)
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "module.cx".to_string())
}

/// Parse and lint one source text (used by `corex lint`).
pub fn lint_source(text: &str) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
    let tokens = lexer::lex(text)?;
    let program = parser::parse(&tokens)?;
    Ok(crate::lint::lint(&program))
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
    build_with_config(
        file,
        output,
        opt_level,
        keep_ir,
        inputs,
        &BuildConfig::default(),
    )
}

pub fn build_with_config(
    file: &SourceFile,
    output: &Path,
    opt_level: u8,
    keep_ir: bool,
    inputs: &LinkInputs,
    config: &BuildConfig,
) -> Result<Artifacts, Vec<Diagnostic>> {
    build_units(
        std::slice::from_ref(file),
        output,
        opt_level,
        keep_ir,
        inputs,
        config,
    )
}

/// Configuration for module discovery and conditional compilation.
#[derive(Clone, Debug)]
pub struct BuildConfig {
    /// Enabled `#[cfg]` keys, e.g. `feature=fast`, `target_os=linux`.
    pub cfg: Vec<String>,
    /// Directories searched for `mod name;` modules.
    pub include_paths: Vec<PathBuf>,
    /// Root units must define `main`/`_start` (false for library targets).
    pub require_entry: bool,
    /// Build the root unit as a test runner (`test_*` functions returning bool).
    pub test_mode: bool,
    /// Emit DWARF line tables and function/variable debug metadata.
    pub debug_info: bool,
    /// Checked integer arithmetic and allocator hardening.
    pub sanitize: bool,
    /// Link against the hosted C runtime (no synthesized `_start`).
    pub hosted: bool,
}

impl Default for BuildConfig {
    fn default() -> Self {
        BuildConfig {
            cfg: Vec::new(),
            include_paths: Vec::new(),
            require_entry: true,
            test_mode: false,
            debug_info: false,
            sanitize: false,
            hosted: false,
        }
    }
}

struct Unit {
    /// Mangled module prefix (`math`); empty for the single-file profile.
    module: String,
    display: String,
    path: Option<PathBuf>,
    text: String,
    deps: Vec<String>,
    /// True for the program entry unit (`main`/`_start` live here).
    is_root: bool,
}

fn source_error(code: &'static str, message: impl Into<String>) -> Vec<Diagnostic> {
    vec![Diagnostic::error(code, Span::new(0, 0), message)]
}

fn unit_from_text(
    display: String,
    path: Option<PathBuf>,
    text: String,
    expected_module: Option<&str>,
    cfg: &[String],
) -> Result<Unit, Vec<Diagnostic>> {
    let file = SourceFile::new(display.clone(), text.clone());
    let tokens = lexer::lex(&text)?;
    let program = parser::parse_for_discovery(&tokens, cfg)?;
    let module = match (&program.module_name, expected_module) {
        (Some(name), Some(expected)) if name != expected => {
            return Err(vec![Diagnostic::error(
                "E0232",
                Span::new(0, 0),
                format!("module `{name}` does not match the dependency name `{expected}`"),
            )
            .with_note(format!("while loading {}", file.name))])
        }
        (Some(name), _) => name.clone(),
        (None, Some(expected)) => {
            return Err(vec![Diagnostic::error(
                "E0232",
                Span::new(0, 0),
                format!("module file must start with `module {expected};`"),
            )
            .with_note(format!("while loading {}", file.name))])
        }
        (None, None) => String::new(),
    };
    Ok(Unit {
        module,
        display,
        path,
        text,
        deps: program.mod_deps,
        is_root: expected_module.is_none(),
    })
}

fn find_module_file(name: &str, from_dir: &Path, include_paths: &[PathBuf]) -> Option<PathBuf> {
    let mut dirs = vec![from_dir.to_path_buf()];
    dirs.extend(include_paths.iter().cloned());
    for dir in dirs {
        for candidate in [dir.join(format!("{name}.cx")), dir.join(name).join("mod.cx")] {
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn discover_units(
    entry: Unit,
    include_paths: &[PathBuf],
    cfg: &[String],
    out: &mut Vec<Unit>,
    visiting: &mut HashSet<String>,
) -> Result<(), Vec<Diagnostic>> {
    for dep in entry.deps.clone() {
        if out.iter().any(|u| u.module == dep) || visiting.contains(&dep) {
            if visiting.contains(&dep) {
                return Err(source_error(
                    "E0232",
                    format!("cyclic module dependency involving `{dep}`"),
                ));
            }
            continue;
        }
        let base = entry
            .path
            .as_ref()
            .and_then(|p| p.parent().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("."));
        let path = find_module_file(&dep, &base, include_paths).ok_or_else(|| {
            source_error(
                "E0232",
                format!("cannot find module `{dep}` (looked for {dep}.cx and {dep}/mod.cx)"),
            )
        })?;
        let text = std::fs::read_to_string(&path).map_err(|e| {
            source_error(
                "E0232",
                format!("cannot read module `{}`: {e}", path.display()),
            )
        })?;
        let unit = unit_from_text(
            path.display().to_string(),
            Some(path),
            text,
            Some(&dep),
            cfg,
        )?;
        visiting.insert(dep.clone());
        discover_units(unit, include_paths, cfg, out, visiting)?;
        visiting.remove(&dep);
    }
    out.push(entry);
    Ok(())
}

fn shift_diags(mut diags: Vec<Diagnostic>, unit_start: usize) -> Vec<Diagnostic> {
    for d in &mut diags {
        if d.span.start >= unit_start {
            d.span = Span::new(
                d.span.start - unit_start,
                d.span.end.saturating_sub(unit_start),
            );
        } else {
            d.span = Span::new(0, 0);
        }
    }
    diags
}

fn render_type(ty: &Type) -> String {
    ty.display()
}

fn quote_str(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &b in bytes {
        match b {
            b'"' => out.push_str("\\\""),
            b'\\' => out.push_str("\\\\"),
            b'\n' => out.push_str("\\n"),
            b'\t' => out.push_str("\\t"),
            b'\r' => out.push_str("\\r"),
            0x20..=0x7e => out.push(b as char),
            other => out.push_str(&format!("\\x{other:02x}")),
        }
    }
    out.push('"');
    out
}

fn render_const(ty: &Type, value: &ConstValue) -> String {
    match value {
        ConstValue::Int(v) => {
            let signed = ty.int_ty().map(|t| t.signed()).unwrap_or(false);
            if signed {
                format!("{}", *v as i64)
            } else {
                format!("{v}")
            }
        }
        ConstValue::Bool(b) => if *b { "true" } else { "false" }.to_string(),
        ConstValue::Char(c) => format!("'{}'", (*c as char).escape_default()),
        ConstValue::Float(v) => format!("{v:?}"),
        ConstValue::Str(bytes) => quote_str(bytes),
        ConstValue::GlobalAddr(name) => format!("&{name}"),
        ConstValue::FnAddr(name) => format!("&{name}"),
        // Interfaces only export scalar, `str`, and pointer constants.
        ConstValue::Array(_) | ConstValue::Struct(_) => "0".to_string(),
    }
}

/// Generate the public interface of one unit as CoreX source.
fn generate_interface(
    program: &Program,
    analysis: &Analysis,
    unit_start: usize,
    prefix: &str,
) -> Result<String, Vec<Diagnostic>> {
    let mut out = String::new();
    out.push_str(&format!("// CoreX interface for module `{prefix}`\n"));

    let mut public_types: HashSet<String> = HashSet::new();
    for s in &program.structs {
        if s.span.start >= unit_start && s.public {
            public_types.insert(s.name.clone());
        }
    }
    for e in &program.enums {
        if e.span.start >= unit_start && e.public {
            public_types.insert(e.name.clone());
        }
    }
    for t in &program.typedefs {
        if t.name_span.start >= unit_start && t.public {
            public_types.insert(t.name.clone());
        }
    }

    let check_type = |ty: &Type, span: Span| -> Result<(), Vec<Diagnostic>> {
        let mut leaked = None;
        fn walk(ty: &Type, prefix: &str, public: &HashSet<String>, leaked: &mut Option<String>) {
            match ty {
                Type::Named(name) => {
                    if name.starts_with(&format!("{prefix}$")) && !public.contains(name) {
                        *leaked = Some(name.clone());
                    }
                }
                Type::Ptr { pointee, .. } => walk(pointee, prefix, public, leaked),
                Type::Array { elem, .. } | Type::Slice(elem) | Type::Vec(elem) => {
                    walk(elem, prefix, public, leaked)
                }
                Type::Fn(sig) => {
                    for p in &sig.params {
                        walk(p, prefix, public, leaked);
                    }
                    walk(&sig.ret, prefix, public, leaked);
                }
                _ => {}
            }
        }
        walk(ty, prefix, &public_types, &mut leaked);
        if let Some(name) = leaked {
            return Err(vec![Diagnostic::error(
                "E0233",
                span,
                format!("public interface exposes private type `{name}`"),
            )]);
        }
        Ok(())
    };

    for s in &program.structs {
        if s.span.start < unit_start || !s.public {
            continue;
        }
        for f in &s.fields {
            check_type(&f.ty, s.name_span)?;
        }
        let mut attrs = String::new();
        if s.packed {
            attrs.push_str("packed ");
        }
        if s.align != 0 {
            attrs.push_str(&format!("align({}) ", s.align));
        }
        out.push_str(&format!(
            "pub {}struct {} {{\n",
            attrs,
            s.name
        ));
        for f in &s.fields {
            if f.bit_width > 0 {
                out.push_str(&format!("    {}: {}: {},\n", f.name, render_type(&f.ty), f.bit_width));
            } else {
                out.push_str(&format!("    {}: {},\n", f.name, render_type(&f.ty)));
            }
        }
        out.push_str("}\n\n");
    }

    for e in &program.enums {
        if e.span.start < unit_start || !e.public {
            continue;
        }
        out.push_str(&format!("pub enum {} {{\n", e.name));
        if let Some(info) = analysis.enums.get(&e.name) {
            for (variant, value) in &info.variants {
                out.push_str(&format!("    {variant} = {value},\n"));
            }
        }
        out.push_str("}\n\n");
    }

    for t in &program.typedefs {
        if t.name_span.start < unit_start || !t.public {
            continue;
        }
        check_type(&t.ty, t.name_span)?;
        out.push_str(&format!(
            "pub typedef {} = {};\n\n",
            t.name,
            render_type(&t.ty)
        ));
    }

    for c in &program.consts {
        if c.name_span.start < unit_start || !c.public {
            continue;
        }
        check_type(&c.ty, c.name_span)?;
        if let Some((ty, value)) = analysis.consts.get(&c.name) {
            out.push_str(&format!(
                "pub const {}: {} = {};\n\n",
                c.name,
                render_type(ty),
                render_const(ty, value)
            ));
        }
    }

    for g in &program.globals {
        if g.span.start < unit_start || !g.public || g.is_extern {
            continue;
        }
        check_type(&g.ty, g.name_span)?;
        out.push_str(&format!(
            "extern static {}: {};\n\n",
            g.name,
            render_type(&g.ty)
        ));
    }

    for f in &program.fns {
        if f.span.start < unit_start || !f.public || f.body.is_none() {
            continue;
        }
        if f.name == "main" || f.name == "_start" {
            continue;
        }
        for p in &f.params {
            check_type(&p.ty, f.name_span)?;
        }
        check_type(&f.ret, f.name_span)?;
        let params: Vec<String> = f
            .params
            .iter()
            .enumerate()
            .map(|(i, p)| format!("a{i}: {}", render_type(&p.ty)))
            .collect();
        let ret = if f.ret == Type::Unit {
            String::new()
        } else {
            format!(" -> {}", render_type(&f.ret))
        };
        out.push_str(&format!(
            "extern \"corex\" fn {}({}){};\n\n",
            f.name,
            params.join(", "),
            ret
        ));
    }

    Ok(out)
}

fn assemble_and_lower(
    ir: &str,
    work: &Path,
    opt_level: u8,
    tag: &str,
) -> Result<PathBuf, Vec<Diagnostic>> {
    let ir_path = work.join(format!("{tag}.ll"));
    std::fs::write(&ir_path, ir).map_err(|e| {
        source_error("E0300", format!("cannot write {}: {e}", ir_path.display()))
    })?;
    let llvm_as = llvm_tool("llvm-as").ok_or_else(|| {
        source_error(
            "E0303",
            "`llvm-as` not found (set COREX_LLVM_BIN or add LLVM 20 to PATH)",
        )
    })?;
    let bc_path = work.join(format!("{tag}.bc"));
    let out = Command::new(&llvm_as)
        .arg(&ir_path)
        .arg("-o")
        .arg(&bc_path)
        .output()
        .map_err(|e| source_error("E0303", format!("cannot run llvm-as: {e}")))?;
    if !out.status.success() {
        let mut note = String::from_utf8_lossy(&out.stderr).to_string();
        if std::env::var("COREX_DUMP_IR").is_ok() {
            note.push_str("\ngenerated IR was:\n");
            note.push_str(&numbered(ir, 400));
        }
        return Err(vec![Diagnostic::error(
            "E0300",
            Span::new(0, 0),
            "generated LLVM IR failed verification (this is a compiler bug)",
        )
        .with_note(note)]);
    }

    let mut object_input = bc_path;
    if opt_level > 0 {
        let opt = llvm_tool("opt").ok_or_else(|| {
            source_error(
                "E0303",
                "`opt` not found (set COREX_LLVM_BIN or add LLVM 20 to PATH)",
            )
        })?;
        let opt_bc = work.join(format!("{tag}.opt.bc"));
        let out = Command::new(&opt)
            .arg(format!("-passes=default<O{}>", opt_level.min(3)))
            .arg(&object_input)
            .arg("-o")
            .arg(&opt_bc)
            .output()
            .map_err(|e| source_error("E0303", format!("cannot run opt: {e}")))?;
        if !out.status.success() {
            return Err(vec![tool_error(
                "opt",
                &String::from_utf8_lossy(&out.stderr),
            )]);
        }
        object_input = opt_bc;
    }

    let llc = llvm_tool("llc").ok_or_else(|| {
        source_error(
            "E0303",
            "`llc` not found (set COREX_LLVM_BIN or add LLVM 20 to PATH)",
        )
    })?;
    let obj_path = work.join(format!("{tag}.o"));
    let out = Command::new(&llc)
        .arg("-filetype=obj")
        .arg(format!("-O{}", opt_level.min(3)))
        .arg("-relocation-model=static")
        .arg(&object_input)
        .arg("-o")
        .arg(&obj_path)
        .output()
        .map_err(|e| source_error("E0303", format!("cannot run llc: {e}")))?;
    if !out.status.success() {
        return Err(vec![tool_error("llc", &String::from_utf8_lossy(&out.stderr))]);
    }
    Ok(obj_path)
}

/// What the driver should produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OutputKind {
    Executable,
    Shared,
    StaticLibrary,
}

impl OutputKind {
    pub fn parse(text: &str) -> Option<OutputKind> {
        match text {
            "exe" | "executable" | "bin" => Some(OutputKind::Executable),
            "shared" | "dylib" => Some(OutputKind::Shared),
            "staticlib" | "static" | "lib" => Some(OutputKind::StaticLibrary),
            _ => None,
        }
    }
}

fn link_objects(
    objects: &[PathBuf],
    output: &Path,
    inputs: &LinkInputs,
) -> Result<(), Vec<Diagnostic>> {
    let cc = std::env::var("COREX_CC").unwrap_or_else(|_| "cc".to_string());
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                source_error(
                    "E0300",
                    format!("cannot create output directory {}: {e}", parent.display()),
                )
            })?;
        }
    }
    let mut command = Command::new(&cc);
    command.args(inputs.link_args(output));
    for obj in objects {
        command.arg(obj);
    }
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
        .map_err(|e| source_error("E0303", format!("cannot run linker driver `{cc}`: {e}")))?;
    if !out.status.success() {
        return Err(vec![tool_error(&cc, &String::from_utf8_lossy(&out.stderr))]);
    }
    Ok(())
}

fn archive_objects(
    objects: &[PathBuf],
    output: &Path,
) -> Result<(), Vec<Diagnostic>> {
    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).map_err(|e| {
                source_error(
                    "E0300",
                    format!("cannot create output directory {}: {e}", parent.display()),
                )
            })?;
        }
    }
    let ar = llvm_tool("llvm-ar")
        .or_else(|| llvm_tool("ar"))
        .unwrap_or_else(|| PathBuf::from("ar"));
    let out = Command::new(&ar)
        .arg("rcs")
        .arg(output)
        .args(objects)
        .output()
        .map_err(|e| source_error("E0303", format!("cannot run archiver `{}`: {e}", ar.display())))?;
    if !out.status.success() {
        return Err(vec![tool_error(
            &ar.display().to_string(),
            &String::from_utf8_lossy(&out.stderr),
        )]);
    }
    Ok(())
}

/// Compile every unit reachable from `roots` into object files.
///
/// Returns the objects in dependency order and the entry unit's IR.
pub fn compile_units(
    roots: &[SourceFile],
    work: &Path,
    opt_level: u8,
    config: &BuildConfig,
) -> Result<(Vec<PathBuf>, String), Vec<Diagnostic>> {
    if roots.is_empty() {
        return Err(source_error("E0300", "no input files"));
    }
    std::fs::create_dir_all(work)
        .map_err(|e| source_error("E0300", format!("cannot create build directory: {e}")))?;

    // Built-in target cfg keys are always available.
    let mut cfg = config.cfg.clone();
    for key in ["target_os=linux", "target_arch=x86_64"] {
        if !cfg.iter().any(|k| k == key) {
            cfg.push(key.to_string());
        }
    }
    if config.sanitize && !cfg.iter().any(|k| k == "sanitize=true") {
        cfg.push("sanitize=true".to_string());
    }

    let mut units: Vec<Unit> = Vec::new();
    for root in roots {
        let path = if root.name.starts_with('<') {
            None
        } else {
            Some(PathBuf::from(&root.name))
        };
        let unit = unit_from_text(
            root.name.clone(),
            path,
            root.text.clone(),
            None,
            &cfg,
        )?;
        if units
            .iter()
            .any(|u| u.display == unit.display && u.module == unit.module)
        {
            continue;
        }
        let mut visiting = HashSet::new();
        discover_units(unit, &config.include_paths, &cfg, &mut units, &mut visiting)?;
    }

    let mut interfaces: HashMap<String, String> = HashMap::new();
    let mut objects: Vec<PathBuf> = Vec::new();
    let mut entry_ir = String::new();
    let mut compiled: Vec<(String, Vec<String>)> = Vec::new();

    for unit in &units {
        let prefix = if unit.module.is_empty() {
            if unit.is_root && unit.deps.is_empty() {
                String::new()
            } else if let Some(path) = &unit.path {
                path.file_stem()
                    .map(|s| s.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            }
        } else {
            unit.module.clone()
        };

        // Collect the transitive interface closure in dependency order.
        let mut closure: Vec<String> = Vec::new();
        let mut stack: Vec<String> = unit.deps.clone();
        while let Some(dep) = stack.pop() {
            if closure.contains(&dep) {
                continue;
            }
            closure.push(dep.clone());
            if let Some((_, deps)) = compiled.iter().rev().find(|(m, _)| m == &dep) {
                stack.extend(deps.clone());
            }
        }
        closure.reverse();

        let mut combined = String::new();
        let mut segments: Vec<(usize, String)> = Vec::new();
        for dep in &closure {
            if let Some(text) = interfaces.get(dep) {
                segments.push((combined.len(), format!("<interface:{dep}>")));
                combined.push_str(text);
                if !combined.ends_with('\n') {
                    combined.push('\n');
                }
            }
        }
        let unit_start = combined.len();
        segments.push((combined.len(), unit.display.clone()));
        combined.push_str(&unit.text);
        if !combined.ends_with('\n') {
            combined.push('\n');
        }
        let combined_file =
            SourceFile::with_segments(unit.display.clone(), combined.clone(), segments);

        let tokens = lexer::lex(&combined).map_err(|d| shift_diags(d, unit_start))?;
        let mut program =
            parser::parse_with_cfg(&tokens, &cfg).map_err(|d| shift_diags(d, unit_start))?;
        if let Some(declared) = &program.module_name {
            if !prefix.is_empty() && declared != &prefix {
                return Err(vec![Diagnostic::error(
                    "E0232",
                    Span::new(unit_start, unit_start),
                    format!("module `{declared}` does not match `{prefix}`"),
                )]);
            }
        }
        mangle::mangle_unit(&mut program, unit_start, &prefix);

        let require_entry =
            unit.is_root && config.require_entry && !config.test_mode;
        let analysis = semantic::analyze_with(&program, require_entry)
            .map_err(|d| shift_diags(d, unit_start))?;

        let interface = generate_interface(&program, &analysis, unit_start, &prefix)?;
        if std::env::var("COREX_DUMP_INTERFACES").is_ok() {
            eprintln!("--- interface {prefix} ---\n{interface}");
        }
        let tag = if prefix.is_empty() {
            "entry".to_string()
        } else {
            prefix.replace('$', "_").replace("::", "_")
        };
        let mut test_names: Option<Vec<String>> = None;
        if config.test_mode && unit.is_root {
            let mut names = Vec::new();
            for f in &program.fns {
                if f.name.starts_with("test_") && f.body.is_some() {
                    if f.ret != Type::Bool {
                        return Err(vec![Diagnostic::error(
                            "E0235",
                            f.name_span,
                            format!(
                                "test `{}` must return `bool` (true = pass)",
                                f.name
                            ),
                        )]);
                    }
                    names.push(f.name.clone());
                }
            }
            if names.is_empty() {
                return Err(vec![Diagnostic::error(
                    "E0235",
                    Span::new(0, 0),
                    "no tests found: define `fn test_something() -> bool`",
                )]);
            }
            test_names = Some(names);
        }
        let ir = match &test_names {
            Some(names) => codegen::generate_tests(&program, &analysis, &combined_file.name, names),
            None => codegen::generate_full(
                &program,
                &analysis,
                &combined_file.name,
                if config.debug_info {
                    Some(&combined_file)
                } else {
                    None
                },
                config.sanitize,
                config.hosted,
            ),
        };
        let object = assemble_and_lower(&ir, work, opt_level, &tag)?;
        objects.push(object);
        if unit.is_root && entry_ir.is_empty() {
            entry_ir = ir;
        }
        interfaces.insert(prefix.clone(), interface);
        compiled.push((prefix, unit.deps.clone()));
    }

    Ok((objects, entry_ir))
}

/// Compile and link one or more root units with file modules (`mod name;`).
pub fn build_units(
    roots: &[SourceFile],
    output: &Path,
    opt_level: u8,
    keep_ir: bool,
    inputs: &LinkInputs,
    config: &BuildConfig,
) -> Result<Artifacts, Vec<Diagnostic>> {
    let work = temp_dir("corex-build");
    let (objects, entry_ir) = compile_units(roots, &work, opt_level, config)?;
    link_objects(&objects, output, inputs)?;
    if keep_ir {
        let kept = output.with_extension("ll");
        let _ = std::fs::write(&kept, &entry_ir);
    }
    if std::env::var("COREX_KEEP_TEMP").is_err() {
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(Artifacts {
        ir: entry_ir,
        executable: output.to_path_buf(),
    })
}

/// Compile a project into an executable, shared library, or static archive.
pub fn build_project(
    roots: &[SourceFile],
    output: &Path,
    opt_level: u8,
    inputs: &LinkInputs,
    config: &BuildConfig,
    kind: OutputKind,
) -> Result<Artifacts, Vec<Diagnostic>> {
    let work = temp_dir("corex-project");
    let (objects, entry_ir) = compile_units(roots, &work, opt_level, config)?;
    match kind {
        OutputKind::Executable => link_objects(&objects, output, inputs)?,
        OutputKind::Shared => {
            let mut shared = inputs.clone();
            shared.shared = true;
            link_objects(&objects, output, &shared)?;
        }
        OutputKind::StaticLibrary => archive_objects(&objects, output)?,
    }
    if std::env::var("COREX_KEEP_TEMP").is_err() {
        let _ = std::fs::remove_dir_all(&work);
    }
    Ok(Artifacts {
        ir: entry_ir,
        executable: output.to_path_buf(),
    })
}

/// A parsed `corex.toml` project manifest (a deliberately small TOML subset).
#[derive(Clone, Debug, Default)]
pub struct Manifest {
    pub name: String,
    pub kind: OutputKind,
    pub sources: Vec<PathBuf>,
    pub include_paths: Vec<PathBuf>,
    pub libs: Vec<String>,
    pub lib_paths: Vec<PathBuf>,
    pub opt: u8,
    pub features: Vec<String>,
    pub output: Option<PathBuf>,
    pub debug: bool,
    pub sanitize: bool,
    pub hosted: bool,
    pub dir: PathBuf,
}

impl Default for OutputKind {
    fn default() -> Self {
        OutputKind::Executable
    }
}

fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '"' => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_string_list(value: &str) -> Result<Vec<String>, String> {
    let value = value.trim();
    if !value.starts_with('[') || !value.ends_with(']') {
        return Err(format!("expected a string array, found `{value}`"));
    }
    let inner = &value[1..value.len() - 1];
    let mut out = Vec::new();
    for piece in inner.split(',') {
        let piece = piece.trim();
        if piece.is_empty() {
            continue;
        }
        let text = piece
            .strip_prefix('"')
            .and_then(|p| p.strip_suffix('"'))
            .ok_or_else(|| format!("expected a quoted string in array, found `{piece}`"))?;
        out.push(text.to_string());
    }
    Ok(out)
}

pub fn parse_manifest(text: &str, dir: PathBuf) -> Result<Manifest, String> {
    let mut manifest = Manifest {
        name: "app".to_string(),
        kind: OutputKind::Executable,
        opt: 2,
        dir,
        ..Default::default()
    };
    let mut section = String::new();
    for (line_no, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_string();
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("line {}: expected `key = value`", line_no + 1));
        };
        let key = key.trim();
        let value = value.trim();
        let unquote = |v: &str| -> Result<String, String> {
            v.strip_prefix('"')
                .and_then(|p| p.strip_suffix('"'))
                .map(|s| s.to_string())
                .ok_or_else(|| format!("line {}: expected a quoted string", line_no + 1))
        };
        match (section.as_str(), key) {
            ("package", "name") => manifest.name = unquote(value)?,
            ("build", "kind") => {
                let kind = unquote(value)?;
                manifest.kind = OutputKind::parse(&kind)
                    .ok_or_else(|| format!("unknown output kind `{kind}`"))?;
            }
            ("build", "sources") => {
                manifest.sources = parse_string_list(value)?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
            }
            ("build", "include_paths") => {
                manifest.include_paths = parse_string_list(value)?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
            }
            ("build", "libs") => manifest.libs = parse_string_list(value)?,
            ("build", "lib_paths") => {
                manifest.lib_paths = parse_string_list(value)?
                    .into_iter()
                    .map(PathBuf::from)
                    .collect();
            }
            ("build", "features") => manifest.features = parse_string_list(value)?,
            ("build", "opt") => {
                manifest.opt = value
                    .parse()
                    .map_err(|_| format!("line {}: invalid opt level `{value}`", line_no + 1))?;
            }
            ("build", "output") => manifest.output = Some(PathBuf::from(unquote(value)?)),
            ("build", "debug") => {
                manifest.debug = matches!(value, "true" | "yes" | "1");
            }
            ("build", "sanitize") => {
                manifest.sanitize = matches!(value, "true" | "yes" | "1");
            }
            ("build", "hosted") => {
                manifest.hosted = matches!(value, "true" | "yes" | "1");
            }
            _ => {
                return Err(format!(
                    "line {}: unknown manifest key `{section}.{key}`",
                    line_no + 1
                ))
            }
        }
    }
    if manifest.sources.is_empty() {
        return Err("manifest has no `build.sources`".to_string());
    }
    Ok(manifest)
}

/// Build a project described by `<dir>/corex.toml`.
pub fn build_manifest(
    dir: &Path,
    opt_override: Option<u8>,
    output_override: Option<&Path>,
    extra_cfg: &[String],
) -> Result<PathBuf, Vec<Diagnostic>> {
    let manifest_path = if dir.is_dir() {
        dir.join("corex.toml")
    } else {
        dir.to_path_buf()
    };
    let dir = manifest_path
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from("."));
    let text = std::fs::read_to_string(&manifest_path).map_err(|e| {
        source_error(
            "E0234",
            format!("cannot read {}: {e}", manifest_path.display()),
        )
    })?;
    let manifest = parse_manifest(&text, dir.clone())
        .map_err(|e| source_error("E0234", format!("{}: {e}", manifest_path.display())))?;

    let mut roots = Vec::new();
    for source in &manifest.sources {
        let path = if source.is_absolute() {
            source.clone()
        } else {
            dir.join(source)
        };
        let text = std::fs::read_to_string(&path).map_err(|e| {
            source_error("E0234", format!("cannot read {}: {e}", path.display()))
        })?;
        roots.push(SourceFile::new(path.display().to_string(), text));
    }

    let mut cfg: Vec<String> = extra_cfg.to_vec();
    for feature in &manifest.features {
        cfg.push(format!("feature={feature}"));
    }
    cfg.push("target_os=linux".to_string());
    cfg.push("target_arch=x86_64".to_string());
    let mut include_paths: Vec<PathBuf> = manifest
        .include_paths
        .iter()
        .map(|p| if p.is_absolute() { p.clone() } else { dir.join(p) })
        .collect();
    for source in &manifest.sources {
        if let Some(parent) = source.parent() {
            include_paths.push(dir.join(parent));
        }
    }
    let config = BuildConfig {
        cfg,
        include_paths,
        require_entry: manifest.kind == OutputKind::Executable,
        test_mode: false,
        debug_info: manifest.debug,
        sanitize: manifest.sanitize,
        hosted: manifest.hosted,
    };
    let opt = opt_override.unwrap_or(manifest.opt);
    let output = output_override.map(|p| p.to_path_buf()).unwrap_or_else(|| {
        if let Some(output) = &manifest.output {
            dir.join(output)
        } else {
            let name = match manifest.kind {
                OutputKind::Executable => manifest.name.clone(),
                OutputKind::Shared => format!("lib{}.so", manifest.name),
                OutputKind::StaticLibrary => format!("lib{}.a", manifest.name),
            };
            dir.join("build").join(name)
        }
    });
    let inputs = LinkInputs {
        libs: manifest.libs.clone(),
        lib_paths: manifest
            .lib_paths
            .iter()
            .map(|p| if p.is_absolute() { p.clone() } else { dir.join(p) })
            .collect(),
        ..Default::default()
    };
    build_project(&roots, &output, opt, &inputs, &config, manifest.kind)?;
    Ok(output)
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

//! Parameterized test backend: run the Rust behaviour suite against either the
//! Rust reference compiler (default) or an external `corexc` binary.
//!
//! Set `COREX_TEST_COMPILER=/path/to/corexc` to route compilation through that
//! binary. The test bodies are unchanged: they call `driver::build`,
//! `driver::check`, `driver::emit_ir`, and friends, and this module forwards
//! those calls to the external compiler with a small CLI adapter. When the
//! variable is unset every function delegates to `corex::driver`, so the
//! default `cargo test` run is byte-for-byte the original suite.
//!
//! The adapter is a test harness, not a re-implementation:
//! - `check` runs `corexc check` for the accept/reject decision and diagnostic
//!   codes, then obtains the `Frontend` value the tests never inspect from the
//!   Rust frontend (the external compiler exposes no AST over the CLI).
//! - `build`/`build_with`/`build_with_config`/`build_project` run
//!   `corexc build` with the equivalent flags.
//! - `emit_ir` runs `corexc emit-ir` and returns its stdout.
//! - `build_manifest` runs `corexc build <dir>` and honours the manifest kind.
//! - `load_source`, `llvm_tool`, and `temp_dir` remain oracle helpers; the
//!   harness blocklist names the tests that depend on oracle-only behaviour.

#![allow(dead_code)]
#![allow(unused_imports)]

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use corex::diagnostics::{Diagnostic, Severity, SourceFile, Span};

pub fn compiler() -> Option<PathBuf> {
    std::env::var_os("COREX_TEST_COMPILER").map(PathBuf::from)
}

pub fn is_corexc() -> bool {
    compiler().is_some()
}

/// Diagnostic codes the bootstrap frontend can print. `Diagnostic::code` is
/// `&'static str`; mapping through this table keeps the adapter allocation-free
/// instead of leaking a string per diagnostic.
const KNOWN_CODES: &[&str] = &[
    "", "E0001", "E0002", "E0003", "E0004", "E0005", "E0006", "E0100", "E0101",
    "E0110", "E0111", "E0201", "E0202", "E0203", "E0204", "E0205", "E0206",
    "E0207", "E0208", "E0209", "E0210", "E0211", "E0212", "E0213", "E0214",
    "E0215", "E0216", "E0217", "E0218", "E0219", "E0220", "E0221", "E0222",
    "E0223", "E0224", "E0225", "E0226", "E0227", "E0228", "E0229", "E0230",
    "E0231", "E0232", "E0233", "E0234", "E0235", "E0300", "E0303",
    "W0001", "W0002", "W0003", "W0004",
];

fn static_code(code: &str) -> &'static str {
    for known in KNOWN_CODES {
        if *known == code {
            return known;
        }
    }
    ""
}

fn parity_error(message: &str) -> Diagnostic {
    Diagnostic::error(static_code("E0300"), Span::new(0, 0), message.to_string())
}

pub mod driver {
    use super::*;

    pub use corex::driver::{Artifacts, BuildConfig, Frontend, LinkInputs, OutputKind, TempDir};

    pub fn temp_dir(prefix: &str) -> PathBuf {
        corex::driver::temp_dir(prefix)
    }

    pub fn llvm_tool(name: &str) -> Option<PathBuf> {
        corex::driver::llvm_tool(name)
    }

    pub fn load_source(file: &SourceFile) -> Result<SourceFile, Vec<Diagnostic>> {
        corex::driver::load_source(file)
    }

    pub fn check(file: &SourceFile) -> Result<Frontend, Vec<Diagnostic>> {
        match compiler() {
            None => corex::driver::check(file),
            Some(bin) => {
                let (path, cleanup) = source_path(file);
                let out = run(&bin, &["check".into(), path_as_arg(&path)]);
                let diagnostics = parse_diagnostics(file, &out.stderr);
                if let Some(dir) = cleanup {
                    let _ = std::fs::remove_dir_all(dir);
                }
                if out.status.success() {
                    // Tests only match `Ok(_)`; the value is never inspected in
                    // the suite. Re-run the oracle frontend so the type is real.
                    corex::driver::check(file)
                } else if diagnostics.is_empty() {
                    Err(vec![parity_error("corexc check failed without a diagnostic")])
                } else {
                    Err(diagnostics)
                }
            }
        }
    }

    pub fn emit_ir(file: &SourceFile) -> Result<String, Vec<Diagnostic>> {
        match compiler() {
            None => corex::driver::emit_ir(file),
            Some(bin) => {
                let (path, cleanup) = source_path(file);
                let out = run(&bin, &["emit-ir".into(), path_as_arg(&path)]);
                let diagnostics = parse_diagnostics(file, &out.stderr);
                if let Some(dir) = cleanup {
                    let _ = std::fs::remove_dir_all(dir);
                }
                if out.status.success() {
                    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
                } else if diagnostics.is_empty() {
                    Err(vec![parity_error("corexc emit-ir failed without a diagnostic")])
                } else {
                    Err(diagnostics)
                }
            }
        }
    }

    pub fn lint_source(text: &str) -> Result<Vec<Diagnostic>, Vec<Diagnostic>> {
        match compiler() {
            None => corex::driver::lint_source(text),
            Some(bin) => {
                let file = SourceFile::new("lint_case.cx", text);
                let (path, cleanup) = source_path(&file);
                let out = run(&bin, &["lint".into(), path_as_arg(&path)]);
                let diagnostics = parse_diagnostics(&file, &out.stderr);
                if let Some(dir) = cleanup {
                    let _ = std::fs::remove_dir_all(dir);
                }
                if out.status.success() {
                    Ok(diagnostics)
                } else {
                    Err(diagnostics)
                }
            }
        }
    }

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
        match compiler() {
            None => corex::driver::build_with_config(file, output, opt_level, keep_ir, inputs, config),
            Some(bin) => corexc_build(
                &bin,
                std::slice::from_ref(file),
                output,
                opt_level,
                keep_ir,
                inputs,
                config,
                OutputKind::Executable,
            ),
        }
    }

    pub fn build_project(
        roots: &[SourceFile],
        output: &Path,
        opt_level: u8,
        inputs: &LinkInputs,
        config: &BuildConfig,
        kind: OutputKind,
    ) -> Result<Artifacts, Vec<Diagnostic>> {
        match compiler() {
            None => corex::driver::build_project(roots, output, opt_level, inputs, config, kind),
            Some(bin) => {
                corexc_build(&bin, roots, output, opt_level, false, inputs, config, kind)
            }
        }
    }

    pub fn build_manifest(
        dir: &Path,
        opt_override: Option<u8>,
        output_override: Option<&Path>,
        extra_cfg: &[String],
    ) -> Result<PathBuf, Vec<Diagnostic>> {
        match compiler() {
            None => corex::driver::build_manifest(dir, opt_override, output_override, extra_cfg),
            Some(bin) => corexc_manifest(&bin, dir, opt_override, output_override, extra_cfg),
        }
    }

    fn corexc_build(
        bin: &Path,
        roots: &[SourceFile],
        output: &Path,
        opt_level: u8,
        keep_ir: bool,
        inputs: &LinkInputs,
        config: &BuildConfig,
        kind: OutputKind,
    ) -> Result<Artifacts, Vec<Diagnostic>> {
        if roots.len() != 1 {
            return Err(vec![parity_error(
                "multi-root builds are not expressible through the `corexc` CLI",
            )]);
        }
        let file = &roots[0];
        if config.debug_info {
            return Err(vec![parity_error(
                "`-g`/DWARF is rejected by contract in the bootstrap (SH-036)",
            )]);
        }
        if config.test_mode {
            return Err(vec![parity_error(
                "test_mode builds need `corexc test`, which runs rather than links",
            )]);
        }
        let (path, cleanup) = source_path(file);
        let mut args: Vec<OsString> = vec!["build".into(), path_as_arg(&path)];
        args.push("-o".into());
        args.push(output.as_os_str().to_os_string());
        args.push("--opt".into());
        args.push(opt_level.to_string().into());
        match kind {
            OutputKind::Executable => {}
            OutputKind::Shared => {
                args.push("--emit".into());
                args.push("shared".into());
            }
            OutputKind::StaticLibrary => {
                args.push("--emit".into());
                args.push("staticlib".into());
            }
        }
        if inputs.hosted || config.hosted {
            args.push("--hosted".into());
        }
        if inputs.shared {
            args.push("--shared".into());
        }
        if config.sanitize {
            args.push("--sanitize".into());
        }
        for key in &config.cfg {
            args.push("--cfg".into());
            args.push(key.into());
        }
        for dir in &config.include_paths {
            args.push("-I".into());
            args.push(dir.as_os_str().to_os_string());
        }
        for object in &inputs.objects {
            args.push("--link".into());
            args.push(object.as_os_str().to_os_string());
        }
        for dir in &inputs.lib_paths {
            args.push("-L".into());
            args.push(dir.as_os_str().to_os_string());
        }
        for lib in &inputs.libs {
            args.push("-l".into());
            args.push(lib.into());
        }
        let out = run(bin, &args);
        let diagnostics = parse_diagnostics(file, &out.stderr);
        if let Some(dir) = cleanup {
            let _ = std::fs::remove_dir_all(dir);
        }
        if !out.status.success() {
            return Err(if diagnostics.is_empty() {
                vec![parity_error(&format!(
                    "corexc build failed ({}): {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                ))]
            } else {
                diagnostics
            });
        }
        if !output.is_file() {
            return Err(vec![parity_error(&format!(
                "corexc reported success but did not create {}",
                output.display()
            ))]);
        }
        let ir = if keep_ir {
            emit_ir(file).unwrap_or_default()
        } else {
            String::new()
        };
        Ok(Artifacts {
            ir,
            executable: output.to_path_buf(),
        })
    }

    fn corexc_manifest(
        bin: &Path,
        dir: &Path,
        opt_override: Option<u8>,
        output_override: Option<&Path>,
        extra_cfg: &[String],
    ) -> Result<PathBuf, Vec<Diagnostic>> {
        let manifest = if dir.is_dir() {
            dir.join("corex.toml")
        } else {
            dir.to_path_buf()
        };
        let root = manifest.parent().unwrap_or(Path::new(".")).to_path_buf();
        let text = std::fs::read_to_string(&manifest)
            .map_err(|e| vec![parity_error(&format!("cannot read {}: {e}", manifest.display()))])?;
        let kind = manifest_value(&text, "kind").unwrap_or_else(|| "exe".to_string());
        let name = manifest_value(&text, "name").unwrap_or_else(|| "out".to_string());
        let out = match output_override {
            Some(path) => path.to_path_buf(),
            None => {
                let file = if kind == "staticlib" {
                    format!("lib{name}.a")
                } else if kind == "shared" {
                    format!("lib{name}.so")
                } else {
                    name.clone()
                };
                corex::driver::temp_dir("corexc-manifest").join(file)
            }
        };
        let mut args: Vec<OsString> = vec!["build".into(), path_as_arg(&root)];
        args.push("-o".into());
        args.push(out.as_os_str().to_os_string());
        if let Some(opt) = opt_override {
            args.push("--opt".into());
            args.push(opt.to_string().into());
        }
        match kind.as_str() {
            "staticlib" => {
                args.push("--emit".into());
                args.push("staticlib".into());
            }
            "shared" => {
                args.push("--emit".into());
                args.push("shared".into());
            }
            _ => {}
        }
        for key in extra_cfg {
            args.push("--cfg".into());
            args.push(key.into());
        }
        let out = run(bin, &args);
        let diagnostics = parse_diagnostics(&SourceFile::new(manifest.display().to_string(), text), &out.stderr);
        if !out.status.success() {
            return Err(if diagnostics.is_empty() {
                vec![parity_error(&format!(
                    "corexc manifest build failed ({}): {}",
                    out.status,
                    String::from_utf8_lossy(&out.stderr).trim()
                ))]
            } else {
                diagnostics
            });
        }
        Ok(out_path_from(args, out))
    }
}

fn run(bin: &Path, args: &[OsString]) -> Output {
    Command::new(bin)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("cannot run {}: {e}", bin.display()))
}

fn path_as_arg(path: &Path) -> OsString {
    path.as_os_str().to_os_string()
}

fn out_path_from(args: Vec<OsString>, _out: Output) -> PathBuf {
    // The `-o` value is the artifact path; recover it from the argument list.
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "-o" {
            if let Some(value) = iter.next() {
                return PathBuf::from(value);
            }
        }
    }
    PathBuf::from("out")
}

/// Write an in-memory source to a unique `.cx` file when its nominal path does
/// not exist on disk (most diagnostic tests use synthetic names).
fn source_path(file: &SourceFile) -> (PathBuf, Option<PathBuf>) {
    let nominal = Path::new(&file.name);
    if nominal.is_file() {
        return (nominal.to_path_buf(), None);
    }
    let dir = corex::driver::temp_dir("corexc-parity-src");
    std::fs::create_dir_all(&dir).expect("create parity source dir");
    let mut name = nominal
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "input.cx".to_string());
    if !name.ends_with(".cx") {
        name.push_str(".cx");
    }
    let path = dir.join(name);
    std::fs::write(&path, &file.text).expect("write parity source");
    (path, Some(dir))
}

fn manifest_value(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix(key) {
            let rest = rest.trim_start();
            if let Some(rest) = rest.strip_prefix('=') {
                return Some(rest.trim().trim_matches('"').to_string());
            }
        }
    }
    None
}

fn offset_of(text: &str, line: usize, col: usize) -> usize {
    let bytes = text.as_bytes();
    let mut current = 1;
    let mut i = 0;
    while i < bytes.len() && current < line {
        if bytes[i] == b'\n' {
            current += 1;
        }
        i += 1;
    }
    let mut column = 1;
    while i < bytes.len() && column < col && bytes[i] != b'\n' {
        i += 1;
        column += 1;
    }
    i
}

fn is_word(byte: u8) -> bool {
    byte == b'_' || byte.is_ascii_alphanumeric()
}

/// Parse the `path:line:col: error[CODE]: message` lines `corexc` prints.
fn parse_diagnostics(file: &SourceFile, stderr: &[u8]) -> Vec<Diagnostic> {
    let stderr = String::from_utf8_lossy(stderr);
    let mut diagnostics = Vec::new();
    for line in stderr.lines() {
        let (marker, severity, bracketed) = if let Some(index) = line.find("error[") {
            (index, Severity::Error, true)
        } else if let Some(index) = line.find("warning[") {
            (index, Severity::Warning, true)
        } else if let Some(index) = line.find(": error: ") {
            (index + 2, Severity::Error, false)
        } else if let Some(index) = line.find(": warning: ") {
            (index + 2, Severity::Warning, false)
        } else {
            continue;
        };
        let prefix = line[..marker].trim_end();
        let prefix = prefix.strip_suffix(':').unwrap_or(prefix).trim_end();
        let mut parts = prefix.rsplitn(3, ':');
        let col: usize = parts.next().and_then(|v| v.trim().parse().ok()).unwrap_or(1);
        let line_no: usize = parts.next().and_then(|v| v.trim().parse().ok()).unwrap_or(1);
        let _path = parts.next().unwrap_or("");
        let after = &line[marker..];
        let (code, message) = if bracketed {
            match after.find("]: ") {
                Some(index) => (
                    after[..index]
                        .trim_start_matches("error[")
                        .trim_start_matches("warning[")
                        .to_string(),
                    after[index + 3..].to_string(),
                ),
                None => (String::new(), after.to_string()),
            }
        } else {
            let message = after
                .strip_prefix("error: ")
                .or_else(|| after.strip_prefix("warning: "))
                .unwrap_or(after)
                .to_string();
            (String::new(), message)
        };
        let start = offset_of(&file.text, line_no, col);
        let bytes = file.text.as_bytes();
        let mut end = start;
        while end < bytes.len() && is_word(bytes[end]) {
            end += 1;
        }
        if end == start && end < bytes.len() {
            end += 1;
        }
        diagnostics.push(Diagnostic {
            code: static_code(&code),
            message,
            span: Span::new(start.min(file.text.len()), end.min(file.text.len())),
            note: None,
            severity,
        });
    }
    diagnostics
}

//! CoreX command-line interface.
//!
//! Exit codes: 0 success, 1 compile error, 2 usage or I/O error, 3 external
//! tool failure. `corex run` and `corex exec` propagate the compiled program's
//! exit status.

use std::io::Read;
use std::path::{Path, PathBuf};

use corex::diagnostics::{Diagnostic, SourceFile};
use corex::driver;

const USAGE: &str = "\
CoreX compiler (corex 0.1.0)

Usage:
  corex build   <file.cx | -> [-o <output>] [--opt <0-3>] [--link <obj|a>]...
                [-L <dir>]... [-l <lib>]... [--hosted] [--shared]
                [--cfg name=value]... [-I <dir>]... [--emit obj|shared|staticlib]
                [-g] [--sanitize]
  corex run     <file.cx | -> [--opt <0-3>] [--link ...] [--hosted] [-- <program args>...]
  corex exec    '<source>' [--opt <0-3>] [-- <program args>...]
  corex repl    [--opt <0-3>]
  corex emit-ir <file.cx | -> [--verify]
  corex check   <file.cx | ->
  corex fmt     <file.cx | -> [-w | --check]
  corex lint    <file.cx | -> [--deny-warnings]
  corex doc     <file.cx | -> [-o <out.md>]
  corex test    <file.cx | -> [--opt <0-3>] [--cfg name=value]... [-I <dir>]...

Commands:
  build    Compile to a freestanding Linux x86-64 ELF executable. Pass a
           directory containing corex.toml to build a project.
  run      Compile, then run; exits with the program's status.
  exec     Run source given directly on the command line (no file).
  repl     Interactive session: type code, finish a block with a blank line.
  emit-ir  Print the generated LLVM 20 IR (--verify assembles it).
  check    Run the frontend only (lex, parse, semantic).
  fmt      Format whitespace; `-w` writes in place, `--check` fails if unformatted.
  lint     Report unused variables/items, unreachable code, constant conditions.
  doc      Generate Markdown documentation from `///` comments.
  test     Build and run `fn test_name() -> bool` functions.

Reading source:
  A file argument of `-` reads the source from stdin, e.g.
    printf 'fn main() -> i32 { return 7; }' | corex run -
    corex run - < examples/hello.cx

Environment:
  COREX_LLVM_BIN  Directory containing llvm-as, opt, llc (LLVM 20).
  COREX_CC        Linker driver used to produce the ELF (default: cc).
";

fn main() {
    let code = real_main();
    std::process::exit(code);
}

fn real_main() -> i32 {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" || args[0] == "-h" {
        print!("{USAGE}");
        return if args.is_empty() { 2 } else { 0 };
    }
    if args[0] == "--version" || args[0] == "-V" {
        println!("corex 0.1.0 (LLVM 20 backend)");
        return 0;
    }

    match args[0].as_str() {
        "build" => cmd_build(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "exec" => cmd_exec(&args[1..]),
        "repl" => cmd_repl(&args[1..]),
        "emit-ir" => cmd_emit_ir(&args[1..]),
        "check" => cmd_check(&args[1..]),
        "fmt" => cmd_fmt(&args[1..]),
        "lint" => cmd_lint(&args[1..]),
        "doc" => cmd_doc(&args[1..]),
        "test" => cmd_test(&args[1..]),
        other => {
            eprintln!("error: unknown command `{other}`\n");
            eprint!("{USAGE}");
            2
        }
    }
}

struct CommonArgs {
    file: Option<PathBuf>,
    output: Option<PathBuf>,
    opt: u8,
    verify: bool,
    program_args: Vec<String>,
    link: driver::LinkInputs,
    cfg: Vec<String>,
    include_paths: Vec<PathBuf>,
    emit: Option<String>,
    debug_info: bool,
    sanitize: bool,
}

fn parse_opt(value: Option<&String>) -> Result<u8, String> {
    let v = value.ok_or("`--opt` requires a value 0-3")?;
    let opt: u8 = v.parse().map_err(|_| format!("invalid --opt value `{v}`"))?;
    if opt > 3 {
        return Err(format!("invalid --opt value `{v}` (expected 0-3)"));
    }
    Ok(opt)
}

fn parse_common(args: &[String], allow_output: bool, allow_opt: bool) -> Result<CommonArgs, String> {
    let mut out = CommonArgs {
        file: None,
        output: None,
        opt: 2,
        verify: false,
        program_args: Vec::new(),
        link: driver::LinkInputs::default(),
        cfg: Vec::new(),
        include_paths: Vec::new(),
        emit: None,
        debug_info: false,
        sanitize: false,
    };
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--opt" if allow_opt => {
                out.opt = parse_opt(args.get(i + 1))?;
                i += 2;
            }
            "--verify" => {
                out.verify = true;
                i += 1;
            }
            "-o" if allow_output => {
                let v = args.get(i + 1).ok_or("`-o` requires a path")?;
                out.output = Some(PathBuf::from(v));
                i += 2;
            }
            "--link" => {
                let v = args.get(i + 1).ok_or("`--link` requires a path")?;
                out.link.objects.push(PathBuf::from(v));
                i += 2;
            }
            "-L" => {
                let v = args.get(i + 1).ok_or("`-L` requires a directory")?;
                out.link.lib_paths.push(PathBuf::from(v));
                i += 2;
            }
            "-l" => {
                let v = args.get(i + 1).ok_or("`-l` requires a library name")?;
                out.link.libs.push(v.clone());
                i += 2;
            }
            "--hosted" => {
                out.link.hosted = true;
                i += 1;
            }
            "--shared" => {
                out.link.shared = true;
                i += 1;
            }
            "--cfg" => {
                let v = args.get(i + 1).ok_or("`--cfg` requires `name=value`")?;
                out.cfg.push(v.clone());
                i += 2;
            }
            "-I" | "--include" => {
                let v = args.get(i + 1).ok_or("`-I` requires a directory")?;
                out.include_paths.push(PathBuf::from(v));
                i += 2;
            }
            "-g" | "--debug" => {
                out.debug_info = true;
                i += 1;
            }
            "--sanitize" => {
                out.sanitize = true;
                i += 1;
            }
            "--emit" => {
                let v = args.get(i + 1).ok_or("`--emit` requires obj, shared, or staticlib")?;
                out.emit = Some(v.clone());
                i += 2;
            }
            "--" => {
                out.program_args = args[i + 1..].to_vec();
                break;
            }
            other if other.starts_with('-') && other != "-" => {
                return Err(format!("unknown option `{other}`"));
            }
            path => {
                if out.file.is_some() {
                    return Err(format!("unexpected extra argument `{path}`"));
                }
                out.file = Some(PathBuf::from(path));
                i += 1;
            }
        }
    }
    if out.file.is_none() {
        return Err("no input file".to_string());
    }
    Ok(out)
}

fn is_stdin(path: &Path) -> bool {
    path == Path::new("-")
}

fn load(file: &Path) -> Result<SourceFile, i32> {
    if is_stdin(file) {
        let mut text = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut text) {
            eprintln!("error: cannot read stdin: {e}");
            return Err(2);
        }
        let base = SourceFile::new("<stdin>", text);
        return match driver::load_source(&base) {
            Ok(f) => Ok(f),
            Err(diags) => {
                report(&base, &diags);
                Err(exit_code_for(&diags))
            }
        };
    }

    let text = match std::fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", file.display());
            return Err(2);
        }
    };
    let base = SourceFile::new(file.display().to_string(), text);
    match driver::load_source(&base) {
        Ok(f) => Ok(f),
        Err(diags) => {
            report(&base, &diags);
            Err(exit_code_for(&diags))
        }
    }
}

fn report(file: &SourceFile, diags: &[Diagnostic]) {
    eprint!("{}", file.render_all(diags));
    let errors = diags.iter().filter(|d| !d.is_warning()).count();
    let warnings = diags.len() - errors;
    if warnings > 0 && errors > 0 {
        eprintln!("\n{errors} error(s), {warnings} warning(s) generated");
    } else if warnings > 0 {
        eprintln!(
            "\n{warnings} warning{} generated",
            if warnings == 1 { "" } else { "s" }
        );
    } else {
        eprintln!(
            "\n{} error{} generated",
            errors,
            if errors == 1 { "" } else { "s" }
        );
    }
}

fn exit_code_for(diags: &[Diagnostic]) -> i32 {
    if diags.iter().any(|d| d.code.starts_with("E03")) {
        3
    } else {
        1
    }
}

fn default_output(file: &Path) -> PathBuf {
    if is_stdin(file) {
        return PathBuf::from("a.out");
    }
    let stem = file
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "a.out".to_string());
    PathBuf::from(stem)
}

/// Build `file` into a temp directory, run it, and return its exit status.
fn run_source(
    file: &SourceFile,
    opt: u8,
    program_args: &[String],
    link: &driver::LinkInputs,
    config: &driver::BuildConfig,
) -> i32 {
    let work = driver::temp_dir("corex-run");
    if let Err(e) = std::fs::create_dir_all(&work) {
        eprintln!("error: cannot create {}: {e}", work.display());
        return 2;
    }
    let exe = work.join("program");

    let artifacts = match driver::build_with_config(file, &exe, opt, false, link, config) {
        Ok(a) => a,
        Err(diags) => {
            report(file, &diags);
            let _ = std::fs::remove_dir_all(&work);
            return exit_code_for(&diags);
        }
    };

    let status = match driver::run(&artifacts.executable, program_args) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("error: {e}");
            let _ = std::fs::remove_dir_all(&work);
            return 2;
        }
    };

    let _ = std::fs::remove_dir_all(&work);

    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(code) = status.code() {
            return code;
        }
        if let Some(sig) = status.signal() {
            return 128 + sig;
        }
    }
    if status.success() {
        0
    } else {
        1
    }
}

fn cmd_build(args: &[String]) -> i32 {
    let parsed = match parse_common(args, true, true) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let input = parsed.file.unwrap();
    if input.is_dir() || input.file_name().map(|n| n == "corex.toml").unwrap_or(false) {
        return match driver::build_manifest(
            &input,
            Some(parsed.opt),
            parsed.output.as_deref(),
            &parsed.cfg,
        ) {
            Ok(path) => {
                println!("built {} -> {}", input.display(), path.display());
                0
            }
            Err(diags) => {
                for d in &diags {
                    eprintln!("error[{}]: {}", d.code, d.message);
                    if let Some(note) = &d.note {
                        eprintln!("  = note: {note}");
                    }
                }
                exit_code_for(&diags)
            }
        };
    }
    let file = match load(&input) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let output = parsed
        .output
        .unwrap_or_else(|| default_output(&input));
    let config = driver::BuildConfig {
        cfg: parsed.cfg.clone(),
        include_paths: parsed.include_paths.clone(),
        require_entry: true,
        test_mode: false,
        debug_info: parsed.debug_info,
        sanitize: parsed.sanitize,
        hosted: parsed.link.hosted,
    };

    if let Some(emit) = &parsed.emit {
        if emit == "obj" {
            let work = driver::temp_dir("corex-obj");
            let result = driver::compile_units(std::slice::from_ref(&file), &work, parsed.opt, &config);
            return match result {
                Ok((objects, _)) if objects.len() == 1 => {
                    match std::fs::copy(&objects[0], &output) {
                        Ok(_) => {
                            println!("compiled {} -> {}", input.display(), output.display());
                            let _ = std::fs::remove_dir_all(&work);
                            0
                        }
                        Err(e) => {
                            eprintln!("error: cannot write {}: {e}", output.display());
                            2
                        }
                    }
                }
                Ok(_) => {
                    eprintln!("error: project has multiple objects; use --emit staticlib");
                    2
                }
                Err(diags) => {
                    report(&file, &diags);
                    exit_code_for(&diags)
                }
            };
        }
        if let Some(kind) = driver::OutputKind::parse(emit) {
            if kind == driver::OutputKind::Executable {
                // Fall through to the normal build path.
            } else {
                let mut project_config = config.clone();
                project_config.require_entry = kind == driver::OutputKind::Executable;
                return match driver::build_project(
                    std::slice::from_ref(&file),
                    &output,
                    parsed.opt,
                    &parsed.link,
                    &project_config,
                    kind,
                ) {
                    Ok(art) => {
                        println!("compiled {} -> {}", input.display(), art.executable.display());
                        0
                    }
                    Err(diags) => {
                        report(&file, &diags);
                        exit_code_for(&diags)
                    }
                };
            }
        } else {
            eprintln!("error: unknown --emit value `{emit}` (expected obj, shared, or staticlib)");
            return 2;
        }
    }

    match driver::build_with_config(&file, &output, parsed.opt, false, &parsed.link, &config) {
        Ok(art) => {
            println!(
                "compiled {} -> {}",
                if is_stdin(&input) { "<stdin>" } else { input.to_str().unwrap_or("") },
                art.executable.display()
            );
            0
        }
        Err(diags) => {
            report(&file, &diags);
            exit_code_for(&diags)
        }
    }
}

fn cmd_run(args: &[String]) -> i32 {
    let parsed = match parse_common(args, false, true) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let input = parsed.file.unwrap();
    let file = match load(&input) {
        Ok(f) => f,
        Err(code) => return code,
    };
    run_source(
        &file,
        parsed.opt,
        &parsed.program_args,
        &parsed.link,
        &driver::BuildConfig {
            cfg: parsed.cfg.clone(),
            include_paths: parsed.include_paths.clone(),
            require_entry: true,
            test_mode: false,
            debug_info: parsed.debug_info,
            sanitize: parsed.sanitize,
            hosted: parsed.link.hosted,
        },
    )
}

fn cmd_exec(args: &[String]) -> i32 {
    let mut opt = 2u8;
    let mut source: Option<String> = None;
    let mut program_args = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--opt" => match parse_opt(args.get(i + 1)) {
                Ok(o) => {
                    opt = o;
                    i += 2;
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return 2;
                }
            },
            "--" => {
                program_args = args[i + 1..].to_vec();
                break;
            }
            other if source.is_none() => {
                source = Some(other.to_string());
                i += 1;
            }
            other => {
                eprintln!("error: unexpected argument `{other}` (quote the source, then use `--` for program arguments)");
                return 2;
            }
        }
    }

    let Some(source) = source else {
        eprintln!("error: `exec` needs source code, or `-` to read it from stdin");
        return 2;
    };

    let file = if source == "-" {
        match load(Path::new("-")) {
            Ok(f) => f,
            Err(code) => return code,
        }
    } else {
        SourceFile::new("<exec>", source)
    };

    run_source(
        &file,
        opt,
        &program_args,
        &driver::LinkInputs::default(),
        &driver::BuildConfig::default(),
    )
}

fn cmd_repl(args: &[String]) -> i32 {
    let mut opt = 2u8;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--opt" => match parse_opt(args.get(i + 1)) {
                Ok(o) => {
                    opt = o;
                    i += 2;
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    return 2;
                }
            },
            other => {
                eprintln!("error: unknown argument `{other}`\n\n{USAGE}");
                return 2;
            }
        }
    }
    corex::repl::run(opt)
}

fn cmd_emit_ir(args: &[String]) -> i32 {
    let parsed = match parse_common(args, false, false) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let input = parsed.file.unwrap();
    let file = match load(&input) {
        Ok(f) => f,
        Err(code) => return code,
    };
    match driver::emit_ir(&file) {
        Ok(ir) => {
            if parsed.verify {
                match verify(&ir) {
                    Ok(()) => eprintln!("IR verified by llvm-as"),
                    Err(msg) => {
                        eprintln!("error: IR verification failed\n{msg}");
                        return 3;
                    }
                }
            }
            print!("{ir}");
            0
        }
        Err(diags) => {
            report(&file, &diags);
            1
        }
    }
}

fn cmd_check(args: &[String]) -> i32 {
    let parsed = match parse_common(args, false, false) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let input = parsed.file.unwrap();
    let file = match load(&input) {
        Ok(f) => f,
        Err(code) => return code,
    };
    match driver::check(&file) {
        Ok(fe) => {
            println!(
                "ok: {} function(s), entry `{}`",
                fe.program.fns.len(),
                fe.analysis.entry
            );
            0
        }
        Err(diags) => {
            report(&file, &diags);
            1
        }
    }
}

fn read_source(path: &Path) -> Result<(String, String), i32> {
    if is_stdin(path) {
        let mut text = String::new();
        if let Err(e) = std::io::stdin().read_to_string(&mut text) {
            eprintln!("error: cannot read stdin: {e}");
            return Err(2);
        }
        return Ok(("<stdin>".to_string(), text));
    }
    match std::fs::read_to_string(path) {
        Ok(text) => Ok((path.display().to_string(), text)),
        Err(e) => {
            eprintln!("error: cannot read {}: {e}", path.display());
            Err(2)
        }
    }
}

fn cmd_fmt(args: &[String]) -> i32 {
    let mut file: Option<PathBuf> = None;
    let mut write = false;
    let mut check = false;
    for arg in args {
        match arg.as_str() {
            "-w" | "--write" => write = true,
            "--check" => check = true,
            other if other.starts_with('-') && other != "-" => {
                eprintln!("error: unknown option `{other}`");
                return 2;
            }
            path => {
                if file.is_some() {
                    eprintln!("error: unexpected extra argument `{path}`");
                    return 2;
                }
                file = Some(PathBuf::from(path));
            }
        }
    }
    let Some(file) = file else {
        eprintln!("error: `fmt` needs a file (or `-` for stdin)");
        return 2;
    };
    let (name, text) = match read_source(&file) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let formatted = corex::format::format_source(&text);
    if check {
        if formatted == text {
            return 0;
        }
        eprintln!("{name}: not formatted");
        return 1;
    }
    if write {
        if is_stdin(&file) {
            print!("{formatted}");
            return 0;
        }
        return match std::fs::write(&file, formatted) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: cannot write {}: {e}", file.display());
                2
            }
        };
    }
    print!("{formatted}");
    0
}

fn cmd_lint(args: &[String]) -> i32 {
    let mut file: Option<PathBuf> = None;
    let mut deny = false;
    for arg in args {
        match arg.as_str() {
            "--deny-warnings" => deny = true,
            other if other.starts_with('-') && other != "-" => {
                eprintln!("error: unknown option `{other}`");
                return 2;
            }
            path => {
                if file.is_some() {
                    eprintln!("error: unexpected extra argument `{path}`");
                    return 2;
                }
                file = Some(PathBuf::from(path));
            }
        }
    }
    let Some(file) = file else {
        eprintln!("error: `lint` needs a file (or `-` for stdin)");
        return 2;
    };
    let (_name, text) = match read_source(&file) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let diagnostics = match driver::lint_source(&text) {
        Ok(diags) => diags,
        Err(diags) => {
            let source = SourceFile::new(file.display().to_string(), text);
            report(&source, &diags);
            return 1;
        }
    };
    if diagnostics.is_empty() {
        return 0;
    }
    let source = SourceFile::new(file.display().to_string(), text);
    report(&source, &diagnostics);
    if deny {
        1
    } else {
        0
    }
}

fn cmd_doc(args: &[String]) -> i32 {
    let mut file: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "-o" => {
                let Some(path) = args.get(i + 1) else {
                    eprintln!("error: `-o` requires a path");
                    return 2;
                };
                output = Some(PathBuf::from(path));
                i += 2;
            }
            other if other.starts_with('-') && other != "-" => {
                eprintln!("error: unknown option `{other}`");
                return 2;
            }
            path => {
                if file.is_some() {
                    eprintln!("error: unexpected extra argument `{path}`");
                    return 2;
                }
                file = Some(PathBuf::from(path));
                i += 1;
            }
        }
    }
    let Some(file) = file else {
        eprintln!("error: `doc` needs a file (or `-` for stdin)");
        return 2;
    };
    let (name, text) = match read_source(&file) {
        Ok(v) => v,
        Err(code) => return code,
    };
    let source = SourceFile::new(name, text);
    let program = match corex::lexer::lex(&source.text)
        .map_err(|d| d)
        .and_then(|tokens| corex::parser::parse(&tokens))
    {
        Ok(p) => p,
        Err(diags) => {
            report(&source, &diags);
            return 1;
        }
    };
    let markdown = corex::docgen::generate(&source, &program);
    match output {
        Some(path) => match std::fs::write(&path, markdown) {
            Ok(()) => 0,
            Err(e) => {
                eprintln!("error: cannot write {}: {e}", path.display());
                2
            }
        },
        None => {
            print!("{markdown}");
            0
        }
    }
}

fn cmd_test(args: &[String]) -> i32 {
    let parsed = match parse_common(args, false, true) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return 2;
        }
    };
    let input = parsed.file.unwrap();
    let file = match load(&input) {
        Ok(f) => f,
        Err(code) => return code,
    };
    let config = driver::BuildConfig {
        cfg: parsed.cfg.clone(),
        include_paths: parsed.include_paths.clone(),
        require_entry: false,
        test_mode: true,
        debug_info: false,
        sanitize: false,
        hosted: false,
    };
    run_source(&file, parsed.opt, &[], &parsed.link, &config)
}

fn verify(ir: &str) -> Result<(), String> {
    driver::verify_ir(ir)
}

//! Driver-facing stdlib helpers (P0-3): `envp` lookup, `pipe`/`dup2` capture,
//! `spawn_argv`/`run_argv`, and `std_fs::read_file` failure distinction.
//!
//! A CoreX driver must be able to run `llvm-as`/`opt`/`llc` with an argument
//! vector, capture their output, and read the exit status.

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver::{self, BuildConfig, LinkInputs};

fn lib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../lib/corex")
}

struct Outcome {
    stdout: String,
    stderr: String,
    code: i32,
}

impl Outcome {
    fn run(build: impl FnOnce(Command) -> Command, source: &str) -> Outcome {
        let work = driver::temp_dir("corex-driver");
        std::fs::create_dir_all(&work).expect("create dir");
        std::fs::write(work.join("empty.txt"), b"").expect("write empty file");
        let src = work.join("program.cx");
        std::fs::write(&src, source).expect("write source");
        let file = SourceFile::new(src.display().to_string(), source);
        let exe = work.join("program");
        let config = BuildConfig {
            include_paths: vec![lib_dir()],
            ..Default::default()
        };
        let artifacts =
            driver::build_with_config(&file, &exe, 2, false, &LinkInputs::default(), &config)
                .unwrap_or_else(|d| panic!("compile failed:\n{}", file.render_all(&d)));

        let mut command = Command::new(&artifacts.executable);
        command
            .arg(work.join("empty.txt"))
            .arg(work.join("missing.txt"))
            .current_dir(&work);
        let out = build(command).output().expect("run program");
        let result = Outcome {
            stdout: String::from_utf8_lossy(&out.stdout).to_string(),
            stderr: String::from_utf8_lossy(&out.stderr).to_string(),
            code: out.status.code().unwrap_or(-1),
        };
        let _ = std::fs::remove_dir_all(&work);
        result
    }
}

const ENV_PROGRAM: &str = r#"
mod std_env;

fn main(argc: i32, argv: **u8, envp: **u8) -> i32 {
    let mut found: bool = false;
    let value: str = std_env::env_get(envp, "COREX_TEST_VAR", &mut found);
    print(found);
    print(value);
    print(std_env::env_has(envp, "COREX_TEST_VAR"));
    print(std_env::env_get_or(envp, "COREX_MISSING_VAR", "fallback"));
    // A missing variable is not an empty value, and prefixes do not match.
    print(std_env::env_has(envp, "COREX_TEST"));
    let mut empty_found: bool = false;
    let empty: str = std_env::env_get(envp, "COREX_EMPTY_VAR", &mut empty_found);
    print(empty_found);
    print(len(empty));
    return 0;
}
"#;

#[test]
fn envp_lookup_reports_presence_and_empty_values() {
    let r = Outcome::run(
        |mut cmd| {
            cmd.env("COREX_TEST_VAR", "value42")
                .env("COREX_EMPTY_VAR", "")
                .env_remove("COREX_TEST");
            cmd
        },
        ENV_PROGRAM,
    );
    assert_eq!(r.stdout, "true\nvalue42\ntrue\nfallback\nfalse\ntrue\n0\n");
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

const PROCESS_PROGRAM: &str = r#"
mod std_io;
mod std_process;
mod std_fs;

fn main(argc: i32, argv: **u8, envp: **u8) -> i32 {
    print(argc);
    let mut fds: [i32; 2] = [0, 0];
    if std_io::pipe(&mut fds as *mut i32) != 0 {
        print("pipe failed");
        return 1;
    }
    let pid: i32 = std_process::fork();
    if pid == 0 {
        let _ = std_io::dup2(fds[1], 1);
        std_io::close(fds[0]);
        std_io::close(fds[1]);
        let mut child: [*u8; 3] = [null, null, null];
        child[0] = c"/bin/echo" as *u8;
        child[1] = c"hello-from-child" as *u8;
        std_process::exec(c"/bin/echo", &mut child as **u8, envp);
        std_process::exit(127);
    }
    std_io::close(fds[1]);
    let mut buf: [u8; 64] = [0; 64];
    let n: isize = std_io::read_all(fds[0], &mut buf as *mut u8, 64);
    std_io::close(fds[0]);
    print(n);
    print(str_from_raw(&buf as *u8, n as usize));
    let status: i32 = std_process::wait(pid);
    print((status >> 8) & 255);

    // run_argv forwards the full argument vector and the exit status.
    let mut sh: [*u8; 4] = [null, null, null, null];
    sh[0] = c"/bin/sh" as *u8;
    sh[1] = c"-c" as *u8;
    sh[2] = c"exit 3" as *u8;
    print(std_process::run_argv(c"/bin/sh", &mut sh as **u8, envp));

    // read_file returns 0 for an empty file and -1 for a missing one.
    let mut rbuf: [u8; 8] = [0; 8];
    let mut empty: isize = 0;
    let mut missing: isize = 0;
    unsafe {
        empty = std_fs::read_file(*(argv + 1) as cstr, &mut rbuf as *mut u8, 8);
        missing = std_fs::read_file(*(argv + 2) as cstr, &mut rbuf as *mut u8, 8);
    }
    print(empty);
    print(missing);
    return 0;
}
"#;

#[test]
fn fork_exec_pipe_capture_and_run_argv() {
    let r = Outcome::run(|cmd| cmd, PROCESS_PROGRAM);
    assert_eq!(
        r.stdout,
        "3\n17\nhello-from-child\n\n0\n3\n0\n-1\n"
    );
    assert_eq!(r.code, 0, "stderr: {}", r.stderr);
}

/// Scratch directories use an unpredictable name and are removed on drop, so
/// no build path can leak them.
#[test]
fn scratch_directories_are_private_and_self_removing() {
    let first_guard = driver::TempDir::new("corex-guard");
    let first = first_guard.path().to_path_buf();
    assert!(first.is_dir(), "TempDir did not create {}", first.display());

    let second_guard = driver::TempDir::new("corex-guard");
    assert_ne!(
        first,
        second_guard.path(),
        "two scratch directories shared a name"
    );

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&first)
            .expect("metadata")
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(mode, 0o700, "scratch directory mode is {mode:o}");
    }

    drop(first_guard);
    if std::env::var("COREX_KEEP_TEMP").is_err() {
        assert!(!first.exists(), "{} survived the guard", first.display());
    }
    drop(second_guard);
}

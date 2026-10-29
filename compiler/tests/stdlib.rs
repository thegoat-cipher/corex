//! Stdlib coverage: allocator API, C memory/string routines, file I/O,
//! formatting, directories, time, math, and process helpers.

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver::{self, BuildConfig, LinkInputs};

fn lib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../lib/corex")
}

fn build_and_run(source: &str) -> (String, String, i32) {
    let work = driver::temp_dir("corex-stdlib");
    std::fs::create_dir_all(&work).expect("create dir");
    let src = work.join("program.cx");
    std::fs::write(&src, source).expect("write source");
    let file = SourceFile::new(src.display().to_string(), source);
    let exe = work.join("program");
    let config = BuildConfig {
        include_paths: vec![lib_dir()],
        ..Default::default()
    };
    let artifacts = driver::build_with_config(&file, &exe, 2, false, &LinkInputs::default(), &config)
        .unwrap_or_else(|d| panic!("compile failed:\n{}", file.render_all(&d)));
    let out = Command::new(&artifacts.executable)
        .output()
        .expect("run program");
    let _ = std::fs::remove_dir_all(&work);
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

#[test]
fn allocator_memory_and_string_library() {
    let (stdout, stderr, code) = build_and_run(
        r#"
        mod std_mem;
        mod std_str;
        mod std_alloc;

        fn main() -> i32 {
            unsafe {
                let p: *mut u8 = std_alloc::malloc(64);
                print(p != null);
                let p2: *mut u8 = std_alloc::calloc(4, 8);
                print(*(p2 + 0) as i32);
                std_mem::memset(p, 65, 8);
                print(*(p + 0) as i32);
                print(std_mem::memcmp(p, p2, 4));
                let p3: *mut u8 = std_alloc::realloc(p, 128);
                print(*(p3 + 0) as i32);
                std_alloc::free(p2);
                std_alloc::free(p3);
                let s: cstr = c"hello";
                print(std_str::strlen(s));
                print(std_str::strcmp(c"abc", c"abd"));
                let buf: *mut u8 = std_alloc::malloc(32);
                std_str::strcpy(buf, c"hi");
                print(std_str::strlen(buf as cstr));
                print(std_str::str_eq("abc", "abc"));
                print(std_str::str_find("hello world", "world"));
                std_alloc::free(buf);
                let mut arena: std_alloc::Arena = std_alloc::arena_new(1024);
                let a1: *mut u8 = std_alloc::arena_alloc(&mut arena, 16, 8);
                let a2: *mut u8 = std_alloc::arena_alloc(&mut arena, 16, 8);
                print(a2 as usize - a1 as usize);
                std_alloc::arena_reset(&mut arena);
                let a3: *mut u8 = std_alloc::arena_alloc(&mut arena, 16, 8);
                print(a3 != null);
                std_alloc::arena_free(&mut arena);
            }
            return 0;
        }
        "#,
    );
    assert_eq!(
        stdout,
        "true\n0\n65\n1\n65\n5\n-1\n2\ntrue\n6\n16\ntrue\n"
    );
    assert_eq!(code, 0, "stderr: {stderr}");
}

#[test]
fn file_io_formatting_time_and_directories() {
    let (stdout, stderr, code) = build_and_run(
        r#"
        mod std_io;
        mod std_fs;
        mod std_time;
        mod std_math;
        mod std_fmt;

        fn main() -> i32 {
            let mut buf: [u8; 128] = [0; 128];
            let mut f: std_fmt::Fmt = std_fmt::new(&mut buf as *mut u8, 128);
            std_fmt::push_str(&mut f, "x=");
            std_fmt::push_i64(&mut f, -12345);
            std_fmt::push_str(&mut f, " hex=");
            std_fmt::push_hex(&mut f, 255);
            std_fmt::push_str(&mut f, " pi=");
            std_fmt::push_f64(&mut f, 3.14159);
            print(std_fmt::finish(&mut f));

            unsafe {
                let fd: i32 = std_io::open(c"/tmp/corex_stdlib_test.txt", std_io::O_WRONLY | std_io::O_CREAT | std_io::O_TRUNC, 420);
                print(fd >= 0);
                std_io::write_all(fd, c"file contents" as *u8, 13);
                std_io::close(fd);
            }
            print(std_fs::exists(c"/tmp/corex_stdlib_test.txt"));
            print(std_fs::file_size(c"/tmp/corex_stdlib_test.txt"));
            unsafe {
                let mut rbuf: [u8; 64] = [0; 64];
                let n: isize = std_fs::read_file(c"/tmp/corex_stdlib_test.txt", &mut rbuf as *mut u8, 64);
                print(n);
                print(str_from_raw(&rbuf as *u8, n as usize));
            }
            print(std_time::monotonic_ns() > 0);
            print(std_math::sqrt(81.0));
            print(std_math::powi(2.0, 10));
            print(std_math::popcount64(255));
            print(std_math::clz64(1));
            let mut d: std_fs::Dir = std_fs::dir_open(c"/tmp");
            let mut count: i32 = 0;
            loop {
                let name: cstr = std_fs::dir_next(&mut d);
                if name == null {
                    break;
                }
                count += 1;
            }
            std_fs::dir_close(&mut d);
            print(count > 0);
            return 0;
        }
        "#,
    );
    assert_eq!(
        stdout,
        "x=-12345 hex=0xff pi=3.141590\ntrue\ntrue\n13\n13\nfile contents\ntrue\n9.000000\n1024.000000\n8\n63\ntrue\n"
    );
    assert_eq!(code, 0, "stderr: {stderr}");
}

/// `aligned_alloc`/`calloc` must report overflow instead of wrapping the size
/// arithmetic and returning a block smaller than the caller asked for.
#[test]
fn allocator_returns_null_when_size_arithmetic_overflows() {
    let (stdout, stderr, code) = build_and_run(
        r#"
        mod std_alloc;

        fn main() -> i32 {
            let huge: usize = 0 - 1;
            unsafe {
                print(std_alloc::aligned_alloc(huge, 16) == null);
                print(std_alloc::malloc(huge) == null);
                print(std_alloc::calloc(1 << 40, 1 << 40) == null);
                print(std_alloc::calloc(huge, 2) == null);
                // A zero count must not divide by zero inside the guard.
                let zero: *mut u8 = std_alloc::calloc(0, 4);
                if zero != null { std_alloc::free(zero); }
                // Ordinary allocations still succeed and stay zero-filled.
                let p: *mut u8 = std_alloc::calloc(4, 8);
                print(p != null);
                print(*(p + 31) as i32);
                std_alloc::free(p);
            }
            return 0;
        }
        "#,
    );
    assert_eq!(
        stdout, "true\ntrue\ntrue\ntrue\ntrue\n0\n",
        "stderr: {stderr}"
    );
    assert_eq!(code, 0);
}

//! Atomics, volatile globals, inline assembly, threads, and futex-based locks.

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver::{self, BuildConfig, LinkInputs};

fn lib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../lib/corex")
}

fn build_and_run(source: &str) -> (String, String, i32) {
    let work = driver::temp_dir("corex-concurrency");
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
fn atomics_volatile_and_inline_asm() {
    let (stdout, stderr, code) = build_and_run(
        r#"
        static mut volatile FLAG: u32 = 0;

        fn main() -> i32 {
            print(FLAG);
            FLAG = 7;
            print(FLAG);
            unsafe {
                let mut x: u32 = 0;
                print(atomic_fetch_add(&mut x, 5));
                print(x);
                print(atomic_load(&x));
                atomic_store(&mut x, 9);
                print(atomic_cas(&mut x, 9, 11));
                print(x);
                print(atomic_fetch_or(&mut x, 4));
                print(x);
                atomic_fence();
                print(volatile_load(&FLAG));
                volatile_store(&mut FLAG, 3);
                print(FLAG);
                let v: usize = asm("movq $$1, $0", "=r");
                print(v);
                asm("nop", "");
                asm("", "~{memory}");
            }
            return 0;
        }
        "#,
    );
    assert_eq!(
        stdout, "0\n7\n0\n5\n5\ntrue\n11\n11\n15\n7\n3\n1\n"
    );
    assert_eq!(code, 0, "stderr: {stderr}");
}

#[test]
fn explicit_thread_local_storage_round_trips() {
    let (stdout, stderr, code) = build_and_run(
        r#"
        mod std_tls;
        fn main() -> i32 {
            let a: *mut u8 = std_tls::tls_alloc(16);
            let b: *mut u8 = std_tls::tls_alloc(16);
            print(a != null);
            print(a == b);
            unsafe { *(a + 0) = 42; }
            let c: *mut u8 = std_tls::tls_get();
            unsafe { print(*(c + 0) as i32); }
            std_tls::tls_release(a);
            print(std_tls::tls_thread_id() > 0);
            return 0;
        }
        "#,
    );
    assert_eq!(stdout, "true
true
42
true
");
    assert_eq!(code, 0, "stderr: {stderr}");
}

#[test]
fn threads_share_memory_and_a_futex_mutex() {
    let (stdout, stderr, code) = build_and_run(
        r#"
        mod std_thread;
        mod std_sync;

        static mut COUNTER: i64 = 0;

        fn worker(arg: i64) -> i32 {
            let mut m: *mut std_sync::Mutex = null;
            unsafe { m = arg as *mut std_sync::Mutex; }
            let mut i: i64 = 0;
            while i < 5000 {
                unsafe {
                    std_sync::mutex_lock(m);
                    COUNTER += 1;
                    std_sync::mutex_unlock(m);
                }
                i += 1;
            }
            return 0;
        }

        fn main() -> i32 {
            let mut m: std_sync::Mutex = std_sync::mutex_new();
            let mut mp: i64 = 0;
            unsafe { mp = &mut m as *mut std_sync::Mutex as i64; }
            let t1: std_thread::Thread = std_thread::spawn(&worker, mp, 65536);
            let t2: std_thread::Thread = std_thread::spawn(&worker, mp, 65536);
            print(t1.tid > 0);
            print(t2.tid > 0);
            while true {
                unsafe {
                    if atomic_load(&COUNTER) == 10000 {
                        break;
                    }
                }
                std_thread::yield_now();
            }
            print(COUNTER);
            print(std_thread::gettid() > 0);
            return 0;
        }
        "#,
    );
    assert_eq!(stdout, "true\ntrue\n10000\ntrue\n");
    assert_eq!(code, 0, "stderr: {stderr}");
}

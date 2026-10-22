//! `std_map` and `std_builder` coverage (P0-2): deterministic insertion order,
//! hash collisions, missing keys, growth, and 64-bit formatting.

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
use corex::driver::{self, BuildConfig, LinkInputs};

fn lib_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../lib/corex")
}

fn build_and_run(source: &str) -> (String, String, i32) {
    let work = driver::temp_dir("corex-collections");
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

/// `k0` and `k22` hash to the same slot, as do `k2`, `k19`, `k20`, and `k33`
/// (FNV-1a low four bits with 16 slots). They force probe-chain lookups.
const COLLECTIONS: &str = r#"
mod std_map;
mod std_builder;

fn main() -> i32 {
    let mut m: std_map::Map = std_map::map_new();
    let keys: [str; 8] = ["k0", "k22", "k2", "k19", "k20", "k33", "alpha", "beta"];
    let mut i: usize = 0;
    while i < 8 {
        if !std_map::map_put(&mut m, keys[i], (i as u64) + 100) {
            print("put failed");
            return 1;
        }
        i += 1;
    }
    let mut found: bool = false;
    print(std_map::map_get(&mut m, "k0", &mut found));
    print(found);
    print(std_map::map_get(&mut m, "k33", &mut found));
    print(found);
    print(std_map::map_get(&mut m, "k33x", &mut found));
    print(found);
    print(std_map::map_has(&mut m, "k19"));
    print(std_map::map_has(&mut m, "nope"));
    print(std_map::map_len(&mut m));
    // Replacement keeps the original insertion position.
    let _ = std_map::map_put(&mut m, "k2", 777);
    print(std_map::map_get(&mut m, "k2", &mut found));
    print(std_map::map_len(&mut m));
    i = 0;
    while i < std_map::map_len(&mut m) {
        print(std_map::map_key_at(&mut m, i));
        i += 1;
    }
    std_map::map_free(&mut m);
    print(std_map::map_len(&mut m));

    // 14 entries force an entry-array reallocation (8 -> 16) and an index
    // rebuild (16 -> 32 slots).
    let many: [str; 14] = ["g0", "g1", "g2", "g3", "g4", "g5", "g6", "g7", "g8", "g9", "g10", "g11", "g12", "g13"];
    let mut big: std_map::Map = std_map::map_new();
    i = 0;
    while i < 14 {
        let _ = std_map::map_put(&mut big, many[i], i as u64);
        i += 1;
    }
    print(std_map::map_len(&mut big));
    print(std_map::map_get(&mut big, "g0", &mut found));
    print(std_map::map_get(&mut big, "g13", &mut found));
    print(std_map::map_key_at(&mut big, 0));
    print(std_map::map_key_at(&mut big, 13));
    std_map::map_free(&mut big);

    let mut s: std_map::Set = std_map::set_new();
    let _ = std_map::set_insert(&mut s, "x");
    let _ = std_map::set_insert(&mut s, "y");
    let _ = std_map::set_insert(&mut s, "x");
    print(std_map::set_len(&mut s));
    print(std_map::set_has(&mut s, "y"));
    print(std_map::set_has(&mut s, "q"));
    print(std_map::set_key_at(&mut s, 0));
    print(std_map::set_key_at(&mut s, 1));
    std_map::set_free(&mut s);

    let mut b: std_builder::Builder = std_builder::builder_new();
    i = 0;
    while i < 500 {
        let _ = std_builder::builder_push_byte(&mut b, 65);
        i += 1;
    }
    print(std_builder::builder_len(&mut b));
    let _ = std_builder::builder_push_char(&mut b, ',');
    let _ = std_builder::builder_push_i64(&mut b, -9223372036854775808);
    let _ = std_builder::builder_push_str(&mut b, ",max=");
    let _ = std_builder::builder_push_u64(&mut b, 18446744073709551615);
    let _ = std_builder::builder_push_str(&mut b, ",hex=");
    let _ = std_builder::builder_push_hex(&mut b, 18446744073709551615);
    let _ = std_builder::builder_push_str(&mut b, ",f=");
    let _ = std_builder::builder_push_f64(&mut b, -1.25);
    let text: str = std_builder::builder_finish(&mut b);
    print(std_builder::builder_len(&mut b));
    print(std_builder::builder_failed(&mut b));
    print(text[499]);
    let n: usize = std_builder::builder_len(&mut b) - 80;
    unsafe {
        print(str_from_raw(std_builder::builder_data(&mut b) + n, 80));
    }
    std_builder::builder_free(&mut b);

    // Capacity overflow latches `failed` instead of aborting.
    let mut latched: std_builder::Builder = std_builder::builder_new();
    let _ = std_builder::builder_push_str(&mut latched, "seed");
    print(std_builder::builder_reserve(&mut latched, 18446744073709551615));
    print(std_builder::builder_failed(&mut latched));
    print(std_builder::builder_push_byte(&mut latched, 1));
    print(std_builder::builder_len(&mut latched));
    std_builder::builder_free(&mut latched);
    return 0;
}
"#;

const COLLECTIONS_OUTPUT: &str = "\
100
true
105
true
0
false
true
false
8
777
8
k0
k22
k2
k19
k20
k33
alpha
beta
0
14
0
13
g0
g13
2
true
false
x
y
500
581
false
65
-9223372036854775808,max=18446744073709551615,hex=0xffffffffffffffff,f=-1.250000
false
true
false
4
";

#[test]
fn map_set_and_builder_behave_deterministically() {
    let (stdout, stderr, code) = build_and_run(COLLECTIONS);
    assert_eq!(stdout, COLLECTIONS_OUTPUT);
    assert_eq!(code, 0, "stderr: {stderr}");
}

#[test]
fn map_rejects_no_key_and_allocates_lazily() {
    // An empty map owns nothing and reports misses; `map_free` is idempotent
    // on a never-populated map.
    let (stdout, stderr, code) = build_and_run(
        r#"
        mod std_map;
        fn main() -> i32 {
            let mut m: std_map::Map = std_map::map_new();
            let mut found: bool = true;
            print(std_map::map_len(&mut m));
            print(std_map::map_has(&mut m, "absent"));
            print(std_map::map_get(&mut m, "absent", &mut found));
            print(found);
            print(std_map::map_key_at(&mut m, 0));
            print(std_map::map_value_at(&mut m, 0));
            std_map::map_free(&mut m);
            std_map::map_free(&mut m);
            return 0;
        }
        "#,
    );
    assert_eq!(stdout, "0\nfalse\n0\nfalse\n\n0\n");
    assert_eq!(code, 0, "stderr: {stderr}");
}

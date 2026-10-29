//! Optimization-level semantic regression tests.
//!
//! CoreX promises defined behavior where C leaves it undefined: wrapping
//! arithmetic, masked shift counts, aborting division/bounds violations, and
//! unrestricted aliasing. Those promises are only real if they survive the LLVM
//! optimizer. Every case below is therefore compiled and executed at `--opt 0`,
//! `1`, `2`, and `3`, and the observable result (stdout, stderr, exit status)
//! must be identical at every level.
//!
//! A second test asserts the *emitter* never attaches `nsw`/`nuw` to integer
//! arithmetic: if it did, LLVM would be entitled to assume no overflow and the
//! wrapping guarantee would disappear under optimization.

use std::path::{Path, PathBuf};
use std::process::Command;

use corex::diagnostics::SourceFile;
mod common;
use common::driver;

const OPT_LEVELS: [u8; 4] = [0, 1, 2, 3];

struct RunResult {
    stdout: String,
    stderr: String,
    code: i32,
}

struct Case {
    name: &'static str,
    source: &'static str,
    stdout: &'static str,
    code: i32,
    stderr_contains: Option<&'static str>,
}

fn opt_levels() -> impl Iterator<Item = u8> {
    OPT_LEVELS.into_iter()
}

fn run_source(source: &str, opt: u8) -> RunResult {
    let work = driver::temp_dir("corex-opt-test");
    std::fs::create_dir_all(&work).expect("create test dir");
    let file = SourceFile::new("case.cx", source);
    let exe = work.join("program");

    let artifacts = match driver::build(&file, &exe, opt, false) {
        Ok(a) => a,
        Err(diags) => {
            panic!(
                "compilation failed at --opt {opt}:\n{}",
                file.render_all(&diags)
            );
        }
    };

    let out = Command::new(&artifacts.executable)
        .output()
        .expect("execute compiled program");
    let result = RunResult {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        stderr: String::from_utf8_lossy(&out.stderr).to_string(),
        code: out.status.code().unwrap_or(-1),
    };
    let _ = std::fs::remove_dir_all(&work);
    result
}

fn cases() -> Vec<Case> {
    vec![
        // The reported case: INT_MAX + 1 wraps to INT_MIN, so the comparison is
        // true and 42 is printed at every optimization level.
        Case {
            name: "signed_add_wrap",
            source: r#"
                fn main() -> i32 {
                    let mut x: i32 = 2147483647;
                    x = x + 1;

                    if x == -2147483647 - 1 {
                        print(42);
                    }

                    return 0;
                }
            "#,
            stdout: "42\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "unsigned_add_wrap",
            source: r#"
                fn main() -> i32 {
                    let mut x: u8 = 255;
                    x = x + 1;
                    print(x);
                    return 0;
                }
            "#,
            stdout: "0\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "mul_wrap",
            source: r#"
                fn main() -> i32 {
                    let mut x: u32 = 65536;
                    x = x * 65536;
                    print(x);
                    return 0;
                }
            "#,
            stdout: "0\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "i64_add_wrap",
            source: r#"
                fn main() -> i32 {
                    let mut x: i64 = 9223372036854775807;
                    x = x + 1;
                    print(x);
                    return 0;
                }
            "#,
            stdout: "-9223372036854775808\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "negate_min_wraps",
            source: r#"
                fn main() -> i32 {
                    let x: i8 = -128;
                    let y: i8 = 0 - x;
                    print(y);
                    return 0;
                }
            "#,
            stdout: "-128\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "masked_shift_left",
            source: r#"
                fn main() -> i32 {
                    let one: u8 = 1;
                    print(one << 8);
                    print(one << 9);
                    return 0;
                }
            "#,
            stdout: "1\n2\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "masked_shift_right_signed",
            source: r#"
                fn main() -> i32 {
                    let value: i32 = -8;
                    print(value >> 32);
                    print(value >> 33);
                    return 0;
                }
            "#,
            stdout: "-8\n-4\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "division_by_zero_aborts",
            source: r#"
                fn main() -> i32 {
                    let divisor: i32 = 0;
                    let value: i32 = 10;
                    print(value / divisor);
                    return 0;
                }
            "#,
            stdout: "",
            code: 101,
            stderr_contains: Some("division by zero"),
        },
        Case {
            name: "remainder_by_zero_aborts",
            source: r#"
                fn main() -> i32 {
                    let divisor: i64 = 0;
                    let value: i64 = 10;
                    print(value % divisor);
                    return 0;
                }
            "#,
            stdout: "",
            code: 101,
            stderr_contains: Some("division by zero"),
        },
        Case {
            name: "signed_division_overflow_aborts",
            source: r#"
                fn main() -> i32 {
                    let lowest: i64 = -9223372036854775808;
                    let minus_one: i64 = -1;
                    print(lowest / minus_one);
                    return 0;
                }
            "#,
            stdout: "",
            code: 101,
            stderr_contains: Some("signed overflow"),
        },
        Case {
            name: "dynamic_index_out_of_bounds_aborts",
            source: r#"
                fn main() -> i32 {
                    let items: [u8; 4] = [1, 2, 3, 4];
                    let mut index: usize = 0;
                    index = index + 5;
                    print(items[index]);
                    return 0;
                }
            "#,
            stdout: "",
            code: 101,
            stderr_contains: Some("index out of bounds"),
        },
        Case {
            name: "dynamic_index_in_bounds",
            source: r#"
                fn main() -> i32 {
                    let items: [u8; 4] = [1, 2, 3, 4];
                    let mut index: usize = 0;
                    index = index + 2;
                    print(items[index]);
                    return 0;
                }
            "#,
            stdout: "3\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "aliasing_through_pointers",
            source: r#"
                fn main() -> i32 {
                    let mut a: [u8; 2] = [1, 2];
                    unsafe {
                        let first: *mut u8 = &mut a[0];
                        *first = 9;
                        let second: *mut u8 = first + 1;
                        *second = 8;
                    }
                    print(a[0]);
                    print(a[1]);
                    return 0;
                }
            "#,
            stdout: "9\n8\n",
            code: 0,
            stderr_contains: None,
        },
        Case {
            name: "print_formats_across_levels",
            source: r#"
                fn main() -> i32 {
                    print(-7);
                    print(true);
                    let byte: u8 = 255;
                    print(byte);
                    let lowest: i64 = -9223372036854775808;
                    print(lowest);
                    return 0;
                }
            "#,
            stdout: "-7\ntrue\n255\n-9223372036854775808\n",
            code: 0,
            stderr_contains: None,
        },
    ]
}

#[test]
fn semantics_are_identical_at_every_optimization_level() {
    for case in cases() {
        for opt in opt_levels() {
            let r = run_source(case.source, opt);
            assert_eq!(
                r.stdout, case.stdout,
                "case `{}` at --opt {opt}: stdout (stderr: {})",
                case.name, r.stderr
            );
            assert_eq!(
                r.code, case.code,
                "case `{}` at --opt {opt}: exit status (stderr: {})",
                case.name, r.stderr
            );
            if let Some(expected) = case.stderr_contains {
                assert!(
                    r.stderr.contains(expected),
                    "case `{}` at --opt {opt}: stderr {:?} lacks {expected:?}",
                    case.name,
                    r.stderr
                );
            }
        }
    }
}

fn examples_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples")
}

fn examples() -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = std::fs::read_dir(examples_dir())
        .expect("examples directory")
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().map(|x| x == "cx").unwrap_or(false))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().to_string();
            let source = std::fs::read_to_string(&p).expect("read example");
            (name, source)
        })
        .collect();
    out.sort();
    out
}

#[test]
fn examples_produce_the_same_output_at_every_optimization_level() {
    let expectations: [(&str, &str, i32); 4] = [
        ("factorial.cx", "3628800\n", 0),
        ("hello.cx", "Hello, World!\n", 0),
        ("loop_sum.cx", "", 51),
        ("pointers.cx", "123456789\n", 0),
    ];

    for (name, expected_stdout, expected_code) in expectations {
        let Some((_, source)) = examples().into_iter().find(|(n, _)| n == name) else {
            panic!("missing example {name}");
        };
        for opt in opt_levels() {
            let r = run_source(&source, opt);
            assert_eq!(r.stdout, expected_stdout, "{name} at --opt {opt}");
            assert_eq!(r.code, expected_code, "{name} at --opt {opt}");
        }
    }
}

/// The emitter must never mark integer arithmetic as non-wrapping: `nsw`/`nuw`
/// would let LLVM assume overflow is impossible and delete the behavior CoreX
/// specifies. (The optimizer may add such flags itself when it proves safety;
/// that is sound and is covered by the execution tests above.)
#[test]
fn emitted_ir_never_claims_non_wrapping_arithmetic() {
    let mut sources: Vec<(String, String)> = cases()
        .into_iter()
        .map(|c| (c.name.to_string(), c.source.to_string()))
        .collect();
    sources.extend(examples());

    for (name, source) in sources {
        let file = SourceFile::new(name.clone(), source);
        let ir = match driver::emit_ir(&file) {
            Ok(ir) => ir,
            Err(diags) => panic!("{} failed:\n{}", name, file.render_all(&diags)),
        };
        for flag in [" nsw", " nuw"] {
            assert!(
                !ir.contains(flag),
                "`{name}`: emitted IR contains `{flag}`"
            );
        }
    }
}

/// The reported program specifically: plain `add i32`, and 42 at every level.
#[test]
fn wrapping_add_regression_from_report() {
    let source = cases()
        .into_iter()
        .find(|c| c.name == "signed_add_wrap")
        .expect("case present")
        .source;

    let file = SourceFile::new("wrap.cx", source);
    let ir = driver::emit_ir(&file).expect("emit");
    assert!(ir.contains("add i32"), "expected a plain add:\n{ir}");
    assert!(!ir.contains("add nsw i32"), "unexpected nsw add:\n{ir}");

    for opt in opt_levels() {
        let r = run_source(source, opt);
        assert_eq!(r.stdout, "42\n", "at --opt {opt}");
        assert_eq!(r.code, 0, "at --opt {opt}");
    }
}

// ---------------------------------------------------------------------------
// Generated differential coverage.
//
// Rather than 1,000 hand-written programs (1,000 compilations), each test
// generated below packs dozens of operations into one program and is executed
// at four optimization levels. The expected values are computed in Rust using
// exactly the rules the specification states: operations wrap modulo 2^N,
// shift counts are masked with N-1, signed division truncates toward zero, and
// signed remainder follows the dividend.
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug)]
enum Ty {
    I32,
    U32,
    I8,
    U8,
}

impl Ty {
    fn bits(self) -> u32 {
        match self {
            Ty::I32 | Ty::U32 => 32,
            Ty::I8 | Ty::U8 => 8,
        }
    }

    fn signed(self) -> bool {
        matches!(self, Ty::I32 | Ty::I8)
    }

    fn name(self) -> &'static str {
        match self {
            Ty::I32 => "i32",
            Ty::U32 => "u32",
            Ty::I8 => "i8",
            Ty::U8 => "u8",
        }
    }

    fn mask(self) -> u64 {
        let bits = self.bits();
        if bits >= 64 {
            u64::MAX
        } else {
            (1u64 << bits) - 1
        }
    }

    fn wrap(self, v: u64) -> u64 {
        v & self.mask()
    }

    /// Two's-complement interpretation used by `print`.
    fn as_i64(self, v: u64) -> i64 {
        let v = self.wrap(v);
        if self.signed() && (v & (1u64 << (self.bits() - 1))) != 0 {
            (v | !self.mask()) as i64
        } else {
            v as i64
        }
    }

    fn format(self, v: u64) -> String {
        if self.signed() {
            self.as_i64(v).to_string()
        } else {
            self.wrap(v).to_string()
        }
    }

    fn min(self) -> i64 {
        if self.signed() {
            -(1i64 << (self.bits() - 1))
        } else {
            0
        }
    }

    fn max(self) -> i64 {
        if self.signed() {
            (1i64 << (self.bits() - 1)) - 1
        } else {
            self.mask() as i64
        }
    }
}

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn range(&mut self, lo: i64, hi: i64) -> i64 {
        debug_assert!(lo <= hi);
        let span = (hi - lo + 1) as u64;
        lo + (self.next() % span) as i64
    }
}

fn signed_div(ty: Ty, a: u64, b: u64) -> Option<u64> {
    let (x, y) = (ty.as_i64(a), ty.as_i64(b));
    if y == 0 {
        return None;
    }
    if x == ty.min() && y == -1 {
        return None; // defined to abort; covered by the fixed cases
    }
    Some(ty.wrap((x / y) as u64))
}

fn signed_rem(ty: Ty, a: u64, b: u64) -> Option<u64> {
    let (x, y) = (ty.as_i64(a), ty.as_i64(b));
    if y == 0 {
        return None;
    }
    if x == ty.min() && y == -1 {
        return None;
    }
    Some(ty.wrap((x % y) as u64))
}

fn unsigned_div(ty: Ty, a: u64, b: u64) -> Option<u64> {
    if b == 0 {
        return None;
    }
    Some(ty.wrap(ty.wrap(a) / ty.wrap(b)))
}

fn unsigned_rem(ty: Ty, a: u64, b: u64) -> Option<u64> {
    if b == 0 {
        return None;
    }
    Some(ty.wrap(ty.wrap(a) % ty.wrap(b)))
}

fn shl(ty: Ty, a: u64, amount: i64) -> u64 {
    let amount = (ty.wrap(amount as u64) & (ty.bits() as u64 - 1)) as u32;
    ty.wrap(ty.wrap(a) << amount)
}

fn shr(ty: Ty, a: u64, amount: i64) -> u64 {
    let amount = (ty.wrap(amount as u64) & (ty.bits() as u64 - 1)) as u32;
    if ty.signed() {
        ty.wrap((ty.as_i64(a) >> amount) as u64)
    } else {
        ty.wrap(ty.wrap(a) >> amount)
    }
}

/// Generate one program per type: a chain of statements over a mutable binding,
/// printing the value after every step, plus comparison prints.
fn generated_program(ty: Ty, seed: u64, steps: usize) -> (String, Vec<String>) {
    let mut rng = Rng(seed);
    let mut body = String::new();
    let mut expected = Vec::new();

    let mut value = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
    body.push_str(&format!(
        "    let mut v: {} = {};\n",
        ty.name(),
        ty.format(value)
    ));

    for step in 0..steps {
        let op = rng.range(0, 10);
        match op {
            0 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                body.push_str(&format!("    v = v + {};\n", ty.format(lit)));
                value = ty.wrap(value.wrapping_add(lit));
            }
            1 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                body.push_str(&format!("    v = v - {};\n", ty.format(lit)));
                value = ty.wrap(value.wrapping_sub(lit));
            }
            2 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                body.push_str(&format!("    v = v * {};\n", ty.format(lit)));
                value = ty.wrap(value.wrapping_mul(lit));
            }
            3 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                let result = if ty.signed() {
                    signed_div(ty, value, lit)
                } else {
                    unsigned_div(ty, value, lit)
                };
                if let Some(next) = result {
                    body.push_str(&format!("    v = v / {};\n", ty.format(lit)));
                    value = next;
                }
            }
            4 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                let result = if ty.signed() {
                    signed_rem(ty, value, lit)
                } else {
                    unsigned_rem(ty, value, lit)
                };
                if let Some(next) = result {
                    body.push_str(&format!("    v = v % {};\n", ty.format(lit)));
                    value = next;
                }
            }
            5 => {
                let amount = rng.range(-(ty.bits() as i64) * 2, ty.bits() as i64 * 2);
                body.push_str(&format!("    v = v << {amount};\n"));
                value = shl(ty, value, amount);
            }
            6 => {
                let amount = rng.range(-(ty.bits() as i64) * 2, ty.bits() as i64 * 2);
                body.push_str(&format!("    v = v >> {amount};\n"));
                value = shr(ty, value, amount);
            }
            7 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                body.push_str(&format!("    v = v & {};\n", ty.format(lit)));
                value = ty.wrap(value & lit);
            }
            8 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                body.push_str(&format!("    v = v | {};\n", ty.format(lit)));
                value = ty.wrap(value | lit);
            }
            9 => {
                let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
                body.push_str(&format!("    v = v ^ {};\n", ty.format(lit)));
                value = ty.wrap(value ^ lit);
            }
            _ => {
                body.push_str("    v = -v;\n");
                value = ty.wrap(0u64.wrapping_sub(value));
            }
        }

        body.push_str("    print(v);\n");
        expected.push(ty.format(value));

        if step % 7 == 3 {
            let lit = ty.wrap(rng.range(ty.min(), ty.max()) as u64);
            body.push_str(&format!("    print(v == {});\n", ty.format(lit)));
            expected.push(if value == lit { "true" } else { "false" }.to_string());
        }
    }

    let source = format!("fn main() -> i32 {{\n{body}    return 0;\n}}\n");
    (source, expected)
}

#[test]
fn generated_arithmetic_matches_the_spec_at_every_optimization_level() {
    for (index, ty) in [Ty::I32, Ty::U32, Ty::I8, Ty::U8].into_iter().enumerate() {
        let (source, expected) = generated_program(ty, 0xC0DE_0000 + index as u64, 60);
        let expected_stdout = format!("{}\n", expected.join("\n"));

        for opt in opt_levels() {
            let r = run_source(&source, opt);
            if r.stdout != expected_stdout {
                let mut diff = String::new();
                for (i, (got, want)) in r
                    .stdout
                    .lines()
                    .zip(expected.iter())
                    .enumerate()
                    .filter(|(_, (got, want))| got != &want.as_str())
                    .take(5)
                {
                    diff.push_str(&format!("  line {i}: got {got}, expected {want}\n"));
                }
                panic!(
                    "{} at --opt {opt}: stdout mismatch (exit {}, stderr {:?})\n{diff}program:\n{source}",
                    ty.name(),
                    r.code,
                    r.stderr
                );
            }
            assert_eq!(r.code, 0, "{} at --opt {opt}", ty.name());
        }
    }
}

//! Robustness: malformed and adversarial input must produce diagnostics, never
//! a panic or an abort. The NFR requires "no panic on malformed input"; this is
//! the deterministic evidence for that claim.

use corex::diagnostics::SourceFile;
use corex::driver;

struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        self.0
    }

    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// Characters biased toward the lexer's decision points.
const ALPHABET: &[u8] = br#"fn let mut if else while loop break continue return unsafe as true false i8 i16 i32 i64 u8 u16 u32 u64 usize isize bool print syscall alloc free
0123456789_abcdefxyz(){}[],;:+-*/%&|^!=<>"\
"#;

#[test]
fn malformed_input_never_panics() {
    let mut rng = Rng(0xC0DE_C0DE);
    for case in 0..600 {
        let len = rng.below(240) as usize;
        let mut source = String::with_capacity(len);
        for _ in 0..len {
            let index = rng.below(ALPHABET.len() as u64) as usize;
            source.push(ALPHABET[index] as char);
        }

        let file = SourceFile::new(format!("fuzz-{case}.cx"), source.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            driver::check(&file)
        }));
        assert!(
            result.is_ok(),
            "compiler panicked on generated input {case}:\n{source:?}"
        );
    }
}

#[test]
fn unbalanced_and_deep_inputs_are_diagnosed() {
    for source in [
        "{".repeat(50_000),
        "(".repeat(50_000),
        "[".repeat(50_000),
        "/*".to_string(),
        "\"".to_string(),
        format!("fn f() {{ {} }}", "(".repeat(10_000)),
        "0x".to_string(),
        "fn".to_string(),
        "\u{7f}\u{0}".to_string(),
    ] {
        let file = SourceFile::new("adversarial.cx", source.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            driver::check(&file)
        }));
        assert!(result.is_ok(), "panic on input of length {}", source.len());
        assert!(
            driver::check(&file).is_err(),
            "expected a diagnostic for adversarial input of length {}",
            source.len()
        );
    }
}

/// Flat left-deep chains parse iteratively (they are not nested syntax), so the
/// parser's depth limit never sees them. Semantic analysis must bound its own
/// recursion: without the guard these inputs abort the process with a stack
/// overflow instead of producing a diagnostic.
#[test]
fn flat_deep_expression_chains_are_diagnosed() {
    let plus = vec!["1"; 4500].join("+");
    let minus = vec!["1"; 4500].join("-");
    let times = vec!["2"; 4500].join("*");
    let casts = format!("1{}", " as i32".repeat(4500));
    for (label, expr) in [
        ("plus", plus),
        ("minus", minus),
        ("times", times),
        ("casts", casts),
    ] {
        let source = format!("fn main() -> i32 {{ return {expr}; }}");
        let file = SourceFile::new(format!("chain-{label}.cx"), source.clone());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            driver::check(&file)
        }));
        assert!(result.is_ok(), "compiler panicked on the {label} chain");
        match driver::check(&file) {
            Ok(_) => panic!("{label} chain of 4500 terms was accepted"),
            Err(diags) => assert!(
                diags.iter().any(|d| d.code == "E0111"),
                "{label} chain produced {diags:?}"
            ),
        }
    }
}

/// `check_binary` used to type-check the left operand twice, so a left-deep
/// `+`/`-` chain cost 2^n. At 60 terms (inside the expression-depth limit) the
/// old code needed 2^60 walks; the fixed checker is linear.
#[test]
fn left_deep_operator_chains_type_check_quickly() {
    for op in ["+", "-"] {
        let expr = vec!["1"; 60].join(op);
        let source = format!("fn main() -> i32 {{ return {expr}; }}");
        let file = SourceFile::new("linear.cx", source);
        let start = std::time::Instant::now();
        driver::check(&file).unwrap_or_else(|diags| {
            panic!("check failed: {}", file.render_all(&diags))
        });
        let elapsed = start.elapsed();
        assert!(
            elapsed < std::time::Duration::from_secs(5),
            "60-term `{op}` chain took {elapsed:?}"
        );
    }
}

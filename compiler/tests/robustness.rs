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

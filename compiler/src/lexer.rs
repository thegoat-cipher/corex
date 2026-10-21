//! Hand-written, allocation-light lexer.
//!
//! The lexer is byte oriented. CoreX source outside string literals is ASCII;
//! any non-ASCII byte is rejected with `E0001`.

use crate::diagnostics::{Diagnostic, Span};
use crate::token::{Token, TokenKind};

pub fn lex(src: &str) -> Result<Vec<Token>, Vec<Diagnostic>> {
    let mut lx = Lexer {
        src: src.as_bytes(),
        pos: 0,
        tokens: Vec::new(),
        diags: Vec::new(),
    };
    lx.run();
    if lx.diags.is_empty() {
        Ok(lx.tokens)
    } else {
        Err(lx.diags)
    }
}

struct Lexer<'a> {
    src: &'a [u8],
    pos: usize,
    tokens: Vec<Token>,
    diags: Vec<Diagnostic>,
}

impl<'a> Lexer<'a> {
    fn peek(&self) -> Option<u8> {
        self.src.get(self.pos).copied()
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.src.get(self.pos + offset).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        Some(b)
    }

    fn push(&mut self, kind: TokenKind, start: usize) {
        self.tokens.push(Token::new(kind, Span::new(start, self.pos)));
    }

    fn run(&mut self) {
        loop {
            self.skip_trivia();
            let start = self.pos;
            let Some(b) = self.bump() else {
                self.tokens.push(Token::new(TokenKind::Eof, Span::new(start, start)));
                return;
            };

            match b {
                b'(' => self.push(TokenKind::LParen, start),
                b')' => self.push(TokenKind::RParen, start),
                b'{' => self.push(TokenKind::LBrace, start),
                b'}' => self.push(TokenKind::RBrace, start),
                b'[' => self.push(TokenKind::LBracket, start),
                b']' => self.push(TokenKind::RBracket, start),
                b',' => self.push(TokenKind::Comma, start),
                b'.' => {
                    if self.peek() == Some(b'.') {
                        self.bump();
                        self.push(TokenKind::DotDot, start);
                    } else {
                        self.push(TokenKind::Dot, start);
                    }
                }
                b';' => self.push(TokenKind::Semi, start),
                b':' => {
                    if self.peek() == Some(b':') {
                        self.bump();
                        self.push(TokenKind::ColonColon, start);
                    } else {
                        self.push(TokenKind::Colon, start);
                    }
                }
                b'+' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::PlusEq, start);
                    } else {
                        self.push(TokenKind::Plus, start);
                    }
                }
                b'*' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::StarEq, start);
                    } else {
                        self.push(TokenKind::Star, start);
                    }
                }
                b'%' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::PercentEq, start);
                    } else {
                        self.push(TokenKind::Percent, start);
                    }
                }
                b'^' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::CaretEq, start);
                    } else {
                        self.push(TokenKind::Caret, start);
                    }
                }
                b'~' => self.diags.push(Diagnostic::error(
                    "E0001",
                    Span::new(start, self.pos),
                    "unexpected character `~`",
                )),
                b'-' => {
                    if self.peek() == Some(b'>') {
                        self.bump();
                        self.push(TokenKind::Arrow, start);
                    } else if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::MinusEq, start);
                    } else {
                        self.push(TokenKind::Minus, start);
                    }
                }
                b'!' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::NotEq, start);
                    } else {
                        self.push(TokenKind::Not, start);
                    }
                }
                b'=' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::EqEq, start);
                    } else {
                        self.push(TokenKind::Assign, start);
                    }
                }
                b'<' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::Le, start);
                    } else if self.peek() == Some(b'<') {
                        self.bump();
                        if self.peek() == Some(b'=') {
                            self.bump();
                            self.push(TokenKind::ShlEq, start);
                        } else {
                            self.push(TokenKind::Shl, start);
                        }
                    } else {
                        self.push(TokenKind::Lt, start);
                    }
                }
                b'>' => {
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::Ge, start);
                    } else if self.peek() == Some(b'>') {
                        self.bump();
                        if self.peek() == Some(b'=') {
                            self.bump();
                            self.push(TokenKind::ShrEq, start);
                        } else {
                            self.push(TokenKind::Shr, start);
                        }
                    } else {
                        self.push(TokenKind::Gt, start);
                    }
                }
                b'&' => {
                    if self.peek() == Some(b'&') {
                        self.bump();
                        self.push(TokenKind::AmpAmp, start);
                    } else if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::AmpEq, start);
                    } else {
                        self.push(TokenKind::Amp, start);
                    }
                }
                b'|' => {
                    if self.peek() == Some(b'|') {
                        self.bump();
                        self.push(TokenKind::PipePipe, start);
                    } else if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::PipeEq, start);
                    } else {
                        self.push(TokenKind::Pipe, start);
                    }
                }
                b'/' => {
                    // Comments are handled by skip_trivia, so a `/` here is division.
                    if self.peek() == Some(b'=') {
                        self.bump();
                        self.push(TokenKind::SlashEq, start);
                    } else {
                        self.push(TokenKind::Slash, start);
                    }
                }
                b'\'' => {
                    if let Some(value) = self.char_literal(start) {
                        self.push(TokenKind::CharLit(value), start);
                    }
                }
                b'"' => {
                    if let Some(bytes) = self.string(start) {
                        self.push(TokenKind::Str(bytes), start);
                    }
                }
                b'0'..=b'9' => {
                    if let Some(kind) = self.number(start, b) {
                        self.push(kind, start);
                    }
                }
                b'_' | b'a'..=b'z' | b'A'..=b'Z' => {
                    while matches!(self.peek(), Some(c) if c == b'_' || c.is_ascii_alphanumeric()) {
                        self.bump();
                    }
                    let text = std::str::from_utf8(&self.src[start..self.pos])
                        .expect("ascii identifier")
                        .to_string();
                    let kind = keyword(&text)
                        .unwrap_or(TokenKind::Ident(text));
                    self.push(kind, start);
                }
                other => {
                    self.diags.push(Diagnostic::error(
                        "E0001",
                        Span::new(start, self.pos),
                        format!("unexpected character `{}`", other.escape_ascii()),
                    ));
                }
            }
        }
    }

    fn skip_trivia(&mut self) {
        loop {
            match self.peek() {
                Some(b' ' | b'\t' | b'\r' | b'\n') => {
                    self.bump();
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while let Some(c) = self.peek() {
                        if c == b'\n' {
                            break;
                        }
                        self.bump();
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    let start = self.pos;
                    self.bump();
                    self.bump();
                    let mut closed = false;
                    while let Some(c) = self.bump() {
                        if c == b'*' && self.peek() == Some(b'/') {
                            self.bump();
                            closed = true;
                            break;
                        }
                    }
                    if !closed {
                        self.diags.push(Diagnostic::error(
                            "E0003",
                            Span::new(start, self.pos),
                            "unterminated block comment",
                        ));
                    }
                }
                _ => return,
            }
        }
    }

    fn number(&mut self, start: usize, first: u8) -> Option<TokenKind> {
        let mut radix = 10;
        if first == b'0' {
            match self.peek() {
                Some(b'x' | b'X') => {
                    radix = 16;
                    self.bump();
                }
                Some(b'b' | b'B') => {
                    radix = 2;
                    self.bump();
                }
                Some(b'o' | b'O') => {
                    radix = 8;
                    self.bump();
                }
                _ => {}
            }
        }

        if radix != 10 && self.peek() == Some(b'_') {
            self.diags.push(Diagnostic::error(
                "E0004",
                Span::new(start, self.pos + 1),
                "underscores must separate digits, not follow a base prefix",
            ));
            return None;
        }

        let digits_start = self.pos;
        let mut value: u64 = if radix == 10 { (first - b'0') as u64 } else { 0 };
        let mut digits = if radix == 10 { 1usize } else { 0usize };

        while let Some(c) = self.peek() {
            if c == b'_' {
                self.bump();
                continue;
            }
            let Some(d) = (c as char).to_digit(radix) else {
                break;
            };
            let overflow = value
                .checked_mul(radix as u64)
                .and_then(|v| v.checked_add(d as u64));
            match overflow {
                Some(v) => value = v,
                None => {
                    while matches!(self.peek(), Some(c) if (c as char).is_digit(16) || c == b'_') {
                        self.bump();
                    }
                    self.diags.push(Diagnostic::error(
                        "E0004",
                        Span::new(start, self.pos),
                        "integer literal does not fit in `u64`",
                    ));
                    return None;
                }
            }
            digits += 1;
            self.bump();
        }

        if digits == 0 {
            self.diags.push(Diagnostic::error(
                "E0004",
                Span::new(start, self.pos.max(digits_start + 1)),
                "integer literal has no digits",
            ));
            return None;
        }

        let dot_float = self.peek() == Some(b'.')
            && matches!(self.peek_at(1), Some(c) if c.is_ascii_digit());
        if radix == 10 && (dot_float || matches!(self.peek(), Some(b'e' | b'E'))) {
            return self.float_number(start, first);
        }

        if matches!(self.peek(), Some(c) if c == b'_' || c.is_ascii_alphanumeric()) {
            let bad_start = self.pos;
            while matches!(self.peek(), Some(c) if c == b'_' || c.is_ascii_alphanumeric()) {
                self.bump();
            }
            self.diags.push(Diagnostic::error(
                "E0004",
                Span::new(bad_start, self.pos),
                "invalid suffix on integer literal (CoreX has no literal suffixes; use `as` to convert)",
            ));
            return None;
        }

        Some(TokenKind::Int(value))
    }

    fn float_number(&mut self, start: usize, _first: u8) -> Option<TokenKind> {
        if self.peek() == Some(b'.') {
            self.bump();
            let frac_start = self.pos;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'_') {
                self.bump();
            }
            if self.pos == frac_start {
                self.diags.push(Diagnostic::error(
                    "E0004",
                    Span::new(start, self.pos),
                    "expected digits after the decimal point",
                ));
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.bump();
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.bump();
            }
            let exp_start = self.pos;
            while matches!(self.peek(), Some(c) if c.is_ascii_digit() || c == b'_') {
                self.bump();
            }
            if self.pos == exp_start {
                self.diags.push(Diagnostic::error(
                    "E0004",
                    Span::new(start, self.pos),
                    "expected digits in the exponent",
                ));
                return None;
            }
        }
        if matches!(self.peek(), Some(c) if c == b'_' || c.is_ascii_alphanumeric()) {
            let bad_start = self.pos;
            while matches!(self.peek(), Some(c) if c == b'_' || c.is_ascii_alphanumeric()) {
                self.bump();
            }
            self.diags.push(Diagnostic::error(
                "E0004",
                Span::new(bad_start, self.pos),
                "invalid suffix on float literal (CoreX has no literal suffixes; use `as` to convert)",
            ));
            return None;
        }
        let text: String = self.src[start..self.pos]
            .iter()
            .map(|&b| b as char)
            .filter(|c| *c != '_')
            .collect();
        match text.parse::<f64>() {
            Ok(v) if v.is_finite() => Some(TokenKind::Float(v.to_bits())),
            _ => {
                self.diags.push(Diagnostic::error(
                    "E0004",
                    Span::new(start, self.pos),
                    "float literal is out of range",
                ));
                None
            }
        }
    }

    fn char_literal(&mut self, start: usize) -> Option<u8> {
        let Some(c) = self.bump() else {
            self.diags.push(Diagnostic::error(
                "E0006",
                Span::new(start, self.pos),
                "unterminated character literal",
            ));
            return None;
        };
        let value = match c {
            b'\\' => {
                let esc_start = self.pos - 1;
                let Some(e) = self.bump() else {
                    self.diags.push(Diagnostic::error(
                        "E0006",
                        Span::new(start, self.pos),
                        "unterminated character literal",
                    ));
                    return None;
                };
                match e {
                    b'n' => b'\n',
                    b't' => b'\t',
                    b'r' => b'\r',
                    b'0' => 0,
                    b'\\' => b'\\',
                    b'"' => b'"',
                    b'\'' => b'\'',
                    b'x' => {
                        let hi = self.peek().and_then(|c| (c as char).to_digit(16));
                        let lo = self.peek_at(1).and_then(|c| (c as char).to_digit(16));
                        match (hi, lo) {
                            (Some(h), Some(l)) => {
                                self.bump();
                                self.bump();
                                (h * 16 + l) as u8
                            }
                            _ => {
                                self.diags.push(Diagnostic::error(
                                    "E0006",
                                    Span::new(esc_start, self.pos.min(self.src.len())),
                                    "`\\x` escape requires two hexadecimal digits",
                                ));
                                return None;
                            }
                        }
                    }
                    other => {
                        self.diags.push(Diagnostic::error(
                            "E0006",
                            Span::new(esc_start, self.pos),
                            format!("unknown escape sequence `\\{}`", other.escape_ascii()),
                        ));
                        return None;
                    }
                }
            }
            b'\'' => {
                self.diags.push(Diagnostic::error(
                    "E0006",
                    Span::new(start, self.pos),
                    "empty character literal",
                ));
                return None;
            }
            other => other,
        };
        if self.peek() == Some(b'\'') {
            self.bump();
            Some(value)
        } else {
            self.diags.push(Diagnostic::error(
                "E0006",
                Span::new(start, self.pos),
                "character literal must contain exactly one byte",
            ));
            None
        }
    }

    fn string(&mut self, start: usize) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        loop {
            let Some(c) = self.bump() else {
                self.diags.push(Diagnostic::error(
                    "E0002",
                    Span::new(start, self.pos),
                    "unterminated string literal",
                ));
                return None;
            };
            match c {
                b'"' => return Some(out),
                b'\\' => {
                    let esc_start = self.pos - 1;
                    let Some(e) = self.bump() else {
                        self.diags.push(Diagnostic::error(
                            "E0002",
                            Span::new(start, self.pos),
                            "unterminated string literal",
                        ));
                        return None;
                    };
                    match e {
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'0' => out.push(0),
                        b'\\' => out.push(b'\\'),
                        b'"' => out.push(b'"'),
                        b'\'' => out.push(b'\''),
                        b'x' => {
                            let hi = self.peek().and_then(|c| (c as char).to_digit(16));
                            let lo = self
                                .peek_at(1)
                                .and_then(|c| (c as char).to_digit(16));
                            match (hi, lo) {
                                (Some(h), Some(l)) => {
                                    self.bump();
                                    self.bump();
                                    out.push((h * 16 + l) as u8);
                                }
                                _ => {
                                    self.diags.push(Diagnostic::error(
                                        "E0005",
                                        Span::new(esc_start, self.pos.min(self.src.len())),
                                        "`\\x` escape requires two hexadecimal digits",
                                    ));
                                }
                            }
                        }
                        other => {
                            self.diags.push(Diagnostic::error(
                                "E0005",
                                Span::new(esc_start, self.pos),
                                format!("unknown escape sequence `\\{}`", other.escape_ascii()),
                            ));
                            out.push(other);
                        }
                    }
                }
                other => out.push(other),
            }
        }
    }
}

fn keyword(text: &str) -> Option<TokenKind> {
    Some(match text {
        "fn" => TokenKind::Fn,
        "import" => TokenKind::Import,
        "static" => TokenKind::Static,
        "const" => TokenKind::Const,
        "enum" => TokenKind::Enum,
        "struct" => TokenKind::Struct,
        "let" => TokenKind::Let,
        "mut" => TokenKind::Mut,
        "if" => TokenKind::If,
        "else" => TokenKind::Else,
        "while" => TokenKind::While,
        "loop" => TokenKind::Loop,
        "break" => TokenKind::Break,
        "continue" => TokenKind::Continue,
        "return" => TokenKind::Return,
        "unsafe" => TokenKind::Unsafe,
        "as" => TokenKind::As,
        "true" => TokenKind::True,
        "false" => TokenKind::False,
        "i8" => TokenKind::I8,
        "i16" => TokenKind::I16,
        "i32" => TokenKind::I32,
        "i64" => TokenKind::I64,
        "u8" => TokenKind::U8,
        "u16" => TokenKind::U16,
        "u32" => TokenKind::U32,
        "u64" => TokenKind::U64,
        "usize" => TokenKind::Usize,
        "isize" => TokenKind::Isize,
        "bool" => TokenKind::Bool,
        "char" => TokenKind::Char,
        "f32" => TokenKind::F32,
        "f64" => TokenKind::F64,
        "str" => TokenKind::StrType,
        "vec" => TokenKind::VecType,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<TokenKind> {
        lex(src).expect("lex ok").into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn lexes_function_signature() {
        assert_eq!(
            kinds("fn add(a: i32, b: i32) -> i32 { return a + b; }"),
            vec![
                TokenKind::Fn,
                TokenKind::Ident("add".into()),
                TokenKind::LParen,
                TokenKind::Ident("a".into()),
                TokenKind::Colon,
                TokenKind::I32,
                TokenKind::Comma,
                TokenKind::Ident("b".into()),
                TokenKind::Colon,
                TokenKind::I32,
                TokenKind::RParen,
                TokenKind::Arrow,
                TokenKind::I32,
                TokenKind::LBrace,
                TokenKind::Return,
                TokenKind::Ident("a".into()),
                TokenKind::Plus,
                TokenKind::Ident("b".into()),
                TokenKind::Semi,
                TokenKind::RBrace,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_numbers_in_all_bases_and_underscores() {
        assert_eq!(
            kinds("0 42 1_000 0xFF 0b1010 0o777"),
            vec![
                TokenKind::Int(0),
                TokenKind::Int(42),
                TokenKind::Int(1000),
                TokenKind::Int(255),
                TokenKind::Int(10),
                TokenKind::Int(511),
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_multichar_operators_greedily() {
        assert_eq!(
            kinds("<<=>>===!=&&||->"),
            vec![
                TokenKind::ShlEq,
                TokenKind::ShrEq,
                TokenKind::EqEq,
                TokenKind::NotEq,
                TokenKind::AmpAmp,
                TokenKind::PipePipe,
                TokenKind::Arrow,
                TokenKind::Eof,
            ]
        );
    }

    #[test]
    fn lexes_strings_with_escapes() {
        let toks = lex(r#""hi\n\x41\0""#).unwrap();
        assert_eq!(toks[0].kind, TokenKind::Str(b"hi\nA\0".to_vec()));
        assert_eq!(toks[0].span, Span::new(0, 12));
    }

    #[test]
    fn skips_line_and_block_comments() {
        assert_eq!(
            kinds("// comment\n/* block\ncomment */ fn"),
            vec![TokenKind::Fn, TokenKind::Eof]
        );
    }

    #[test]
    fn records_source_locations() {
        let toks = lex("fn main").unwrap();
        assert_eq!(toks[0].span, Span::new(0, 2));
        assert_eq!(toks[1].span, Span::new(3, 7));
        assert_eq!(toks[2].span, Span::new(7, 7));
    }

    #[test]
    fn rejects_unterminated_string_and_comment() {
        let e = lex("\"abc").unwrap_err();
        assert_eq!(e[0].code, "E0002");
        let e = lex("/* abc").unwrap_err();
        assert_eq!(e[0].code, "E0003");
    }

    #[test]
    fn rejects_underscore_after_base_prefix() {
        let e = lex("0x_FF").unwrap_err();
        assert_eq!(e[0].code, "E0004");
        assert_eq!(kinds("0xFF_00"), vec![TokenKind::Int(0xFF00), TokenKind::Eof]);
    }

    #[test]
    fn rejects_unknown_char_and_overflow() {
        let e = lex("@").unwrap_err();
        assert_eq!(e[0].code, "E0001");
        let e = lex("99999999999999999999999").unwrap_err();
        assert_eq!(e[0].code, "E0004");
        let e = lex("12abc").unwrap_err();
        assert_eq!(e[0].code, "E0004");
    }
}

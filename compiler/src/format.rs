//! Deterministic whitespace formatter.
//!
//! The formatter preserves line structure, comments, and string literals. It
//! normalizes indentation from brace nesting, collapses runs of spaces outside
//! literals and comments, and enforces the spacing conventions used by the
//! examples: four-space indentation, one space after commas, no space before
//! `,;)]`, and no space around `.` and `::`.
//!
//! Because it never reorders or reflows tokens, formatting is semantics
//! preserving by construction and idempotent.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Code,
    Str,
    Char,
    LineComment,
    BlockComment,
}

/// Format one CoreX source file.
pub fn format_source(src: &str) -> String {
    let mut out = String::new();
    let mut depth: i32 = 0;
    let mut state = State::Code;
    let mut last_blank = true;

    for raw_line in src.lines() {
        let line = raw_line.trim_end();
        let content = line.trim_start();
        if content.is_empty() {
            if !last_blank && !out.is_empty() {
                out.push('\n');
                last_blank = true;
            }
            continue;
        }

        // A line that starts with closing braces is indented one level less per
        // brace, matching the block it closes.
        let mut lead = 0i32;
        let in_code = state == State::Code;
        for ch in content.chars() {
            if !in_code {
                break;
            }
            match ch {
                '}' => lead += 1,
                '/' if false => {}
                _ => break,
            }
        }
        let indent = (depth - lead).max(0) as usize;
        out.push_str(&" ".repeat(indent * 4));
        out.push_str(&normalize_spacing(content));

        depth += brace_delta(content, &mut state);
        if depth < 0 {
            depth = 0;
        }
        out.push('\n');
        last_blank = false;
    }

    // Collapse a trailing run of blank lines to exactly one newline.
    while out.ends_with("\n\n") {
        out.pop();
    }
    if out.is_empty() && !src.is_empty() {
        out.push('\n');
    }
    out
}

/// Net brace nesting change on a line, ignoring literals and comments.
fn brace_delta(line: &str, state: &mut State) -> i32 {
    let bytes = line.as_bytes();
    let mut i = 0;
    let mut delta = 0i32;
    while i < bytes.len() {
        let b = bytes[i];
        match *state {
            State::Code => match b {
                b'"' => *state = State::Str,
                b'\'' => *state = State::Char,
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                    *state = State::LineComment;
                    break;
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    *state = State::BlockComment;
                    i += 1;
                }
                b'{' => delta += 1,
                b'}' => delta -= 1,
                _ => {}
            },
            State::Str => {
                if b == b'\\' {
                    i += 1;
                } else if b == b'"' {
                    *state = State::Code;
                }
            }
            State::Char => {
                if b == b'\\' {
                    i += 1;
                } else if b == b'\'' {
                    *state = State::Code;
                }
            }
            State::LineComment => {}
            State::BlockComment => {
                if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    *state = State::Code;
                    i += 1;
                }
            }
        }
        i += 1;
    }
    if *state == State::LineComment {
        *state = State::Code;
    }
    delta
}

/// Collapse whitespace outside literals and remove space in the positions the
/// style guide forbids.
fn normalize_spacing(line: &str) -> String {
    let bytes = line.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(line.len());
    let mut i = 0;
    let mut state = State::Code;
    while i < bytes.len() {
        let b = bytes[i];
        match state {
            State::Code => match b {
                b'"' | b'\'' => {
                    state = if b == b'"' { State::Str } else { State::Char };
                    out.push(b);
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'/' => {
                    // Keep line comments verbatim (including their leading //).
                    out.push(b'/');
                    out.push(b'/');
                    out.extend_from_slice(&bytes[i + 2..]);
                    break;
                }
                b'/' if i + 1 < bytes.len() && bytes[i + 1] == b'*' => {
                    out.push(b'/');
                    out.push(b'*');
                    i += 1;
                    state = State::BlockComment;
                }
                b' ' | b'\t' => {
                    if out.last() != Some(&b' ') && !out.is_empty() {
                        out.push(b' ');
                    }
                }
                b',' => {
                    while out.last() == Some(&b' ') {
                        out.pop();
                    }
                    out.push(b',');
                    if i + 1 < bytes.len() && bytes[i + 1] != b' ' && bytes[i + 1] != b')' {
                        out.push(b' ');
                    }
                }
                b';' | b')' | b']' | b'}' => {
                    while out.last() == Some(&b' ') {
                        out.pop();
                    }
                    out.push(b);
                }
                b'.' => {
                    while out.last() == Some(&b' ') {
                        out.pop();
                    }
                    out.push(b'.');
                    // Skip whitespace after `.` (but not `..`).
                    if i + 1 >= bytes.len() || bytes[i + 1] != b'.' {
                        while i + 1 < bytes.len() && (bytes[i + 1] == b' ' || bytes[i + 1] == b'\t') {
                            i += 1;
                        }
                    }
                }
                b':' if i + 1 < bytes.len() && bytes[i + 1] == b':' => {
                    while out.last() == Some(&b' ') {
                        out.pop();
                    }
                    out.push(b':');
                    out.push(b':');
                    i += 1;
                    while i + 1 < bytes.len() && (bytes[i + 1] == b' ' || bytes[i + 1] == b'\t') {
                        i += 1;
                    }
                }
                b'(' | b'[' => {
                    // No space after opening delimiters.
                    out.push(b);
                    while i + 1 < bytes.len() && (bytes[i + 1] == b' ' || bytes[i + 1] == b'\t') {
                        i += 1;
                    }
                }
                other => out.push(other),
            },
            State::Str | State::Char => {
                out.push(b);
                if b == b'\\' && i + 1 < bytes.len() {
                    out.push(bytes[i + 1]);
                    i += 1;
                } else if (state == State::Str && b == b'"')
                    || (state == State::Char && b == b'\'')
                {
                    state = State::Code;
                }
            }
            State::LineComment => out.push(b),
            State::BlockComment => {
                out.push(b);
                if b == b'*' && i + 1 < bytes.len() && bytes[i + 1] == b'/' {
                    out.push(b'/');
                    i += 1;
                    state = State::Code;
                }
            }
        }
        i += 1;
    }
    while out.last() == Some(&b' ') {
        out.pop();
    }
    String::from_utf8(out).expect("formatter only manipulates ASCII whitespace")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn indents_blocks_and_preserves_strings() {
        let src = "fn main() -> i32 {\nlet s = \"a  b\";\nif 1 == 1 {\nreturn 0;\n}\n}\n";
        let out = format_source(src);
        assert_eq!(
            out,
            "fn main() -> i32 {\n    let s = \"a  b\";\n    if 1 == 1 {\n        return 0;\n    }\n}\n"
        );
        assert_eq!(format_source(&out), out);
    }

    #[test]
    fn normalizes_comma_and_delimiter_spacing() {
        let out = format_source("fn f(a : i32 ,b : i32) {\nprint(a , b) ;\n}\n");
        assert!(out.contains("fn f(a : i32, b : i32) {"));
        assert!(out.contains("print(a, b);"));
    }

    #[test]
    fn keeps_comments_and_blank_lines() {
        let out = format_source("// top\n\nfn f() {\n// inner\n}\n");
        assert_eq!(out, "// top\n\nfn f() {\n    // inner\n}\n");
    }

    #[test]
    fn does_not_count_braces_in_literals_or_comments() {
        let out = format_source("fn f() {\nlet c = '{';\nlet s = \"}\";\n// }\n}\n");
        assert_eq!(format_source(&out), out);
    }
}

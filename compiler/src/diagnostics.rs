//! Source spans and structured diagnostics.
//!
//! Every diagnostic carries a stable code (`E####`), a message, and a byte
//! span. Rendering is the only place that knows about line/column numbers, so
//! analysis stages stay free of presentation concerns.

use std::fmt::Write as _;

/// Half-open byte range into the source file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub fn new(start: usize, end: usize) -> Span {
        debug_assert!(start <= end);
        Span { start, end }
    }

    pub fn merge(self, other: Span) -> Span {
        Span {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
    pub note: Option<String>,
    pub severity: Severity,
}

impl Diagnostic {
    pub fn error(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code,
            message: message.into(),
            span,
            note: None,
            severity: Severity::Error,
        }
    }

    pub fn warning(code: &'static str, span: Span, message: impl Into<String>) -> Diagnostic {
        Diagnostic {
            code,
            message: message.into(),
            span,
            note: None,
            severity: Severity::Warning,
        }
    }

    pub fn is_warning(&self) -> bool {
        self.severity == Severity::Warning
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Diagnostic {
        self.note = Some(note.into());
        self
    }

    pub fn render(&self, file: &SourceFile) -> String {
        let (global_line, _) = file.location(self.span.start);
        let (source_name, line, col) = file.source_location(self.span.start);
        let gutter = line.to_string().len().max(global_line.to_string().len());
        let raw_len = self.span.end.saturating_sub(self.span.start);
        let (line_text, caret_start) = crop_line(file.line_text(global_line), col.saturating_sub(1));
        let caret_len = raw_len
            .max(1)
            .min(line_text.chars().count().saturating_sub(caret_start).max(1));

        let mut out = String::new();
        let label = if self.severity == Severity::Warning {
            "warning"
        } else {
            "error"
        };
        let _ = writeln!(out, "{label}[{}]: {}", self.code, self.message);
        let _ = writeln!(out, "{:width$}--> {}:{}:{}", "", source_name, line, col, width = gutter + 1);
        let _ = writeln!(out, "{:width$} |", "", width = gutter);
        let _ = writeln!(out, "{:>width$} | {}", line, line_text, width = gutter);
        let _ = writeln!(
            out,
            "{:width$} | {}{}",
            "",
            " ".repeat(caret_start),
            "^".repeat(caret_len),
            width = gutter
        );
        if let Some(note) = &self.note {
            let _ = writeln!(out, "{:width$} = note: {}", "", note, width = gutter);
        }
        out
    }
}

/// Maximum characters of source shown for one line. Longer lines are cropped
/// around the caret so a diagnostic stays readable (for example, generated input
/// with 10,000 nested parentheses).
const MAX_SHOWN_LINE: usize = 160;

/// Crop a source line around a caret given as a byte offset, returning the text
/// to display and the caret's column within that text.
fn crop_line(line: &str, caret_byte: usize) -> (String, usize) {
    let chars: Vec<char> = line.chars().collect();
    let caret = line
        .char_indices()
        .take_while(|(i, _)| *i < caret_byte)
        .count();

    if chars.len() <= MAX_SHOWN_LINE {
        return (line.to_string(), caret);
    }

    let start = caret
        .saturating_sub(MAX_SHOWN_LINE / 2)
        .min(chars.len() - MAX_SHOWN_LINE);
    let mut shown = String::new();
    let mut column = caret - start;
    if start > 0 {
        shown.push_str("...");
        column += 3;
    }
    shown.extend(&chars[start..start + MAX_SHOWN_LINE]);
    if start + MAX_SHOWN_LINE < chars.len() {
        shown.push_str("...");
    }
    (shown, column)
}

/// A parsed source file; knows how to map byte offsets to line/column.
#[derive(Clone)]
pub struct SourceFile {
    pub name: String,
    pub text: String,
    line_starts: Vec<usize>,
    segments: Vec<SourceSegment>,
    pub(crate) expanded: bool,
}

#[derive(Clone, Debug)]
struct SourceSegment {
    start: usize,
    name: String,
}

impl SourceFile {
    pub fn new(name: impl Into<String>, text: impl Into<String>) -> SourceFile {
        let text: String = text.into();
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let name = name.into();
        SourceFile {
            segments: vec![SourceSegment {
                start: 0,
                name: name.clone(),
            }],
            name,
            text,
            line_starts,
            expanded: false,
        }
    }

    /// Source text of the first unit. For a file returned by
    /// `driver::load_source` this is the root source before imports were
    /// concatenated, so an external compiler can rebuild it and resolve the
    /// imports against their original directory.
    pub fn root_text(&self) -> &str {
        let end = self
            .segments
            .get(1)
            .map(|segment| segment.start)
            .unwrap_or(self.text.len());
        &self.text[..end]
    }

    /// Build a virtual source file assembled from several real files. Segment
    /// starts must be byte offsets into `text`, in ascending order.
    pub fn with_segments(
        name: impl Into<String>,
        text: impl Into<String>,
        segments: Vec<(usize, String)>,
    ) -> SourceFile {
        let text: String = text.into();
        let name: String = name.into();
        let mut line_starts = vec![0];
        for (i, b) in text.bytes().enumerate() {
            if b == b'\n' {
                line_starts.push(i + 1);
            }
        }
        let mut segments: Vec<SourceSegment> = segments
            .into_iter()
            .map(|(start, name)| SourceSegment { start, name })
            .collect();
        segments.sort_by_key(|s| s.start);
        if segments.first().map(|s| s.start) != Some(0) {
            segments.insert(
                0,
                SourceSegment {
                    start: 0,
                    name: name.clone(),
                },
            );
        }
        SourceFile {
            name,
            text,
            line_starts,
            segments,
            expanded: true,
        }
    }

    pub fn line_count(&self) -> usize {
        self.line_starts.len()
    }

    /// 1-based (line, column) for a byte offset.
    pub fn location(&self, pos: usize) -> (usize, usize) {
        let pos = pos.min(self.text.len());
        let idx = match self.line_starts.binary_search(&pos) {
            Ok(i) => i,
            Err(i) => i - 1,
        };
        (idx + 1, pos - self.line_starts[idx] + 1)
    }

    /// Source name and 1-based local line/column for a byte offset. Without
    /// segments this is identical to `location`.
    pub fn source_location(&self, pos: usize) -> (String, usize, usize) {
        let pos = pos.min(self.text.len());
        let (global_line, col) = self.location(pos);
        let seg = self
            .segments
            .iter()
            .rev()
            .find(|s| s.start <= pos)
            .or_else(|| self.segments.first());
        let Some(seg) = seg else {
            return (self.name.clone(), global_line, col);
        };
        let (seg_line, _) = self.location(seg.start);
        (seg.name.clone(), global_line - seg_line + 1, col)
    }

    /// Byte offset for a 1-based `(line, col)` inside the named source unit.
    /// Used by the external-compiler harness to map `file:line:col`
    /// diagnostics back into an expanded (`load_source`) buffer.
    pub fn offset_in(&self, path: &str, line: usize, col: usize) -> usize {
        let unit = self
            .segments
            .iter()
            .find(|segment| segment.name == path || segment.name.ends_with(path))
            .or_else(|| {
                self.segments
                    .iter()
                    .find(|segment| path.ends_with(&segment.name))
            })
            .or_else(|| self.segments.first());
        let base = unit.map(|segment| segment.start).unwrap_or(0);
        let end = self
            .segments
            .iter()
            .map(|segment| segment.start)
            .filter(|&start| start > base)
            .min()
            .unwrap_or(self.text.len());
        let slice = &self.text[base..end];
        let bytes = slice.as_bytes();
        let mut i = 0;
        let mut current = 1;
        while i < bytes.len() && current < line {
            if bytes[i] == b'\n' {
                current += 1;
            }
            i += 1;
        }
        let mut column = 1;
        while i < bytes.len() && column < col && bytes[i] != b'\n' {
            i += 1;
            column += 1;
        }
        (base + i).min(self.text.len())
    }

    pub fn line_text(&self, line: usize) -> &str {
        let idx = line.saturating_sub(1).min(self.line_starts.len() - 1);
        let start = self.line_starts[idx];
        let end = self
            .line_starts
            .get(idx + 1)
            .copied()
            .unwrap_or(self.text.len());
        self.text[start..end].trim_end_matches(['\n', '\r'])
    }

    pub fn render_all(&self, diagnostics: &[Diagnostic]) -> String {
        let mut out = String::new();
        for (i, d) in diagnostics.iter().enumerate() {
            if i > 0 {
                out.push('\n');
            }
            out.push_str(&d.render(self));
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_offsets_to_lines_and_columns() {
        let f = SourceFile::new("t.cx", "fn main() -> i32 {\n    return 0;\n}\n");
        assert_eq!(f.location(0), (1, 1));
        assert_eq!(f.location(18), (1, 19));
        assert_eq!(f.location(19), (2, 1));
        assert_eq!(f.location(f.text.len() - 1), (3, 2));
    }

    #[test]
    fn render_includes_code_and_caret() {
        let text = "fn main() -> i32 {\n    return 0;\n}\n";
        let f = SourceFile::new("t.cx", text);
        let start = text.find("0").unwrap();
        let d = Diagnostic::error("E0203", Span::new(start, start + 1), "test failure")
            .with_note("helpful");
        let rendered = d.render(&f);
        assert!(rendered.contains("error[E0203]: test failure"));
        assert!(rendered.contains("t.cx:2:12"));
        assert!(rendered.contains('^'));
        assert!(rendered.contains("note: helpful"));
    }

    #[test]
    fn crops_long_lines_around_the_caret() {
        let long = format!("fn main() -> i32 {{ return {}; }}", "1 + ".repeat(200));
        let start = long.find("return").unwrap() + 12;
        let f = SourceFile::new("long.cx", &long);
        let d = Diagnostic::error("E9999", Span::new(start, start + 1), "test");
        let rendered = d.render(&f);
        let body: Vec<&str> = rendered.lines().collect();
        let source_line = body[3];
        assert!(source_line.chars().count() <= MAX_SHOWN_LINE + 20, "{source_line}");
        assert!(source_line.contains("..."), "{source_line}");
        assert!(rendered.contains('^'));
    }

    #[test]
    fn span_merge_covers_both() {
        let s = Span::new(4, 6).merge(Span::new(1, 2));
        assert_eq!(s, Span::new(1, 6));
    }
}

//! Minimal Language Server Protocol implementation over stdio.
//!
//! Supported: diagnostics (parse/semantic/lint), completion, go-to-definition,
//! references, rename, hover, and whole-document formatting. The server is
//! dependency-free and speaks JSON-RPC 2.0 with `Content-Length` framing.

use std::collections::{BTreeMap, HashMap};
use std::io::{BufRead, BufReader, Write};

use crate::ast::*;
use crate::diagnostics::{Diagnostic, SourceFile, Span};

#[derive(Clone, Debug, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(BTreeMap<String, Json>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(map) => map.get(key),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Json::Num(n) => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items),
            _ => None,
        }
    }

    fn obj(pairs: Vec<(&str, Json)>) -> Json {
        let mut map = BTreeMap::new();
        for (key, value) in pairs {
            map.insert(key.to_string(), value);
        }
        Json::Obj(map)
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Num(n) => {
                if n.fract() == 0.0 && n.abs() < 1e15 {
                    out.push_str(&format!("{}", *n as i64));
                } else {
                    out.push_str(&format!("{n}"));
                }
            }
            Json::Str(s) => write_json_string(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(map) => {
                out.push('{');
                for (i, (key, value)) in map.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(key, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse_json(text: &str) -> Result<Json, String> {
    let mut p = JsonParser {
        bytes: text.as_bytes(),
        pos: 0,
    };
    let value = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err("trailing input".to_string());
    }
    Ok(value)
}

struct JsonParser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> JsonParser<'a> {
    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n')
        {
            self.pos += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.bytes.get(self.pos).copied()
    }

    fn value(&mut self) -> Result<Json, String> {
        match self.peek().ok_or("unexpected end of JSON")? {
            b'n' => {
                self.expect_literal("null")?;
                Ok(Json::Null)
            }
            b't' => {
                self.expect_literal("true")?;
                Ok(Json::Bool(true))
            }
            b'f' => {
                self.expect_literal("false")?;
                Ok(Json::Bool(false))
            }
            b'"' => Ok(Json::Str(self.string()?)),
            b'[' => {
                self.pos += 1;
                let mut items = Vec::new();
                loop {
                    if self.peek() == Some(b']') {
                        self.pos += 1;
                        break;
                    }
                    items.push(self.value()?);
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b']') => {
                            self.pos += 1;
                            break;
                        }
                        _ => return Err("expected `,` or `]`".to_string()),
                    }
                }
                Ok(Json::Arr(items))
            }
            b'{' => {
                self.pos += 1;
                let mut map = BTreeMap::new();
                loop {
                    if self.peek() == Some(b'}') {
                        self.pos += 1;
                        break;
                    }
                    let key = self.string()?;
                    if self.peek() != Some(b':') {
                        return Err("expected `:`".to_string());
                    }
                    self.pos += 1;
                    map.insert(key, self.value()?);
                    match self.peek() {
                        Some(b',') => self.pos += 1,
                        Some(b'}') => {
                            self.pos += 1;
                            break;
                        }
                        _ => return Err("expected `,` or `}`".to_string()),
                    }
                }
                Ok(Json::Obj(map))
            }
            _ => {
                let start = self.pos;
                while self.pos < self.bytes.len()
                    && matches!(
                        self.bytes[self.pos],
                        b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9'
                    )
                {
                    self.pos += 1;
                }
                let text = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| "invalid number".to_string())?;
                text.parse::<f64>()
                    .map(Json::Num)
                    .map_err(|_| format!("invalid number `{text}`"))
            }
        }
    }

    fn expect_literal(&mut self, text: &str) -> Result<(), String> {
        if self.bytes[self.pos..].starts_with(text.as_bytes()) {
            self.pos += text.len();
            Ok(())
        } else {
            Err(format!("expected `{text}`"))
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.skip_ws();
        if self.bytes.get(self.pos) != Some(&b'"') {
            return Err("expected string".to_string());
        }
        self.pos += 1;
        let mut out = String::new();
        while let Some(&b) = self.bytes.get(self.pos) {
            self.pos += 1;
            match b {
                b'"' => return Ok(out),
                b'\\' => {
                    let esc = *self.bytes.get(self.pos).ok_or("unterminated escape")?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let hex = std::str::from_utf8(
                                self.bytes.get(self.pos..self.pos + 4).ok_or("bad escape")?,
                            )
                            .map_err(|_| "bad escape".to_string())?;
                            let code = u32::from_str_radix(hex, 16)
                                .map_err(|_| "bad escape".to_string())?;
                            self.pos += 4;
                            out.push(char::from_u32(code).unwrap_or('\u{fffd}'));
                        }
                        _ => return Err("unknown escape".to_string()),
                    }
                }
                other => out.push(other as char),
            }
        }
        Err("unterminated string".to_string())
    }
}

#[derive(Default)]
struct State {
    docs: HashMap<String, String>,
    shutdown: bool,
}

pub fn run() -> i32 {
    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut state = State::default();
    while let Some(message) = read_message(&mut reader) {
        if handle(&mut state, message) {
            break;
        }
    }
    0
}

fn read_message<R: BufRead>(reader: &mut R) -> Option<Json> {
    let mut length: Option<usize> = None;
    loop {
        let mut line = String::new();
        if reader.read_line(&mut line).ok()? == 0 {
            return None;
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if let Some(value) = trimmed.strip_prefix("Content-Length:") {
            length = value.trim().parse().ok();
        }
    }
    let length = length?;
    let mut buffer = vec![0u8; length];
    reader.read_exact(&mut buffer).ok()?;
    let text = String::from_utf8_lossy(&buffer);
    parse_json(&text).ok()
}

fn send(value: Json) {
    let text = value.dump();
    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    let _ = write!(out, "Content-Length: {}\r\n\r\n{}", text.len(), text);
    let _ = out.flush();
}

fn respond(id: &Json, result: Json) {
    send(Json::obj(vec![
        ("jsonrpc", Json::Str("2.0".to_string())),
        ("id", id.clone()),
        ("result", result),
    ]));
}

fn notify(method: &str, params: Json) {
    send(Json::obj(vec![
        ("jsonrpc", Json::Str("2.0".to_string())),
        ("method", Json::Str(method.to_string())),
        ("params", params),
    ]));
}

/// Returns true when the server must exit.
fn handle(state: &mut State, message: Json) -> bool {
    let method = message.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let id = message.get("id").cloned().unwrap_or(Json::Null);
    let params = message.get("params").cloned().unwrap_or(Json::Null);
    match method {
        "initialize" => {
            let capabilities = Json::obj(vec![
                ("textDocumentSync", Json::Num(1.0)),
                (
                    "completionProvider",
                    Json::obj(vec![(
                        "triggerCharacters",
                        Json::Arr(vec![Json::Str(".".to_string()), Json::Str(":".to_string())]),
                    )]),
                ),
                ("definitionProvider", Json::Bool(true)),
                ("referencesProvider", Json::Bool(true)),
                ("renameProvider", Json::Bool(true)),
                ("documentFormattingProvider", Json::Bool(true)),
                ("hoverProvider", Json::Bool(true)),
            ]);
            respond(
                &id,
                Json::obj(vec![
                    ("capabilities", capabilities),
                    (
                        "serverInfo",
                        Json::obj(vec![
                            ("name", Json::Str("corex-lsp".to_string())),
                            ("version", Json::Str(env!("CARGO_PKG_VERSION").to_string())),
                        ]),
                    ),
                ]),
            );
        }
        "initialized" => {}
        "shutdown" => {
            state.shutdown = true;
            respond(&id, Json::Null);
        }
        "exit" => return true,
        "textDocument/didOpen" | "textDocument/didChange" => {
            let Some(doc) = params.get("textDocument") else {
                return false;
            };
            let Some(uri) = doc.get("uri").and_then(|u| u.as_str()) else {
                return false;
            };
            let text = if method == "textDocument/didOpen" {
                doc.get("text").and_then(|t| t.as_str()).unwrap_or("")
            } else {
                params
                    .get("contentChanges")
                    .and_then(|c| c.as_array())
                    .and_then(|c| c.last())
                    .and_then(|c| c.get("text"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("")
            };
            state.docs.insert(uri.to_string(), text.to_string());
            publish_diagnostics(uri, text);
        }
        "textDocument/didClose" => {
            if let Some(uri) = params
                .get("textDocument")
                .and_then(|d| d.get("uri"))
                .and_then(|u| u.as_str())
            {
                state.docs.remove(uri);
                notify(
                    "textDocument/publishDiagnostics",
                    Json::obj(vec![
                        ("uri", Json::Str(uri.to_string())),
                        ("diagnostics", Json::Arr(Vec::new())),
                    ]),
                );
            }
        }
        "textDocument/completion" => {
            let Some(uri) = uri_of(&params) else {
                respond(&id, Json::Arr(Vec::new()));
                return false;
            };
            let text = state.docs.get(&uri).cloned().unwrap_or_default();
            let items = completion_items(&uri, &text);
            respond(&id, Json::Arr(items));
        }
        "textDocument/definition" => {
            let Some(uri) = uri_of(&params) else {
                respond(&id, Json::Null);
                return false;
            };
            let text = state.docs.get(&uri).cloned().unwrap_or_default();
            let (offset, word) = word_at(&text, &params);
            let _ = offset;
            match find_definition(&uri, &text, &word) {
                Some(span) => respond(&id, location(&uri, &text, span)),
                None => respond(&id, Json::Null),
            }
        }
        "textDocument/references" => {
            let Some(uri) = uri_of(&params) else {
                respond(&id, Json::Arr(Vec::new()));
                return false;
            };
            let text = state.docs.get(&uri).cloned().unwrap_or_default();
            let (_offset, word) = word_at(&text, &params);
            let mut locations = Vec::new();
            for span in identifier_spans(&text, &word) {
                locations.push(location(&uri, &text, span));
            }
            respond(&id, Json::Arr(locations));
        }
        "textDocument/rename" => {
            let Some(uri) = uri_of(&params) else {
                respond(&id, Json::Null);
                return false;
            };
            let text = state.docs.get(&uri).cloned().unwrap_or_default();
            let (_offset, word) = word_at(&text, &params);
            let new_name = params
                .get("newName")
                .and_then(|n| n.as_str())
                .unwrap_or("");
            if !valid_identifier(new_name) {
                respond(&id, Json::Null);
                return false;
            }
            let edits: Vec<Json> = identifier_spans(&text, &word)
                .into_iter()
                .map(|span| {
                    Json::obj(vec![
                        ("range", range(&text, span)),
                        ("newText", Json::Str(new_name.to_string())),
                    ])
                })
                .collect();
            let mut changes = BTreeMap::new();
            changes.insert(uri.clone(), Json::Arr(edits));
            respond(
                &id,
                Json::obj(vec![("changes", Json::Obj(changes))]),
            );
        }
        "textDocument/formatting" => {
            let Some(uri) = uri_of(&params) else {
                respond(&id, Json::Null);
                return false;
            };
            let text = state.docs.get(&uri).cloned().unwrap_or_default();
            let formatted = crate::format::format_source(&text);
            let edit = Json::obj(vec![
                (
                    "range",
                    range(
                        &text,
                        Span::new(0, text.len()),
                    ),
                ),
                ("newText", Json::Str(formatted)),
            ]);
            respond(&id, Json::Arr(vec![edit]));
        }
        "textDocument/hover" => {
            let Some(uri) = uri_of(&params) else {
                respond(&id, Json::Null);
                return false;
            };
            let text = state.docs.get(&uri).cloned().unwrap_or_default();
            let (_offset, word) = word_at(&text, &params);
            match hover_text(&text, &word) {
                Some(value) => respond(
                    &id,
                    Json::obj(vec![(
                        "contents",
                        Json::obj(vec![
                            ("kind", Json::Str("markdown".to_string())),
                            ("value", Json::Str(value)),
                        ]),
                    )]),
                ),
                None => respond(&id, Json::Null),
            }
        }
        _ => {
            if !matches!(id, Json::Null) {
                respond(&id, Json::Null);
            }
        }
    }
    false
}

fn uri_of(params: &Json) -> Option<String> {
    params
        .get("textDocument")
        .and_then(|d| d.get("uri"))
        .and_then(|u| u.as_str())
        .map(|s| s.to_string())
}

fn publish_diagnostics(uri: &str, text: &str) {
    let file = SourceFile::new(uri.to_string(), text.to_string());
    let mut diags: Vec<Diagnostic> = Vec::new();
    match crate::lexer::lex(text) {
        Ok(tokens) => match crate::parser::parse(&tokens) {
            Ok(program) => {
                diags.extend(crate::lint::lint(&program));
                if let Err(errors) = crate::semantic::analyze(&program) {
                    diags.extend(errors);
                }
            }
            Err(errors) => diags.extend(errors),
        },
        Err(errors) => diags.extend(errors),
    }
    let items: Vec<Json> = diags
        .iter()
        .map(|d| {
            Json::obj(vec![
                ("range", range(text, d.span)),
                (
                    "severity",
                    Json::Num(if d.is_warning() { 2.0 } else { 1.0 }),
                ),
                ("code", Json::Str(d.code.to_string())),
                ("source", Json::Str("corex".to_string())),
                ("message", Json::Str(d.message.clone())),
            ])
        })
        .collect();
    let _ = file;
    notify(
        "textDocument/publishDiagnostics",
        Json::obj(vec![
            ("uri", Json::Str(uri.to_string())),
            ("diagnostics", Json::Arr(items)),
        ]),
    );
}

fn position_to_offset(text: &str, line: i64, character: i64) -> usize {
    let mut current_line = 0i64;
    let mut offset = 0usize;
    for (index, ch) in text.char_indices() {
        if current_line == line {
            break;
        }
        if ch == '\n' {
            current_line += 1;
            offset = index + 1;
        }
    }
    let line_start = offset;
    let line_end = text[line_start..]
        .find('\n')
        .map(|i| line_start + i)
        .unwrap_or(text.len());
    let mut col = 0i64;
    let mut byte = line_start;
    for ch in text[line_start..line_end].chars() {
        if col >= character {
            break;
        }
        byte += ch.len_utf8();
        col += 1;
    }
    byte
}

fn offset_to_position(text: &str, offset: usize) -> (i64, i64) {
    let offset = offset.min(text.len());
    let mut line = 0i64;
    let mut line_start = 0usize;
    for (index, ch) in text.char_indices() {
        if index >= offset {
            break;
        }
        if ch == '\n' {
            line += 1;
            line_start = index + 1;
        }
    }
    let col = text[line_start..offset].chars().count() as i64;
    (line, col)
}

fn range(text: &str, span: Span) -> Json {
    let (start_line, start_col) = offset_to_position(text, span.start);
    let (end_line, end_col) = offset_to_position(text, span.end);
    Json::obj(vec![
        (
            "start",
            Json::obj(vec![
                ("line", Json::Num(start_line as f64)),
                ("character", Json::Num(start_col as f64)),
            ]),
        ),
        (
            "end",
            Json::obj(vec![
                ("line", Json::Num(end_line as f64)),
                ("character", Json::Num(end_col as f64)),
            ]),
        ),
    ])
}

fn location(uri: &str, text: &str, span: Span) -> Json {
    Json::obj(vec![
        ("uri", Json::Str(uri.to_string())),
        ("range", range(text, span)),
    ])
}

fn word_at(text: &str, params: &Json) -> (usize, String) {
    let line = params
        .get("position")
        .and_then(|p| p.get("line"))
        .and_then(|l| l.as_i64())
        .unwrap_or(0);
    let character = params
        .get("position")
        .and_then(|p| p.get("character"))
        .and_then(|c| c.as_i64())
        .unwrap_or(0);
    let offset = position_to_offset(text, line, character);
    let bytes = text.as_bytes();
    let mut start = offset.min(bytes.len());
    while start > 0 && is_word_byte(bytes[start - 1]) {
        start -= 1;
    }
    let mut end = offset.min(bytes.len());
    while end < bytes.len() && is_word_byte(bytes[end]) {
        end += 1;
    }
    (offset, text[start..end].to_string())
}

fn is_word_byte(b: u8) -> bool {
    b == b'_' || b.is_ascii_alphanumeric()
}

fn valid_identifier(text: &str) -> bool {
    let mut chars = text.chars();
    match chars.next() {
        Some(c) if c == '_' || c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

fn identifier_spans(text: &str, word: &str) -> Vec<Span> {
    if word.is_empty() {
        return Vec::new();
    }
    match crate::lexer::lex(text) {
        Ok(tokens) => tokens
            .into_iter()
            .filter_map(|t| match &t.kind {
                crate::token::TokenKind::Ident(name) if name == word => Some(t.span),
                _ => None,
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn collect_defs(program: &Program) -> Vec<(String, Span, String)> {
    let mut defs = Vec::new();
    for f in &program.fns {
        let sig = format!(
            "fn {}({}){}",
            f.name,
            f.params
                .iter()
                .map(|p| format!("{}: {}", p.name, p.ty.display()))
                .collect::<Vec<_>>()
                .join(", "),
            if f.ret == Type::Unit {
                String::new()
            } else {
                format!(" -> {}", f.ret.display())
            }
        );
        defs.push((f.name.clone(), f.name_span, sig));
        if let Some(body) = &f.body {
            collect_body_defs(body, &mut defs);
        }
    }
    for s in &program.structs {
        defs.push((
            s.name.clone(),
            s.name_span,
            format!("{} {}", if s.is_union { "union" } else { "struct" }, s.name),
        ));
    }
    for e in &program.enums {
        defs.push((e.name.clone(), e.name_span, format!("enum {}", e.name)));
    }
    for c in &program.consts {
        defs.push((
            c.name.clone(),
            c.name_span,
            format!("const {}: {}", c.name, c.ty.display()),
        ));
    }
    for g in &program.globals {
        defs.push((
            g.name.clone(),
            g.name_span,
            format!("static {}: {}", g.name, g.ty.display()),
        ));
    }
    for t in &program.typedefs {
        defs.push((
            t.name.clone(),
            t.name_span,
            format!("typedef {} = {}", t.name, t.ty.display()),
        ));
    }
    defs
}

fn collect_body_defs(block: &Block, defs: &mut Vec<(String, Span, String)>) {
    for stmt in &block.stmts {
        match stmt {
            Stmt::Let {
                name,
                name_span,
                init,
                ..
            } => {
                let ty = match &init.kind {
                    ExprKind::Int(_) => "i32".to_string(),
                    _ => "value".to_string(),
                };
                let _ = ty;
                defs.push((name.clone(), *name_span, format!("let {name}")));
            }
            Stmt::If {
                then_block,
                else_branch,
                ..
            } => {
                collect_body_defs(then_block, defs);
                if let Some(b) = else_branch {
                    if let Stmt::Block(block) = &**b {
                        collect_body_defs(block, defs);
                    }
                }
            }
            Stmt::While { body, .. }
            | Stmt::Loop { body, .. }
            | Stmt::For { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Block(body) => collect_body_defs(body, defs),
            Stmt::Switch { arms, .. } => {
                for arm in arms {
                    collect_body_defs(&arm.body, defs);
                }
            }
            Stmt::Label { stmt, .. } => {
                if let Stmt::Block(block) = &**stmt {
                    collect_body_defs(block, defs);
                }
            }
            _ => {}
        }
    }
}

fn find_definition(_uri: &str, text: &str, word: &str) -> Option<Span> {
    let tokens = crate::lexer::lex(text).ok()?;
    let program = crate::parser::parse(&tokens).ok()?;
    let mut defs = collect_defs(&program);
    for f in &program.fns {
        for p in &f.params {
            defs.push((p.name.clone(), p.span, format!("parameter {}", p.name)));
        }
    }
    defs.into_iter()
        .find(|(name, _, _)| name == word)
        .map(|(_, span, _)| span)
}

fn hover_text(text: &str, word: &str) -> Option<String> {
    let tokens = crate::lexer::lex(text).ok()?;
    let program = crate::parser::parse(&tokens).ok()?;
    let mut defs = collect_defs(&program);
    for f in &program.fns {
        for p in &f.params {
            defs.push((p.name.clone(), p.span, format!("parameter {}", p.name)));
        }
    }
    defs.into_iter()
        .find(|(name, _, _)| name == word)
        .map(|(_, _, detail)| format!("```corex\n{detail}\n```"))
}

fn completion_items(_uri: &str, text: &str) -> Vec<Json> {
    const KEYWORDS: &[&str] = &[
        "fn", "let", "mut", "if", "else", "while", "loop", "for", "do", "switch", "case",
        "default", "break", "continue", "return", "unsafe", "struct", "union", "enum",
        "typedef", "static", "const", "extern", "pub", "mod", "module", "as", "true",
        "false", "null", "sizeof", "alignof", "offsetof", "goto", "cstr", "str", "vec",
        "i8", "i16", "i32", "i64", "u8", "u16", "u32", "u64", "usize", "isize", "bool",
        "char", "f32", "f64",
    ];
    let mut items: Vec<Json> = KEYWORDS
        .iter()
        .map(|k| {
            Json::obj(vec![
                ("label", Json::Str((*k).to_string())),
                ("kind", Json::Num(14.0)),
            ])
        })
        .collect();
    if let Ok(tokens) = crate::lexer::lex(text) {
        if let Ok(program) = crate::parser::parse(&tokens) {
            for (name, _, detail) in collect_defs(&program) {
                items.push(Json::obj(vec![
                    ("label", Json::Str(name)),
                    ("kind", Json::Num(3.0)),
                    ("detail", Json::Str(detail)),
                ]));
            }
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_json() {
        let text = r#"{"a":[1,2,true,null,"x\n"]}"#;
        let value = parse_json(text).unwrap();
        assert_eq!(value.dump(), text);
    }

    #[test]
    fn computes_ranges_and_offsets() {
        let text = "fn main() {\n    return 0;\n}\n";
        let offset = position_to_offset(text, 1, 4);
        assert_eq!(&text[offset..offset + 6], "return");
        let (line, col) = offset_to_position(text, offset);
        assert_eq!((line, col), (1, 4));
    }

    #[test]
    fn finds_definition_and_references() {
        let text = "fn helper() -> i32 { return 1; }\nfn main() -> i32 { return helper(); }\n";
        let span = find_definition("file:///x.cx", text, "helper").unwrap();
        assert_eq!(&text[span.start..span.end], "helper");
        assert_eq!(identifier_spans(text, "helper").len(), 2);
    }
}

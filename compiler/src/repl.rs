//! Interactive REPL.
//!
//! The REPL keeps the source text of every top-level function the user has
//! submitted. A submission is parsed; each function it defines replaces any
//! earlier definition with the same name (exactly the useful part of a REPL:
//! redefining a function and re-running). The combined program is re-parsed and
//! re-analyzed from source on every run, so diagnostics always point at real
//! text.
//!
//! Input rules:
//!   - a blank line submits the accumulated block;
//!   - `:command` lines are handled only when no block is pending;
//!   - EOF submits any pending block and exits.
//!
//! A submission that is not a set of items is wrapped as
//! `fn main() -> i32 { <submission> return 0; }` so single statements can be
//! executed directly. That wrapper replaces a previous `main`.
//!
//! Limitation: the REPL keeps stdin buffered, so programs that read stdin
//! should be run with `corex run file.cx` or `corex run -` instead.

use std::collections::BTreeMap;

use crate::ast::Program;
use crate::diagnostics::{Diagnostic, SourceFile};
use crate::driver;
use crate::lexer;
use crate::lineedit::{Editor, Input};
use crate::parser;

pub fn run(opt_level: u8) -> i32 {
    let mut session = Session {
        defs: BTreeMap::new(),
        opt: opt_level,
    };

    println!("CoreX REPL — finish a block with an empty line. Type :help for commands.\n");

    let mut editor = Editor::new();
    let mut chunk = String::new();

    loop {
        let prompt = if chunk.trim().is_empty() {
            "corex> "
        } else {
            "   ...> "
        };

        match editor.read(prompt) {
            Input::Eof => {
                if !chunk.trim().is_empty() {
                    session.submit(chunk.trim_end(), true);
                }
                println!();
                break;
            }
            Input::Interrupted => {
                chunk.clear();
                continue;
            }
            Input::Line(line) => {
                if let Some(cmd) = line.trim().strip_prefix(':') {
                    match cmd {
                        "submit" | "go" => {
                            if chunk.trim().is_empty() {
                                println!("[repl] nothing to submit");
                            } else {
                                let block = std::mem::take(&mut chunk);
                                session.submit(block.trim_end(), false);
                            }
                        }
                        "history" => {
                            for entry in &editor.history {
                                println!("{entry}");
                            }
                        }
                        _ => {
                            if session.command(cmd) {
                                break;
                            }
                        }
                    }
                    continue;
                }

                if line.trim().is_empty() {
                    if chunk.trim().is_empty() {
                        continue;
                    }
                    // A blank line inside a function body is not a submission:
                    // only submit when the block can be complete.
                    if needs_more_input(&chunk) {
                        chunk.push('\n');
                        continue;
                    }
                    if !has_code(&chunk) {
                        // Comments only: nothing to run.
                        chunk.clear();
                        continue;
                    }
                    let block = std::mem::take(&mut chunk);
                    session.submit(block.trim_end(), true);
                    continue;
                }

                chunk.push_str(&line);
                chunk.push('\n');

                // Python-like: a complete one-liner runs on Enter. A multi-line
                // block runs as soon as it is complete and defines the entry
                // point (so pasting a whole program works without an extra
                // blank line). Anything else waits for a blank line or
                // `:submit`, which lets several statements run together.
                if !needs_more_input(&chunk) && should_auto_submit(&chunk) {
                    let block = std::mem::take(&mut chunk);
                    session.submit(block.trim_end(), true);
                }
            }
        }
    }

    0
}

struct Session {
    /// Function name -> exact source text of that function.
    defs: BTreeMap<String, String>,
    opt: u8,
}

impl Session {
    fn combined(&self) -> String {
        self.defs
            .values()
            .cloned()
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Handle a `:command`. Returns true when the REPL should exit.
    fn command(&mut self, cmd: &str) -> bool {
        match cmd {
            "q" | "quit" | "exit" => return true,
            "help" | "h" => {
                println!(
                    "\
  <code>          a complete one-liner runs on Enter; other blocks finish with an empty line
  fn f(..) {{ }}    definitions accumulate; a redefinition replaces the old one
  :submit         compile and run the pending block even if it looks incomplete
  :run            re-run the current program
  :history        show the input history (also persisted in ~/.corex_history)
  :ir             print the LLVM IR and verify it
  :show           print the combined program
  :reset          forget all definitions
  :quit           leave the REPL"
                );
            }
            "reset" | "clear" => {
                self.defs.clear();
                println!("[repl] cleared");
            }
            "show" => {
                if self.defs.is_empty() {
                    println!("[repl] nothing defined yet");
                } else {
                    println!("{}", self.combined());
                }
            }
            "run" => self.run_current(false),
            "ir" => self.print_ir(),
            other => println!("[repl] unknown command `:{other}` (try :help)"),
        }
        false
    }

    fn submit(&mut self, chunk: &str, quiet: bool) {
        let chunk = chunk.trim_end();
        if chunk.trim().is_empty() {
            return;
        }

        // 1. A chunk that parses as top-level items is a definition update.
        match parse_program(chunk) {
            Ok(program) if !program.fns.is_empty() => {
                self.install(chunk, &program);
                self.run_current(quiet);
                return;
            }
            Ok(_) => {}
            Err(_) => {}
        }

        // 2. Otherwise try an expression that prints its value, then a
        //    statement block. Candidates are checked together with the
        //    definitions already in the session, so `fact(10)` resolves `fact`.
        //    The first candidate that *analyzes* cleanly wins, so `print(x)`
        //    (whose argument is void) falls back to statement mode instead of
        //    failing.
        let mut last_error: Option<(String, Vec<Diagnostic>)> = None;
        for candidate in candidates(chunk) {
            let combined = self.text_with(&candidate);
            match crate::driver::check(&SourceFile::new("<repl>", combined.clone())) {
                Ok(_) => {
                    if let Ok(program) = parse_program(&candidate) {
                        if !program.fns.is_empty() {
                            self.install(&candidate, &program);
                            self.run_current(quiet);
                            return;
                        }
                    }
                }
                Err(diags) => last_error = Some((combined, diags)),
            }
        }

        // 3. Nothing analyzed: report the diagnostics from the last candidate
        //    (the statement wrapper), which includes the user's code.
        let mut reported: Vec<Diagnostic> = Vec::new();
        if let Some((text, diags)) = last_error {
            let file = SourceFile::new("<repl>", text);
            eprint!("{}", file.render_all(&diags));
            reported = diags;
        } else if let Err(diags) = parse_program(chunk) {
            let file = SourceFile::new("<repl>", chunk.to_string());
            eprint!("{}", file.render_all(&diags));
            reported = diags;
        }

        if reported.iter().any(|d| d.code.starts_with("E0201")) {
            println!(
                "[repl] bindings do not persist between submissions (each run is a new program);\n       \
                 submit related statements together, or wrap them in {{ ... }} and use :submit"
            );
        }
    }

    /// Session definitions plus a candidate program, excluding any stored entry
    /// point (the candidate supplies its own `main`).
    fn text_with(&self, candidate: &str) -> String {
        let mut text = String::new();
        for (name, source) in &self.defs {
            if name == "main" || name == "_start" {
                continue;
            }
            text.push_str(source);
            text.push_str("\n\n");
        }
        text.push_str(candidate);
        text
    }

    fn install(&mut self, text: &str, program: &Program) {
        for f in &program.fns {
            let source = text
                .get(f.span.start..f.span.end)
                .unwrap_or("")
                .to_string();
            self.defs.insert(f.name.clone(), source);
        }
    }

    fn print_ir(&mut self) {
        if self.defs.is_empty() {
            println!("[repl] nothing defined yet");
            return;
        }
        let text = self.combined();
        let file = SourceFile::new("<repl>", text);
        match driver::emit_ir(&file) {
            Ok(ir) => match driver::verify_ir(&ir) {
                Ok(()) => {
                    println!("{ir}");
                    println!("[repl] IR verified by llvm-as");
                }
                Err(message) => {
                    println!("{ir}");
                    eprintln!("[repl] IR verification failed:\n{message}");
                }
            },
            Err(diags) => report(&file, &diags),
        }
    }

    fn run_current(&mut self, quiet: bool) {
        if self.defs.is_empty() {
            println!("[repl] nothing defined yet");
            return;
        }

        let text = self.combined();
        let file = SourceFile::new("<repl>", text);

        let work = driver::temp_dir("corex-repl");
        if let Err(e) = std::fs::create_dir_all(&work) {
            eprintln!("[repl] cannot create {}: {e}", work.display());
            return;
        }
        let exe = work.join("program");

        match driver::build(&file, &exe, self.opt, false) {
            Ok(artifacts) => match driver::run(&artifacts.executable, &[]) {
                Ok(status) => {
                    let code = status.code().unwrap_or(-1);
                    println!("[exit {code}]");
                }
                Err(e) => eprintln!("[repl] {e}"),
            },
            Err(diags) => {
                if diags.iter().all(|d| d.code == "E0212") {
                    if !quiet {
                        println!(
                            "[repl] no entry point yet: add `fn main() -> i32` or `fn _start()`"
                        );
                    }
                } else {
                    report(&file, &diags);
                }
            }
        }

        let _ = std::fs::remove_dir_all(&work);
    }
}

/// Heuristic: does the buffered text need more input before it can be a
/// complete block? True when brackets are unbalanced, a string or block comment
/// is open, or the last token cannot end a statement. This keeps blank lines
/// inside a function body from submitting a half-typed function.
fn needs_more_input(chunk: &str) -> bool {
    let chars: Vec<char> = chunk.chars().collect();
    let mut depth: i32 = 0;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut in_string = false;
    let mut escaped = false;

    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        let next = chars.get(i + 1).copied();

        if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
            }
            i += 1;
            continue;
        }
        if in_block_comment {
            if c == '*' && next == Some('/') {
                in_block_comment = false;
                i += 2;
            } else {
                i += 1;
            }
            continue;
        }
        if in_string {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        match (c, next) {
            ('/', Some('/')) => {
                in_line_comment = true;
                i += 2;
            }
            ('/', Some('*')) => {
                in_block_comment = true;
                i += 2;
            }
            ('"', _) => {
                in_string = true;
                i += 1;
            }
            ('{' | '(' | '[', _) => {
                depth += 1;
                i += 1;
            }
            ('}' | ')' | ']', _) => {
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }

    if depth > 0 || in_string || in_block_comment {
        return true;
    }

    let trimmed = chunk.trim_end();
    let last_char = trimmed.chars().last();
    if matches!(
        last_char,
        Some('=' | '+' | '-' | '*' | '/' | '%' | '&' | '|' | '^' | '<' | '>' | ',' | ':' | '.' | '(' | '{' | '[')
    ) {
        return true;
    }

    let last_word = trimmed.split_whitespace().last().unwrap_or("");
    matches!(
        last_word,
        "let" | "mut" | "fn" | "return" | "as" | "if" | "else" | "while" | "loop" | "unsafe"
    )
}

/// Should a complete chunk run immediately, without waiting for a blank line?
/// Yes for a single line, and for multi-line blocks that define `main` or
/// `_start`. Other multi-line statement blocks are left for the blank line so
/// they can share state.
fn should_auto_submit(chunk: &str) -> bool {
    if !has_code(chunk) {
        return false;
    }
    let trimmed = chunk.trim_end();
    if !trimmed.contains('\n') {
        return true;
    }
    match parse_program(trimmed) {
        Ok(program) => program
            .fns
            .iter()
            .any(|f| f.name == "main" || f.name == "_start"),
        Err(_) => false,
    }
}

/// True when the buffer contains at least one token (that is, not only
/// whitespace and comments).
fn has_code(chunk: &str) -> bool {
    match lexer::lex(chunk) {
        Ok(tokens) => tokens.len() > 1,
        Err(_) => false,
    }
}

/// Candidate programs for a REPL submission, most specific first:
///   1. the expression itself, printed (no trailing semicolon),
///   2. the statements, run for their side effects.
fn candidates(chunk: &str) -> Vec<String> {
    let mut out = Vec::new();
    if !chunk.trim_end().ends_with(';') {
        out.push(format!(
            "fn main() -> i32 {{\n    print({chunk});\n    return 0;\n}}\n"
        ));
    }
    let mut statements = String::from(chunk);
    if !chunk.trim_end().ends_with(';') && !chunk.trim_end().ends_with('}') {
        statements.push(';');
    }
    out.push(format!(
        "fn main() -> i32 {{\n{statements}\n    return 0;\n}}\n"
    ));
    out
}

fn parse_program(text: &str) -> Result<Program, Vec<Diagnostic>> {
    let tokens = lexer::lex(text)?;
    parser::parse(&tokens)
}

fn report(file: &SourceFile, diags: &[Diagnostic]) {
    eprint!("{}", file.render_all(diags));
}

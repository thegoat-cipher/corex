//! Minimal terminal line editor.
//!
//! When stdin is a terminal, the REPL needs raw mode to see individual keys:
//! otherwise arrow keys and backspace never arrive before Enter. Raw mode is
//! entered with the external `stty` utility (no libc bindings, no crates) and
//! restored by `Drop`, so an early exit or panic still leaves the terminal
//! usable.
//!
//! Supported keys: printable ASCII, Backspace, Delete, Left/Right, Home/End,
//! Ctrl-A/E/B/F/U, Up/Down for history, Enter, Ctrl-C (interrupt the current
//! input), Ctrl-D (end of input).
//!
//! When stdin is not a terminal (pipes, tests) this falls back to plain
//! line-buffered reads and never touches terminal settings.

use std::io::{self, IsTerminal, Read, Write};
use std::path::PathBuf;
use std::process::Command;

pub enum Input {
    Line(String),
    Eof,
    Interrupted,
}

pub struct Editor {
    tty: bool,
    saved: Option<String>,
    pub history: Vec<String>,
    history_file: Option<PathBuf>,
}

const MAX_HISTORY: usize = 500;

impl Editor {
    pub fn new() -> Editor {
        let tty = io::stdin().is_terminal();
        let mut editor = Editor {
            tty,
            saved: None,
            history: Vec::new(),
            history_file: history_path(),
        };
        editor.load_history();
        if tty {
            editor.saved = stty(&["-g"]);
            let _ = stty(&["raw", "-echo"]);
        }
        editor
    }

    pub fn read(&mut self, prompt: &str) -> Input {
        if self.tty {
            self.read_tty(prompt)
        } else {
            self.read_plain(prompt)
        }
    }

    fn read_plain(&mut self, prompt: &str) -> Input {
        print!("{prompt}");
        let _ = io::stdout().flush();
        let mut line = String::new();
        match io::stdin().read_line(&mut line) {
            Ok(0) => Input::Eof,
            Ok(_) => {
                let line = line.trim_end_matches(['\n', '\r']).to_string();
                self.record(&line);
                Input::Line(line)
            }
            Err(_) => Input::Eof,
        }
    }

    fn read_tty(&mut self, prompt: &str) -> Input {
        let mut buf: Vec<char> = Vec::new();
        let mut cursor = 0usize;
        let mut history_index = self.history.len();
        let mut stash = String::new();

        draw(prompt, &buf, cursor);

        let mut stdin = io::stdin();
        loop {
            let mut byte = [0u8; 1];
            match stdin.read(&mut byte) {
                Ok(0) => {
                    print!("\r\n");
                    let _ = io::stdout().flush();
                    return Input::Eof;
                }
                Ok(_) => {}
                Err(_) => return Input::Eof,
            }

            match byte[0] {
                0x0d | 0x0a => {
                    print!("\r\n");
                    let _ = io::stdout().flush();
                    let line: String = buf.iter().collect();
                    self.record(&line);
                    return Input::Line(line);
                }
                0x03 => {
                    print!("^C\r\n");
                    let _ = io::stdout().flush();
                    return Input::Interrupted;
                }
                0x04 => {
                    if buf.is_empty() {
                        print!("\r\n");
                        let _ = io::stdout().flush();
                        return Input::Eof;
                    }
                    if cursor < buf.len() {
                        buf.remove(cursor);
                    }
                }
                0x7f | 0x08 => {
                    if cursor > 0 {
                        cursor -= 1;
                        buf.remove(cursor);
                    }
                }
                0x01 => cursor = 0,
                0x05 => cursor = buf.len(),
                0x02 => cursor = cursor.saturating_sub(1),
                0x06 => {
                    if cursor < buf.len() {
                        cursor += 1;
                    }
                }
                0x15 => {
                    buf.truncate(cursor);
                }
                0x1b => {
                    let mut seq = [0u8; 2];
                    if stdin.read(&mut seq).unwrap_or(0) < 2 || seq[0] != b'[' {
                        continue;
                    }
                    match seq[1] {
                        b'A' => {
                            if history_index > 0 {
                                if history_index == self.history.len() {
                                    stash = buf.iter().collect();
                                }
                                history_index -= 1;
                                buf = self.history[history_index].chars().collect();
                                cursor = buf.len();
                            }
                        }
                        b'B' => {
                            if history_index < self.history.len() {
                                history_index += 1;
                                if history_index == self.history.len() {
                                    buf = stash.chars().collect();
                                } else {
                                    buf = self.history[history_index].chars().collect();
                                }
                                cursor = buf.len();
                            }
                        }
                        b'C' => {
                            if cursor < buf.len() {
                                cursor += 1;
                            }
                        }
                        b'D' => {
                            if cursor > 0 {
                                cursor -= 1;
                            }
                        }
                        b'H' => cursor = 0,
                        b'F' => cursor = buf.len(),
                        b'3' => {
                            let mut tilde = [0u8; 1];
                            let _ = stdin.read(&mut tilde);
                            if cursor < buf.len() {
                                buf.remove(cursor);
                            }
                        }
                        _ => {}
                    }
                }
                0x20..=0x7e => {
                    buf.insert(cursor, byte[0] as char);
                    cursor += 1;
                }
                _ => {}
            }

            draw(prompt, &buf, cursor);
        }
    }

    pub fn record(&mut self, line: &str) {
        let trimmed = line.trim();
        // REPL commands are not useful to recall.
        if trimmed.is_empty() || trimmed.starts_with(':') {
            return;
        }
        if self.history.last().map(|h| h == line).unwrap_or(false) {
            return;
        }
        self.history.push(line.to_string());
        if self.history.len() > MAX_HISTORY {
            self.history.remove(0);
        }
        if let Some(path) = &self.history_file {
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
            {
                let _ = writeln!(f, "{line}");
            }
        }
    }

    fn load_history(&mut self) {
        let Some(path) = &self.history_file else {
            return;
        };
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let mut lines: Vec<String> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| l.to_string())
            .collect();
        if lines.len() > MAX_HISTORY {
            lines.drain(0..lines.len() - MAX_HISTORY);
        }
        self.history = lines;
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        if let Some(saved) = self.saved.as_deref() {
            let _ = stty(&[saved]);
        }
    }
}

fn draw(prompt: &str, buf: &[char], cursor: usize) {
    let mut out = String::new();
    out.push('\r');
    out.push_str("\x1b[K");
    out.push_str(prompt);
    out.extend(buf.iter());
    let back = buf.len().saturating_sub(cursor);
    if back > 0 {
        out.push_str(&format!("\x1b[{back}D"));
    }
    print!("{out}");
    let _ = io::stdout().flush();
}

fn stty(args: &[&str]) -> Option<String> {
    let out = Command::new("stty").args(args).output().ok()?;
    if out.status.success() {
        Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        None
    }
}

fn history_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join(".corex_history"))
}

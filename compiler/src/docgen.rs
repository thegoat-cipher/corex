//! Markdown documentation generator for `///` doc comments.
//!
//! Doc comments are associated with the next item (attributes are transparent).
//! The output lists the module, functions with signatures, types with fields,
//! enums, constants, and globals.

use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostics::SourceFile;

pub fn generate(file: &SourceFile, program: &Program) -> String {
    let docs = collect_docs(&file.text);
    let mut out = String::new();
    out.push_str(&format!("# `{}`\n\n", file.name));
    if let Some(module) = &program.module_name {
        out.push_str(&format!("Module `{module}`.\n\n"));
    }
    if !program.mod_deps.is_empty() {
        out.push_str("Dependencies: ");
        let deps: Vec<String> = program
            .mod_deps
            .iter()
            .map(|d| format!("`{d}`"))
            .collect();
        out.push_str(&deps.join(", "));
        out.push_str("\n\n");
    }

    let line_of = |offset: usize| file.location(offset).0;
    let doc_for = |line: usize| -> String {
        docs.get(&line)
            .map(|lines| {
                let body: Vec<String> = lines
                    .iter()
                    .map(|l| l.trim_start_matches("///").trim().to_string())
                    .collect();
                format!("{}\n\n", body.join("\n"))
            })
            .unwrap_or_default()
    };

    if !program.typedefs.is_empty() {
        out.push_str("## Type aliases\n\n");
        for t in &program.typedefs {
            out.push_str(&format!("### `typedef {} = {}`\n\n", t.name, t.ty.display()));
            out.push_str(&doc_for(line_of(t.name_span.start)));
        }
    }

    if !program.enums.is_empty() {
        out.push_str("## Enums\n\n");
        for e in &program.enums {
            out.push_str(&format!("### `enum {}`\n\n", e.name));
            out.push_str(&doc_for(line_of(e.name_span.start)));
            if !e.variants.is_empty() {
                out.push_str("| Variant |\n|---|\n");
                for v in &e.variants {
                    out.push_str(&format!("| `{}` |\n", v.name));
                }
                out.push('\n');
            }
        }
    }

    if !program.structs.is_empty() {
        out.push_str("## Types\n\n");
        for s in &program.structs {
            let kind = if s.is_union { "union" } else { "struct" };
            out.push_str(&format!("### `{kind} {}`\n\n", s.name));
            out.push_str(&doc_for(line_of(s.name_span.start)));
            if !s.fields.is_empty() {
                out.push_str("| Field | Type |\n|---|---|\n");
                for f in &s.fields {
                    let ty = if f.bit_width > 0 {
                        format!("{} : {}", f.ty.display(), f.bit_width)
                    } else {
                        f.ty.display()
                    };
                    out.push_str(&format!("| `{}` | `{}` |\n", f.name, ty));
                }
                out.push('\n');
            }
        }
    }

    if !program.consts.is_empty() {
        out.push_str("## Constants\n\n");
        for c in &program.consts {
            out.push_str(&format!("### `const {}: {}`\n\n", c.name, c.ty.display()));
            out.push_str(&doc_for(line_of(c.name_span.start)));
        }
    }

    if !program.globals.is_empty() {
        out.push_str("## Globals\n\n");
        for g in &program.globals {
            out.push_str(&format!(
                "### `static {}{}: {}`\n\n",
                if g.mutable { "mut " } else { "" },
                g.name,
                g.ty.display()
            ));
            out.push_str(&doc_for(line_of(g.name_span.start)));
        }
    }

    if !program.fns.is_empty() {
        out.push_str("## Functions\n\n");
        for f in &program.fns {
            out.push_str(&format!("### `{}`\n\n", signature(f)));
            out.push_str(&doc_for(line_of(f.name_span.start)));
        }
    }

    out
}

fn signature(f: &FnDecl) -> String {
    let mut params: Vec<String> = f
        .params
        .iter()
        .map(|p| format!("{}: {}", p.name, p.ty.display()))
        .collect();
    if f.variadic {
        params.push("...".to_string());
    }
    let abi = match f.extern_abi.as_deref() {
        Some("C") => "extern \"C\" ",
        Some("corex") => "extern \"corex\" ",
        _ => "",
    };
    let ret = if f.ret == Type::Unit {
        String::new()
    } else {
        format!(" -> {}", f.ret.display())
    };
    format!("{abi}fn {}({}){ret}", f.name, params.join(", "))
}

/// Map item line number -> contiguous `///` doc comment lines.
fn collect_docs(source: &str) -> HashMap<usize, Vec<String>> {
    let mut docs: HashMap<usize, Vec<String>> = HashMap::new();
    let mut pending: Vec<String> = Vec::new();
    let mut in_block_comment = false;

    for (index, raw) in source.lines().enumerate() {
        let line = raw.trim();
        if in_block_comment {
            if line.contains("*/") {
                in_block_comment = false;
            }
            continue;
        }
        if line.starts_with("///") {
            pending.push(line.to_string());
            continue;
        }
        if line.starts_with("//") || line.is_empty() {
            if line.is_empty() && !pending.is_empty() {
                // Blank lines inside a doc block are allowed; blank between
                // doc and item breaks the association.
            }
            continue;
        }
        if line.starts_with("#[") {
            // Attributes stay between doc comments and the item.
            continue;
        }
        if line.starts_with("/*") {
            if !line.contains("*/") {
                in_block_comment = true;
            }
            continue;
        }
        if !pending.is_empty() {
            docs.insert(index + 1, std::mem::take(&mut pending));
        }
    }
    docs
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;
    use crate::parser;

    #[test]
    fn generates_docs_for_items() {
        let source = r#"
/// Adds two numbers.
///
/// Returns the sum.
pub fn add(a: i32, b: i32) -> i32 { return a + b; }

/// A point in 2D.
pub struct Point {
    /// Horizontal coordinate.
    x: i32,
    y: i32,
}

/// A counter.
static COUNTER: i32 = 0;
"#;
        let file = SourceFile::new("test.cx", source);
        let program = parser::parse(&lexer::lex(source).unwrap()).unwrap();
        let markdown = generate(&file, &program);
        assert!(markdown.contains("### `fn add(a: i32, b: i32) -> i32`"));
        assert!(markdown.contains("Adds two numbers."));
        assert!(markdown.contains("Returns the sum."));
        assert!(markdown.contains("### `struct Point`"));
        assert!(markdown.contains("| `x` | `i32` |"));
        assert!(markdown.contains("### `static COUNTER: i32`"));
    }
}

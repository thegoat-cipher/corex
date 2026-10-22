//! Recursive-descent parser with precedence climbing.
//!
//! Grammar is defined in `docs/grammar.ebnf`. The parser recovers at statement
//! and function granularity so one mistake does not hide the rest of the file.

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};
use crate::token::{Token, TokenKind};

/// Maximum syntactic nesting depth. Bounds parser recursion (and therefore
/// downstream AST recursion in semantic analysis and code generation) so that
/// adversarial input produces a diagnostic instead of a stack overflow.
const MAX_NESTING: usize = 128;

pub fn parse(tokens: &[Token]) -> Result<Program, Vec<Diagnostic>> {
    parse_with_cfg(tokens, &[])
}

/// Parse with a set of enabled `#[cfg]` keys, e.g. `feature=fast` or
/// `target_os=linux`.
pub fn parse_with_cfg(tokens: &[Token], cfg: &[String]) -> Result<Program, Vec<Diagnostic>> {
    parse_inner(tokens, cfg, false)
}

/// Parse for module-dependency discovery. The driver parses each unit in
/// isolation before any generated interfaces exist, so an array length that
/// names a `const` from another module cannot be resolved yet. These ASTs are
/// discarded after `mod`/`module` extraction; the real compile re-parses with
/// the interfaces prepended and reports unresolved lengths there.
pub fn parse_for_discovery(
    tokens: &[Token],
    cfg: &[String],
) -> Result<Program, Vec<Diagnostic>> {
    parse_inner(tokens, cfg, true)
}

fn parse_inner(
    tokens: &[Token],
    cfg: &[String],
    lenient_array_lengths: bool,
) -> Result<Program, Vec<Diagnostic>> {
    let mut p = Parser {
        tokens,
        pos: 0,
        diags: Vec::new(),
        next_id: 0,
        depth: 0,
        cfg: cfg.to_vec(),
        macros: std::collections::HashMap::new(),
        macro_depth: 0,
        const_ints: std::collections::HashMap::new(),
        vec_depth: 0,
        pending_gt: 0,
        lenient_array_lengths,
    };
    let mut program = Program {
        module_name: None,
        mod_deps: Vec::new(),
        imports: Vec::new(),
        fns: Vec::new(),
        typedefs: Vec::new(),
        macros: Vec::new(),
        globals: Vec::new(),
        consts: Vec::new(),
        enums: Vec::new(),
        structs: Vec::new(),
    };
    while !p.at(&TokenKind::Eof) && p.diags.len() < 100 {
        let before = p.pos;
        if p.parse_item(&mut program).is_err() {
            // `parse_item` records the diagnostic; recover at the next item.
        }
        if p.pos == before {
            p.advance();
        }
        p.recover_to_item();
    }
    if p.diags.is_empty() {
        Ok(program)
    } else {
        Err(p.diags)
    }
}

struct Parser<'a> {
    tokens: &'a [Token],
    pos: usize,
    diags: Vec<Diagnostic>,
    next_id: NodeId,
    depth: usize,
    cfg: Vec<String>,
    macros: std::collections::HashMap<String, MacroDecl>,
    macro_depth: usize,
    /// Integer `const` values seen so far, used for array lengths. Only
    /// previously declared constants are visible, matching the forward-reference
    /// rule for constant expressions.
    const_ints: std::collections::HashMap<String, u64>,
    /// Nesting depth of `vec<...>` types, used to split `>>` into two closers.
    vec_depth: usize,
    /// Extra `>` closers produced by splitting a `>>` token in type position.
    pending_gt: u32,
    /// Accept unresolved array lengths without a diagnostic (discovery parse).
    lenient_array_lengths: bool,
}

#[derive(Default, Clone, Copy)]
struct StructAttrs {
    packed: bool,
    align: u64,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
    }

    fn peek_at(&self, offset: usize) -> &TokenKind {
        self.tokens
            .get(self.pos + offset)
            .map(|t| &t.kind)
            .unwrap_or(&TokenKind::Eof)
    }

    fn peek_span(&self) -> Span {
        self.tokens[self.pos].span
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek() == kind
    }

    fn advance(&mut self) -> Token {
        let t = self.tokens[self.pos].clone();
        if self.pos + 1 < self.tokens.len() {
            self.pos += 1;
        }
        t
    }

    fn error<T>(&mut self, code: &'static str, span: Span, message: impl Into<String>) -> Result<T, ()> {
        self.diags.push(Diagnostic::error(code, span, message));
        Err(())
    }

    // ----- items -------------------------------------------------------

    fn parse_item(&mut self, program: &mut Program) -> Result<(), ()> {
        let enabled = self.parse_cfg_attrs()?;
        if !enabled {
            // Parse the item for recovery, then discard its declarations.
            let mut scratch = Program {
                module_name: None,
                mod_deps: Vec::new(),
                imports: Vec::new(),
                fns: Vec::new(),
                typedefs: Vec::new(),
                macros: Vec::new(),
                globals: Vec::new(),
                consts: Vec::new(),
                enums: Vec::new(),
                structs: Vec::new(),
            };
            let result = self.parse_item_inner(&mut scratch);
            if result.is_err() {
                self.skip_stmt();
            }
            return result;
        }
        self.parse_item_inner(program)
    }

    /// Parse zero or more `#[cfg(...)]` attributes. Returns false when a
    /// condition evaluates to false and the item must be skipped.
    fn parse_cfg_attrs(&mut self) -> Result<bool, ()> {
        let mut enabled = true;
        while self.at(&TokenKind::Hash) {
            self.advance();
            self.expect(&TokenKind::LBracket, "after `#`")?;
            let (name, span) = self.expect_ident("attribute name")?;
            if name != "cfg" {
                let _: Result<(), ()> =
                    self.error("E0100", span, format!("unknown attribute `{name}`"));
                // Skip to `]` so parsing can continue.
                while !self.at(&TokenKind::RBracket) && !self.at(&TokenKind::Eof) {
                    self.advance();
                }
                self.expect(&TokenKind::RBracket, "after attribute")?;
                continue;
            }
            self.expect(&TokenKind::LParen, "after `cfg`")?;
            let value = self.parse_cfg_expr()?;
            self.expect(&TokenKind::RParen, "after cfg expression")?;
            self.expect(&TokenKind::RBracket, "after cfg attribute")?;
            if !value {
                enabled = false;
            }
        }
        Ok(enabled)
    }

    fn parse_cfg_expr(&mut self) -> Result<bool, ()> {
        let (name, span) = self.expect_ident("cfg key")?;
        match name.as_str() {
            "not" => {
                self.expect(&TokenKind::LParen, "after `not`")?;
                let inner = self.parse_cfg_expr()?;
                self.expect(&TokenKind::RParen, "after `not` expression")?;
                Ok(!inner)
            }
            "all" | "any" => {
                self.expect(&TokenKind::LParen, "after cfg combinator")?;
                let mut values = Vec::new();
                while !self.at(&TokenKind::RParen) {
                    values.push(self.parse_cfg_expr()?);
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "after cfg combinator")?;
                Ok(if name == "all" {
                    values.iter().all(|v| *v)
                } else {
                    values.iter().any(|v| *v)
                })
            }
            key => {
                self.expect(&TokenKind::Assign, "after cfg key")?;
                let value = match self.peek().clone() {
                    TokenKind::Str(bytes) => {
                        self.advance();
                        String::from_utf8_lossy(&bytes).to_string()
                    }
                    other => {
                        let span = self.peek_span();
                        return self.error(
                            "E0100",
                            span,
                            format!("expected a string cfg value, found {}", other.describe()),
                        );
                    }
                };
                let _ = span;
                Ok(self.cfg.iter().any(|k| k == &format!("{key}={value}")))
            }
        }
    }

    fn parse_item_inner(&mut self, program: &mut Program) -> Result<(), ()> {
        let start_span = self.peek_span();
        let public = if self.at(&TokenKind::Pub) {
            self.advance();
            true
        } else {
            false
        };

        // Layout attributes may precede `struct` or `union` in any order.
        let mut attrs = StructAttrs::default();
        loop {
            match self.peek() {
                TokenKind::Repr => {
                    self.advance();
                    self.expect(&TokenKind::LParen, "after `repr`")?;
                    let (name, _) = self.expect_ident("representation name")?;
                    if name != "C" && name != "c" {
                        return self.error(
                            "E0100",
                            self.peek_span(),
                            "only `repr(C)` is supported",
                        );
                    }
                    self.expect(&TokenKind::RParen, "after representation name")?;
                }
                TokenKind::Packed => {
                    self.advance();
                    attrs.packed = true;
                }
                TokenKind::Align => {
                    self.advance();
                    self.expect(&TokenKind::LParen, "after `align`")?;
                    let n = match self.peek().clone() {
                        TokenKind::Int(v) => {
                            self.advance();
                            v
                        }
                        other => {
                            let span = self.peek_span();
                            return self.error(
                                "E0100",
                                span,
                                format!("expected alignment value, found {}", other.describe()),
                            );
                        }
                    };
                    if n == 0 || !n.is_power_of_two() {
                        return self.error(
                            "E0110",
                            self.peek_span(),
                            "alignment must be a power of two",
                        );
                    }
                    self.expect(&TokenKind::RParen, "after alignment value")?;
                    attrs.align = n;
                }
                _ => break,
            }
        }

        match self.peek().clone() {
            TokenKind::Macro => {
                if public {
                    return self.error("E0100", start_span, "`pub` does not apply to `macro`");
                }
                let kw = self.advance();
                let (name, _name_span) = self.expect_ident("macro name")?;
                self.expect(&TokenKind::LParen, "after macro name")?;
                let mut params = Vec::new();
                while !self.at(&TokenKind::RParen) {
                    let (param, _) = self.expect_ident("macro parameter")?;
                    params.push(param);
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "after macro parameters")?;
                self.expect(&TokenKind::Assign, "before macro body")?;
                let body = self.parse_expr()?;
                let semi = self.expect(&TokenKind::Semi, "after macro body")?;
                let decl = MacroDecl {
                    name: name.clone(),
                    params,
                    body,
                    span: kw.span.merge(semi.span),
                };
                self.macros.insert(name, decl.clone());
                program.macros.push(decl);
                Ok(())
            }
            TokenKind::Module => {
                if public {
                    return self.error("E0100", start_span, "`pub` does not apply to `module`");
                }
                self.advance();
                let (name, _) = self.expect_ident("module name")?;
                self.expect(&TokenKind::Semi, "after module name")?;
                if program.module_name.is_some() {
                    return self.error(
                        "E0202",
                        start_span,
                        "a file may declare only one `module`",
                    );
                }
                program.module_name = Some(name);
                Ok(())
            }
            TokenKind::Mod => {
                self.advance();
                let (name, _) = self.expect_ident("module dependency name")?;
                if self.at(&TokenKind::LBrace) {
                    return self.error(
                        "E0100",
                        self.peek_span(),
                        "inline modules are not supported; put the module in its own file and write `mod name;`",
                    );
                }
                self.expect(&TokenKind::Semi, "after module dependency")?;
                if !program.mod_deps.contains(&name) {
                    program.mod_deps.push(name);
                }
                Ok(())
            }
            TokenKind::Fn => {
                let f = self.parse_fn(public, None)?;
                program.fns.push(f);
                Ok(())
            }
            TokenKind::Import => {
                let import = self.parse_import()?;
                program.imports.push(import);
                Ok(())
            }
            TokenKind::Static => {
                let g = self.parse_global(public, false)?;
                program.globals.push(g);
                Ok(())
            }
            TokenKind::Const => {
                let c = self.parse_const(public)?;
                program.consts.push(c);
                Ok(())
            }
            TokenKind::Enum => {
                let e = self.parse_enum(public)?;
                program.enums.push(e);
                Ok(())
            }
            TokenKind::Struct => {
                let st = self.parse_struct(public, attrs, false)?;
                program.structs.push(st);
                Ok(())
            }
            TokenKind::Union => {
                let st = self.parse_struct(public, attrs, true)?;
                program.structs.push(st);
                Ok(())
            }
            TokenKind::Typedef => {
                let t = self.parse_typedef(public)?;
                program.typedefs.push(t);
                Ok(())
            }
            TokenKind::Extern => self.parse_extern(program),
            _ => {
                let found = self.peek().describe();
                self.error(
                    "E0100",
                    start_span,
                    format!(
                        "expected an item (`fn`, `static`, `const`, `enum`, `struct`, `union`, `typedef`, or `extern`), found {found}"
                    ),
                )
            }
        }
    }

    fn parse_extern(&mut self, program: &mut Program) -> Result<(), ()> {
        self.expect(&TokenKind::Extern, "start of extern declaration")?;
        let mut abi = "C".to_string();
        if let TokenKind::Str(bytes) = self.peek().clone() {
            self.advance();
            abi = String::from_utf8(bytes).map_err(|_| {
                self.diags.push(Diagnostic::error(
                    "E0232",
                    self.peek_span(),
                    "ABI name must be valid UTF-8",
                ));
            })?;
            if abi != "C" && abi != "corex" {
                return self.error(
                    "E0100",
                    self.peek_span(),
                    format!(
                        "unsupported ABI `{abi}`; expected `extern \"C\"` or `extern \"corex\"`"
                    ),
                );
            }
        }
        match self.peek().clone() {
            TokenKind::Fn => {
                let f = self.parse_fn(false, Some(abi))?;
                program.fns.push(f);
                Ok(())
            }
            TokenKind::Static => {
                let g = self.parse_global(false, true)?;
                program.globals.push(g);
                Ok(())
            }
            other => {
                let span = self.peek_span();
                self.error(
                    "E0100",
                    span,
                    format!(
                        "expected `fn` or `static` after `extern`, found {}",
                        other.describe()
                    ),
                )
            }
        }
    }

    fn expect(&mut self, kind: &TokenKind, context: &str) -> Result<Token, ()> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            let span = self.peek_span();
            let found = self.peek().describe();
            self.error(
                "E0100",
                span,
                format!("expected `{}`, found {} ({context})", kind.symbol(), found),
            )
        }
    }

    fn expect_ident(&mut self, context: &str) -> Result<(String, Span), ()> {
        match self.peek().clone() {
            TokenKind::Ident(name) => {
                let span = self.advance().span;
                Ok((name, span))
            }
            other => {
                let span = self.peek_span();
                self.error(
                    "E0100",
                    span,
                    format!("expected identifier, found {} ({context})", other.describe()),
                )
            }
        }
    }

    /// Consume one `>` closing a `vec<...>` type. A `>>` token closes two
    /// nested `vec`s (`vec<vec<u8>>`); the second `>` is remembered until the
    /// enclosing `vec` asks for it. The lexer is untouched, so `>>` and `>>=`
    /// stay shift operators in expressions.
    fn expect_vec_gt(&mut self, context: &str) -> Result<(), ()> {
        if self.pending_gt > 0 {
            self.pending_gt -= 1;
            return Ok(());
        }
        match self.peek() {
            TokenKind::Gt => {
                self.advance();
                Ok(())
            }
            TokenKind::Shr if self.vec_depth > 1 => {
                self.advance();
                self.pending_gt += 1;
                Ok(())
            }
            other => {
                let span = self.peek_span();
                let found = other.describe();
                self.error(
                    "E0100",
                    span,
                    format!("expected `>`, found {found} ({context})"),
                )
            }
        }
    }

    fn recover_to_item(&mut self) {
        while !matches!(
            self.peek(),
            TokenKind::Fn
                | TokenKind::Static
                | TokenKind::Const
                | TokenKind::Import
                | TokenKind::Enum
                | TokenKind::Struct
                | TokenKind::Union
                | TokenKind::Typedef
                | TokenKind::Extern
                | TokenKind::Pub
                | TokenKind::Mod
                | TokenKind::Module
                | TokenKind::Macro
                | TokenKind::Hash
                | TokenKind::Repr
                | TokenKind::Packed
                | TokenKind::Align
                | TokenKind::Eof
        ) {
            self.advance();
        }
    }

    fn skip_stmt(&mut self) {
        loop {
            match self.peek() {
                TokenKind::Semi => {
                    self.advance();
                    return;
                }
                TokenKind::RBrace | TokenKind::Eof => return,
                _ => {
                    self.advance();
                }
            }
        }
    }

    fn too_deep<T>(&mut self, span: Span) -> Result<T, ()> {
        self.error(
            "E0111",
            span,
            format!("nesting depth exceeds the limit of {MAX_NESTING}"),
        )
    }

    fn mk_expr(&mut self, kind: ExprKind, span: Span) -> Expr {
        let id = self.next_id;
        self.next_id += 1;
        Expr { id, kind, span }
    }

    // ----- items -------------------------------------------------------

    fn parse_fn(&mut self, public: bool, extern_abi: Option<String>) -> Result<FnDecl, ()> {
        let start = self.expect(&TokenKind::Fn, "start of function")?.span.start;
        let (name, name_span) = self.expect_ident("function name")?;
        self.expect(&TokenKind::LParen, "after function name")?;

        let mut params = Vec::new();
        let mut variadic = false;
        while !self.at(&TokenKind::RParen) {
            if self.at(&TokenKind::DotDotDot) {
                self.advance();
                variadic = true;
                if self.at(&TokenKind::Comma) {
                    self.advance();
                }
                break;
            }
            let (pname, pspan) = self.expect_ident("parameter name")?;
            self.expect(&TokenKind::Colon, "after parameter name")?;
            let ty = self.parse_type()?;
            params.push(Param {
                name: pname,
                span: pspan,
                ty,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        self.expect(&TokenKind::RParen, "after parameters")?;

        let (ret, ret_span) = if self.at(&TokenKind::Arrow) {
            let arrow = self.advance();
            let ty = self.parse_type()?;
            (ty, arrow.span.merge(self.tokens[self.pos - 1].span))
        } else {
            (Type::Unit, self.tokens[self.pos - 1].span)
        };

        let (body, end) = if self.at(&TokenKind::LBrace) {
            let body = self.parse_block()?;
            let end = body.span.end;
            (Some(body), end)
        } else if extern_abi.is_some() {
            let semi = self.expect(&TokenKind::Semi, "after extern function declaration")?;
            (None, semi.span.end)
        } else {
            return self.error(
                "E0100",
                self.peek_span(),
                "expected `{` to start the function body",
            );
        };

        Ok(FnDecl {
            name,
            name_span,
            params,
            ret,
            ret_span,
            body,
            span: Span::new(start, end),
            extern_abi,
            variadic,
            public,
        })
    }

    fn parse_type(&mut self) -> Result<Type, ()> {
        if self.depth >= MAX_NESTING {
            return self.too_deep(self.peek_span());
        }
        self.depth += 1;
        let result = self.parse_type_inner();
        self.depth -= 1;
        result
    }

    fn parse_type_inner(&mut self) -> Result<Type, ()> {
        let kind = self.peek().clone();
        let ty = match kind {
            TokenKind::I8 => Type::Int(IntTy::I8),
            TokenKind::I16 => Type::Int(IntTy::I16),
            TokenKind::I32 => Type::Int(IntTy::I32),
            TokenKind::I64 => Type::Int(IntTy::I64),
            TokenKind::U8 => Type::Int(IntTy::U8),
            TokenKind::U16 => Type::Int(IntTy::U16),
            TokenKind::U32 => Type::Int(IntTy::U32),
            TokenKind::U64 => Type::Int(IntTy::U64),
            TokenKind::Usize => Type::Int(IntTy::Usize),
            TokenKind::Isize => Type::Int(IntTy::Isize),
            TokenKind::Bool => Type::Bool,
            TokenKind::Char => Type::Char,
            TokenKind::F32 => Type::Float(FloatTy::F32),
            TokenKind::F64 => Type::Float(FloatTy::F64),
            TokenKind::StrType => Type::Str,
            TokenKind::CStrType => Type::CStr,
            TokenKind::Fn => {
                self.advance();
                self.expect(&TokenKind::LParen, "after `fn` in a function type")?;
                let mut params = Vec::new();
                let mut variadic = false;
                while !self.at(&TokenKind::RParen) {
                    if self.at(&TokenKind::DotDotDot) {
                        self.advance();
                        variadic = true;
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                        }
                        break;
                    }
                    params.push(self.parse_type()?);
                    if self.at(&TokenKind::Comma) {
                        self.advance();
                    } else {
                        break;
                    }
                }
                self.expect(&TokenKind::RParen, "after function type parameters")?;
                let ret = if self.at(&TokenKind::Arrow) {
                    self.advance();
                    self.parse_type()?
                } else {
                    Type::Unit
                };
                Type::Fn(FnType {
                    params,
                    ret: Box::new(ret),
                    variadic,
                })
            }
            TokenKind::VecType => {
                self.advance();
                self.expect(&TokenKind::Lt, "after `vec`")?;
                self.vec_depth += 1;
                let elem = match self.parse_type() {
                    Ok(t) => t,
                    Err(e) => {
                        self.vec_depth -= 1;
                        return Err(e);
                    }
                };
                self.expect_vec_gt("after `vec` element type")?;
                self.vec_depth -= 1;
                Type::Vec(Box::new(elem))
            }
            TokenKind::Star => {
                self.advance();
                let mutable = if self.at(&TokenKind::Mut) {
                    self.advance();
                    true
                } else {
                    false
                };
                let pointee = self.parse_type()?;
                Type::ptr_to(pointee, mutable)
            }
            TokenKind::Ident(ref name) => {
                self.advance();
                let mut path = name.clone();
                while self.at(&TokenKind::ColonColon) {
                    self.advance();
                    let (seg, _) = self.expect_ident("type name segment")?;
                    path.push_str("::");
                    path.push_str(&seg);
                }
                Type::Named(path)
            }
            TokenKind::LBracket => {
                self.advance();
                let elem = self.parse_type()?;
                if self.at(&TokenKind::RBracket) {
                    self.advance();
                    return Ok(Type::Slice(Box::new(elem)));
                }
                self.expect(&TokenKind::Semi, "inside array type")?;
                let len_expr = self.parse_expr()?;
                let len = match const_int_expr(&len_expr, &self.const_ints) {
                    Some(len) => len,
                    None if self.lenient_array_lengths => 0,
                    None => {
                        return self.error(
                            "E0100",
                            len_expr.span,
                            "array length must be a constant integer expression (literal or previously declared `const`)",
                        );
                    }
                };
                self.expect(&TokenKind::RBracket, "after array length")?;
                // Zero-length arrays are allowed as flexible array members; the
                // semantic pass restricts them to the last field of a struct.
                Type::array_of(elem, len)
            }
            other => {
                let span = self.peek_span();
                return self.error(
                    "E0100",
                    span,
                    format!("expected type, found {}", other.describe()),
                );
            }
        };
        self.advance_if_type_token(&kind);
        Ok(ty)
    }

    /// Primitive type tokens are consumed here; composite types were consumed
    /// by the branches above.
    fn advance_if_type_token(&mut self, original: &TokenKind) {
        if matches!(
            original,
            TokenKind::I8
                | TokenKind::I16
                | TokenKind::I32
                | TokenKind::I64
                | TokenKind::U8
                | TokenKind::U16
                | TokenKind::U32
                | TokenKind::U64
                | TokenKind::Usize
                | TokenKind::Isize
                | TokenKind::Bool
                | TokenKind::Char
                | TokenKind::F32
                | TokenKind::F64
                | TokenKind::StrType
                | TokenKind::CStrType
        ) {
            self.advance();
        }
    }

    // ----- statements --------------------------------------------------

    fn parse_block(&mut self) -> Result<Block, ()> {
        if self.depth >= MAX_NESTING {
            return self.too_deep(self.peek_span());
        }
        self.depth += 1;
        let result = self.parse_block_inner();
        self.depth -= 1;
        result
    }

    fn parse_block_inner(&mut self) -> Result<Block, ()> {
        let open = self.expect(&TokenKind::LBrace, "start of block")?;
        let mut stmts = Vec::new();
        loop {
            if self.at(&TokenKind::RBrace) || self.at(&TokenKind::Eof) {
                break;
            }
            match self.parse_stmt() {
                Ok(s) => stmts.push(s),
                Err(()) => self.skip_stmt(),
            }
        }
        let close = self.expect(&TokenKind::RBrace, "end of block")?;
        Ok(Block {
            stmts,
            span: open.span.merge(close.span),
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ()> {
        // `name: stmt` is a label; no expression statement can start this way.
        if matches!(self.peek(), TokenKind::Ident(_)) && self.peek_at(1) == &TokenKind::Colon {
            let (name, span) = self.expect_ident("label name")?;
            self.expect(&TokenKind::Colon, "after label name")?;
            let stmt = self.parse_stmt()?;
            let full = span.merge(stmt.span());
            return Ok(Stmt::Label {
                name,
                stmt: Box::new(stmt),
                span: full,
            });
        }
        match self.peek().clone() {
            TokenKind::Let => self.parse_let(),
            TokenKind::If => self.parse_if(),
            TokenKind::Switch => self.parse_switch(),
            TokenKind::For => self.parse_for(),
            TokenKind::Do => self.parse_do_while(),
            TokenKind::Goto => {
                let kw = self.advance();
                let (label, _) = self.expect_ident("goto target")?;
                let semi = self.expect(&TokenKind::Semi, "after `goto` target")?;
                Ok(Stmt::Goto {
                    label,
                    span: kw.span.merge(semi.span),
                })
            }
            TokenKind::While => {
                let kw = self.advance();
                let cond = self.parse_expr()?;
                let body = self.parse_block()?;
                let span = kw.span.merge(body.span);
                Ok(Stmt::While { cond, body, span })
            }
            TokenKind::Loop => {
                let kw = self.advance();
                let body = self.parse_block()?;
                let span = kw.span.merge(body.span);
                Ok(Stmt::Loop { body, span })
            }
            TokenKind::Break => {
                let kw = self.advance();
                self.expect(&TokenKind::Semi, "after `break`")?;
                Ok(Stmt::Break(kw.span))
            }
            TokenKind::Continue => {
                let kw = self.advance();
                self.expect(&TokenKind::Semi, "after `continue`")?;
                Ok(Stmt::Continue(kw.span))
            }
            TokenKind::Return => {
                let kw = self.advance();
                if self.at(&TokenKind::Semi) {
                    let semi = self.advance();
                    Ok(Stmt::Return {
                        value: None,
                        span: kw.span.merge(semi.span),
                    })
                } else {
                    let value = self.parse_expr()?;
                    let semi = self.expect(&TokenKind::Semi, "after return value")?;
                    Ok(Stmt::Return {
                        value: Some(value),
                        span: kw.span.merge(semi.span),
                    })
                }
            }
            TokenKind::Unsafe => {
                let kw = self.advance();
                let body = self.parse_block()?;
                let span = kw.span.merge(body.span);
                Ok(Stmt::Unsafe { body, span })
            }
            TokenKind::Semi => {
                let semi = self.advance();
                Ok(Stmt::Empty(semi.span))
            }
            TokenKind::LBrace => Ok(Stmt::Block(self.parse_block()?)),
            _ => {
                let target = self.parse_expr()?;
                let compound = match self.peek() {
                    TokenKind::PlusEq => Some(BinOp::Add),
                    TokenKind::MinusEq => Some(BinOp::Sub),
                    TokenKind::StarEq => Some(BinOp::Mul),
                    TokenKind::SlashEq => Some(BinOp::Div),
                    TokenKind::PercentEq => Some(BinOp::Rem),
                    TokenKind::AmpEq => Some(BinOp::BitAnd),
                    TokenKind::PipeEq => Some(BinOp::BitOr),
                    TokenKind::CaretEq => Some(BinOp::BitXor),
                    TokenKind::ShlEq => Some(BinOp::Shl),
                    TokenKind::ShrEq => Some(BinOp::Shr),
                    _ => None,
                };
                if let Some(op) = compound {
                    self.advance();
                    let value = self.parse_expr()?;
                    let semi = self.expect(&TokenKind::Semi, "after compound assignment")?;
                    let span = target.span.merge(semi.span);
                    Ok(Stmt::CompoundAssign {
                        target,
                        op,
                        value,
                        span,
                    })
                } else if self.at(&TokenKind::Assign) {
                    self.advance();
                    let value = self.parse_expr()?;
                    let semi = self.expect(&TokenKind::Semi, "after assignment")?;
                    let span = target.span.merge(semi.span);
                    Ok(Stmt::Assign {
                        target,
                        value,
                        span,
                    })
                } else {
                    let semi = self.expect(&TokenKind::Semi, "after expression")?;
                    let span = target.span.merge(semi.span);
                    Ok(Stmt::Expr(Expr { span, ..target }))
                }
            }
        }
    }

    fn parse_let(&mut self) -> Result<Stmt, ()> {
        let kw = self.advance();
        let mutable = if self.at(&TokenKind::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let (name, name_span) = self.expect_ident("binding name")?;
        let ty = if self.at(&TokenKind::Colon) {
            self.advance();
            Some(self.parse_type()?)
        } else {
            None
        };
        self.expect(&TokenKind::Assign, "before binding initializer")?;
        let init = self.parse_expr()?;
        let semi = self.expect(&TokenKind::Semi, "after binding initializer")?;
        Ok(Stmt::Let {
            name,
            name_span,
            ty,
            mutable,
            init,
            span: kw.span.merge(semi.span),
        })
    }

    fn parse_if(&mut self) -> Result<Stmt, ()> {
        let kw = self.advance();
        let cond = self.parse_expr()?;
        let then_block = self.parse_block()?;
        let mut end = then_block.span;
        let else_branch = if self.at(&TokenKind::Else) {
            self.advance();
            // grammar.ebnf: `else` takes an `if` chain or a block, nothing else.
            let branch = if self.at(&TokenKind::If) {
                self.parse_if()?
            } else if self.at(&TokenKind::LBrace) {
                Stmt::Block(self.parse_block()?)
            } else {
                let found = self.peek().describe();
                return self.error(
                    "E0100",
                    self.peek_span(),
                    format!("expected `if` or `{{` after `else`, found {found}"),
                );
            };
            end = branch.span();
            Some(Box::new(branch))
        } else {
            None
        };
        Ok(Stmt::If {
            cond,
            then_block,
            else_branch,
            span: kw.span.merge(end),
        })
    }

    fn parse_switch(&mut self) -> Result<Stmt, ()> {
        let kw = self.expect(&TokenKind::Switch, "start of switch")?;
        let value = self.parse_expr()?;
        self.expect(&TokenKind::LBrace, "after switch value")?;
        let mut arms = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let arm_span = self.peek_span();
            if self.at(&TokenKind::Case) {
                self.advance();
                let case_value = self.parse_expr()?;
                if self.at(&TokenKind::Colon) {
                    self.advance();
                }
                let body = self.parse_block()?;
                let span = arm_span.merge(body.span);
                arms.push(SwitchArm {
                    value: Some(case_value),
                    body,
                    span,
                });
            } else if self.at(&TokenKind::Default) {
                self.advance();
                if self.at(&TokenKind::Colon) {
                    self.advance();
                }
                let body = self.parse_block()?;
                let span = arm_span.merge(body.span);
                arms.push(SwitchArm {
                    value: None,
                    body,
                    span,
                });
            } else {
                let found = self.peek().describe();
                return self.error(
                    "E0100",
                    self.peek_span(),
                    format!("expected `case` or `default` in switch, found {found}"),
                );
            }
        }
        let close = self.expect(&TokenKind::RBrace, "after switch arms")?;
        Ok(Stmt::Switch {
            value,
            arms,
            span: kw.span.merge(close.span),
        })
    }

    fn parse_for(&mut self) -> Result<Stmt, ()> {
        let kw = self.expect(&TokenKind::For, "start of for loop")?;
        self.expect(&TokenKind::LParen, "after `for`")?;
        let init = if self.at(&TokenKind::Semi) {
            self.advance();
            None
        } else if self.at(&TokenKind::Let) {
            Some(Box::new(self.parse_let()?))
        } else {
            let start = self.peek_span();
            let e = self.parse_expr()?;
            let semi = self.expect(&TokenKind::Semi, "after for initializer")?;
            Some(Box::new(Stmt::Expr(Expr {
                span: start.merge(semi.span),
                ..e
            })))
        };
        let cond = if self.at(&TokenKind::Semi) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(&TokenKind::Semi, "after for condition")?;
        let step = if self.at(&TokenKind::RParen) {
            None
        } else {
            Some(Box::new(self.parse_for_step()?))
        };
        self.expect(&TokenKind::RParen, "after for clauses")?;
        let body = self.parse_block()?;
        let span = kw.span.merge(body.span);
        Ok(Stmt::For {
            init,
            cond,
            step,
            body,
            span,
        })
    }

    /// A `for` initializer or step: an expression, assignment, or compound
    /// assignment. The trailing `;`/`)` is left to the caller.
    fn parse_for_step(&mut self) -> Result<Stmt, ()> {
        let start = self.peek_span();
        let target = self.parse_expr()?;
        let compound = match self.peek() {
            TokenKind::PlusEq => Some(BinOp::Add),
            TokenKind::MinusEq => Some(BinOp::Sub),
            TokenKind::StarEq => Some(BinOp::Mul),
            TokenKind::SlashEq => Some(BinOp::Div),
            TokenKind::PercentEq => Some(BinOp::Rem),
            TokenKind::AmpEq => Some(BinOp::BitAnd),
            TokenKind::PipeEq => Some(BinOp::BitOr),
            TokenKind::CaretEq => Some(BinOp::BitXor),
            TokenKind::ShlEq => Some(BinOp::Shl),
            TokenKind::ShrEq => Some(BinOp::Shr),
            _ => None,
        };
        if let Some(op) = compound {
            self.advance();
            let value = self.parse_expr()?;
            let span = start.merge(value.span);
            Ok(Stmt::CompoundAssign {
                target,
                op,
                value,
                span,
            })
        } else if self.at(&TokenKind::Assign) {
            self.advance();
            let value = self.parse_expr()?;
            let span = start.merge(value.span);
            Ok(Stmt::Assign {
                target,
                value,
                span,
            })
        } else {
            Ok(Stmt::Expr(Expr { span: start, ..target }))
        }
    }

    fn parse_do_while(&mut self) -> Result<Stmt, ()> {
        let kw = self.expect(&TokenKind::Do, "start of do-while")?;
        let body = self.parse_block()?;
        self.expect(&TokenKind::While, "after do block")?;
        let cond = self.parse_expr()?;
        let semi = self.expect(&TokenKind::Semi, "after do-while condition")?;
        Ok(Stmt::DoWhile {
            body,
            cond,
            span: kw.span.merge(semi.span),
        })
    }

    // ----- expressions -------------------------------------------------

    fn parse_global(&mut self, public: bool, is_extern: bool) -> Result<GlobalDecl, ()> {
        let kw = self.expect(&TokenKind::Static, "start of global")?;
        let mutable = if self.at(&TokenKind::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let volatile = if self.at(&TokenKind::Volatile) {
            self.advance();
            true
        } else {
            false
        };
        let (name, name_span) = self.expect_ident("global name")?;
        self.expect(&TokenKind::Colon, "after global name")?;
        let ty = self.parse_type()?;
        let (init, end) = if is_extern {
            let semi = self.expect(&TokenKind::Semi, "after extern global declaration")?;
            (
                Expr {
                    id: 0,
                    kind: ExprKind::Int(0),
                    span: name_span,
                },
                semi.span.end,
            )
        } else {
            self.expect(&TokenKind::Assign, "before global initializer")?;
            let init = self.parse_expr()?;
            let semi = self.expect(&TokenKind::Semi, "after global initializer")?;
            (init, semi.span.end)
        };
        Ok(GlobalDecl {
            name,
            name_span,
            ty,
            mutable,
            init,
            span: Span::new(kw.span.start, end),
            is_extern,
            public,
            volatile,
        })
    }

    fn parse_import(&mut self) -> Result<ImportDecl, ()> {
        let kw = self.expect(&TokenKind::Import, "start of import")?;
        let span = self.peek_span();
        let path = match self.peek().clone() {
            TokenKind::Str(bytes) => {
                self.advance();
                match String::from_utf8(bytes) {
                    Ok(path) => path,
                    Err(_) => {
                        return self.error(
                            "E0232",
                            span,
                            "import path must be valid UTF-8",
                        );
                    }
                }
            }
            other => {
                return self.error(
                    "E0232",
                    span,
                    format!("expected a string import path, found {}", other.describe()),
                );
            }
        };
        let semi = self.expect(&TokenKind::Semi, "after import path")?;
        Ok(ImportDecl {
            path,
            span: kw.span.merge(semi.span),
        })
    }

    fn parse_const(&mut self, public: bool) -> Result<ConstDecl, ()> {
        let kw = self.expect(&TokenKind::Const, "start of constant")?;
        let (name, name_span) = self.expect_ident("constant name")?;
        self.expect(&TokenKind::Colon, "after constant name")?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Assign, "before constant value")?;
        let value = self.parse_expr()?;
        let semi = self.expect(&TokenKind::Semi, "after constant value")?;
        if matches!(ty, Type::Int(_)) {
            if let Some(v) = const_int_expr(&value, &self.const_ints) {
                self.const_ints.insert(name.clone(), v);
            }
        }
        Ok(ConstDecl {
            name,
            name_span,
            ty,
            value,
            span: kw.span.merge(semi.span),
            public,
        })
    }

    fn parse_enum(&mut self, public: bool) -> Result<EnumDecl, ()> {
        let kw = self.expect(&TokenKind::Enum, "start of enum")?;
        let (name, name_span) = self.expect_ident("enum name")?;
        self.expect(&TokenKind::LBrace, "after enum name")?;
        let mut variants = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let (vname, vspan) = self.expect_ident("variant name")?;
            let value = if self.at(&TokenKind::Assign) {
                self.advance();
                Some(self.parse_expr()?)
            } else {
                None
            };
            variants.push(EnumVariant {
                name: vname,
                span: vspan,
                value,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let close = self.expect(&TokenKind::RBrace, "after enum variants")?;
        Ok(EnumDecl {
            name,
            name_span,
            variants,
            span: kw.span.merge(close.span),
            public,
        })
    }

    fn parse_struct(
        &mut self,
        public: bool,
        attrs: StructAttrs,
        is_union: bool,
    ) -> Result<StructDecl, ()> {
        let kw = if is_union {
            self.expect(&TokenKind::Union, "start of union")?
        } else {
            self.expect(&TokenKind::Struct, "start of struct")?
        };
        let (name, name_span) = self.expect_ident("type name")?;
        self.expect(&TokenKind::LBrace, "after type name")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let (fname, fspan) = self.expect_ident("field name")?;
            self.expect(&TokenKind::Colon, "after field name")?;
            let ty = self.parse_type()?;
            let bit_width = if self.at(&TokenKind::Colon) {
                self.advance();
                match self.peek().clone() {
                    TokenKind::Int(v) if v > 0 && v <= 64 => {
                        self.advance();
                        v as u32
                    }
                    other => {
                        let span = self.peek_span();
                        return self.error(
                            "E0110",
                            span,
                            format!(
                                "expected a bitfield width between 1 and 64, found {}",
                                other.describe()
                            ),
                        );
                    }
                }
            } else {
                0
            };
            fields.push(StructFieldDecl {
                name: fname,
                span: fspan,
                ty,
                bit_width,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else if !self.at(&TokenKind::Semi) {
                break;
            } else {
                self.advance();
            }
        }
        let close = self.expect(&TokenKind::RBrace, "after fields")?;
        Ok(StructDecl {
            name,
            name_span,
            fields,
            span: kw.span.merge(close.span),
            is_union,
            packed: attrs.packed,
            align: attrs.align,
            public,
        })
    }

    fn parse_typedef(&mut self, public: bool) -> Result<TypedefDecl, ()> {
        let kw = self.expect(&TokenKind::Typedef, "start of typedef")?;
        let (name, name_span) = self.expect_ident("type alias name")?;
        self.expect(&TokenKind::Assign, "after type alias name")?;
        let ty = self.parse_type()?;
        let semi = self.expect(&TokenKind::Semi, "after typedef")?;
        Ok(TypedefDecl {
            name,
            name_span,
            ty,
            public,
            span: kw.span.merge(semi.span),
        })
    }

    fn expand_macro(
        &mut self,
        mac: &MacroDecl,
        args: Vec<Expr>,
        span: Span,
    ) -> Result<Expr, ()> {
        if args.len() != mac.params.len() {
            return self.error(
                "E0204",
                span,
                format!(
                    "macro `{}` takes {} argument(s), found {}",
                    mac.name,
                    mac.params.len(),
                    args.len()
                ),
            );
        }
        if self.macro_depth >= 64 {
            return self.error("E0111", span, "macro expansion depth exceeds 64");
        }
        let subs: std::collections::HashMap<String, Expr> = mac
            .params
            .iter()
            .cloned()
            .zip(args)
            .collect();
        self.macro_depth += 1;
        let result = self.subst_expr(&mac.body, &subs);
        self.macro_depth -= 1;
        let mut out = result;
        out.span = span;
        Ok(out)
    }

    /// Clone `e` with fresh node ids, replacing macro parameters.
    fn subst_expr(
        &mut self,
        e: &Expr,
        subs: &std::collections::HashMap<String, Expr>,
    ) -> Expr {
        let kind = match &e.kind {
            ExprKind::Ident(name) => {
                if let Some(arg) = subs.get(name) {
                    return self.subst_expr(arg, &std::collections::HashMap::new());
                }
                ExprKind::Ident(name.clone())
            }
            ExprKind::Path(segments) => ExprKind::Path(segments.clone()),
            ExprKind::EnumVariant {
                enum_name,
                variant,
            } => ExprKind::EnumVariant {
                enum_name: enum_name.clone(),
                variant: variant.clone(),
            },
            ExprKind::StructLit { name, fields } => ExprKind::StructLit {
                name: name.clone(),
                fields: fields
                    .iter()
                    .map(|(name, value)| (name.clone(), self.subst_expr(value, subs)))
                    .collect(),
            },
            ExprKind::Field { base, name } => ExprKind::Field {
                base: Box::new(self.subst_expr(base, subs)),
                name: name.clone(),
            },
            ExprKind::Unary { op, expr } => ExprKind::Unary {
                op: *op,
                expr: Box::new(self.subst_expr(expr, subs)),
            },
            ExprKind::Binary { op, lhs, rhs } => ExprKind::Binary {
                op: *op,
                lhs: Box::new(self.subst_expr(lhs, subs)),
                rhs: Box::new(self.subst_expr(rhs, subs)),
            },
            ExprKind::Call { callee, args } => ExprKind::Call {
                callee: Box::new(self.subst_expr(callee, subs)),
                args: args.iter().map(|a| self.subst_expr(a, subs)).collect(),
            },
            ExprKind::Index { base, index } => ExprKind::Index {
                base: Box::new(self.subst_expr(base, subs)),
                index: Box::new(self.subst_expr(index, subs)),
            },
            ExprKind::SliceRange { base, start, end } => ExprKind::SliceRange {
                base: Box::new(self.subst_expr(base, subs)),
                start: start
                    .as_ref()
                    .map(|e| Box::new(self.subst_expr(e, subs))),
                end: end
                    .as_ref()
                    .map(|e| Box::new(self.subst_expr(e, subs))),
            },
            ExprKind::Cast { expr, ty } => ExprKind::Cast {
                expr: Box::new(self.subst_expr(expr, subs)),
                ty: ty.clone(),
            },
            ExprKind::Array(elems) => {
                ExprKind::Array(elems.iter().map(|e| self.subst_expr(e, subs)).collect())
            }
            ExprKind::ArrayRepeat { value, count } => ExprKind::ArrayRepeat {
                value: Box::new(self.subst_expr(value, subs)),
                count: *count,
            },
            ExprKind::CompoundLit { ty, fields } => ExprKind::CompoundLit {
                ty: ty.clone(),
                fields: fields
                    .iter()
                    .map(|(name, value)| (name.clone(), self.subst_expr(value, subs)))
                    .collect(),
            },
            other => other.clone(),
        };
        self.mk_expr(kind, e.span)
    }

    fn looks_like_struct_literal(&self) -> bool {
        // `Name { field: ... }`. Requiring at least one `field:` keeps block
        // statements (`if flag { ... }`) unambiguous.
        matches!(
            self.tokens.get(self.pos).map(|t| &t.kind),
            Some(TokenKind::LBrace)
        ) && matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.kind),
            Some(TokenKind::Ident(_))
        ) && matches!(
            self.tokens.get(self.pos + 2).map(|t| &t.kind),
            Some(TokenKind::Colon)
        )
    }

    fn parse_expr(&mut self) -> Result<Expr, ()> {
        if self.depth >= MAX_NESTING {
            return self.too_deep(self.peek_span());
        }
        self.depth += 1;
        let result = self.parse_binary(1);
        self.depth -= 1;
        result
    }

    fn binding_power(kind: &TokenKind) -> Option<(u8, BinOp)> {
        Some(match kind {
            TokenKind::PipePipe => (1, BinOp::LogicalOr),
            TokenKind::AmpAmp => (2, BinOp::LogicalAnd),
            TokenKind::Pipe => (3, BinOp::BitOr),
            TokenKind::Caret => (4, BinOp::BitXor),
            TokenKind::Amp => (5, BinOp::BitAnd),
            TokenKind::EqEq => (6, BinOp::Eq),
            TokenKind::NotEq => (6, BinOp::Ne),
            TokenKind::Lt => (7, BinOp::Lt),
            TokenKind::Le => (7, BinOp::Le),
            TokenKind::Gt => (7, BinOp::Gt),
            TokenKind::Ge => (7, BinOp::Ge),
            TokenKind::Shl => (8, BinOp::Shl),
            TokenKind::Shr => (8, BinOp::Shr),
            TokenKind::Plus => (9, BinOp::Add),
            TokenKind::Minus => (9, BinOp::Sub),
            TokenKind::Star => (10, BinOp::Mul),
            TokenKind::Slash => (10, BinOp::Div),
            TokenKind::Percent => (10, BinOp::Rem),
            _ => return None,
        })
    }

    fn parse_binary(&mut self, min_prec: u8) -> Result<Expr, ()> {
        let mut lhs = self.parse_cast()?;
        loop {
            let Some((prec, op)) = Self::binding_power(self.peek()) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.advance();
            let rhs = self.parse_binary(prec + 1)?;
            let span = lhs.span.merge(rhs.span);
            lhs = self.mk_expr(
                ExprKind::Binary {
                    op,
                    lhs: Box::new(lhs),
                    rhs: Box::new(rhs),
                },
                span,
            );
        }
        Ok(lhs)
    }

    fn parse_cast(&mut self) -> Result<Expr, ()> {
        let mut expr = self.parse_unary()?;
        while self.at(&TokenKind::As) {
            self.advance();
            let ty = self.parse_type()?;
            let span = expr.span.merge(self.tokens[self.pos - 1].span);
            expr = self.mk_expr(
                ExprKind::Cast {
                    expr: Box::new(expr),
                    ty,
                },
                span,
            );
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ()> {
        if self.depth >= MAX_NESTING {
            return self.too_deep(self.peek_span());
        }
        self.depth += 1;
        let result = self.parse_unary_inner();
        self.depth -= 1;
        result
    }

    fn parse_unary_inner(&mut self) -> Result<Expr, ()> {
        let op = match self.peek() {
            TokenKind::Minus => Some(UnOp::Neg),
            TokenKind::Not => Some(UnOp::Not),
            TokenKind::Amp => Some(UnOp::AddrOf),
            TokenKind::Star => Some(UnOp::Deref),
            _ => None,
        };
        if let Some(op) = op {
            let tok = self.advance();
            let op = if op == UnOp::AddrOf && self.at(&TokenKind::Mut) {
                self.advance();
                UnOp::AddrOfMut
            } else {
                op
            };
            let expr = self.parse_unary()?;
            let span = tok.span.merge(expr.span);
            return Ok(self.mk_expr(
                ExprKind::Unary {
                    op,
                    expr: Box::new(expr),
                },
                span,
            ));
        }
        self.parse_postfix()
    }

    /// Look ahead for `[ Type ; Int ] {` (an array compound literal) without
    /// committing: on failure the token position and diagnostics are restored.
    fn try_compound_array_type(&mut self) -> Option<(Type, u64)> {
        let save_pos = self.pos;
        let save_diags = self.diags.len();
        let save_depth = self.depth;
        let mut ok = false;
        let mut elem = Type::Unit;
        let mut len = 0;
        if self.at(&TokenKind::LBracket) {
            self.advance();
            if let Ok(ty) = self.parse_type() {
                if self.at(&TokenKind::Semi) {
                    self.advance();
                    if let TokenKind::Int(v) = self.peek().clone() {
                        self.advance();
                        if self.at(&TokenKind::RBracket) {
                            self.advance();
                            if self.at(&TokenKind::LBrace) {
                                elem = ty;
                                len = v;
                                ok = true;
                            }
                        }
                    }
                }
            }
        }
        if !ok {
            self.pos = save_pos;
            self.diags.truncate(save_diags);
            self.depth = save_depth;
            return None;
        }
        self.pos = save_pos;
        self.diags.truncate(save_diags);
        self.depth = save_depth;
        Some((elem, len))
    }

    fn parse_postfix(&mut self) -> Result<Expr, ()> {
        let mut expr = self.parse_primary()?;
        loop {
            if let TokenKind::LParen = self.peek() {
                if let ExprKind::Ident(name) = &expr.kind {
                    if let Some(mac) = self.macros.get(name).cloned() {
                        let open = self.advance();
                        let mut args = Vec::new();
                        while !self.at(&TokenKind::RParen) {
                            args.push(self.parse_expr()?);
                            if self.at(&TokenKind::Comma) {
                                self.advance();
                            } else {
                                break;
                            }
                        }
                        let close = self.expect(&TokenKind::RParen, "after macro arguments")?;
                        expr = self.expand_macro(&mac, args, open.span.merge(close.span))?;
                        continue;
                    }
                }
            }
            match self.peek() {
                TokenKind::LParen => {
                    self.advance();
                    let mut args = Vec::new();
                    while !self.at(&TokenKind::RParen) {
                        args.push(self.parse_expr()?);
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let close = self.expect(&TokenKind::RParen, "after call arguments")?;
                    let span = expr.span.merge(close.span);
                    expr = self.mk_expr(
                        ExprKind::Call {
                            callee: Box::new(expr),
                            args,
                        },
                        span,
                    );
                }
                TokenKind::Arrow => {
                    self.advance();
                    let (field, fspan) = self.expect_ident("field name")?;
                    let arrow_span = expr.span;
                    let deref = self.mk_expr(
                        ExprKind::Unary {
                            op: UnOp::Deref,
                            expr: Box::new(expr),
                        },
                        arrow_span,
                    );
                    let span = arrow_span.merge(fspan);
                    expr = self.mk_expr(
                        ExprKind::Field {
                            base: Box::new(deref),
                            name: field,
                        },
                        span,
                    );
                }
                TokenKind::Dot => {
                    self.advance();
                    let (field, fspan) = self.expect_ident("field name")?;
                    let span = expr.span.merge(fspan);
                    expr = self.mk_expr(
                        ExprKind::Field {
                            base: Box::new(expr),
                            name: field,
                        },
                        span,
                    );
                }
                TokenKind::LBracket => {
                    self.advance();
                    let start = if self.at(&TokenKind::DotDot) {
                        None
                    } else {
                        Some(Box::new(self.parse_expr()?))
                    };
                    if self.at(&TokenKind::DotDot) {
                        self.advance();
                        let end = if self.at(&TokenKind::RBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_expr()?))
                        };
                        let close = self.expect(&TokenKind::RBracket, "after slice range")?;
                        let span = expr.span.merge(close.span);
                        expr = self.mk_expr(
                            ExprKind::SliceRange {
                                base: Box::new(expr),
                                start,
                                end,
                            },
                            span,
                        );
                    } else {
                        let Some(index) = start else { unreachable!() };
                        let close = self.expect(&TokenKind::RBracket, "after index expression")?;
                        let span = expr.span.merge(close.span);
                        expr = self.mk_expr(
                            ExprKind::Index {
                                base: Box::new(expr),
                                index,
                            },
                            span,
                        );
                    }
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_primary(&mut self) -> Result<Expr, ()> {
        let kind = self.peek().clone();
        let span = self.peek_span();
        match kind {
            TokenKind::Int(v) => {
                self.advance();
                Ok(self.mk_expr(ExprKind::Int(v), span))
            }
            TokenKind::Str(bytes) => {
                self.advance();
                Ok(self.mk_expr(ExprKind::Str(bytes), span))
            }
            TokenKind::Float(bits) => {
                self.advance();
                Ok(self.mk_expr(ExprKind::Float(f64::from_bits(bits)), span))
            }
            TokenKind::CharLit(value) => {
                self.advance();
                Ok(self.mk_expr(ExprKind::Char(value), span))
            }
            TokenKind::CStr(bytes) => {
                self.advance();
                Ok(self.mk_expr(ExprKind::CStr(bytes), span))
            }
            TokenKind::Null => {
                self.advance();
                Ok(self.mk_expr(ExprKind::Null, span))
            }
            TokenKind::Sizeof | TokenKind::Alignof => {
                self.advance();
                self.expect(&TokenKind::LParen, "after `sizeof`/`alignof`")?;
                let ty = self.parse_type()?;
                let close = self.expect(&TokenKind::RParen, "after type")?;
                let kind = if matches!(kind, TokenKind::Sizeof) {
                    ExprKind::SizeOf(ty)
                } else {
                    ExprKind::AlignOf(ty)
                };
                Ok(self.mk_expr(kind, span.merge(close.span)))
            }
            TokenKind::Offsetof => {
                self.advance();
                self.expect(&TokenKind::LParen, "after `offsetof`")?;
                let ty = self.parse_type()?;
                self.expect(&TokenKind::Comma, "between type and field")?;
                let (field, _) = self.expect_ident("field name")?;
                let close = self.expect(&TokenKind::RParen, "after field name")?;
                Ok(self.mk_expr(ExprKind::OffsetOf(ty, field), span.merge(close.span)))
            }
            TokenKind::True | TokenKind::False => {
                self.advance();
                Ok(self.mk_expr(ExprKind::Bool(matches!(kind, TokenKind::True)), span))
            }
            TokenKind::Ident(name) => {
                self.advance();
                let mut segments = vec![name];
                while self.at(&TokenKind::ColonColon) {
                    self.advance();
                    let (seg, _) = self.expect_ident("path segment")?;
                    segments.push(seg);
                }
                if self.looks_like_struct_literal() {
                    self.advance();
                    let mut fields = Vec::new();
                    let mut partial = false;
                    while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
                        if self.at(&TokenKind::DotDotDot) {
                            self.advance();
                            partial = true;
                            break;
                        }
                        let (fname, _fspan) = self.expect_ident("field name")?;
                        self.expect(&TokenKind::Colon, "after field name")?;
                        let value = self.parse_expr()?;
                        fields.push((fname, value));
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let close = self.expect(&TokenKind::RBrace, "after struct literal")?;
                    let full_span = span.merge(close.span);
                    let joined = segments.join("::");
                    let kind = if partial {
                        ExprKind::CompoundLit {
                            ty: Type::Named(joined),
                            fields,
                        }
                    } else {
                        ExprKind::StructLit {
                            name: joined,
                            fields,
                        }
                    };
                    return Ok(self.mk_expr(kind, full_span));
                }
                if segments.len() == 1 {
                    Ok(self.mk_expr(ExprKind::Ident(segments[0].clone()), span))
                } else {
                    Ok(self.mk_expr(ExprKind::Path(segments), span))
                }
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "after parenthesized expression")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
                if let Some((elem, len)) = self.try_compound_array_type() {
                    self.advance(); // `[`
                    let _ = self.parse_type()?;
                    self.expect(&TokenKind::Semi, "inside compound literal type")?;
                    self.expect(&TokenKind::Int(len), "array length")?;
                    self.expect(&TokenKind::RBracket, "after array length")?;
                    self.expect(&TokenKind::LBrace, "start of compound literal")?;
                    let mut fields = Vec::new();
                    while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
                        let index = match self.peek().clone() {
                            TokenKind::Int(v) => {
                                self.advance();
                                v
                            }
                            other => {
                                let span = self.peek_span();
                                return self.error(
                                    "E0100",
                                    span,
                                    format!("expected array index, found {}", other.describe()),
                                );
                            }
                        };
                        self.expect(&TokenKind::Colon, "after array index")?;
                        let value = self.parse_expr()?;
                        fields.push((index.to_string(), value));
                        if self.at(&TokenKind::Comma) {
                            self.advance();
                        } else {
                            break;
                        }
                    }
                    let close = self.expect(&TokenKind::RBrace, "after compound literal")?;
                    let ty = Type::array_of(elem, len);
                    return Ok(self.mk_expr(
                        ExprKind::CompoundLit { ty, fields },
                        span.merge(close.span),
                    ));
                }
                self.advance();
                if self.at(&TokenKind::RBracket) {
                    return self.error("E0110", span, "empty array literal");
                }
                let first = self.parse_expr()?;
                if self.at(&TokenKind::Semi) {
                    self.advance();
                    let count = match self.peek().clone() {
                        TokenKind::Int(v) => {
                            self.advance();
                            v
                        }
                        other => {
                            let span = self.peek_span();
                            return self.error(
                                "E0100",
                                span,
                                format!("expected repeat count, found {}", other.describe()),
                            );
                        }
                    };
                    let close =
                        self.expect(&TokenKind::RBracket, "after array repeat count")?;
                    if count == 0 {
                        return self.error(
                            "E0110",
                            span.merge(close.span),
                            "array repeat count must be greater than zero",
                        );
                    }
                    return Ok(self.mk_expr(
                        ExprKind::ArrayRepeat {
                            value: Box::new(first),
                            count,
                        },
                        span.merge(close.span),
                    ));
                }
                let mut elems = vec![first];
                while self.at(&TokenKind::Comma) {
                    self.advance();
                    if self.at(&TokenKind::RBracket) {
                        break;
                    }
                    elems.push(self.parse_expr()?);
                }
                let close = self.expect(&TokenKind::RBracket, "after array elements")?;
                if elems.is_empty() {
                    return self.error("E0110", span.merge(close.span), "empty array literal");
                }
                Ok(self.mk_expr(ExprKind::Array(elems), span.merge(close.span)))
            }
            other => {
                let msg = format!("expected expression, found {}", other.describe());
                self.error("E0101", span, msg)
            }
        }
    }
}

/// Evaluate an integer constant expression over literals and previously
/// declared integer `const` names. This is the subset array lengths accept;
/// anything else returns `None` and the caller reports `E0100`.
fn const_int_expr(e: &Expr, env: &std::collections::HashMap<String, u64>) -> Option<u64> {
    match &e.kind {
        ExprKind::Int(v) => Some(*v),
        ExprKind::Ident(name) => env.get(name).copied(),
        ExprKind::Path(segments) => env.get(&segments.join("$")).copied(),
        ExprKind::Unary { op, expr } => {
            let v = const_int_expr(expr, env)?;
            match op {
                UnOp::Neg => Some(v.wrapping_neg()),
                UnOp::Not => Some(!v),
                UnOp::AddrOf | UnOp::AddrOfMut | UnOp::Deref => None,
            }
        }
        ExprKind::Binary { op, lhs, rhs } => {
            let a = const_int_expr(lhs, env)?;
            let b = const_int_expr(rhs, env)?;
            match op {
                BinOp::Add => Some(a.wrapping_add(b)),
                BinOp::Sub => Some(a.wrapping_sub(b)),
                BinOp::Mul => Some(a.wrapping_mul(b)),
                BinOp::Div => {
                    if b == 0 { None } else { Some(a / b) }
                }
                BinOp::Rem => {
                    if b == 0 { None } else { Some(a % b) }
                }
                BinOp::Shl => Some(a.wrapping_shl((b & 63) as u32)),
                BinOp::Shr => Some(a.wrapping_shr((b & 63) as u32)),
                BinOp::BitAnd => Some(a & b),
                BinOp::BitOr => Some(a | b),
                BinOp::BitXor => Some(a ^ b),
                BinOp::Eq
                | BinOp::Ne
                | BinOp::Lt
                | BinOp::Le
                | BinOp::Gt
                | BinOp::Ge
                | BinOp::LogicalAnd
                | BinOp::LogicalOr => None,
            }
        }
        ExprKind::Cast { expr, ty } if ty.is_integer() => const_int_expr(expr, env),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;

    fn parse_src(src: &str) -> Result<Program, Vec<Diagnostic>> {
        let toks = lex(src).expect("lex ok");
        parse(&toks)
    }

    fn one_fn(src: &str) -> FnDecl {
        let mut p = parse_src(src).expect("parse ok");
        assert_eq!(p.fns.len(), 1);
        p.fns.remove(0)
    }

    #[test]
    fn parses_function_with_params_and_return() {
        let f = one_fn("fn add(a: i32, b: i32) -> i32 { return a + b; }");
        assert_eq!(f.name, "add");
        assert_eq!(f.params.len(), 2);
        assert_eq!(f.params[0].ty, Type::Int(IntTy::I32));
        assert_eq!(f.ret, Type::Int(IntTy::I32));
        match &f.body.as_ref().unwrap().stmts[0] {
            Stmt::Return { value: Some(e), .. } => {
                assert!(matches!(e.kind, ExprKind::Binary { op: BinOp::Add, .. }))
            }
            other => panic!("unexpected stmt: {other:?}"),
        }
    }

    #[test]
    fn default_return_type_is_unit() {
        let f = one_fn("fn f() { }");
        assert_eq!(f.ret, Type::Unit);
    }

    #[test]
    fn precedence_multiplication_before_addition() {
        let f = one_fn("fn f() -> i32 { return 1 + 2 * 3; }");
        let Stmt::Return { value: Some(e), .. } = &f.body.as_ref().unwrap().stmts[0] else {
            panic!("expected return")
        };
        let ExprKind::Binary { op, rhs, .. } = &e.kind else {
            panic!("expected binary")
        };
        assert_eq!(*op, BinOp::Add);
        assert!(matches!(rhs.kind, ExprKind::Binary { op: BinOp::Mul, .. }));
    }

    #[test]
    fn assignment_binds_looser_than_or() {
        let f = one_fn("fn f() -> i32 { let mut x: i32 = 0; x = 1 | 2; return x; }");
        match &f.body.as_ref().unwrap().stmts[1] {
            Stmt::Assign { value, .. } => {
                assert!(matches!(value.kind, ExprKind::Binary { op: BinOp::BitOr, .. }))
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn parses_composite_types() {
        let f = one_fn("fn f(a: *mut u8, b: [i32; 4], c: *[u8; 2]) { }");
        assert_eq!(f.params[0].ty, Type::ptr_to(Type::Int(IntTy::U8), true));
        assert_eq!(
            f.params[1].ty,
            Type::array_of(Type::Int(IntTy::I32), 4)
        );
        assert_eq!(
            f.params[2].ty,
            Type::ptr_to(Type::array_of(Type::Int(IntTy::U8), 2), false)
        );
    }

    #[test]
    fn parses_control_flow() {
        let f = one_fn(
            "fn f(n: i32) -> i32 { while n > 0 { if n == 3 { break; } else { continue; } } loop { return 1; } }",
        );
        assert!(matches!(f.body.as_ref().unwrap().stmts[0], Stmt::While { .. }));
        assert!(matches!(f.body.as_ref().unwrap().stmts[1], Stmt::Loop { .. }));
    }

    #[test]
    fn parses_calls_index_cast_and_unsafe() {
        let f = one_fn(
            "fn f(p: *mut u8, a: [u8; 4]) { unsafe { *p = a[0] as u8; } }",
        );
        match &f.body.as_ref().unwrap().stmts[0] {
            Stmt::Unsafe { body, .. } => match &body.stmts[0] {
                Stmt::Assign { target, value, .. } => {
                    assert!(matches!(
                        target.kind,
                        ExprKind::Unary { op: UnOp::Deref, .. }
                    ));
                    assert!(matches!(value.kind, ExprKind::Cast { .. }));
                }
                other => panic!("unexpected: {other:?}"),
            },
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn reports_missing_semicolon_with_location() {
        let errs = parse_src("fn main() -> i32 { return 0 }").unwrap_err();
        assert_eq!(errs[0].code, "E0100");
        assert!(errs[0].message.contains("expected `;`"));
    }

    #[test]
    fn recovers_to_next_function() {
        let errs =
            parse_src("fn a() -> i32 { return 1 }\nfn b() -> i32 { return 1; }").unwrap_err();
        assert_eq!(errs[0].code, "E0100");
    }

    #[test]
    fn parses_array_repeat_literal() {
        let f = one_fn("fn f() -> [u8; 4] { return [0; 4]; }");
        let Stmt::Return { value: Some(e), .. } = &f.body.as_ref().unwrap().stmts[0] else {
            panic!("expected return");
        };
        assert!(matches!(e.kind, ExprKind::ArrayRepeat { count: 4, .. }));
    }

    #[test]
    fn rejects_excessive_nesting_without_overflowing() {
        let deep = format!(
            "fn main() -> i32 {{ return {}1{}; }}",
            "(".repeat(10_000),
            ")".repeat(10_000)
        );
        let errs = parse_src(&deep).unwrap_err();
        assert!(errs.iter().any(|d| d.code == "E0111"), "{errs:?}");
    }

    #[test]
    fn rejects_deep_unary_and_type_chains() {
        let deep_unary = format!("fn main() -> i32 {{ return {}1; }}", "-".repeat(10_000));
        let errs = parse_src(&deep_unary).unwrap_err();
        assert!(errs.iter().any(|d| d.code == "E0111"), "{errs:?}");

        let deep_type = format!("fn f(p: {}i32) {{ }} fn main() -> i32 {{ return 0; }}", "*".repeat(10_000));
        let errs = parse_src(&deep_type).unwrap_err();
        assert!(errs.iter().any(|d| d.code == "E0111"), "{errs:?}");
    }

    #[test]
    fn rejects_non_block_else_branch() {
        let errs = parse_src("fn f(b: bool) { if b { } else while b { } }").unwrap_err();
        assert!(errs.iter().any(|d| d.code == "E0100"), "{errs:?}");
    }

    #[test]
    fn rejects_empty_array_literal() {
        let errs = parse_src("fn f() { let a: [u8; 1] = []; }").unwrap_err();
        assert_eq!(errs[0].code, "E0110");
    }

    #[test]
    fn parses_nested_vec_types_without_whitespace() {
        let program = parse_src(
            "fn f(a: vec<vec<u8>>, b: vec<vec<vec<i32>>>) { }
             fn main() -> i32 { let mut v: vec<vec<u8>> = vec_new(1); vec_free(&mut v); return 0; }",
        )
        .expect("parse ok");
        let f = &program.fns[0];
        assert_eq!(
            f.params[0].ty,
            Type::Vec(Box::new(Type::Vec(Box::new(Type::Int(IntTy::U8)))))
        );
        assert_eq!(
            f.params[1].ty,
            Type::Vec(Box::new(Type::Vec(Box::new(Type::Vec(Box::new(
                Type::Int(IntTy::I32)
            ))))))
        );
        let main = &program.fns[1];
        let Stmt::Let { ty: Some(ty), .. } = &main.body.as_ref().unwrap().stmts[0] else {
            panic!("expected let");
        };
        assert_eq!(
            *ty,
            Type::Vec(Box::new(Type::Vec(Box::new(Type::Int(IntTy::U8)))))
        );
    }

    #[test]
    fn shift_operators_stay_expressions() {
        let f = one_fn(
            "fn f(a: i32) -> i32 { let b: i32 = a >> 2; let mut c: i32 = b << 1; c >>= 1; return c; }",
        );
        let stmts = &f.body.as_ref().unwrap().stmts;
        let Stmt::Let { init, .. } = &stmts[0] else {
            panic!("expected let");
        };
        assert!(
            matches!(init.kind, ExprKind::Binary { op: BinOp::Shr, .. }),
            "{init:?}"
        );
        let Stmt::CompoundAssign { op, .. } = &stmts[2] else {
            panic!("expected compound assignment");
        };
        assert_eq!(*op, BinOp::Shr);
    }

    #[test]
    fn const_integer_names_and_expressions_are_array_lengths() {
        let program = parse_src(
            "const N: usize = 4;
             fn f(a: [u8; N]) { }
             fn g() { let b: [i32; N * 2] = [0; 8]; }",
        )
        .expect("parse ok");
        assert_eq!(
            program.fns[0].params[0].ty,
            Type::array_of(Type::Int(IntTy::U8), 4)
        );
        let Stmt::Let { ty: Some(ty), .. } = &program.fns[1].body.as_ref().unwrap().stmts[0] else {
            panic!("expected let");
        };
        assert_eq!(*ty, Type::array_of(Type::Int(IntTy::I32), 8));
    }

    #[test]
    fn rejects_non_constant_array_lengths() {
        let errs = parse_src("fn f(a: [u8; Q]) { }").unwrap_err();
        assert!(errs.iter().any(|d| d.code == "E0100"), "{errs:?}");
    }
}

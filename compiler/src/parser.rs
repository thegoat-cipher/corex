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
const MAX_NESTING: usize = 256;

pub fn parse(tokens: &[Token]) -> Result<Program, Vec<Diagnostic>> {
    let mut p = Parser {
        tokens,
        pos: 0,
        diags: Vec::new(),
        next_id: 0,
        depth: 0,
    };
    let mut fns = Vec::new();
    let mut globals = Vec::new();
    let mut enums = Vec::new();
    let mut structs = Vec::new();
    while !p.at(&TokenKind::Eof) && p.diags.len() < 100 {
        let before = p.pos;
        match p.peek() {
            TokenKind::Fn => {
                if let Ok(f) = p.parse_fn() {
                    fns.push(f);
                }
            }
            TokenKind::Static => {
                if let Ok(g) = p.parse_global() {
                    globals.push(g);
                }
            }
            TokenKind::Enum => {
                if let Ok(en) = p.parse_enum() {
                    enums.push(en);
                }
            }
            TokenKind::Struct => {
                if let Ok(st) = p.parse_struct() {
                    structs.push(st);
                }
            }
            _ => {
                let span = p.peek_span();
                let found = p.peek().describe();
                p.diags.push(Diagnostic::error(
                    "E0100",
                    span,
                    format!("expected `fn`, `static`, or `enum`, found {found}"),
                ));
            }
        }
        if p.pos == before {
            p.advance();
        }
        p.recover_to_item();
    }
    if p.diags.is_empty() {
        Ok(Program {
            fns,
            globals,
            enums,
            structs,
        })
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
}

impl<'a> Parser<'a> {
    fn peek(&self) -> &TokenKind {
        &self.tokens[self.pos].kind
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

    fn recover_to_item(&mut self) {
        while !matches!(
            self.peek(),
            TokenKind::Fn
                | TokenKind::Static
                | TokenKind::Enum
                | TokenKind::Struct
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

    fn parse_fn(&mut self) -> Result<FnDecl, ()> {
        let start = self.expect(&TokenKind::Fn, "start of function")?.span.start;
        let (name, name_span) = self.expect_ident("function name")?;
        self.expect(&TokenKind::LParen, "after function name")?;

        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) {
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

        let body = self.parse_block()?;
        let span = Span::new(start, body.span.end);
        Ok(FnDecl {
            name,
            name_span,
            params,
            ret,
            ret_span,
            body,
            span,
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
                Type::Named(name.clone())
            }
            TokenKind::LBracket => {
                self.advance();
                let elem = self.parse_type()?;
                if self.at(&TokenKind::RBracket) {
                    self.advance();
                    return Ok(Type::Slice(Box::new(elem)));
                }
                self.expect(&TokenKind::Semi, "inside array type")?;
                let len_tok = self.peek().clone();
                let len = match len_tok {
                    TokenKind::Int(v) => {
                        self.advance();
                        v
                    }
                    other => {
                        let span = self.peek_span();
                        return self.error(
                            "E0100",
                            span,
                            format!("expected array length, found {}", other.describe()),
                        );
                    }
                };
                self.expect(&TokenKind::RBracket, "after array length")?;
                if len == 0 {
                    let span = self.peek_span();
                    return self.error("E0110", span, "array length must be greater than zero");
                }
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
        match self.peek().clone() {
            TokenKind::Let => self.parse_let(),
            TokenKind::If => self.parse_if(),
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

    // ----- expressions -------------------------------------------------

    fn parse_global(&mut self) -> Result<GlobalDecl, ()> {
        let kw = self.expect(&TokenKind::Static, "start of global")?;
        let mutable = if self.at(&TokenKind::Mut) {
            self.advance();
            true
        } else {
            false
        };
        let (name, name_span) = self.expect_ident("global name")?;
        self.expect(&TokenKind::Colon, "after global name")?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Assign, "before global initializer")?;
        let init = self.parse_expr()?;
        let semi = self.expect(&TokenKind::Semi, "after global initializer")?;
        Ok(GlobalDecl {
            name,
            name_span,
            ty,
            mutable,
            init,
            span: kw.span.merge(semi.span),
        })
    }

    fn parse_enum(&mut self) -> Result<EnumDecl, ()> {
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
        })
    }

    fn parse_struct(&mut self) -> Result<StructDecl, ()> {
        let kw = self.expect(&TokenKind::Struct, "start of struct")?;
        let (name, name_span) = self.expect_ident("struct name")?;
        self.expect(&TokenKind::LBrace, "after struct name")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
            let (fname, fspan) = self.expect_ident("field name")?;
            self.expect(&TokenKind::Colon, "after field name")?;
            let ty = self.parse_type()?;
            fields.push(StructFieldDecl {
                name: fname,
                span: fspan,
                ty,
            });
            if self.at(&TokenKind::Comma) {
                self.advance();
            } else {
                break;
            }
        }
        let close = self.expect(&TokenKind::RBrace, "after struct fields")?;
        Ok(StructDecl {
            name,
            name_span,
            fields,
            span: kw.span.merge(close.span),
        })
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

    fn parse_postfix(&mut self) -> Result<Expr, ()> {
        let mut expr = self.parse_primary()?;
        loop {
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
            TokenKind::True | TokenKind::False => {
                self.advance();
                Ok(self.mk_expr(ExprKind::Bool(matches!(kind, TokenKind::True)), span))
            }
            TokenKind::Ident(name) => {
                self.advance();
                if self.at(&TokenKind::ColonColon) {
                    self.advance();
                    let (variant, vspan) = self.expect_ident("enum variant")?;
                    let span = span.merge(vspan);
                    return Ok(self.mk_expr(
                        ExprKind::EnumVariant {
                            enum_name: name,
                            variant,
                        },
                        span,
                    ));
                }
                if self.looks_like_struct_literal() {
                    self.advance();
                    let mut fields = Vec::new();
                    while !self.at(&TokenKind::RBrace) && !self.at(&TokenKind::Eof) {
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
                    let span = span.merge(close.span);
                    return Ok(self.mk_expr(ExprKind::StructLit { name, fields }, span));
                }
                Ok(self.mk_expr(ExprKind::Ident(name), span))
            }
            TokenKind::LParen => {
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(&TokenKind::RParen, "after parenthesized expression")?;
                Ok(inner)
            }
            TokenKind::LBracket => {
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
        match &f.body.stmts[0] {
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
        let Stmt::Return { value: Some(e), .. } = &f.body.stmts[0] else {
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
        match &f.body.stmts[1] {
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
        assert!(matches!(f.body.stmts[0], Stmt::While { .. }));
        assert!(matches!(f.body.stmts[1], Stmt::Loop { .. }));
    }

    #[test]
    fn parses_calls_index_cast_and_unsafe() {
        let f = one_fn(
            "fn f(p: *mut u8, a: [u8; 4]) { unsafe { *p = a[0] as u8; } }",
        );
        match &f.body.stmts[0] {
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
        let Stmt::Return { value: Some(e), .. } = &f.body.stmts[0] else {
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
}

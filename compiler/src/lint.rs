//! Static linter: unused bindings and items, unreachable statements, and
//! constant conditions. Returns warnings only; errors come from the frontend.

use std::collections::HashSet;

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};

struct Scope {
    bindings: Vec<(String, Span, bool)>,
}

struct Lint<'a> {
    diags: Vec<Diagnostic>,
    scopes: Vec<Scope>,
    refs: HashSet<String>,
    program: &'a Program,
}

pub fn lint(program: &Program) -> Vec<Diagnostic> {
    let mut l = Lint {
        diags: Vec::new(),
        scopes: Vec::new(),
        refs: HashSet::new(),
        program,
    };
    for f in &program.fns {
        if let Some(body) = &f.body {
            l.function(body);
        }
    }
    l.check_unused_items();
    l.diags
}

impl<'a> Lint<'a> {
    fn warn(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags
            .push(Diagnostic::warning(code, span, message));
    }

    fn push(&mut self) {
        self.scopes.push(Scope {
            bindings: Vec::new(),
        });
    }

    fn pop(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for (name, span, used) in scope.bindings {
                if !used && !name.starts_with('_') {
                    self.warn(
                        "W0001",
                        span,
                        format!("unused variable `{name}` (prefix with `_` to silence)"),
                    );
                }
            }
        }
    }

    fn declare(&mut self, name: &str, span: Span) {
        if name == "_" {
            return;
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.bindings.push((name.to_string(), span, false));
        }
    }

    fn mark_used(&mut self, name: &str) {
        for scope in self.scopes.iter_mut().rev() {
            for binding in scope.bindings.iter_mut().rev() {
                if binding.0 == name {
                    binding.2 = true;
                    return;
                }
            }
        }
    }

    fn function(&mut self, body: &Block) {
        self.push();
        self.block(body);
        self.pop();
    }

    fn block(&mut self, block: &Block) {
        self.push();
        let mut diverged: Option<Span> = None;
        for stmt in &block.stmts {
            if diverged.is_some() && !matches!(stmt, Stmt::Label { .. }) {
                self.warn(
                    "W0002",
                    stmt.span(),
                    "unreachable statement (the previous statement diverges)",
                );
                diverged = None;
            }
            self.stmt(stmt);
            if stmt_diverges(stmt) {
                diverged = Some(stmt.span());
            }
        }
        self.pop();
    }

    fn stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                name_span,
                init,
                ..
            } => {
                self.expr(init);
                self.declare(name, *name_span);
            }
            Stmt::Assign { target, value, .. } | Stmt::CompoundAssign { target, value, .. } => {
                self.expr(target);
                self.expr(value);
            }
            Stmt::Expr(e) => self.expr(e),
            Stmt::If {
                cond,
                then_block,
                else_branch,
                ..
            } => {
                if matches!(cond.kind, ExprKind::Bool(_)) {
                    self.warn("W0004", cond.span, "condition is a constant");
                }
                self.expr(cond);
                self.block(then_block);
                if let Some(branch) = else_branch {
                    self.stmt(branch);
                }
            }
            Stmt::While { cond, body, .. } => {
                if matches!(cond.kind, ExprKind::Bool(false)) {
                    self.warn("W0004", cond.span, "`while false` never runs");
                }
                self.expr(cond);
                self.block(body);
            }
            Stmt::Loop { body, .. } => self.block(body),
            Stmt::For {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.push();
                if let Some(init) = init {
                    self.stmt(init);
                }
                if let Some(cond) = cond {
                    self.expr(cond);
                }
                if let Some(step) = step {
                    self.stmt(step);
                }
                self.block(body);
                self.pop();
            }
            Stmt::DoWhile { body, cond, .. } => {
                self.block(body);
                self.expr(cond);
            }
            Stmt::Switch { value, arms, .. } => {
                self.expr(value);
                for arm in arms {
                    if let Some(v) = &arm.value {
                        self.expr(v);
                    }
                    self.block(&arm.body);
                }
            }
            Stmt::Label { stmt, .. } => self.stmt(stmt),
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            Stmt::Unsafe { body, .. } | Stmt::Block(body) => self.block(body),
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Goto { .. } | Stmt::Empty(_) => {}
        }
    }

    fn expr(&mut self, e: &Expr) {
        match &e.kind {
            ExprKind::Ident(name) => {
                self.mark_used(name);
                self.refs.insert(name.clone());
            }
            ExprKind::Path(segments) => {
                self.mark_used(&segments[0]);
                self.refs.insert(segments.join("$"));
                self.refs.insert(segments[0].clone());
            }
            ExprKind::EnumVariant { enum_name, .. } => {
                self.refs.insert(enum_name.clone());
            }
            ExprKind::StructLit { name, fields } => {
                self.refs.insert(name.clone());
                for (_, value) in fields {
                    self.expr(value);
                }
            }
            ExprKind::CompoundLit { ty, fields } => {
                self.ty(ty);
                for (_, value) in fields {
                    self.expr(value);
                }
            }
            ExprKind::Field { base, .. } => self.expr(base),
            ExprKind::Unary { expr, .. } => self.expr(expr),
            ExprKind::Binary { lhs, rhs, .. } => {
                self.expr(lhs);
                self.expr(rhs);
            }
            ExprKind::Call { callee, args } => {
                self.expr(callee);
                for a in args {
                    self.expr(a);
                }
            }
            ExprKind::Index { base, index } => {
                self.expr(base);
                self.expr(index);
            }
            ExprKind::SliceRange { base, start, end } => {
                self.expr(base);
                if let Some(s) = start {
                    self.expr(s);
                }
                if let Some(e) = end {
                    self.expr(e);
                }
            }
            ExprKind::Cast { expr, ty } => {
                self.ty(ty);
                self.expr(expr);
            }
            ExprKind::Array(elems) => {
                for el in elems {
                    self.expr(el);
                }
            }
            ExprKind::ArrayRepeat { value, .. } => self.expr(value),
            ExprKind::SizeOf(ty) | ExprKind::AlignOf(ty) => self.ty(ty),
            ExprKind::OffsetOf(ty, field) => {
                self.refs.insert(field.clone());
                self.ty(ty);
            }
            ExprKind::Int(_)
            | ExprKind::Float(_)
            | ExprKind::Str(_)
            | ExprKind::CStr(_)
            | ExprKind::Char(_)
            | ExprKind::Bool(_)
            | ExprKind::Null => {}
        }
    }

    fn ty(&mut self, ty: &Type) {
        match ty {
            Type::Named(name) => {
                self.refs.insert(name.clone());
            }
            Type::Ptr { pointee, .. } => self.ty(pointee),
            Type::Array { elem, .. } | Type::Slice(elem) | Type::Vec(elem) => self.ty(elem),
            Type::Fn(sig) => {
                for p in &sig.params {
                    self.ty(p);
                }
                self.ty(&sig.ret);
            }
            _ => {}
        }
    }

    fn check_unused_items(&mut self) {
        for f in &self.program.fns {
            if f.public || f.name == "main" || f.name == "_start" {
                continue;
            }
            if f.name.starts_with('_') {
                continue;
            }
            if !self.refs.contains(&f.name) {
                self.warn(
                    "W0003",
                    f.name_span,
                    format!("private function `{}` is never used", f.name),
                );
            }
        }
        for g in &self.program.globals {
            if g.public || g.is_extern || g.name.starts_with('_') {
                continue;
            }
            if !self.refs.contains(&g.name) {
                self.warn(
                    "W0003",
                    g.name_span,
                    format!("private global `{}` is never used", g.name),
                );
            }
        }
        for c in &self.program.consts {
            if c.public || c.name.starts_with('_') {
                continue;
            }
            if !self.refs.contains(&c.name) {
                self.warn(
                    "W0003",
                    c.name_span,
                    format!("private constant `{}` is never used", c.name),
                );
            }
        }
    }
}

fn expr_diverges(e: &Expr) -> bool {
    let ExprKind::Call { callee, args } = &e.kind else {
        return false;
    };
    let name = match &callee.kind {
        ExprKind::Ident(name) => name.as_str(),
        ExprKind::Path(segments) => segments.last().map(|s| s.as_str()).unwrap_or(""),
        _ => return false,
    };
    if name == "panic" {
        return true;
    }
    if name != "syscall" {
        return false;
    }
    matches!(
        args.first().map(|a| &a.kind),
        Some(ExprKind::Int(60)) | Some(ExprKind::Int(231))
    )
}

fn stmt_diverges(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } | Stmt::Break(_) | Stmt::Continue(_) | Stmt::Goto { .. } => true,
        Stmt::Expr(e) => expr_diverges(e),
        Stmt::Block(block) => block.stmts.iter().any(stmt_diverges),
        Stmt::Unsafe { body, .. } => body.stmts.iter().any(stmt_diverges),
        _ => false,
    }
}

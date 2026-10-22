//! Deterministic module name mangling.
//!
//! A compilation unit declares `module math;`. Its public and private items are
//! emitted under the symbol `math$name`; references inside the unit to local
//! items are rewritten to the same symbol. Qualified references to other
//! modules (`math::add`) become `math$add` so a generated interface can declare
//! them with plain CoreX syntax.
//!
//! Entry points (`main`, `_start`) are never mangled, so the operating system
//! can always find them.

use std::collections::{HashMap, HashSet};

use crate::ast::*;

pub fn mangle_unit(program: &mut Program, unit_start: usize, prefix: &str) {
    if prefix.is_empty() {
        // Still rewrite qualified paths inside the unit.
        let mut mapper = Mapper::new(HashMap::new());
        mapper.program(program);
        return;
    }

    let mut locals = HashSet::new();
    for f in &program.fns {
        if f.span.start >= unit_start && f.name != "main" && f.name != "_start" {
            locals.insert(f.name.clone());
        }
    }
    for g in &program.globals {
        if g.span.start >= unit_start {
            locals.insert(g.name.clone());
        }
    }
    for c in &program.consts {
        if c.span.start >= unit_start {
            locals.insert(c.name.clone());
        }
    }
    for e in &program.enums {
        if e.span.start >= unit_start {
            locals.insert(e.name.clone());
        }
    }
    for s in &program.structs {
        if s.span.start >= unit_start {
            locals.insert(s.name.clone());
        }
    }
    for t in &program.typedefs {
        if t.name_span.start >= unit_start {
            locals.insert(t.name.clone());
        }
    }

    let map: HashMap<String, String> = locals
        .iter()
        .map(|name| (name.clone(), format!("{prefix}${name}")))
        .collect();

    for f in program.fns.iter_mut() {
        if f.span.start < unit_start {
            continue;
        }
        if let Some(mangled) = map.get(&f.name) {
            f.name = mangled.clone();
        }
    }
    for g in program.globals.iter_mut() {
        if g.span.start < unit_start {
            continue;
        }
        if let Some(mangled) = map.get(&g.name) {
            g.name = mangled.clone();
        }
    }
    for c in program.consts.iter_mut() {
        if c.span.start < unit_start {
            continue;
        }
        if let Some(mangled) = map.get(&c.name) {
            c.name = mangled.clone();
        }
    }
    for e in program.enums.iter_mut() {
        if e.span.start < unit_start {
            continue;
        }
        if let Some(mangled) = map.get(&e.name) {
            e.name = mangled.clone();
        }
    }
    for s in program.structs.iter_mut() {
        if s.span.start < unit_start {
            continue;
        }
        if let Some(mangled) = map.get(&s.name) {
            s.name = mangled.clone();
        }
    }
    for t in program.typedefs.iter_mut() {
        if t.name_span.start < unit_start {
            continue;
        }
        if let Some(mangled) = map.get(&t.name) {
            t.name = mangled.clone();
        }
    }

    let mut mapper = Mapper::new(map);
    // Only rewrite declarations that belong to this unit; interface
    // declarations are already fully mangled.
    for f in program.fns.iter_mut() {
        if f.span.start < unit_start {
            continue;
        }
        mapper.fn_decl(f);
    }
    for g in program.globals.iter_mut() {
        if g.span.start < unit_start {
            continue;
        }
        mapper.ty(&mut g.ty);
        mapper.expr(&mut g.init);
    }
    for c in program.consts.iter_mut() {
        if c.name_span.start < unit_start {
            continue;
        }
        mapper.ty(&mut c.ty);
        mapper.expr(&mut c.value);
    }
    for e in program.enums.iter_mut() {
        if e.span.start < unit_start {
            continue;
        }
        for v in &mut e.variants {
            if let Some(value) = &mut v.value {
                mapper.expr(value);
            }
        }
    }
    for s in program.structs.iter_mut() {
        if s.span.start < unit_start {
            continue;
        }
        for f in &mut s.fields {
            mapper.ty(&mut f.ty);
        }
    }
    for t in program.typedefs.iter_mut() {
        if t.name_span.start < unit_start {
            continue;
        }
        mapper.ty(&mut t.ty);
    }
}

struct Mapper {
    map: HashMap<String, String>,
    scopes: Vec<HashSet<String>>,
}

impl Mapper {
    fn new(map: HashMap<String, String>) -> Mapper {
        Mapper {
            map,
            scopes: Vec::new(),
        }
    }

    fn push(&mut self) {
        self.scopes.push(HashSet::new());
    }

    fn pop(&mut self) {
        self.scopes.pop();
    }

    fn shadowed(&self, name: &str) -> bool {
        self.scopes.iter().any(|s| s.contains(name))
    }

    fn name(&self, name: &str) -> String {
        if self.shadowed(name) {
            return name.to_string();
        }
        if let Some(mangled) = self.map.get(name) {
            return mangled.clone();
        }
        if name.contains("::") {
            return name.replace("::", "$");
        }
        name.to_string()
    }

    fn ty(&self, ty: &mut Type) {
        match ty {
            Type::Named(name) => *name = self.name(name),
            Type::Ptr { pointee, .. } => self.ty(pointee),
            Type::Array { elem, .. } | Type::Slice(elem) | Type::Vec(elem) => self.ty(elem),
            Type::Fn(sig) => {
                for p in &mut sig.params {
                    self.ty(p);
                }
                self.ty(&mut sig.ret);
            }
            _ => {}
        }
    }

    fn program(&mut self, program: &mut Program) {
        for f in &mut program.fns {
            self.fn_decl(f);
        }
        for g in &mut program.globals {
            self.ty(&mut g.ty);
            self.expr(&mut g.init);
        }
        for c in &mut program.consts {
            self.ty(&mut c.ty);
            self.expr(&mut c.value);
        }
        for e in &mut program.enums {
            for v in &mut e.variants {
                if let Some(value) = &mut v.value {
                    self.expr(value);
                }
            }
        }
        for s in &mut program.structs {
            for f in &mut s.fields {
                self.ty(&mut f.ty);
            }
        }
        for t in &mut program.typedefs {
            self.ty(&mut t.ty);
        }
    }

    fn fn_decl(&mut self, f: &mut FnDecl) {
        for p in &mut f.params {
            self.ty(&mut p.ty);
        }
        self.ty(&mut f.ret);
        let Some(body) = &mut f.body else {
            return;
        };
        self.push();
        for p in &f.params {
            self.scopes.last_mut().unwrap().insert(p.name.clone());
        }
        self.block(body);
        self.pop();
    }

    fn block(&mut self, block: &mut Block) {
        self.push();
        for stmt in &mut block.stmts {
            self.stmt(stmt);
        }
        self.pop();
    }

    fn stmt(&mut self, stmt: &mut Stmt) {
        match stmt {
            Stmt::Let { name, ty, init, .. } => {
                if let Some(ty) = ty {
                    self.ty(ty);
                }
                self.expr(init);
                self.scopes.last_mut().unwrap().insert(name.clone());
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
                self.expr(cond);
                self.block(then_block);
                if let Some(branch) = else_branch {
                    self.stmt(branch);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.expr(cond);
                self.block(body);
            }
            Stmt::Loop { body, .. }
            | Stmt::DoWhile { body, .. }
            | Stmt::Unsafe { body, .. }
            | Stmt::Block(body) => self.block(body),
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
            Stmt::Switch { value, arms, .. } => {
                self.expr(value);
                for arm in arms {
                    if let Some(v) = &mut arm.value {
                        self.expr(v);
                    }
                    self.block(&mut arm.body);
                }
            }
            Stmt::Label { stmt, .. } => self.stmt(stmt),
            Stmt::Return { value, .. } => {
                if let Some(value) = value {
                    self.expr(value);
                }
            }
            Stmt::Break(_) | Stmt::Continue(_) | Stmt::Goto { .. } | Stmt::Empty(_) => {}
        }
    }

    fn expr(&mut self, e: &mut Expr) {
        match &mut e.kind {
            ExprKind::Ident(name) => *name = self.name(name),
            ExprKind::Path(segments) => {
                if let Some(first) = segments.first_mut() {
                    *first = self.name(first);
                }
            }
            ExprKind::EnumVariant { enum_name, .. } => *enum_name = self.name(enum_name),
            ExprKind::StructLit { name, fields } => {
                *name = self.name(name);
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
            ExprKind::OffsetOf(ty, _) => self.ty(ty),
            _ => {}
        }
    }
}

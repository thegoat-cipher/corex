//! Semantic analysis: name resolution, type checking, mutability, `unsafe`
//! gating, and control-flow validation.
//!
//! Output is a side-table `Analysis` consumed by code generation. Analysis
//! never mutates the AST, so the tree remains usable for diagnostics and tests.
//!
//! Guarantees produced here (compile-time):
//!   - every identifier resolves to a declared local or function;
//!   - every expression has exactly one concrete type (integer literals are
//!     resolved against context, defaulting to `i32`);
//!   - assignment targets are mutable places;
//!   - pointer dereference, pointer arithmetic, integer<->pointer casts, heap
//!     allocation, and syscalls appear inside an `unsafe` block;
//!   - non-void functions do not fall off the end;
//!   - constant array indices are within bounds.
//!
//! Not guaranteed (programmer responsibility): pointer validity, allocation
//! lifetime, aliasing, alignment of hand-built pointers, and integer overflow.

use std::collections::HashMap;

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};

/// Names that are always in scope and cannot be redefined.
pub const BUILTINS: [&str; 4] = ["syscall", "alloc", "free", "print"];

/// Maximum size of a stack-allocated value. Keeps `alloca` within a sane
/// fraction of the default 8 MiB process stack and prevents overflow in size
/// arithmetic.
pub const MAX_VALUE_BYTES: u64 = 1 << 20;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Callee {
    User(String),
    Syscall,
    Alloc,
    Free,
    Print,
}

#[derive(Clone, Debug)]
pub struct FnSig {
    pub params: Vec<Type>,
    pub ret: Type,
}

#[derive(Clone, Debug)]
pub struct LocalRef {
    pub slot: usize,
    pub ty: Type,
}

#[derive(Clone, Debug, Default)]
pub struct FnLayout {
    /// Slot index -> type. Parameters occupy slots `0..params.len()`.
    pub slot_types: Vec<Type>,
}

#[derive(Clone, Debug, Default)]
pub struct BuiltinUsage {
    pub syscall: bool,
    pub alloc: bool,
    pub free: bool,
    pub print: bool,
}

#[derive(Clone, Debug, Default)]
pub struct Analysis {
    pub expr_types: HashMap<NodeId, Type>,
    pub idents: HashMap<NodeId, LocalRef>,
    /// `let` initializer expression id -> slot holding the binding.
    pub let_slots: HashMap<NodeId, usize>,
    pub calls: HashMap<NodeId, Callee>,
    pub fn_sigs: HashMap<String, FnSig>,
    pub layouts: HashMap<String, FnLayout>,
    pub builtins: BuiltinUsage,
    /// Functions whose body provably never falls through.
    pub diverging: std::collections::HashSet<String>,
    /// Name of the user program entry point, either `_start` or `main`.
    pub entry: String,
}

pub fn analyze(program: &Program) -> Result<Analysis, Vec<Diagnostic>> {
    let mut a = Analyzer {
        sigs: HashMap::new(),
        expr_types: HashMap::new(),
        idents: HashMap::new(),
        let_slots: HashMap::new(),
        calls: HashMap::new(),
        layouts: HashMap::new(),
        builtins: BuiltinUsage::default(),
        diverging: std::collections::HashSet::new(),
        diags: Vec::new(),
        scopes: Vec::new(),
        slot_types: Vec::new(),
        slot_mut: HashMap::new(),
        ret_ty: Type::Unit,
        loop_depth: 0,
        unsafe_depth: 0,
        current_fn: String::new(),
    };

    a.collect_fns(program);
    let entry = a.check_entry(program);

    for f in &program.fns {
        a.check_fn(f);
    }

    if a.diags.is_empty() {
        Ok(Analysis {
            expr_types: a.expr_types,
            idents: a.idents,
            let_slots: a.let_slots,
            calls: a.calls,
            fn_sigs: a.sigs,
            layouts: a.layouts,
            builtins: a.builtins,
            diverging: a.diverging,
            entry: entry.unwrap_or_default(),
        })
    } else {
        Err(a.diags)
    }
}

struct Analyzer {
    sigs: HashMap<String, FnSig>,
    expr_types: HashMap<NodeId, Type>,
    idents: HashMap<NodeId, LocalRef>,
    let_slots: HashMap<NodeId, usize>,
    calls: HashMap<NodeId, Callee>,
    layouts: HashMap<String, FnLayout>,
    builtins: BuiltinUsage,
    diverging: std::collections::HashSet<String>,
    diags: Vec<Diagnostic>,

    scopes: Vec<HashMap<String, LocalRef>>,
    slot_types: Vec<Type>,
    slot_mut: HashMap<usize, bool>,
    ret_ty: Type,
    loop_depth: u32,
    unsafe_depth: u32,
    current_fn: String,
}

impl Analyzer {
    fn error(&mut self, code: &'static str, span: Span, message: impl Into<String>) {
        self.diags.push(Diagnostic::error(code, span, message));
    }

    fn type_mismatch(&mut self, span: Span, expected: &Type, found: &Type, context: &str) {
        self.diags.push(
            Diagnostic::error(
                "E0203",
                span,
                format!(
                    "type mismatch{context}: expected `{}`, found `{}`",
                    expected.display(),
                    found.display()
                ),
            )
            .with_note("CoreX never converts integers implicitly; use `as` to convert"),
        );
    }

    // ----- top level ---------------------------------------------------

    fn collect_fns(&mut self, program: &Program) {
        for f in &program.fns {
            if BUILTINS.contains(&f.name.as_str()) {
                self.error(
                    "E0202",
                    f.name_span,
                    format!("`{}` is a reserved builtin name", f.name),
                );
            }
            if self.sigs.contains_key(&f.name) {
                self.error(
                    "E0202",
                    f.name_span,
                    format!("duplicate function `{}`", f.name),
                );
            }
            if f.ret.is_aggregate() {
                self.error(
                    "E0216",
                    f.ret_span,
                    "arrays cannot be returned by value; return a pointer instead",
                );
            }
            self.check_value_size(&f.ret, f.ret_span);
            for p in &f.params {
                if p.ty.is_aggregate() {
                    self.error(
                        "E0216",
                        p.span,
                        "arrays cannot be passed by value; pass a pointer instead",
                    );
                }
                if p.ty == Type::Unit {
                    self.error("E0216", p.span, "parameters cannot have type `void`");
                }
                self.check_value_size(&p.ty, p.span);
            }
            self.sigs.insert(
                f.name.clone(),
                FnSig {
                    params: f.params.iter().map(|p| p.ty.clone()).collect(),
                    ret: f.ret.clone(),
                },
            );
        }
    }

    fn check_entry(&mut self, program: &Program) -> Option<String> {
        if let Some(f) = program.fns.iter().find(|f| f.name == "_start") {
            if !f.params.is_empty() || f.ret != Type::Unit {
                self.error(
                    "E0217",
                    f.name_span,
                    "`_start` must be declared as `fn _start()` with no parameters and no return type",
                );
            }
            return Some("_start".to_string());
        }
        if let Some(f) = program.fns.iter().find(|f| f.name == "main") {
            if !f.params.is_empty() || f.ret != Type::Int(IntTy::I32) {
                self.error(
                    "E0217",
                    f.name_span,
                    "`main` must be declared as `fn main() -> i32`",
                );
            }
            return Some("main".to_string());
        }
        self.error(
            "E0212",
            Span::new(0, 0),
            "program has no entry point: define `fn main() -> i32` or `fn _start()`",
        );
        None
    }

    fn check_fn(&mut self, f: &FnDecl) {
        self.current_fn = f.name.clone();
        self.scopes = vec![HashMap::new()];
        self.slot_types = Vec::new();
        self.slot_mut = HashMap::new();
        self.ret_ty = f.ret.clone();
        self.loop_depth = 0;
        self.unsafe_depth = 0;

        // Parameters are immutable: the specification requires copying into a
        // `let mut` binding before mutating (E0205).
        for p in &f.params {
            self.declare(&p.name, p.span, p.ty.clone(), false);
        }

        self.check_block(&f.body);

        let diverges = block_diverges(&f.body);
        if diverges {
            self.diverging.insert(f.name.clone());
        }

        if f.ret != Type::Unit && !diverges {
            self.diags.push(
                Diagnostic::error(
                    "E0206",
                    f.name_span,
                    format!(
                        "function `{}` can reach the end of its body without returning a value of type `{}`",
                        f.name,
                        f.ret.display()
                    ),
                )
                .with_note(
                    "every path must end in `return`, a `loop` without `break`, a `while true` without `break`, \
                     or an `exit`/`exit_group` syscall (60 or 231); use `fn _start()` for programs that only \
                     terminate through the kernel",
                ),
            );
        }

        self.layouts.insert(
            f.name.clone(),
            FnLayout {
                slot_types: std::mem::take(&mut self.slot_types),
            },
        );
    }

    // ----- scopes ------------------------------------------------------

    fn push_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn declare(&mut self, name: &str, span: Span, ty: Type, mutable: bool) -> LocalRef {
        if BUILTINS.contains(&name) {
            self.error(
                "E0202",
                span,
                format!("`{name}` is a reserved builtin name and cannot be a binding"),
            );
        }
        if let Some(existing) = self.scopes.last().and_then(|s| s.get(name)) {
            let slot = existing.slot;
            self.error(
                "E0202",
                span,
                format!("`{name}` is already declared in this scope"),
            );
            return LocalRef { slot, ty };
        }
        let slot = self.slot_types.len();
        self.slot_types.push(ty.clone());
        self.slot_mut.insert(slot, mutable);
        let r = LocalRef { slot, ty };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), r.clone());
        }
        r
    }

    fn lookup(&self, name: &str) -> Option<LocalRef> {
        for scope in self.scopes.iter().rev() {
            if let Some(r) = scope.get(name) {
                return Some(r.clone());
            }
        }
        None
    }

    // ----- statements --------------------------------------------------

    fn check_block(&mut self, block: &Block) {
        self.push_scope();
        for s in &block.stmts {
            self.check_stmt(s);
        }
        self.pop_scope();
    }

    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let {
                name,
                name_span,
                ty,
                mutable,
                init,
                ..
            } => {
                self.check_value_size(ty, *name_span);
                if let Ok(found) = self.check_expr(init, Some(ty)) {
                    if !self.compatible(ty, &found) {
                        self.type_mismatch(init.span, ty, &found, " in `let` initializer");
                    }
                }
                let local = self.declare(name, *name_span, ty.clone(), *mutable);
                self.let_slots.insert(init.id, local.slot);
            }
            Stmt::Assign {
                target,
                value,
                span,
            } => {
                let Ok((ty, mutable)) = self.check_place(target) else {
                    return;
                };
                if let Ok(found) = self.check_expr(value, Some(&ty)) {
                    if !self.compatible(&ty, &found) {
                        self.type_mismatch(value.span, &ty, &found, " in assignment");
                    }
                }
                if !mutable {
                    self.error(
                        "E0205",
                        *span,
                        "cannot assign to an immutable place",
                    );
                }
            }
            Stmt::Expr(e) => {
                let _ = self.check_expr(e, None);
            }
            Stmt::If {
                cond,
                then_block,
                else_branch,
                ..
            } => {
                self.check_condition(cond);
                self.check_block(then_block);
                if let Some(branch) = else_branch {
                    self.check_stmt(branch);
                }
            }
            Stmt::While { cond, body, .. } => {
                self.check_condition(cond);
                self.loop_depth += 1;
                self.check_block(body);
                self.loop_depth -= 1;
            }
            Stmt::Loop { body, .. } => {
                self.loop_depth += 1;
                self.check_block(body);
                self.loop_depth -= 1;
            }
            Stmt::Break(span) => {
                if self.loop_depth == 0 {
                    self.error("E0207", *span, "`break` outside of a loop");
                }
            }
            Stmt::Continue(span) => {
                if self.loop_depth == 0 {
                    self.error("E0208", *span, "`continue` outside of a loop");
                }
            }
            Stmt::Return { value, span } => match (value, self.ret_ty.clone()) {
                (None, Type::Unit) => {}
                (None, ret) => {
                    self.error(
                        "E0206",
                        *span,
                        format!("`return` without a value in a function returning `{}`", ret.display()),
                    );
                }
                (Some(_), Type::Unit) => {
                    self.error(
                        "E0219",
                        *span,
                        "`return` with a value in a function returning no value",
                    );
                }
                (Some(e), ret) => {
                    if let Ok(found) = self.check_expr(e, Some(&ret)) {
                        if !self.compatible(&ret, &found) {
                            self.type_mismatch(e.span, &ret, &found, " in `return`");
                        }
                    }
                }
            },
            Stmt::Unsafe { body, .. } => {
                self.unsafe_depth += 1;
                self.check_block(body);
                self.unsafe_depth -= 1;
            }
            Stmt::Block(block) => {
                self.check_block(block);
            }
            Stmt::Empty(_) => {}
        }
    }

    fn check_condition(&mut self, cond: &Expr) {
        if let Ok(t) = self.check_expr(cond, Some(&Type::Bool)) {
            if t != Type::Bool {
                self.diags.push(
                    Diagnostic::error(
                        "E0214",
                        cond.span,
                        format!("condition must be `bool`, found `{}`", t.display()),
                    )
                    .with_note("CoreX has no implicit truthiness; compare explicitly"),
                );
            }
        }
    }

    /// A `*mut T` may be used where `*T` is expected; everything else must match
    /// exactly.
    /// Reject stack values larger than `MAX_VALUE_BYTES` (E0223), including
    /// types whose size would overflow 64-bit arithmetic (for example
    /// `[u64; 2^61]`). Overflow must be caught here: wrapping the multiplication
    /// could otherwise produce a small size and let an impossible array through.
    fn check_value_size(&mut self, ty: &Type, span: Span) {
        match checked_size(ty) {
            None => {
                self.error(
                    "E0223",
                    span,
                    format!(
                        "type `{}` has a size that does not fit in 64 bits",
                        ty.display()
                    ),
                );
            }
            Some(size) if size > MAX_VALUE_BYTES => {
                self.error(
                    "E0223",
                    span,
                    format!(
                        "value of type `{}` is {size} bytes; the stack allocation limit is {MAX_VALUE_BYTES} bytes",
                        ty.display()
                    ),
                );
            }
            _ => {}
        }
    }

    fn compatible(&self, expected: &Type, found: &Type) -> bool {
        if expected == found {
            return true;
        }
        matches!(
            (expected, found),
            (
                Type::Ptr { mutable: false, pointee: ep },
                Type::Ptr { mutable: true, pointee: fp }
            ) if ep == fp
        )
    }

    // ----- expressions -------------------------------------------------

    fn check_expr(&mut self, e: &Expr, expected: Option<&Type>) -> Result<Type, ()> {
        let ty = match &e.kind {
            ExprKind::Int(v) => {
                let t = match expected {
                    Some(Type::Int(t)) => *t,
                    _ => IntTy::I32,
                };
                if *v > t.max_literal() {
                    self.error(
                        "E0215",
                        e.span,
                        format!(
                            "integer literal `{v}` does not fit in `{}` (maximum {})",
                            t.name(),
                            t.max_literal()
                        ),
                    );
                    return Err(());
                }
                Type::Int(t)
            }
            ExprKind::Str(bytes) => Type::array_of(Type::Int(IntTy::U8), bytes.len() as u64),
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::Ident(name) => {
                if let Some(local) = self.lookup(name) {
                    self.idents.insert(
                        e.id,
                        LocalRef {
                            slot: local.slot,
                            ty: local.ty.clone(),
                        },
                    );
                    local.ty
                } else if self.sigs.contains_key(name) || BUILTINS.contains(&name.as_str()) {
                    self.error(
                        "E0218",
                        e.span,
                        format!("`{name}` is a function and cannot be used as a value"),
                    );
                    return Err(());
                } else {
                    self.error("E0201", e.span, format!("undefined name `{name}`"));
                    return Err(());
                }
            }
            ExprKind::Unary { op, expr } => match op {
                UnOp::Neg => {
                    let t = if let ExprKind::Int(v) = expr.kind {
                        match expected {
                            Some(Type::Int(t)) => {
                                let limit = if t.signed() {
                                    t.max_literal() + 1
                                } else {
                                    u64::MAX
                                };
                                if v > limit {
                                    self.error(
                                        "E0215",
                                        expr.span,
                                        format!(
                                            "integer literal `-{v}` does not fit in `{}`",
                                            t.name()
                                        ),
                                    );
                                    return Err(());
                                }
                                Type::Int(*t)
                            }
                            _ => Type::Int(IntTy::I32),
                        }
                    } else {
                        let t = self.check_expr(expr, expected)?;
                        if !t.is_integer() {
                            self.error(
                                "E0210",
                                e.span,
                                format!("cannot negate a value of type `{}`", t.display()),
                            );
                            return Err(());
                        }
                        t
                    };
                    self.expr_types.insert(expr.id, t.clone());
                    t
                }
                UnOp::Not => {
                    let t = self.check_expr(expr, expected)?;
                    if t != Type::Bool && !t.is_integer() {
                        self.error(
                            "E0210",
                            e.span,
                            format!("cannot apply `!` to a value of type `{}`", t.display()),
                        );
                        return Err(());
                    }
                    t
                }
                UnOp::AddrOf => {
                    let (t, _) = self.check_place(expr)?;
                    Type::ptr_to(t, false)
                }
                UnOp::AddrOfMut => {
                    let (t, mutable) = self.check_place(expr)?;
                    if !mutable {
                        self.error(
                            "E0220",
                            e.span,
                            "`&mut` requires a mutable place",
                        );
                    }
                    Type::ptr_to(t, true)
                }
                UnOp::Deref => {
                    if self.unsafe_depth == 0 {
                        self.error(
                            "E0209",
                            e.span,
                            "pointer dereference is only allowed inside an `unsafe` block",
                        );
                    }
                    let pt = self.check_expr(expr, None)?;
                    match pt.pointee() {
                        Some(p) => {
                            self.expr_types.insert(expr.id, pt.clone());
                            p.clone()
                        }
                        None => {
                            self.error(
                                "E0210",
                                e.span,
                                format!("cannot dereference a value of type `{}`", pt.display()),
                            );
                            return Err(());
                        }
                    }
                }
            },
            ExprKind::Binary { op, lhs, rhs } => self.check_binary(e, *op, lhs, rhs, expected)?,
            ExprKind::Call { callee, args } => self.check_call(e, callee, args)?,
            ExprKind::Index { .. } => {
                let (t, _) = self.check_place(e)?;
                t
            }
            ExprKind::Cast { expr, ty } => {
                let src = self.check_expr(expr, None)?;
                match (&src, ty) {
                    (Type::Int(_), Type::Int(_)) | (Type::Ptr { .. }, Type::Ptr { .. }) => {}
                    (Type::Int(_), Type::Ptr { .. }) | (Type::Ptr { .. }, Type::Int(_)) => {
                        if self.unsafe_depth == 0 {
                            self.error(
                                "E0209",
                                e.span,
                                "integer/pointer conversion is only allowed inside an `unsafe` block",
                            );
                        }
                    }
                    _ => {
                        self.error(
                            "E0210",
                            e.span,
                            format!(
                                "invalid cast from `{}` to `{}`",
                                src.display(),
                                ty.display()
                            ),
                        );
                        return Err(());
                    }
                }
                ty.clone()
            }
            ExprKind::Array(elems) => {
                if elems.is_empty() {
                    self.error("E0110", e.span, "empty array literal");
                    return Err(());
                }
                let expected_elem = match expected {
                    Some(Type::Array { elem, len }) => {
                        if *len != elems.len() as u64 {
                            self.error(
                                "E0203",
                                e.span,
                                format!(
                                    "array literal has {} elements but type expects {}",
                                    elems.len(),
                                    len
                                ),
                            );
                        }
                        Some((**elem).clone())
                    }
                    Some(other) if *other != Type::Unit => {
                        let found = Type::array_of(Type::Int(IntTy::I32), elems.len() as u64);
                        self.type_mismatch(e.span, other, &found, " in array literal");
                        return Err(());
                    }
                    _ => None,
                };

                let first_ty = match expected_elem {
                    Some(t) => t,
                    None => self.check_expr(&elems[0], None)?,
                };
                for el in elems.iter() {
                    if let Ok(t) = self.check_expr(el, Some(&first_ty)) {
                        if !self.compatible(&first_ty, &t) {
                            self.type_mismatch(el.span, &first_ty, &t, " in array element");
                        }
                    }
                }
                if first_ty == Type::Unit {
                    self.error("E0216", e.span, "array elements cannot be `void`");
                    return Err(());
                }
                Type::array_of(first_ty, elems.len() as u64)
            }
            ExprKind::ArrayRepeat { value, count } => {
                if *count == 0 {
                    self.error("E0110", e.span, "array repeat count must be greater than zero");
                    return Err(());
                }
                if *count > 1024 {
                    self.error(
                        "E0221",
                        e.span,
                        format!("array repeat count {count} exceeds the limit of 1024"),
                    );
                    return Err(());
                }
                let expected_elem = match expected {
                    Some(Type::Array { elem, len }) => {
                        if *len != *count {
                            self.error(
                                "E0203",
                                e.span,
                                format!(
                                    "array literal has {count} elements but type expects {len}"
                                ),
                            );
                        }
                        Some((**elem).clone())
                    }
                    Some(other) if *other != Type::Unit => {
                        let found = Type::array_of(Type::Int(IntTy::I32), *count);
                        self.type_mismatch(e.span, other, &found, " in array literal");
                        return Err(());
                    }
                    _ => None,
                };
                let t = self.check_expr(value, expected_elem.as_ref())?;
                if t == Type::Unit {
                    self.error("E0216", e.span, "array elements cannot be `void`");
                    return Err(());
                }
                Type::array_of(t, *count)
            }
        };
        self.expr_types.insert(e.id, ty.clone());
        Ok(ty)
    }

    fn check_binary(
        &mut self,
        e: &Expr,
        op: BinOp,
        lhs: &Expr,
        rhs: &Expr,
        expected: Option<&Type>,
    ) -> Result<Type, ()> {
        if matches!(op, BinOp::LogicalAnd | BinOp::LogicalOr) {
            for side in [lhs, rhs] {
                if let Ok(t) = self.check_expr(side, Some(&Type::Bool)) {
                    if t != Type::Bool {
                        self.error(
                            "E0203",
                            side.span,
                            format!(
                                "`{}` requires `bool` operands, found `{}`",
                                op.describe(),
                                t.display()
                            ),
                        );
                    }
                }
            }
            return Ok(Type::Bool);
        }

        // Pointer arithmetic: `*T + usize` / `*T - usize`.
        if matches!(op, BinOp::Add | BinOp::Sub) {
            let lhs_is_literal = is_int_literal(lhs);
            if !lhs_is_literal {
                let lt = self.check_expr(lhs, None)?;
                if lt.is_pointer() {
                    let ut = Type::Int(IntTy::Usize);
                    if let Ok(rt) = self.check_expr(rhs, Some(&ut)) {
                        if rt != ut {
                            self.error(
                                "E0203",
                                rhs.span,
                                format!(
                                    "pointer arithmetic offset must be `usize`, found `{}`",
                                    rt.display()
                                ),
                            );
                        }
                    }
                    if self.unsafe_depth == 0 {
                        self.error(
                            "E0209",
                            e.span,
                            "pointer arithmetic is only allowed inside an `unsafe` block",
                        );
                    }
                    // Re-check lhs under no expectation already stored.
                    return Ok(lt);
                }
            }
        }

        let decided = self.unify_operands(lhs, rhs, expected)?;

        if op.is_comparison() {
            match &decided {
                Type::Int(_) => {}
                Type::Ptr { .. } if matches!(op, BinOp::Eq | BinOp::Ne) => {}
                Type::Ptr { .. } => {
                    self.error(
                        "E0210",
                        e.span,
                        "pointers support only `==` and `!=` comparisons; convert with `as usize` for ordering"
                            .to_string(),
                    );
                }
                other => {
                    self.error(
                        "E0210",
                        e.span,
                        format!(
                            "`{}` cannot compare values of type `{}`",
                            op.describe(),
                            other.display()
                        ),
                    );
                    return Err(());
                }
            }
            return Ok(Type::Bool);
        }

        if !decided.is_integer() {
            self.error(
                "E0210",
                e.span,
                format!(
                    "`{}` requires integer operands, found `{}`",
                    op.describe(),
                    decided.display()
                ),
            );
            return Err(());
        }
        Ok(decided)
    }

    /// Resolve operand types, letting untyped integer literals adopt the type of
    /// the other operand or the expected type. No implicit conversion happens
    /// between non-literal operands.
    fn unify_operands(
        &mut self,
        lhs: &Expr,
        rhs: &Expr,
        expected: Option<&Type>,
    ) -> Result<Type, ()> {
        let lhs_lit = is_int_literal(lhs);
        let rhs_lit = is_int_literal(rhs);

        let (lt, rt) = if lhs_lit && !rhs_lit {
            let rt = self.check_expr(rhs, expected)?;
            let lt = self.check_expr(lhs, Some(&rt))?;
            (lt, rt)
        } else if rhs_lit && !lhs_lit {
            let lt = self.check_expr(lhs, expected)?;
            let rt = self.check_expr(rhs, Some(&lt))?;
            (lt, rt)
        } else {
            let lt = self.check_expr(lhs, expected)?;
            let rt = self.check_expr(rhs, Some(&lt))?;
            (lt, rt)
        };

        if lt != rt {
            self.type_mismatch(rhs.span, &lt, &rt, " between operands");
            return Err(());
        }
        Ok(lt)
    }

    fn check_call(&mut self, e: &Expr, callee: &Expr, args: &[Expr]) -> Result<Type, ()> {
        let ExprKind::Ident(name) = &callee.kind else {
            self.error(
                "E0218",
                callee.span,
                "call target must be a function name (CoreX has no function pointers)",
            );
            return Err(());
        };

        // `print` is polymorphic over integers and `bool`, so it is checked
        // before the uniform argument loop below.
        if name == "print" {
            self.builtins.print = true;
            if args.len() != 1 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`print` takes 1 argument, found {}", args.len()),
                );
                return Err(());
            }
            let arg_ty = self.check_expr(&args[0], None)?;
            if !arg_ty.is_integer() && arg_ty != Type::Bool {
                self.diags.push(
                    Diagnostic::error(
                        "E0203",
                        args[0].span,
                        format!(
                            "`print` accepts integer or `bool` values, found `{}`",
                            arg_ty.display()
                        ),
                    )
                    .with_note(
                        "CoreX has no generic formatting; pointers and arrays are not printable",
                    ),
                );
                return Err(());
            }
            self.calls.insert(e.id, Callee::Print);
            return Ok(Type::Unit);
        }

        let (params, ret, builtin) = match name.as_str() {
            "syscall" => {
                self.builtins.syscall = true;
                if args.is_empty() || args.len() > 7 {
                    self.error(
                        "E0204",
                        e.span,
                        format!("`syscall` takes 1 to 7 arguments, found {}", args.len()),
                    );
                    return Err(());
                }
                if self.unsafe_depth == 0 {
                    self.error(
                        "E0209",
                        e.span,
                        "`syscall` is only allowed inside an `unsafe` block",
                    );
                }
                let p = vec![Type::Int(IntTy::Usize); args.len()];
                (p, Type::Int(IntTy::Isize), Callee::Syscall)
            }
            "alloc" => {
                self.builtins.alloc = true;
                if args.len() != 1 {
                    self.error(
                        "E0204",
                        e.span,
                        format!("`alloc` takes 1 argument, found {}", args.len()),
                    );
                    return Err(());
                }
                if self.unsafe_depth == 0 {
                    self.error(
                        "E0209",
                        e.span,
                        "`alloc` is only allowed inside an `unsafe` block",
                    );
                }
                (
                    vec![Type::Int(IntTy::Usize)],
                    Type::ptr_to(Type::Int(IntTy::U8), true),
                    Callee::Alloc,
                )
            }
            "free" => {
                self.builtins.free = true;
                if args.len() != 2 {
                    self.error(
                        "E0204",
                        e.span,
                        format!("`free` takes 2 arguments (pointer, byte count), found {}", args.len()),
                    );
                    return Err(());
                }
                if self.unsafe_depth == 0 {
                    self.error(
                        "E0209",
                        e.span,
                        "`free` is only allowed inside an `unsafe` block",
                    );
                }
                (
                    vec![
                        Type::ptr_to(Type::Int(IntTy::U8), true),
                        Type::Int(IntTy::Usize),
                    ],
                    Type::Unit,
                    Callee::Free,
                )
            }
            other => {
                let Some(sig) = self.sigs.get(other).cloned() else {
                    self.error("E0201", callee.span, format!("undefined function `{other}`"));
                    return Err(());
                };
                if sig.params.len() != args.len() {
                    self.error(
                        "E0204",
                        e.span,
                        format!(
                            "`{other}` takes {} argument(s), found {}",
                            sig.params.len(),
                            args.len()
                        ),
                    );
                    return Err(());
                }
                (sig.params.clone(), sig.ret.clone(), Callee::User(other.to_string()))
            }
        };

        for (arg, param) in args.iter().zip(params.iter()) {
            if let Ok(t) = self.check_expr(arg, Some(param)) {
                if !self.compatible(param, &t) {
                    self.type_mismatch(arg.span, param, &t, " in argument");
                }
            }
        }

        self.calls.insert(e.id, builtin);
        Ok(ret)
    }

    /// Type and mutability of a place expression, and checks it.
    fn check_place(&mut self, e: &Expr) -> Result<(Type, bool), ()> {
        match &e.kind {
            ExprKind::Ident(name) => {
                let Some(local) = self.lookup(name) else {
                    if self.sigs.contains_key(name) || BUILTINS.contains(&name.as_str()) {
                        self.error(
                            "E0218",
                            e.span,
                            format!("`{name}` is a function, not a place"),
                        );
                    } else {
                        self.error("E0201", e.span, format!("undefined name `{name}`"));
                    }
                    return Err(());
                };
                self.idents.insert(
                    e.id,
                    LocalRef {
                        slot: local.slot,
                        ty: local.ty.clone(),
                    },
                );
                let mutable = *self.slot_mut.get(&local.slot).unwrap_or(&false);
                self.expr_types.insert(e.id, local.ty.clone());
                Ok((local.ty, mutable))
            }
            ExprKind::Unary { op: UnOp::Deref, expr } => {
                if self.unsafe_depth == 0 {
                    self.error(
                        "E0209",
                        e.span,
                        "pointer dereference is only allowed inside an `unsafe` block",
                    );
                }
                let pt = self.check_expr(expr, None)?;
                let Some(pointee) = pt.pointee() else {
                    self.error(
                        "E0210",
                        e.span,
                        format!("cannot dereference a value of type `{}`", pt.display()),
                    );
                    return Err(());
                };
                self.expr_types.insert(e.id, pointee.clone());
                Ok((pointee.clone(), pt.is_mutable_pointer()))
            }
            ExprKind::Index { base, index } => {
                let base_ty = self.check_expr(base, None)?;
                let (elem, len) = match &base_ty {
                    Type::Array { elem, len } => ((**elem).clone(), *len),
                    Type::Ptr { .. } => {
                        self.error(
                            "E0213",
                            base.span,
                            "cannot index a pointer directly; compute an offset inside `unsafe` and dereference it",
                        );
                        return Err(());
                    }
                    other => {
                        self.error(
                            "E0213",
                            base.span,
                            format!("cannot index a value of type `{}`", other.display()),
                        );
                        return Err(());
                    }
                };
                let usize_ty = Type::Int(IntTy::Usize);
                if let Ok(it) = self.check_expr(index, Some(&usize_ty)) {
                    if it != usize_ty {
                        self.error(
                            "E0213",
                            index.span,
                            format!(
                                "array index must be `usize`, found `{}`",
                                it.display()
                            ),
                        );
                    }
                }
                if let ExprKind::Int(v) = index.kind {
                    if v >= len {
                        self.error(
                            "E0222",
                            index.span,
                            format!(
                                "constant index {v} is out of bounds for `[{}; {len}]`",
                                elem.display()
                            ),
                        );
                    }
                }
                let base_mutable = self.place_mutability(base);
                self.expr_types.insert(e.id, elem.clone());
                Ok((elem, base_mutable))
            }
            ExprKind::Str(bytes) => Ok((
                Type::array_of(Type::Int(IntTy::U8), bytes.len() as u64),
                false,
            )),
            _ => {
                self.error("E0211", e.span, "expression is not a place (cannot take address or assign)");
                Err(())
            }
        }
    }

    fn place_mutability(&self, e: &Expr) -> bool {
        match &e.kind {
            ExprKind::Ident(name) => self
                .lookup(name)
                .map(|l| *self.slot_mut.get(&l.slot).unwrap_or(&false))
                .unwrap_or(false),
            ExprKind::Unary { op: UnOp::Deref, expr } => self
                .expr_types
                .get(&expr.id)
                .map(|t| t.is_mutable_pointer())
                .unwrap_or(false),
            ExprKind::Index { base, .. } => self.place_mutability(base),
            _ => false,
        }
    }
}

/// Size of a type in bytes, using checked arithmetic at every level.
fn checked_size(ty: &Type) -> Option<u64> {
    match ty {
        Type::Unit => Some(0),
        Type::Bool => Some(1),
        Type::Int(t) => Some((t.bits() / 8) as u64),
        Type::Ptr { .. } => Some(8),
        Type::Array { elem, len } => checked_size(elem)?.checked_mul(*len),
    }
}

fn is_int_literal(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Int(_) => true,
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } => matches!(expr.kind, ExprKind::Int(_)),
        _ => false,
    }
}

fn block_diverges(block: &Block) -> bool {
    block.stmts.iter().any(stmt_diverges)
}

fn stmt_diverges(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return { .. } => true,
        Stmt::If {
            then_block,
            else_branch: Some(branch),
            ..
        } => block_diverges(then_block) && stmt_diverges(branch),
        Stmt::Loop { body, .. } => !block_contains_break(body),
        // `while true { ... }` behaves like `loop` as long as it has no `break`.
        Stmt::While { cond, body, .. } => {
            matches!(cond.kind, ExprKind::Bool(true)) && !block_contains_break(body)
        }
        Stmt::Unsafe { body, .. } => block_diverges(body),
        Stmt::Block(body) => block_diverges(body),
        // `exit` (60) and `exit_group` (231) never return to the caller.
        Stmt::Expr(e) => is_process_exit(e),
        _ => false,
    }
}

/// True for `syscall(60, ...)` and `syscall(231, ...)`, which terminate the
/// process and therefore never fall through.
fn is_process_exit(e: &Expr) -> bool {
    let ExprKind::Call { callee, args } = &e.kind else {
        return false;
    };
    let ExprKind::Ident(name) = &callee.kind else {
        return false;
    };
    if name != "syscall" {
        return false;
    }
    matches!(args.first().and_then(const_int), Some(60) | Some(231))
}

/// Integer literal constants, including `as` casts of literals (`60 as usize`).
fn const_int(e: &Expr) -> Option<u64> {
    match &e.kind {
        ExprKind::Int(v) => Some(*v),
        ExprKind::Cast { expr, ty } if ty.is_integer() => const_int(expr),
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } => const_int(expr).map(|v| v.wrapping_neg()),
        _ => None,
    }
}

/// Breaks belonging to nested loops do not make the outer loop fall through.
fn block_contains_break(block: &Block) -> bool {
    block.stmts.iter().any(stmt_contains_break)
}

fn stmt_contains_break(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Break(_) => true,
        Stmt::If {
            then_block,
            else_branch,
            ..
        } => {
            block_contains_break(then_block)
                || else_branch.as_ref().map(|b| stmt_contains_break(b)).unwrap_or(false)
        }
        Stmt::Unsafe { body, .. } => block_contains_break(body),
        Stmt::Block(body) => block_contains_break(body),
        Stmt::While { .. } | Stmt::Loop { .. } => false,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer::lex;
    use crate::parser::parse;

    fn errs(src: &str) -> Vec<Diagnostic> {
        let toks = lex(src).expect("lex ok");
        let program = parse(&toks).expect("parse ok");
        analyze(&program).expect_err("analysis should fail")
    }

    fn ok(src: &str) -> Analysis {
        let toks = lex(src).expect("lex ok");
        let program = parse(&toks).expect("parse ok");
        analyze(&program).expect("analysis should pass")
    }

    const FACT: &str = r#"
        fn fact(n: u64) -> u64 {
            if n <= 1 { return 1; }
            return n * fact(n - 1);
        }
        fn main() -> i32 {
            let n: u64 = 10;
            let r: u64 = fact(n);
            return 0;
        }
    "#;

    #[test]
    fn accepts_factorial_program() {
        let a = ok(FACT);
        assert_eq!(a.entry, "main");
        let sig = a.fn_sigs.get("fact").unwrap();
        assert_eq!(sig.params, vec![Type::Int(IntTy::U64)]);
        assert_eq!(sig.ret, Type::Int(IntTy::U64));
    }

    #[test]
    fn rejects_type_mismatch_without_implicit_conversion() {
        let e = errs("fn main() -> i32 { let x: i32 = 1; let y: u8 = 2; return x + y; }");
        assert!(e.iter().any(|d| d.code == "E0203"), "{e:?}");
    }

    #[test]
    fn rejects_immutable_assignment() {
        let e = errs("fn main() -> i32 { let x: i32 = 1; x = 2; return x; }");
        assert!(e.iter().any(|d| d.code == "E0205"), "{e:?}");
    }

    #[test]
    fn rejects_mutable_pointer_via_immutable_binding() {
        let e = errs("fn main() -> i32 { let p: *mut i32 = 0 as *mut i32; unsafe { *p = 1; } return 0; }");
        assert!(!e.is_empty());
    }

    #[test]
    fn rejects_deref_outside_unsafe() {
        let e = errs("fn f(p: *mut u8) -> u8 { return *p; } fn main() -> i32 { return 0; }");
        assert!(e.iter().any(|d| d.code == "E0209"), "{e:?}");
    }

    #[test]
    fn accepts_deref_inside_unsafe() {
        let a = ok(
            "fn f(p: *mut u8) -> u8 { unsafe { return *p; } }\nfn main() -> i32 { return 0; }",
        );
        assert!(a.expr_types.values().any(|t| *t == Type::Int(IntTy::U8)));
    }

    #[test]
    fn rejects_missing_return() {
        let e = errs("fn f(b: bool) -> i32 { if b { return 1; } } fn main() -> i32 { return 0; }");
        assert!(e.iter().any(|d| d.code == "E0206"), "{e:?}");
    }

    #[test]
    fn accepts_diverging_loop_without_return() {
        ok("fn f() -> i32 { loop { } } fn main() -> i32 { return 0; }");
    }

    #[test]
    fn accepts_exit_syscall_as_a_diverging_tail() {
        ok("fn main() -> i32 { unsafe { syscall(60, 0, 0, 0, 0, 0, 0); } }");
        ok("fn main() -> i32 { unsafe { syscall(231, 0, 0, 0, 0, 0, 0); } }");
        // A cast of a literal is still a constant exit number.
        ok("fn main() -> i32 { unsafe { syscall(60 as usize, 0, 0, 0, 0, 0, 0); } }");
    }

    #[test]
    fn builtin_names_cannot_be_bindings_or_parameters() {
        for src in [
            "fn main() -> i32 { let print: i32 = 1; return 0; }",
            "fn f(syscall: i32) {} fn main() -> i32 { return 0; }",
            "fn main() -> i32 { let alloc: u8 = 0; return 0; }",
            "fn main() -> i32 { let free: u8 = 0; return 0; }",
        ] {
            let e = errs(src);
            assert!(e.iter().any(|d| d.code == "E0202"), "{src} -> {e:?}");
        }
    }

    #[test]
    fn rejects_stack_values_above_one_mib() {
        let e = errs("fn main() -> i32 { let a: [u8; 1048577] = [0; 1]; return 0; }");
        assert!(e.iter().any(|d| d.code == "E0223"), "{e:?}");
    }

    #[test]
    fn accepts_while_true_without_break() {
        ok("fn main() -> i32 { while true { } }");
        ok("fn main() -> i32 { while true { unsafe { syscall(1, 1, 0 as usize, 0); } } }");
    }

    #[test]
    fn rejects_while_true_with_break() {
        let e = errs("fn main() -> i32 { while true { break; } }");
        assert!(e.iter().any(|d| d.code == "E0206"), "{e:?}");
    }

    #[test]
    fn rejects_non_exit_syscall_fallthrough() {
        let e = errs("fn main() -> i32 { unsafe { syscall(1, 1, 0 as usize, 1); } }");
        assert!(e.iter().any(|d| d.code == "E0206"), "{e:?}");
    }

    #[test]
    fn rejects_break_outside_loop() {
        let e = errs("fn main() -> i32 { break; }");
        assert!(e.iter().any(|d| d.code == "E0207"), "{e:?}");
    }

    #[test]
    fn rejects_unknown_name_and_function_value() {
        let e = errs("fn main() -> i32 { return nope; }");
        assert!(e.iter().any(|d| d.code == "E0201"), "{e:?}");
        let e = errs("fn g() {} fn main() -> i32 { let x: u8 = 1; let y: i32 = g; return 0; }");
        assert!(e.iter().any(|d| d.code == "E0218" || d.code == "E0203"), "{e:?}");
    }

    #[test]
    fn rejects_wrong_arity_and_condition_type() {
        let e = errs("fn g(a: i32) -> i32 { return a; } fn main() -> i32 { return g(1, 2); }");
        assert!(e.iter().any(|d| d.code == "E0204"), "{e:?}");
        let e = errs("fn main() -> i32 { if 1 { return 1; } return 0; }");
        assert!(e.iter().any(|d| d.code == "E0214"), "{e:?}");
    }

    #[test]
    fn rejects_literal_overflow_and_bad_index_type() {
        let e = errs("fn main() -> i32 { let x: u8 = 256; return 0; }");
        assert!(e.iter().any(|d| d.code == "E0215"), "{e:?}");
        let e = errs("fn main() -> i32 { let a: [u8; 2] = [1, 2]; let i: u32 = 1; return a[i] as i32; }");
        assert!(e.iter().any(|d| d.code == "E0213"), "{e:?}");
        let e = errs("fn main() -> i32 { let a: [u8; 2] = [1, 2]; return a[2] as i32; }");
        assert!(e.iter().any(|d| d.code == "E0222"), "{e:?}");
    }

    #[test]
    fn rejects_array_type_whose_size_overflows_u64() {
        let e = errs(
            "fn main() -> i32 { let a: [u64; 2305843009213693952] = [0; 1]; return 0; }",
        );
        assert!(e.iter().any(|d| d.code == "E0223"), "{e:?}");
    }

    #[test]
    fn parameters_are_immutable() {
        let e = errs("fn f(n: i32) -> i32 { n = n + 1; return n; } fn main() -> i32 { return 0; }");
        assert!(e.iter().any(|d| d.code == "E0205"), "{e:?}");

        let e = errs("fn f(n: i32) { let p: *mut i32 = &mut n; } fn main() -> i32 { return 0; }");
        assert!(e.iter().any(|d| d.code == "E0220"), "{e:?}");
    }

    #[test]
    fn print_is_safe_and_accepts_integers_and_bool() {
        ok("fn main() -> i32 { print(42); print(true); print(-7); return 0; }");
    }

    #[test]
    fn print_rejects_non_scalar_and_wrong_arity() {
        let e = errs("fn main() -> i32 { print(); return 0; }");
        assert!(e.iter().any(|d| d.code == "E0204"), "{e:?}");
        let e = errs("fn main() -> i32 { let a: [u8; 2] = [1, 2]; print(a); return 0; }");
        assert!(e.iter().any(|d| d.code == "E0203"), "{e:?}");
        let e = errs("fn f(p: *mut u8) { print(p); } fn main() -> i32 { return 0; }");
        assert!(e.iter().any(|d| d.code == "E0203"), "{e:?}");
    }

    #[test]
    fn rejects_builtin_outside_unsafe_and_missing_entry() {
        let e = errs("fn main() -> i32 { syscall(60, 1); return 0; }");
        assert!(e.iter().any(|d| d.code == "E0209"), "{e:?}");
        let e = errs("fn f() {}");
        assert!(e.iter().any(|d| d.code == "E0212"), "{e:?}");
    }

    #[test]
    fn records_layouts_and_slots() {
        let a = ok("fn main() -> i32 { let mut x: i32 = 1; let y: u8 = 2; return x; }");
        let layout = a.layouts.get("main").unwrap();
        assert_eq!(
            layout.slot_types,
            vec![Type::Int(IntTy::I32), Type::Int(IntTy::U8)]
        );
    }

    #[test]
    fn default_integer_literal_is_i32_and_adapts() {
        let a = ok("fn f(x: u8) -> u8 { return x; } fn main() -> i32 { let a: i32 = 1; let b: u8 = 2; let c: u8 = f(3); return a; }");
        // `let a: i32 = 1;` literal resolved to i32, `f(3)` argument to u8.
        let lit_types: Vec<&Type> = a.expr_types.values().collect();
        assert!(lit_types.iter().any(|t| **t == Type::Int(IntTy::U8)));
        assert!(lit_types.iter().any(|t| **t == Type::Int(IntTy::I32)));
    }

    #[test]
    fn pointer_and_array_places_are_typed() {
        let a = ok("fn main() -> i32 { let mut a: [u8; 2] = [1, 2]; let p: *mut u8 = &mut a[0]; unsafe { *p = 3; } return a[0] as i32; }");
        assert_eq!(a.layouts.get("main").unwrap().slot_types.len(), 2);
    }
}

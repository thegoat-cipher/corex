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

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};

use crate::ast::*;
use crate::diagnostics::{Diagnostic, Span};

/// Names that are always in scope and cannot be redefined.
pub const BUILTINS: [&str; 13] = ["syscall", "alloc", "free", "print", "len", "vec_new", "vec_push", "vec_free", "vec_slice", "read_file", "write_file", "panic", "assert"];

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
    Len,
    VecNew,
    VecPush,
    VecFree,
    VecSlice,
    ReadFile,
    WriteFile,
    Panic,
    Assert,
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

#[derive(Clone, Debug)]
pub struct GlobalDef {
    pub name: String,
    pub ty: Type,
    pub mutable: bool,
    pub value: ConstValue,
}

#[derive(Clone, Debug, Default)]
pub struct EnumInfo {
    /// Variants in declaration order with their i32 discriminants.
    pub variants: Vec<(String, i32)>,
}

#[derive(Clone, Debug, Default)]
pub struct BuiltinUsage {
    pub syscall: bool,
    pub alloc: bool,
    pub free: bool,
    pub print: bool,
    pub len: bool,
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
    pub globals: Vec<GlobalDef>,
    pub consts: HashMap<String, (Type, ConstValue)>,
    pub enums: HashMap<String, EnumInfo>,
    pub cast_types: HashMap<NodeId, Type>,
    /// `&array` expression id -> array length when coerced to a slice.
    pub slice_from_array: HashMap<NodeId, u64>,
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
        globals: HashMap::new(),
        consts: HashMap::new(),
        global_order: Vec::new(),
        global_inits: HashMap::new(),
        enum_defs: HashMap::new(),
        struct_defs: HashMap::new(),
        struct_state: HashMap::new(),
        cast_types: HashMap::new(),
        slice_from_array: HashMap::new(),
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

    a.collect_enums(program);
    a.collect_structs(program);
    a.collect_fns(program);
    a.collect_globals(program);
    a.collect_consts(program);
    let entry = a.check_entry(program);
    a.check_global_inits(program);

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
            consts: a.consts,
            globals: a
                .global_order
                .iter()
                .filter_map(|name| {
                    a.globals.get(name).map(|(ty, mutable)| GlobalDef {
                        name: name.clone(),
                        ty: ty.clone(),
                        mutable: *mutable,
                        value: a
                            .global_inits
                            .get(name)
                            .cloned()
                            .unwrap_or(ConstValue::Int(0)),
                    })
                })
                .collect(),
            enums: a.enum_defs.clone(),
            cast_types: a.cast_types,
            slice_from_array: a.slice_from_array,
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
    globals: HashMap<String, (Type, bool)>,
    consts: HashMap<String, (Type, ConstValue)>,
    global_order: Vec<String>,
    global_inits: HashMap<String, ConstValue>,
    enum_defs: HashMap<String, EnumInfo>,
    struct_defs: HashMap<String, std::rc::Rc<StructInfo>>,
    struct_state: HashMap<String, u8>,
    cast_types: HashMap<NodeId, Type>,
    slice_from_array: HashMap<NodeId, u64>,
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

    fn collect_enums(&mut self, program: &Program) {
        for ed in &program.enums {
            if self.enum_defs.contains_key(&ed.name) {
                self.error("E0228", ed.name_span, format!("duplicate type `{}`", ed.name));
            }
            let mut seen = HashSet::new();
            let mut next: i64 = 0;
            let mut variants = Vec::new();
            for v in &ed.variants {
                if !seen.insert(v.name.clone()) {
                    self.error(
                        "E0228",
                        v.span,
                        format!("duplicate variant `{}` in enum `{}`", v.name, ed.name),
                    );
                }
                let value = match &v.value {
                    Some(expr) => match eval_int_const(expr) {
                        Some(n) => n,
                        None => {
                            self.error(
                                "E0221",
                                expr.span,
                                "enum discriminants must be integer constant expressions",
                            );
                            0
                        }
                    },
                    None => next,
                };
                if value < i32::MIN as i64 || value > i32::MAX as i64 {
                    self.error(
                        "E0215",
                        v.span,
                        format!("enum discriminant `{value}` does not fit in `i32`"),
                    );
                }
                next = value.saturating_add(1);
                variants.push((v.name.clone(), value as i32));
            }
            self.enum_defs.insert(
                ed.name.clone(),
                EnumInfo {
                    variants,
                },
            );
        }
    }

    fn resolve_type(&mut self, ty: &Type, span: Span) -> Type {
        match ty {
            Type::Named(name) => {
                if self.enum_defs.contains_key(name) {
                    Type::Enum(name.clone())
                } else if let Some(info) = self.struct_defs.get(name) {
                    Type::Struct(info.clone())
                } else {
                    self.error("E0201", span, format!("undefined type `{name}`"));
                    Type::Unit
                }
            }
            Type::Ptr { mutable, pointee } => {
                let pointee = self.resolve_type(pointee, span);
                Type::ptr_to(pointee, *mutable)
            }
            Type::Array { elem, len } => {
                let elem = self.resolve_type(elem, span);
                Type::array_of(elem, *len)
            }
            Type::Slice(elem) => Type::Slice(Box::new(self.resolve_type(elem, span))),
            Type::Vec(elem) => Type::Vec(Box::new(self.resolve_type(elem, span))),
            other => other.clone(),
        }
    }

    fn collect_structs(&mut self, program: &Program) {
        for st in &program.structs {
            if self.enum_defs.contains_key(&st.name) || self.struct_defs.contains_key(&st.name) {
                self.error("E0228", st.name_span, format!("duplicate type `{}`", st.name));
            }
            if st.fields.is_empty() {
                self.error("E0228", st.name_span, "structs must have at least one field");
            }
            let info = std::rc::Rc::new(StructInfo {
                name: st.name.clone(),
                fields: RefCell::new(Vec::new()),
                size: Cell::new(0),
                align: Cell::new(1),
                resolved: Cell::new(false),
            });
            self.struct_defs.entry(st.name.clone()).or_insert(info);
        }

        for st in &program.structs {
            let Some(info) = self.struct_defs.get(&st.name).cloned() else {
                continue;
            };
            let mut seen = HashSet::new();
            let mut resolved_fields = Vec::new();
            for f in &st.fields {
                if !seen.insert(f.name.clone()) {
                    self.error(
                        "E0228",
                        f.span,
                        format!("duplicate field `{}` in struct `{}`", f.name, st.name),
                    );
                }
                let ty = self.resolve_type(&f.ty, f.span);
                resolved_fields.push(StructField {
                    name: f.name.clone(),
                    ty,
                    offset: 0,
                });
            }
            *info.fields.borrow_mut() = resolved_fields;
        }

        for st in &program.structs {
            self.finalize_struct(&st.name, st.name_span);
        }
    }

    fn finalize_struct(&mut self, name: &str, span: Span) -> Option<(u64, u64)> {
        match self.struct_state.get(name).copied() {
            Some(2) => {
                let info = self.struct_defs.get(name)?;
                return Some((info.size.get(), info.align.get().max(1)));
            }
            Some(1) => {
                self.error(
                    "E0224",
                    span,
                    format!("struct `{name}` contains itself by value; use a pointer"),
                );
                return None;
            }
            _ => {}
        }
        self.struct_state.insert(name.to_string(), 1);
        let info = self.struct_defs.get(name)?.clone();

        let mut fields = info.fields.borrow().clone();
        for field in fields.iter_mut() {
            self.ensure_layout(&field.ty, span);
        }

        let mut offset: u64 = 0;
        let mut align: u64 = 1;
        for field in fields.iter_mut() {
            let fa = field.ty.align_bytes().max(1);
            offset = align_to(offset, fa);
            field.offset = offset;
            offset = offset.checked_add(field.ty.size_bytes())?;
            align = align.max(fa);
        }
        let size = align_to(offset, align);
        if size > MAX_VALUE_BYTES {
            self.error(
                "E0223",
                span,
                format!("struct `{name}` is {size} bytes; the stack allocation limit is {MAX_VALUE_BYTES} bytes"),
            );
        }
        *info.fields.borrow_mut() = fields;
        info.size.set(size);
        info.align.set(align);
        info.resolved.set(true);
        self.struct_state.insert(name.to_string(), 2);
        Some((size, align))
    }

    fn ensure_layout(&mut self, ty: &Type, span: Span) {
        match ty {
            Type::Struct(info) => {
                if !info.resolved.get() {
                    self.finalize_struct(&info.name, span);
                }
            }
            Type::Array { elem, .. } => self.ensure_layout(elem, span),
            _ => {}
        }
    }

    fn collect_globals(&mut self, program: &Program) {
        for g in &program.globals {
            if BUILTINS.contains(&g.name.as_str()) {
                self.error(
                    "E0202",
                    g.name_span,
                    format!("`{}` is a reserved builtin name", g.name),
                );
            }
            if self.sigs.contains_key(&g.name) {
                self.error(
                    "E0202",
                    g.name_span,
                    format!("duplicate global `{}`: a function has the same name", g.name),
                );
            }
            if self.globals.contains_key(&g.name) {
                self.error(
                    "E0202",
                    g.name_span,
                    format!("duplicate global `{}`", g.name),
                );
                continue;
            }
            let ty = self.resolve_type(&g.ty, g.name_span);
            if ty == Type::Unit {
                self.error("E0216", g.name_span, "globals cannot have type `void`");
            }
            self.check_value_size(&ty, g.name_span);
            self.globals.insert(g.name.clone(), (ty, g.mutable));
            self.global_order.push(g.name.clone());
        }
    }

    fn collect_consts(&mut self, program: &Program) {
        for c in &program.consts {
            if BUILTINS.contains(&c.name.as_str()) {
                self.error(
                    "E0202",
                    c.name_span,
                    format!("`{}` is a reserved builtin name", c.name),
                );
            }
            if self.sigs.contains_key(&c.name)
                || self.globals.contains_key(&c.name)
                || self.consts.contains_key(&c.name)
            {
                self.error(
                    "E0202",
                    c.name_span,
                    format!("duplicate declaration `{}`", c.name),
                );
                continue;
            }
            let ty = self.resolve_type(&c.ty, c.name_span);
            self.check_value_size(&ty, c.name_span);
            let allowed = matches!(
                ty,
                Type::Int(_)
                    | Type::Bool
                    | Type::Char
                    | Type::Float(_)
                    | Type::Enum(_)
                    | Type::Str
                    | Type::Ptr { .. }
            );
            if !allowed {
                self.error(
                    "E0231",
                    c.name_span,
                    format!(
                        "`const` values must be scalar, `str`, or a pointer; found `{}`",
                        ty.display()
                    ),
                );
                continue;
            }
            let Ok(found) = self.check_expr(&c.value, Some(&ty)) else {
                continue;
            };
            if !self.compatible(&ty, &found) {
                self.type_mismatch(c.value.span, &ty, &found, " in constant initializer");
                continue;
            }
            if let Some(value) = self.const_value(&c.value, &ty) {
                self.consts.insert(c.name.clone(), (ty, value));
            }
        }
    }

    fn check_global_inits(&mut self, program: &Program) {
        for g in &program.globals {
            if !self.globals.contains_key(&g.name) {
                continue;
            }
            let Some((ty, _)) = self.globals.get(&g.name).cloned() else {
                continue;
            };
            let Ok(found) = self.check_expr(&g.init, Some(&ty)) else {
                continue;
            };
            if !self.compatible(&ty, &found) {
                self.type_mismatch(g.init.span, &ty, &found, " in global initializer");
                continue;
            }
            if let Some(value) = self.const_value(&g.init, &ty) {
                self.global_inits.insert(g.name.clone(), value);
            }
        }
    }

    fn const_value(&mut self, e: &Expr, ty: &Type) -> Option<ConstValue> {
        let value = match &e.kind {
            ExprKind::Int(v) => ConstValue::Int(*v),
            ExprKind::Float(v) => ConstValue::Float(*v),
            ExprKind::Bool(b) => ConstValue::Bool(*b),
            ExprKind::Char(c) => ConstValue::Char(*c),
            ExprKind::Str(bytes) => ConstValue::Str(bytes.clone()),
            ExprKind::Ident(name) => {
                if let Some((_, value)) = self.consts.get(name) {
                    return Some(value.clone());
                }
                let (_, mutable) = self
                    .globals
                    .get(name)
                    .cloned()
                    .unwrap_or((Type::Int(IntTy::I32), true));
                if mutable {
                    self.error(
                        "E0226",
                        e.span,
                        "a constant initializer cannot read a mutable global",
                    );
                    return None;
                }
                match self.global_inits.get(name) {
                    Some(value) => value.clone(),
                    None => {
                        self.error(
                            "E0226",
                            e.span,
                            format!(
                                "global `{name}` is not a previously initialized constant"
                            ),
                        );
                        return None;
                    }
                }
            }
            ExprKind::EnumVariant { enum_name, variant } => {
                let v = self
                    .enum_defs
                    .get(enum_name)
                    .and_then(|info| info.variants.iter().find(|(n, _)| n == variant))
                    .map(|(_, v)| *v as i64 as u64)?;
                ConstValue::Int(v)
            }
            ExprKind::Unary {
                op: UnOp::AddrOf | UnOp::AddrOfMut,
                expr,
            } => {
                if ty.is_slice() {
                    self.error(
                        "E0226",
                        e.span,
                        "slice/str globals must be initialized from a string literal",
                    );
                    return None;
                }
                if let ExprKind::Ident(name) = &expr.kind {
                    if self.globals.contains_key(name) {
                        ConstValue::GlobalAddr(name.clone())
                    } else {
                        self.error(
                            "E0226",
                            e.span,
                            "address-of in a global initializer must name a global",
                        );
                        return None;
                    }
                } else {
                    self.error(
                        "E0226",
                        e.span,
                        "address-of in a global initializer must name a global",
                    );
                    return None;
                }
            }
            ExprKind::Unary { op, expr } => {
                let inner = self.const_value(expr, ty)?;
                match (op, inner) {
                    (UnOp::Neg, ConstValue::Int(v)) => {
                        ConstValue::Int(mask_width((v as i64).wrapping_neg() as u64, ty))
                    }
                    (UnOp::Neg, ConstValue::Float(v)) => ConstValue::Float(-v),
                    (UnOp::Not, ConstValue::Bool(b)) => ConstValue::Bool(!b),
                    (UnOp::Not, ConstValue::Int(v)) => ConstValue::Int(mask_width(!v, ty)),
                    _ => {
                        self.error("E0226", e.span, "unsupported constant expression");
                        return None;
                    }
                }
            }
            ExprKind::Binary { op, lhs, rhs } => {
                let l = self.const_value(lhs, ty)?;
                let r = self.const_value(rhs, ty)?;
                let operand_ty = self
                    .expr_types
                    .get(&lhs.id)
                    .cloned()
                    .unwrap_or_else(|| ty.clone());
                const_binary(*op, l, r, &operand_ty).or_else(|| {
                    self.error(
                        "E0226",
                        e.span,
                        "unsupported constant expression (division by zero or invalid operands)",
                    );
                    None
                })?
            }
            ExprKind::Cast { expr, ty: to } => {
                let inner = self.const_value(expr, to)?;
                let from = self
                    .expr_types
                    .get(&expr.id)
                    .cloned()
                    .unwrap_or(Type::Int(IntTy::I32));
                convert_const(inner, &from, to)?
            }
            _ => {
                self.error(
                    "E0226",
                    e.span,
                    "global initializers must be constant expressions (literals, arithmetic, or `&global`)",
                );
                return None;
            }
        };
        Some(value)
    }

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
            let ret = self.resolve_type(&f.ret, f.ret_span);
            if matches!(ret, Type::Array { .. }) {
                self.error(
                    "E0216",
                    f.ret_span,
                    "arrays cannot be returned by value; return a pointer instead",
                );
            }
            self.check_value_size(&ret, f.ret_span);
            let mut param_types = Vec::new();
            for p in &f.params {
                let ty = self.resolve_type(&p.ty, p.span);
                if matches!(ty, Type::Array { .. }) {
                    self.error(
                        "E0216",
                        p.span,
                        "arrays cannot be passed by value; pass a pointer instead",
                    );
                }
                if ty == Type::Unit {
                    self.error("E0216", p.span, "parameters cannot have type `void`");
                }
                self.check_value_size(&ty, p.span);
                param_types.push(ty);
            }
            self.sigs.insert(
                f.name.clone(),
                FnSig {
                    params: param_types,
                    ret,
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
        let sig = self
            .sigs
            .get(&f.name)
            .cloned()
            .unwrap_or(FnSig {
                params: Vec::new(),
                ret: self.resolve_type(&f.ret, f.ret_span),
            });
        self.ret_ty = sig.ret.clone();
        self.loop_depth = 0;
        self.unsafe_depth = 0;

        // Parameters are immutable: the specification requires copying into a
        // `mut` binding before mutating (E0205).
        for (p, ty) in f.params.iter().zip(sig.params.into_iter()) {
            self.declare(&p.name, p.span, ty, false);
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
                let binding_ty = match ty {
                    Some(declared) => {
                        let declared = self.resolve_type(declared, *name_span);
                        self.check_value_size(&declared, *name_span);
                        if let Ok(found) = self.check_expr(init, Some(&declared)) {
                            if !self.compatible(&declared, &found) {
                                self.type_mismatch(
                                    init.span,
                                    &declared,
                                    &found,
                                    " in `let` initializer",
                                );
                            }
                        }
                        declared
                    }
                    None => match self.check_expr(init, None) {
                        Ok(Type::Unit) => {
                            self.error(
                                "E0225",
                                init.span,
                                "cannot infer a binding type from a value of type `void`",
                            );
                            Type::Int(IntTy::I32)
                        }
                        Ok(found) => {
                            self.check_value_size(&found, *name_span);
                            found
                        }
                        Err(()) => Type::Int(IntTy::I32),
                    },
                };
                let local = self.declare(name, *name_span, binding_ty, *mutable);
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
            Stmt::CompoundAssign {
                target,
                op,
                value,
                span,
            } => {
                let Ok((ty, mutable)) = self.check_place(target) else {
                    return;
                };
                if !mutable {
                    self.error("E0205", *span, "cannot assign to an immutable place");
                }
                if ty.is_float() {
                    if !matches!(
                        op,
                        BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
                    ) {
                        self.error(
                            "E0210",
                            *span,
                            format!(
                                "`{}` is not defined for floating-point compound assignment",
                                op.describe()
                            ),
                        );
                    }
                } else if !ty.is_integer() {
                    self.error(
                        "E0210",
                        *span,
                        format!(
                            "compound assignment requires an integer or float place, found `{}`",
                            ty.display()
                        ),
                    );
                }
                if let Ok(found) = self.check_expr(value, Some(&ty)) {
                    if !self.compatible(&ty, &found) {
                        self.type_mismatch(value.span, &ty, &found, " in compound assignment");
                    }
                }
                let _ = op;
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
        self.validate_type(ty, span);
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

    fn validate_type(&mut self, ty: &Type, span: Span) {
        match ty {
            Type::Enum(name) => {
                if !self.enum_defs.contains_key(name) {
                    self.error("E0201", span, format!("undefined type `{name}`"));
                }
            }
            Type::Ptr { pointee, .. } => self.validate_type(pointee, span),
            Type::Array { elem, .. } => self.validate_type(elem, span),
            Type::Slice(elem) | Type::Vec(elem) => self.validate_type(elem, span),
            Type::Named(name) => {
                self.error("E0201", span, format!("undefined type `{name}`"));
            }
            Type::Struct(info) => {
                if !info.resolved.get() {
                    self.error(
                        "E0224",
                        span,
                        format!("struct `{}` has an incomplete layout", info.name),
                    );
                }
            }
            _ => {}
        }
    }

    fn compatible(&self, expected: &Type, found: &Type) -> bool {
        if expected == found {
            return true;
        }
        let u8_ty = Type::Int(IntTy::U8);
        matches!(
            (expected, found),
            (
                Type::Ptr { mutable: false, pointee: ep },
                Type::Ptr { mutable: true, pointee: fp }
            ) if ep == fp
        ) || matches!(
            (expected, found),
            (Type::Str, Type::Slice(elem)) if **elem == u8_ty
        ) || matches!(
            (expected, found),
            (Type::Slice(elem), Type::Str) if **elem == u8_ty
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
            ExprKind::Float(v) => {
                let t = match expected {
                    Some(Type::Float(t)) => *t,
                    _ => FloatTy::F64,
                };
                let _ = v;
                Type::Float(t)
            }
            ExprKind::Str(bytes) => {
                let n = bytes.len() as u64;
                let u8_ty = Type::Int(IntTy::U8);
                match expected {
                    Some(Type::Array { elem, len }) if **elem == u8_ty && *len == n => {
                        Type::array_of(u8_ty, n)
                    }
                    _ => Type::Str,
                }
            }
            ExprKind::Char(_) => Type::Char,
            ExprKind::Bool(_) => Type::Bool,
            ExprKind::EnumVariant { enum_name, variant } => {
                let Some(info) = self.enum_defs.get(enum_name) else {
                    self.error("E0201", e.span, format!("undefined enum `{enum_name}`"));
                    return Err(());
                };
                if !info.variants.iter().any(|(n, _)| n == variant) {
                    self.error(
                        "E0222",
                        e.span,
                        format!("enum `{enum_name}` has no variant `{variant}`"),
                    );
                    return Err(());
                }
                Type::Enum(enum_name.clone())
            }
            ExprKind::StructLit { name, fields } => {
                let Some(info) = self.struct_defs.get(name).cloned() else {
                    self.error("E0201", e.span, format!("undefined struct `{name}`"));
                    return Err(());
                };
                if let Some(Type::Struct(expected)) = expected {
                    if expected.name != *name {
                        self.error(
                            "E0203",
                            e.span,
                            format!(
                                "type mismatch: expected struct `{}`, found `{name}`",
                                expected.name
                            ),
                        );
                    }
                }
                let layout = info.fields.borrow().clone();
                let mut seen = HashSet::new();
                for (fname, value) in fields {
                    let Some(field) = layout.iter().find(|f| f.name == *fname) else {
                        self.error(
                            "E0227",
                            value.span,
                            format!("struct `{name}` has no field `{fname}`"),
                        );
                        continue;
                    };
                    if !seen.insert(fname.clone()) {
                        self.error(
                            "E0228",
                            value.span,
                            format!("field `{fname}` is initialized twice"),
                        );
                    }
                    if let Ok(found) = self.check_expr(value, Some(&field.ty)) {
                        if !self.compatible(&field.ty, &found) {
                            self.type_mismatch(
                                value.span,
                                &field.ty,
                                &found,
                                " in struct field",
                            );
                        }
                    }
                }
                for field in &layout {
                    if !seen.contains(&field.name) {
                        self.error(
                            "E0229",
                            e.span,
                            format!("missing field `{}` in struct literal", field.name),
                        );
                    }
                }
                Type::Struct(info)
            }
            ExprKind::Field { base, name } => {
                let base_is_place = matches!(
                    base.kind,
                    ExprKind::Ident(_)
                        | ExprKind::Field { .. }
                        | ExprKind::Index { .. }
                        | ExprKind::Unary { op: UnOp::Deref, .. }
                );
                let base_ty = if base_is_place {
                    self.check_place(base)?.0
                } else {
                    self.check_expr(base, None)?
                };
                let ty = self.field_type(&base_ty, name, e.span)?;
                self.expr_types.insert(e.id, ty.clone());
                ty
            }
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
                } else if let Some((ty, _)) = self.globals.get(name) {
                    let ty = ty.clone();
                    self.expr_types.insert(e.id, ty.clone());
                    ty
                } else if let Some((ty, _)) = self.consts.get(name) {
                    let ty = ty.clone();
                    self.expr_types.insert(e.id, ty.clone());
                    ty
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
                        if !t.is_integer() && !t.is_float() {
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
                    if let (Some(Type::Slice(elem)), Type::Array { elem: ae, len }) =
                        (expected, &t)
                    {
                        if elem == ae {
                            self.slice_from_array.insert(e.id, *len);
                            let ty = Type::Slice(elem.clone());
                            self.expr_types.insert(e.id, ty.clone());
                            return Ok(ty);
                        }
                    }
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
                    if let (Some(Type::Slice(elem)), Type::Array { elem: ae, len }) =
                        (expected, &t)
                    {
                        if elem == ae {
                            self.slice_from_array.insert(e.id, *len);
                            let ty = Type::Slice(elem.clone());
                            self.expr_types.insert(e.id, ty.clone());
                            return Ok(ty);
                        }
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
            ExprKind::Call { callee, args } => {
                self.check_call(e, callee, args, expected)?
            }
            ExprKind::Index { .. } => {
                let (t, _) = self.check_place(e)?;
                t
            }
            ExprKind::SliceRange { base, start, end } => {
                let base_ty = self.check_expr(base, None)?;
                let (_, result) = match &base_ty {
                    Type::Array { elem, .. } => {
                        ((**elem).clone(), Type::Slice(elem.clone()))
                    }
                    Type::Slice(elem) => ((**elem).clone(), base_ty.clone()),
                    Type::Str => (Type::Int(IntTy::U8), Type::Str),
                    other => {
                        self.error(
                            "E0213",
                            base.span,
                            format!("cannot slice a value of type `{}`", other.display()),
                        );
                        return Err(());
                    }
                };
                let usize_ty = Type::Int(IntTy::Usize);
                for bound in [start, end].into_iter().flatten() {
                    if let Ok(t) = self.check_expr(bound, Some(&usize_ty)) {
                        if t != usize_ty {
                            self.error(
                                "E0213",
                                bound.span,
                                format!("slice bound must be `usize`, found `{}`", t.display()),
                            );
                        }
                    }
                }
                let literal_len = match &base.kind {
                    ExprKind::Str(bytes) => Some(bytes.len() as u64),
                    _ => None,
                };
                let bound_len = match &base_ty {
                    Type::Array { len, .. } => Some(*len),
                    Type::Str => literal_len,
                    _ => None,
                };
                let start_lit = start
                    .as_ref()
                    .and_then(|e| match e.kind { ExprKind::Int(v) => Some(v), _ => None })
                    .unwrap_or(0);
                let end_lit = end.as_ref().and_then(|e| match e.kind {
                    ExprKind::Int(v) => Some(v),
                    _ => None,
                });
                if let Some(end_v) = end_lit {
                    if start_lit > end_v {
                        self.error(
                            "E0222",
                            e.span,
                            format!("slice start {start_lit} is greater than end {end_v}"),
                        );
                    }
                }
                if let Some(len) = bound_len {
                    if start_lit > len {
                        self.error(
                            "E0222",
                            start.as_ref().map(|e| e.span).unwrap_or(e.span),
                            format!("slice start {start_lit} exceeds length {len}"),
                        );
                    }
                    if let Some(end_v) = end_lit {
                        if end_v > len {
                            self.error(
                                "E0222",
                                end.as_ref().unwrap().span,
                                format!("slice end {end_v} exceeds length {len}"),
                            );
                        }
                    }
                }
                result
            }
            ExprKind::Cast { expr, ty } => {
                let src = self.check_expr(expr, None)?;
                let to = self.resolve_type(ty, e.span);
                self.cast_types.insert(e.id, to.clone());
                match (&src, &to) {
                    (Type::Int(_), Type::Int(_))
                    | (Type::Ptr { .. }, Type::Ptr { .. })
                    | (Type::Char, Type::Char)
                    | (Type::Char, Type::Int(_))
                    | (Type::Int(_), Type::Char)
                    | (Type::Enum(_), Type::Int(_))
                    | (Type::Int(_), Type::Float(_))
                    | (Type::Float(_), Type::Int(_))
                    | (Type::Float(_), Type::Float(_)) => {}
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
                                to.display()
                            ),
                        );
                        return Err(());
                    }
                }
                to
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
                Type::Int(_) | Type::Char | Type::Enum(_) | Type::Float(_) => {}
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

        if decided.is_float() {
            if !matches!(
                op,
                BinOp::Add | BinOp::Sub | BinOp::Mul | BinOp::Div | BinOp::Rem
            ) {
                self.error(
                    "E0210",
                    e.span,
                    format!(
                        "`{}` is not defined for floating-point operands",
                        op.describe()
                    ),
                );
                return Err(());
            }
            return Ok(decided);
        }
        if !decided.is_integer() {
            self.error(
                "E0210",
                e.span,
                format!(
                    "`{}` requires integer or float operands, found `{}`",
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

    fn check_byte_string_arg(&mut self, e: &Expr, context: &str) -> Result<(), ()> {
        let t = self.check_expr(e, Some(&Type::Str))?;
        let ok = match &t {
            Type::Str => true,
            Type::Slice(elem) => **elem == Type::Int(IntTy::U8),
            _ => false,
        };
        if !ok {
            self.error(
                "E0203",
                e.span,
                format!("{context} must be `str` or `[u8]`, found `{}`", t.display()),
            );
            return Err(());
        }
        Ok(())
    }

    fn check_call(
        &mut self,
        e: &Expr,
        callee: &Expr,
        args: &[Expr],
        expected: Option<&Type>,
    ) -> Result<Type, ()> {
        let ExprKind::Ident(name) = &callee.kind else {
            self.error(
                "E0218",
                callee.span,
                "call target must be a function name (CoreX has no function pointers)",
            );
            return Err(());
        };

        if name == "panic" {
            if args.len() != 1 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`panic` takes 1 argument, found {}", args.len()),
                );
                return Err(());
            }
            if self.check_byte_string_arg(&args[0], "`panic` message").is_err() {
                return Err(());
            }
            self.calls.insert(e.id, Callee::Panic);
            return Ok(Type::Unit);
        }
        if name == "assert" {
            if args.len() != 2 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`assert` takes 2 arguments, found {}", args.len()),
                );
                return Err(());
            }
            if let Ok(t) = self.check_expr(&args[0], Some(&Type::Bool)) {
                if t != Type::Bool {
                    self.error(
                        "E0203",
                        args[0].span,
                        format!("`assert` condition must be `bool`, found `{}`", t.display()),
                    );
                    return Err(());
                }
            }
            if self
                .check_byte_string_arg(&args[1], "`assert` message")
                .is_err()
            {
                return Err(());
            }
            self.calls.insert(e.id, Callee::Assert);
            return Ok(Type::Unit);
        }
        if name == "read_file" {
            if args.len() != 1 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`read_file` takes 1 argument, found {}", args.len()),
                );
                return Err(());
            }
            if self
                .check_byte_string_arg(&args[0], "`read_file` path")
                .is_err()
            {
                return Err(());
            }
            self.calls.insert(e.id, Callee::ReadFile);
            return Ok(Type::Vec(Box::new(Type::Int(IntTy::U8))));
        }
        if name == "write_file" {
            if args.len() != 2 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`write_file` takes 2 arguments, found {}", args.len()),
                );
                return Err(());
            }
            if self
                .check_byte_string_arg(&args[0], "`write_file` path")
                .is_err()
            {
                return Err(());
            }
            let data_ty = Type::Slice(Box::new(Type::Int(IntTy::U8)));
            if let Ok(t) = self.check_expr(&args[1], Some(&data_ty)) {
                let ok = match &t {
                    Type::Str => true,
                    Type::Slice(elem) => **elem == Type::Int(IntTy::U8),
                    _ => false,
                };
                if !ok {
                    self.error(
                        "E0203",
                        args[1].span,
                        format!(
                            "`write_file` expects `str` or `[u8]` data, found `{}`",
                            t.display()
                        ),
                    );
                    return Err(());
                }
            }
            self.calls.insert(e.id, Callee::WriteFile);
            return Ok(Type::Int(IntTy::I32));
        }
        if name == "vec_new" {
            if args.len() != 1 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`vec_new` takes 1 argument, found {}", args.len()),
                );
                return Err(());
            }
            let Some(Type::Vec(elem)) = expected.cloned() else {
                self.error(
                    "E0230",
                    e.span,
                    "cannot infer the element type of `vec_new`; annotate the binding or context",
                );
                return Err(());
            };
            if let Ok(t) = self.check_expr(&args[0], Some(&Type::Int(IntTy::Usize))) {
                if t != Type::Int(IntTy::Usize) {
                    self.error(
                        "E0203",
                        args[0].span,
                        format!("`vec_new` capacity must be `usize`, found `{}`", t.display()),
                    );
                    return Err(());
                }
            }
            if *elem == Type::Unit {
                self.error("E0216", e.span, "`vec<void>` is not allowed");
                return Err(());
            }
            self.calls.insert(e.id, Callee::VecNew);
            return Ok(Type::Vec(elem));
        }
        if name == "vec_push" {
            if args.len() != 2 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`vec_push` takes 2 arguments, found {}", args.len()),
                );
                return Err(());
            }
            let first = self.check_expr(&args[0], None)?;
            let elem = match &first {
                Type::Ptr {
                    mutable: true,
                    pointee,
                } => match &**pointee {
                    Type::Vec(elem) => (**elem).clone(),
                    other => {
                        self.error(
                            "E0203",
                            args[0].span,
                            format!("`vec_push` expects `*mut vec<T>`, found `{}`", other.display()),
                        );
                        return Err(());
                    }
                },
                other => {
                    self.error(
                        "E0203",
                        args[0].span,
                        format!("`vec_push` expects `*mut vec<T>`, found `{}`", other.display()),
                    );
                    return Err(());
                }
            };
            if let Ok(t) = self.check_expr(&args[1], Some(&elem)) {
                if !self.compatible(&elem, &t) {
                    self.type_mismatch(args[1].span, &elem, &t, " in `vec_push`");
                }
            }
            self.calls.insert(e.id, Callee::VecPush);
            return Ok(Type::Unit);
        }
        if name == "vec_free" {
            if args.len() != 1 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`vec_free` takes 1 argument, found {}", args.len()),
                );
                return Err(());
            }
            let first = self.check_expr(&args[0], None)?;
            if !matches!(
                &first,
                Type::Ptr {
                    mutable: true,
                    pointee
                } if matches!(&**pointee, Type::Vec(_))
            ) {
                self.error(
                    "E0203",
                    args[0].span,
                    format!("`vec_free` expects `*mut vec<T>`, found `{}`", first.display()),
                );
                return Err(());
            }
            self.calls.insert(e.id, Callee::VecFree);
            return Ok(Type::Unit);
        }
        if name == "vec_slice" {
            if args.len() != 1 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`vec_slice` takes 1 argument, found {}", args.len()),
                );
                return Err(());
            }
            let first = self.check_expr(&args[0], None)?;
            let elem = match &first {
                Type::Ptr { pointee, .. } => match &**pointee {
                    Type::Vec(elem) => (**elem).clone(),
                    other => {
                        self.error(
                            "E0203",
                            args[0].span,
                            format!("`vec_slice` expects `*vec<T>`, found `*{}`", other.display()),
                        );
                        return Err(());
                    }
                },
                other => {
                    self.error(
                        "E0203",
                        args[0].span,
                        format!("`vec_slice` expects `*vec<T>`, found `{}`", other.display()),
                    );
                    return Err(());
                }
            };
            self.calls.insert(e.id, Callee::VecSlice);
            return Ok(Type::Slice(Box::new(elem)));
        }

        if name == "len" {
            self.builtins.len = true;
            if args.len() != 1 {
                self.error(
                    "E0204",
                    e.span,
                    format!("`len` takes 1 argument, found {}", args.len()),
                );
                return Err(());
            }
            let arg_ty = self.check_expr(&args[0], None)?;
            if !matches!(
                arg_ty,
                Type::Array { .. } | Type::Slice(_) | Type::Str | Type::Vec(_)
            ) {
                self.error(
                    "E0210",
                    args[0].span,
                    format!(
                        "`len` requires an array, slice, or `str`, found `{}`",
                        arg_ty.display()
                    ),
                );
                return Err(());
            }
            self.calls.insert(e.id, Callee::Len);
            return Ok(Type::Int(IntTy::Usize));
        }

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
            if !arg_ty.is_integer()
                && arg_ty != Type::Bool
                && arg_ty != Type::Char
                && !arg_ty.is_enum()
                && !arg_ty.is_float()
                && !matches!(&arg_ty, Type::Str)
                && !matches!(&arg_ty, Type::Slice(elem) if **elem == Type::Int(IntTy::U8))
            {
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
    fn field_type(&mut self, base: &Type, name: &str, span: Span) -> Result<Type, ()> {
        let Type::Struct(info) = base else {
            self.error(
                "E0227",
                span,
                format!("cannot access field `{name}` on type `{}`", base.display()),
            );
            return Err(());
        };
        let fields = info.fields.borrow();
        match fields.iter().find(|f| f.name == name) {
            Some(field) => Ok(field.ty.clone()),
            None => {
                self.error(
                    "E0227",
                    span,
                    format!("struct `{}` has no field `{name}`", info.name),
                );
                Err(())
            }
        }
    }

    fn check_place(&mut self, e: &Expr) -> Result<(Type, bool), ()> {
        match &e.kind {
            ExprKind::Ident(name) => {
                let Some(local) = self.lookup(name) else {
                    if self.consts.contains_key(name) {
                        self.error(
                            "E0211",
                            e.span,
                            "a `const` is a compile-time value and is not a place",
                        );
                        return Err(());
                    }
                    if let Some((ty, mutable)) = self.globals.get(name).cloned() {
                        self.expr_types.insert(e.id, ty.clone());
                        return Ok((ty, mutable));
                    }
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
            ExprKind::Field { base, name } => {
                let (base_ty, mutable) = self.check_place(base)?;
                let ty = self.field_type(&base_ty, name, e.span)?;
                self.expr_types.insert(e.id, ty.clone());
                Ok((ty, mutable))
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
                    Type::Array { elem, len } => ((**elem).clone(), Some(*len)),
                    Type::Slice(elem) | Type::Vec(elem) => ((**elem).clone(), None),
                    Type::Str => (Type::Int(IntTy::U8), None),
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
                if let (ExprKind::Int(v), Some(len)) = (&index.kind, len) {
                    if *v >= len {
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
                .or_else(|| self.globals.get(name).map(|(_, mutable)| *mutable))
                .unwrap_or(false),
            ExprKind::Unary { op: UnOp::Deref, expr } => self
                .expr_types
                .get(&expr.id)
                .map(|t| t.is_mutable_pointer())
                .unwrap_or(false),
            ExprKind::Index { base, .. } => self.place_mutability(base),
            ExprKind::Field { base, .. } => self.place_mutability(base),
            _ => false,
        }
    }
}

/// Size of a type in bytes, using checked arithmetic at every level.
fn checked_size(ty: &Type) -> Option<u64> {
    match ty {
        Type::Unit => Some(0),
        Type::Bool | Type::Char => Some(1),
        Type::Float(t) => Some((t.bits() / 8) as u64),
        Type::Int(t) => Some((t.bits() / 8) as u64),
        Type::Ptr { .. } => Some(8),
        Type::Array { elem, len } => checked_size(elem)?.checked_mul(*len),
        Type::Enum(_) => Some(4),
        Type::Slice(_) | Type::Str => Some(16),
        Type::Vec(_) => Some(24),
        Type::Struct(info) => {
            if info.resolved.get() {
                Some(info.size.get())
            } else {
                None
            }
        }
        Type::Named(_) => None,
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
    if name == "panic" {
        return true;
    }
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


fn align_to(value: u64, align: u64) -> u64 {
    let a = align.max(1);
    (value + a - 1) & !(a - 1)
}

/// Bit width of a scalar constant type.
fn const_bits(ty: &Type) -> u32 {
    match ty {
        Type::Char => 8,
        Type::Enum(_) => 32,
        Type::Int(t) => t.bits(),
        _ => 64,
    }
}

fn mask_width(v: u64, ty: &Type) -> u64 {
    let bits = const_bits(ty);
    if bits < 64 {
        v & ((1u64 << bits) - 1)
    } else {
        v
    }
}

fn signed_width(v: u64, ty: &Type) -> i64 {
    let bits = const_bits(ty);
    let masked = mask_width(v, ty);
    if bits >= 64 {
        masked as i64
    } else if masked & (1u64 << (bits - 1)) != 0 {
        (masked as i128 - (1i128 << bits)) as i64
    } else {
        masked as i64
    }
}

fn eval_int_const(e: &Expr) -> Option<i64> {
    match &e.kind {
        ExprKind::Int(v) if *v <= i64::MAX as u64 => Some(*v as i64),
        ExprKind::Unary {
            op: UnOp::Neg,
            expr,
        } => eval_int_const(expr).map(|v| v.wrapping_neg()),
        ExprKind::Cast { expr, ty } if ty.is_integer() => eval_int_const(expr),
        ExprKind::Binary { op, lhs, rhs } => {
            let l = eval_int_const(lhs)?;
            let r = eval_int_const(rhs)?;
            let v = match op {
                BinOp::Add => l.wrapping_add(r),
                BinOp::Sub => l.wrapping_sub(r),
                BinOp::Mul => l.wrapping_mul(r),
                BinOp::Div => l.checked_div(r)?,
                BinOp::Rem => l.checked_rem(r)?,
                BinOp::Shl => l.wrapping_shl((r as u32) & 63),
                BinOp::Shr => l.wrapping_shr((r as u32) & 63),
                BinOp::BitAnd => l & r,
                BinOp::BitOr => l | r,
                BinOp::BitXor => l ^ r,
                _ => return None,
            };
            Some(v)
        }
        _ => None,
    }
}

fn const_binary(op: BinOp, l: ConstValue, r: ConstValue, ty: &Type) -> Option<ConstValue> {
    if ty.is_float() {
        let (ConstValue::Float(a), ConstValue::Float(b)) = (l, r) else {
            return None;
        };
        let result = match op {
            BinOp::Add => a + b,
            BinOp::Sub => a - b,
            BinOp::Mul => a * b,
            BinOp::Div => a / b,
            BinOp::Rem => a % b,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
                let ord = a.partial_cmp(&b);
                let result = match op {
                    BinOp::Eq => ord == Some(std::cmp::Ordering::Equal),
                    BinOp::Ne => ord != Some(std::cmp::Ordering::Equal),
                    BinOp::Lt => ord == Some(std::cmp::Ordering::Less),
                    BinOp::Le => matches!(ord, Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)),
                    BinOp::Gt => ord == Some(std::cmp::Ordering::Greater),
                    BinOp::Ge => matches!(ord, Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)),
                    _ => unreachable!(),
                };
                return Some(ConstValue::Bool(result));
            }
            _ => return None,
        };
        return Some(ConstValue::Float(result));
    }
    let signed = ty.is_integer() && ty.int_ty().map(|t| t.signed()).unwrap_or(false)
        || ty.is_enum();
    if matches!(op, BinOp::LogicalAnd | BinOp::LogicalOr) {
        if let (ConstValue::Bool(a), ConstValue::Bool(b)) = (&l, &r) {
            return Some(ConstValue::Bool(if op == BinOp::LogicalAnd {
                *a && *b
            } else {
                *a || *b
            }));
        }
        return None;
    }
    if ty == &Type::Bool {
        if let (ConstValue::Bool(a), ConstValue::Bool(b)) = (&l, &r) {
            return match op {
                BinOp::Eq => Some(ConstValue::Bool(a == b)),
                BinOp::Ne => Some(ConstValue::Bool(a != b)),
                _ => None,
            };
        }
        return None;
    }
    let (ConstValue::Int(a), ConstValue::Int(b)) = (l, r) else {
        return None;
    };
    let value = match op {
        BinOp::Add => a.wrapping_add(b),
        BinOp::Sub => a.wrapping_sub(b),
        BinOp::Mul => a.wrapping_mul(b),
        BinOp::BitAnd => a & b,
        BinOp::BitOr => a | b,
        BinOp::BitXor => a ^ b,
        BinOp::Div | BinOp::Rem => {
            if b == 0 {
                return None;
            }
            if signed {
                let sa = signed_width(a, ty);
                let sb = signed_width(b, ty);
                if op == BinOp::Div {
                    sa.checked_div(sb)? as u64
                } else {
                    sa.checked_rem(sb)? as u64
                }
            } else if op == BinOp::Div {
                mask_width(a, ty) / mask_width(b, ty)
            } else {
                mask_width(a, ty) % mask_width(b, ty)
            }
        }
        BinOp::Shl | BinOp::Shr => {
            let bits = const_bits(ty);
            let shift = (b as u32) & (bits - 1);
            if op == BinOp::Shl {
                mask_width(a, ty) << shift
            } else if signed {
                (signed_width(a, ty) >> shift) as u64
            } else {
                mask_width(a, ty) >> shift
            }
        }
        BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge => {
            let ord = if signed {
                signed_width(a, ty).cmp(&signed_width(b, ty))
            } else {
                mask_width(a, ty).cmp(&mask_width(b, ty))
            };
            let result = match op {
                BinOp::Eq => ord.is_eq(),
                BinOp::Ne => ord.is_ne(),
                BinOp::Lt => ord.is_lt(),
                BinOp::Le => ord.is_le(),
                BinOp::Gt => ord.is_gt(),
                BinOp::Ge => ord.is_ge(),
                _ => unreachable!(),
            };
            return Some(ConstValue::Bool(result));
        }
        BinOp::LogicalAnd | BinOp::LogicalOr => unreachable!(),
    };
    Some(ConstValue::Int(mask_width(value, ty)))
}

fn convert_const(value: ConstValue, from: &Type, to: &Type) -> Option<ConstValue> {
    match value {
        ConstValue::Str(bytes) => Some(ConstValue::Str(bytes)),
        ConstValue::GlobalAddr(name) => Some(ConstValue::GlobalAddr(name)),
        ConstValue::Bool(b) => Some(ConstValue::Bool(b)),
        ConstValue::Float(v) => match to {
            Type::Float(FloatTy::F32) => Some(ConstValue::Float(v as f32 as f64)),
            Type::Float(FloatTy::F64) => Some(ConstValue::Float(v)),
            Type::Int(t) => {
                let raw = if t.signed() {
                    v as i64 as u64
                } else {
                    v as u64
                };
                Some(ConstValue::Int(mask_width(raw, &Type::Int(*t))))
            }
            _ => None,
        },
        ConstValue::Char(c) => match to {
            Type::Char => Some(ConstValue::Char(c)),
            Type::Int(_) => Some(ConstValue::Int(c as u64)),
            _ => None,
        },
        ConstValue::Int(v) => match to {
            Type::Char => Some(ConstValue::Char((v & 0xFF) as u8)),
            Type::Int(_) | Type::Enum(_) => Some(ConstValue::Int(mask_width(v, to))),
            Type::Float(FloatTy::F32) => {
                let f = if from.is_integer() && from.int_ty().map(|t| t.signed()).unwrap_or(false)
                    || from.is_enum()
                {
                    signed_width(v, from) as f64
                } else {
                    mask_width(v, from) as f64
                };
                Some(ConstValue::Float(f as f32 as f64))
            }
            Type::Float(FloatTy::F64) => {
                let f = if from.is_integer() && from.int_ty().map(|t| t.signed()).unwrap_or(false)
                    || from.is_enum()
                {
                    signed_width(v, from) as f64
                } else {
                    mask_width(v, from) as f64
                };
                Some(ConstValue::Float(f))
            }
            _ => None,
        },
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

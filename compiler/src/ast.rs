//! CoreX abstract syntax tree.
//!
//! The AST is deliberately small and untyped: types are attached by the
//! semantic pass in side tables keyed by `NodeId`. That keeps the tree cheap to
//! construct and lets code generation consume a single tree plus analysis
//! results.

use std::cell::{Cell, RefCell};
use std::fmt;
use std::rc::Rc;

use crate::diagnostics::Span;

pub type NodeId = u32;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum IntTy {
    I8,
    I16,
    I32,
    I64,
    U8,
    U16,
    U32,
    U64,
    Usize,
    Isize,
}

impl IntTy {
    pub fn bits(self) -> u32 {
        match self {
            IntTy::I8 | IntTy::U8 => 8,
            IntTy::I16 | IntTy::U16 => 16,
            IntTy::I32 | IntTy::U32 => 32,
            IntTy::I64 | IntTy::U64 | IntTy::Usize | IntTy::Isize => 64,
        }
    }

    pub fn signed(self) -> bool {
        matches!(
            self,
            IntTy::I8 | IntTy::I16 | IntTy::I32 | IntTy::I64 | IntTy::Isize
        )
    }

    pub fn name(self) -> &'static str {
        match self {
            IntTy::I8 => "i8",
            IntTy::I16 => "i16",
            IntTy::I32 => "i32",
            IntTy::I64 => "i64",
            IntTy::U8 => "u8",
            IntTy::U16 => "u16",
            IntTy::U32 => "u32",
            IntTy::U64 => "u64",
            IntTy::Usize => "usize",
            IntTy::Isize => "isize",
        }
    }

    /// Largest value representable by an unsigned integer literal of this type.
    pub fn max_literal(self) -> u64 {
        if self.signed() {
            (1u64 << (self.bits() - 1)) - 1
        } else {
            if self.bits() == 64 {
                u64::MAX
            } else {
                (1u64 << self.bits()) - 1
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FloatTy {
    F32,
    F64,
}

impl FloatTy {
    pub fn bits(self) -> u32 {
        match self {
            FloatTy::F32 => 32,
            FloatTy::F64 => 64,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            FloatTy::F32 => "f32",
            FloatTy::F64 => "f64",
        }
    }
}

#[derive(Clone, Debug)]
pub struct StructField {
    pub name: String,
    pub ty: Type,
    /// Byte offset within the struct, filled in by semantic analysis.
    pub offset: u64,
    /// Bit width for a C-style bitfield, or 0 for a normal field.
    pub bit_width: u32,
    /// Bit offset within the storage unit for a bitfield.
    pub bit_offset: u32,
}

/// Struct layout shared by every `Type::Struct` value. Interior mutability lets
/// recursive pointer fields (`next: *Node`) reference the struct while its
/// fields are still being resolved.
#[derive(Debug)]
pub struct StructInfo {
    pub name: String,
    pub fields: RefCell<Vec<StructField>>,
    pub size: Cell<u64>,
    pub align: Cell<u64>,
    pub resolved: Cell<bool>,
    /// C `union` layout: every field starts at offset 0 and the size is the
    /// largest member.
    pub is_union: bool,
    /// No padding between fields; alignment is 1 unless raised explicitly.
    pub packed: bool,
    /// Minimum alignment from `align(N)`; 0 means natural alignment.
    pub explicit_align: u64,
}

impl StructInfo {
    fn has_bitfields(&self) -> bool {
        self.fields.borrow().iter().any(|f| f.bit_width != 0)
    }

    fn has_flexible_array(&self) -> bool {
        self.fields
            .borrow()
            .last()
            .map(|f| matches!(f.ty, Type::Array { len: 0, .. }))
            .unwrap_or(false)
    }

    /// True when LLVM's default struct layout matches this type's layout, so
    /// the LLVM literal struct form can be used (needed for C ABI interop).
    pub fn is_plain_c(&self) -> bool {
        !self.is_union
            && !self.packed
            && self.explicit_align == 0
            && !self.has_bitfields()
            && !self.has_flexible_array()
    }
}

impl PartialEq for StructInfo {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name
    }
}
impl Eq for StructInfo {}
impl std::hash::Hash for StructInfo {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state);
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Unit,
    Bool,
    Char,
    Float(FloatTy),
    Int(IntTy),
    Ptr { mutable: bool, pointee: Box<Type> },
    Array { elem: Box<Type>, len: u64 },
    Enum(String),
    Slice(Box<Type>),
    Str,
    Vec(Box<Type>),
    Struct(Rc<StructInfo>),
    /// Function pointer type. Its representation is a bare code pointer.
    Fn(FnType),
    /// NUL-terminated C string: layout is a single pointer.
    CStr,
    /// Unresolved named type produced by the parser (`Color`, `Point`).
    Named(String),
}

/// Signature of a function pointer type.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FnType {
    pub params: Vec<Type>,
    pub ret: Box<Type>,
    pub variadic: bool,
}

impl Type {
    pub fn ptr_to(pointee: Type, mutable: bool) -> Type {
        Type::Ptr {
            mutable,
            pointee: Box::new(pointee),
        }
    }

    pub fn array_of(elem: Type, len: u64) -> Type {
        Type::Array {
            elem: Box::new(elem),
            len,
        }
    }

    pub fn is_integer(&self) -> bool {
        matches!(self, Type::Int(_))
    }

    pub fn is_char(&self) -> bool {
        matches!(self, Type::Char)
    }

    pub fn is_enum(&self) -> bool {
        matches!(self, Type::Enum(_))
    }

    pub fn is_struct(&self) -> bool {
        matches!(self, Type::Struct(_))
    }

    pub fn is_slice(&self) -> bool {
        matches!(self, Type::Slice(_) | Type::Str)
    }

    pub fn is_vec(&self) -> bool {
        matches!(self, Type::Vec(_))
    }

    pub fn vec_elem(&self) -> Option<Type> {
        match self {
            Type::Vec(elem) => Some((**elem).clone()),
            _ => None,
        }
    }

    pub fn slice_elem(&self) -> Option<Type> {
        match self {
            Type::Slice(elem) => Some((**elem).clone()),
            Type::Str => Some(Type::Int(IntTy::U8)),
            _ => None,
        }
    }

    pub fn struct_info(&self) -> Option<&Rc<StructInfo>> {
        match self {
            Type::Struct(info) => Some(info),
            _ => None,
        }
    }

    pub fn is_float(&self) -> bool {
        matches!(self, Type::Float(_))
    }

    pub fn float_ty(&self) -> Option<FloatTy> {
        match self {
            Type::Float(t) => Some(*t),
            _ => None,
        }
    }

    pub fn int_ty(&self) -> Option<IntTy> {
        match self {
            Type::Int(t) => Some(*t),
            _ => None,
        }
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self, Type::Ptr { .. })
    }

    pub fn is_fn(&self) -> bool {
        matches!(self, Type::Fn(_))
    }

    pub fn fn_sig(&self) -> Option<&FnType> {
        match self {
            Type::Fn(sig) => Some(sig),
            _ => None,
        }
    }

    pub fn is_cstr(&self) -> bool {
        matches!(self, Type::CStr)
    }

    pub fn is_str(&self) -> bool {
        matches!(self, Type::Str)
    }

    /// True for types whose LLVM representation is a single `ptr`.
    pub fn is_pointer_like(&self) -> bool {
        matches!(self, Type::Ptr { .. } | Type::Fn(_) | Type::CStr)
    }

    pub fn is_aggregate(&self) -> bool {
        matches!(
            self,
            Type::Array { .. }
                | Type::Struct(_)
                | Type::Slice(_)
                | Type::Str
                | Type::Vec(_)
        )
    }

    pub fn pointee(&self) -> Option<&Type> {
        match self {
            Type::Ptr { pointee, .. } => Some(pointee),
            _ => None,
        }
    }

    pub fn is_mutable_pointer(&self) -> bool {
        matches!(self, Type::Ptr { mutable: true, .. })
    }

    /// Size in bytes. `Unit` is zero-sized.
    pub fn size_bytes(&self) -> u64 {
        match self {
            Type::Unit => 0,
            Type::Bool => 1,
            Type::Char => 1,
            Type::Float(t) => (t.bits() / 8) as u64,
            Type::Int(t) => (t.bits() / 8) as u64,
            Type::Ptr { .. } | Type::Fn(_) | Type::CStr => 8,
            Type::Array { elem, len } => elem.size_bytes() * len,
            Type::Enum(_) => 4,
            Type::Struct(info) => info.size.get(),
            Type::Slice(_) | Type::Str => 16,
            Type::Vec(_) => 24,
            Type::Named(_) => 0,
        }
    }

    pub fn align_bytes(&self) -> u64 {
        match self {
            Type::Unit => 1,
            Type::Array { elem, .. } => elem.align_bytes(),
            Type::Enum(_) => 4,
            Type::Struct(info) => info.align.get().max(1),
            Type::Slice(_) | Type::Str | Type::Fn(_) | Type::CStr => 8,
            Type::Vec(_) => 8,
            Type::Named(_) => 1,
            other => other.size_bytes().max(1),
        }
    }

    /// LLVM 20 type spelling. Opaque pointers are always `ptr`.
    pub fn llvm(&self) -> String {
        match self {
            Type::Unit => "void".to_string(),
            Type::Bool => "i8".to_string(),
            Type::Char => "i8".to_string(),
            Type::Float(FloatTy::F32) => "float".to_string(),
            Type::Float(FloatTy::F64) => "double".to_string(),
            Type::Int(t) => format!("i{}", t.bits()),
            Type::Ptr { .. } | Type::Fn(_) | Type::CStr => "ptr".to_string(),
            Type::Array { elem, len } => format!("[{} x {}]", len, elem.llvm()),
            Type::Enum(_) => "i32".to_string(),
            Type::Struct(info) => {
                if info.is_plain_c() {
                    let fields = info.fields.borrow();
                    let body: Vec<String> = fields.iter().map(|f| f.ty.llvm()).collect();
                    format!("{{ {} }}", body.join(", "))
                } else {
                    // Unions, packed/aligned structs, bitfields, and flexible
                    // array members have a layout LLVM's default struct layout
                    // cannot express; store them as opaque bytes and access
                    // fields through byte offsets computed by semantic analysis.
                    format!("[{} x i8]", info.size.get())
                }
            }
            Type::Slice(elem) => format!("{{ ptr, i64 }} /* [{}] */", elem.display()),
            Type::Str => "{ ptr, i64 }".to_string(),
            Type::Vec(_) => "{ ptr, i64, i64 }".to_string(),
            Type::Named(name) => format!("<unresolved {name}>"),
        }
    }

    pub fn display(&self) -> String {
        match self {
            Type::Unit => "void".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Char => "char".to_string(),
            Type::Float(t) => t.name().to_string(),
            Type::Int(t) => t.name().to_string(),
            Type::Ptr { mutable, pointee } => {
                if *mutable {
                    format!("*mut {}", pointee.display())
                } else {
                    format!("*{}", pointee.display())
                }
            }
            Type::Array { elem, len } => format!("[{}; {}]", elem.display(), len),
            Type::Enum(name) => name.clone(),
            Type::Struct(info) => info.name.clone(),
            Type::Slice(elem) => format!("[{}]", elem.display()),
            Type::Str => "str".to_string(),
            Type::Vec(elem) => format!("vec<{}>", elem.display()),
            Type::Fn(f) => {
                let params: Vec<String> = f.params.iter().map(|p| p.display()).collect();
                let mut sig = params.join(", ");
                if f.variadic {
                    if !sig.is_empty() {
                        sig.push_str(", ");
                    }
                    sig.push_str("...");
                }
                format!("fn({sig}) -> {}", f.ret.display())
            }
            Type::CStr => "cstr".to_string(),
            Type::Named(name) => name.clone(),
        }
    }
}

impl fmt::Display for Type {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.display())
    }
}

#[derive(Clone, Debug)]
pub struct Program {
    pub imports: Vec<ImportDecl>,
    pub fns: Vec<FnDecl>,
    pub typedefs: Vec<TypedefDecl>,
    pub globals: Vec<GlobalDecl>,
    pub consts: Vec<ConstDecl>,
    pub enums: Vec<EnumDecl>,
    pub structs: Vec<StructDecl>,
}

#[derive(Clone, Debug)]
pub struct StructDecl {
    pub name: String,
    pub name_span: Span,
    pub fields: Vec<StructFieldDecl>,
    pub span: Span,
    pub is_union: bool,
    pub packed: bool,
    pub align: u64,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub struct TypedefDecl {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub struct StructFieldDecl {
    pub name: String,
    pub span: Span,
    pub ty: Type,
    pub bit_width: u32,
}

#[derive(Clone, Debug)]
pub struct GlobalDecl {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub mutable: bool,
    pub init: Expr,
    pub span: Span,
    pub is_extern: bool,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub struct ImportDecl {
    pub path: String,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct ConstDecl {
    pub name: String,
    pub name_span: Span,
    pub ty: Type,
    pub value: Expr,
    pub span: Span,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub struct EnumDecl {
    pub name: String,
    pub name_span: Span,
    pub variants: Vec<EnumVariant>,
    pub span: Span,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub struct EnumVariant {
    pub name: String,
    pub span: Span,
    pub value: Option<Expr>,
}

/// Compile-time constant value of a global initializer.
#[derive(Clone, Debug, PartialEq)]
pub enum ConstValue {
    Int(u64),
    Float(f64),
    Bool(bool),
    Char(u8),
    Str(Vec<u8>),
    GlobalAddr(String),
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub ret: Type,
    pub ret_span: Span,
    /// `None` for an `extern` declaration without a body.
    pub body: Option<Block>,
    pub span: Span,
    /// `Some("C")` for a function using the C ABI / unmangled symbol.
    pub extern_abi: Option<String>,
    /// Trailing `...` in the parameter list (C varargs).
    pub variadic: bool,
    pub public: bool,
}

#[derive(Clone, Debug)]
pub struct Param {
    pub name: String,
    pub span: Span,
    pub ty: Type,
}

#[derive(Clone, Debug)]
pub struct Block {
    pub stmts: Vec<Stmt>,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub struct SwitchArm {
    /// `None` is the `default` arm.
    pub value: Option<Expr>,
    pub body: Block,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Let {
        name: String,
        name_span: Span,
        ty: Option<Type>,
        mutable: bool,
        init: Expr,
        span: Span,
    },
    Assign {
        target: Expr,
        value: Expr,
        span: Span,
    },
    CompoundAssign {
        target: Expr,
        op: BinOp,
        value: Expr,
        span: Span,
    },
    Expr(Expr),
    If {
        cond: Expr,
        then_block: Block,
        else_branch: Option<Box<Stmt>>,
        span: Span,
    },
    While {
        cond: Expr,
        body: Block,
        span: Span,
    },
    /// `for (init; cond; step) { body }`; any clause may be omitted.
    For {
        init: Option<Box<Stmt>>,
        cond: Option<Expr>,
        step: Option<Box<Stmt>>,
        body: Block,
        span: Span,
    },
    /// `do { body } while cond;`
    DoWhile {
        body: Block,
        cond: Expr,
        span: Span,
    },
    /// C `switch` with fallthrough between arms.
    Switch {
        value: Expr,
        arms: Vec<SwitchArm>,
        span: Span,
    },
    /// A labeled statement: `name: stmt`.
    Label {
        name: String,
        stmt: Box<Stmt>,
        span: Span,
    },
    Goto {
        label: String,
        span: Span,
    },
    Loop {
        body: Block,
        span: Span,
    },
    Break(Span),
    Continue(Span),
    Return {
        value: Option<Expr>,
        span: Span,
    },
    Unsafe {
        body: Block,
        span: Span,
    },
    /// A bare `{ ... }` block statement (used by `else { ... }`).
    Block(Block),
    Empty(Span),
}

impl Stmt {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, .. }
            | Stmt::Assign { span, .. }
            | Stmt::CompoundAssign { span, .. }
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Loop { span, .. }
            | Stmt::For { span, .. }
            | Stmt::DoWhile { span, .. }
            | Stmt::Switch { span, .. }
            | Stmt::Label { span, .. }
            | Stmt::Goto { span, .. }
            | Stmt::Return { span, .. }
            | Stmt::Unsafe { span, .. }
            | Stmt::Empty(span)
            | Stmt::Break(span)
            | Stmt::Continue(span) => *span,
            Stmt::Expr(e) => e.span,
            Stmt::Block(b) => b.span,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub id: NodeId,
    pub kind: ExprKind,
    pub span: Span,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(u64),
    Float(f64),
    Str(Vec<u8>),
    CStr(Vec<u8>),
    Char(u8),
    Bool(bool),
    /// Typed null pointer (`null`).
    Null,
    SizeOf(Type),
    AlignOf(Type),
    OffsetOf(Type, String),
    Ident(String),
    EnumVariant {
        enum_name: String,
        variant: String,
    },
    StructLit {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    Field {
        base: Box<Expr>,
        name: String,
    },
    Unary {
        op: UnOp,
        expr: Box<Expr>,
    },
    Binary {
        op: BinOp,
        lhs: Box<Expr>,
        rhs: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Index {
        base: Box<Expr>,
        index: Box<Expr>,
    },
    /// `base[start..end]`; omitted bounds default to 0 and `len(base)`.
    SliceRange {
        base: Box<Expr>,
        start: Option<Box<Expr>>,
        end: Option<Box<Expr>>,
    },
    Cast {
        expr: Box<Expr>,
        ty: Type,
    },
    Array(Vec<Expr>),
    /// `[value; count]` array construction.
    ArrayRepeat {
        value: Box<Expr>,
        count: u64,
    },
    /// Typed compound literal: `Point { x: 1, y: 2 }` with fields omitted
    /// zero-initialized, or `[i32; 4] { 0: 1, 3: 7 }` for arrays.
    CompoundLit {
        ty: Type,
        fields: Vec<(String, Expr)>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
    AddrOf,
    AddrOfMut,
    Deref,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
    LogicalAnd,
    LogicalOr,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinOp {
    pub fn describe(self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Le => "<=",
            BinOp::Gt => ">",
            BinOp::Ge => ">=",
            BinOp::LogicalAnd => "&&",
            BinOp::LogicalOr => "||",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        }
    }

    pub fn is_comparison(self) -> bool {
        matches!(
            self,
            BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge
        )
    }

    pub fn is_arithmetic(self) -> bool {
        matches!(
            self,
            BinOp::Add
                | BinOp::Sub
                | BinOp::Mul
                | BinOp::Div
                | BinOp::Rem
                | BinOp::BitAnd
                | BinOp::BitOr
                | BinOp::BitXor
                | BinOp::Shl
                | BinOp::Shr
        )
    }
}

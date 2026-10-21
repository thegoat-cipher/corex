//! CoreX abstract syntax tree.
//!
//! The AST is deliberately small and untyped: types are attached by the
//! semantic pass in side tables keyed by `NodeId`. That keeps the tree cheap to
//! construct and lets code generation consume a single tree plus analysis
//! results.

use std::fmt;

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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Type {
    Unit,
    Bool,
    Int(IntTy),
    Ptr { mutable: bool, pointee: Box<Type> },
    Array { elem: Box<Type>, len: u64 },
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

    pub fn int_ty(&self) -> Option<IntTy> {
        match self {
            Type::Int(t) => Some(*t),
            _ => None,
        }
    }

    pub fn is_pointer(&self) -> bool {
        matches!(self, Type::Ptr { .. })
    }

    pub fn is_aggregate(&self) -> bool {
        matches!(self, Type::Array { .. })
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
            Type::Int(t) => (t.bits() / 8) as u64,
            Type::Ptr { .. } => 8,
            Type::Array { elem, len } => elem.size_bytes() * len,
        }
    }

    pub fn align_bytes(&self) -> u64 {
        match self {
            Type::Unit => 1,
            Type::Array { elem, .. } => elem.align_bytes(),
            other => other.size_bytes().max(1),
        }
    }

    /// LLVM 20 type spelling. Opaque pointers are always `ptr`.
    pub fn llvm(&self) -> String {
        match self {
            Type::Unit => "void".to_string(),
            Type::Bool => "i8".to_string(),
            Type::Int(t) => format!("i{}", t.bits()),
            Type::Ptr { .. } => "ptr".to_string(),
            Type::Array { elem, len } => format!("[{} x {}]", len, elem.llvm()),
        }
    }

    pub fn display(&self) -> String {
        match self {
            Type::Unit => "void".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Int(t) => t.name().to_string(),
            Type::Ptr { mutable, pointee } => {
                if *mutable {
                    format!("*mut {}", pointee.display())
                } else {
                    format!("*{}", pointee.display())
                }
            }
            Type::Array { elem, len } => format!("[{}; {}]", elem.display(), len),
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
    pub fns: Vec<FnDecl>,
}

#[derive(Clone, Debug)]
pub struct FnDecl {
    pub name: String,
    pub name_span: Span,
    pub params: Vec<Param>,
    pub ret: Type,
    pub ret_span: Span,
    pub body: Block,
    pub span: Span,
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
pub enum Stmt {
    Let {
        name: String,
        name_span: Span,
        ty: Type,
        mutable: bool,
        init: Expr,
        span: Span,
    },
    Assign {
        target: Expr,
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
            | Stmt::If { span, .. }
            | Stmt::While { span, .. }
            | Stmt::Loop { span, .. }
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
    Str(Vec<u8>),
    Bool(bool),
    Ident(String),
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

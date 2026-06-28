use crate::span::Span;
use crate::symbol::Symbol;
pub type ExprId = u32;


#[derive(Debug, Clone)]
pub enum Expr<'arena> {
    Integer(i64, Span, ExprId),
    Bool(bool, Span, ExprId),
    StringLiteral(&'arena str, Span, ExprId),
    Identifier(Symbol, Span, ExprId),
    BinaryOp {
        left: &'arena Expr<'arena>,
        op:   BinaryOperator,
        right: &'arena Expr<'arena>,
        span: Span,
        id: ExprId,
    },
    UnaryOp {
        op: UnaryOperator,
        operand:  &'arena Expr<'arena>,
        span: Span,
        id: ExprId,
    },
    Call {
        name: Symbol,
        args: &'arena [Expr<'arena>],
        span: Span,
        id: ExprId,
    },
    Cast {
        expr: &'arena Expr<'arena>,
        target_type: Type,
        span: Span,
        id: ExprId,
    },
    ArrayLiteral(&'arena [Expr<'arena>], Span, ExprId),
    Index {
        array: &'arena Expr<'arena>,
        index: &'arena Expr<'arena>,
        span: Span,
        id: ExprId,
    },
    StructLiteral{
        name: Symbol,
        fields: &'arena [(Symbol, Expr<'arena>)],
        span: Span,
        id: ExprId,
    },
    FieldAccess {
        object: &'arena Expr<'arena>,
        field:  Symbol,
        span: Span,
        id: ExprId,
    },
    Box {
        value: &'arena Expr<'arena>,
        span: Span,
        id: ExprId,
    },

    Deref {
        target: &'arena Expr<'arena>,
        span: Span,
        id: ExprId,
    },
    EnumVariant{
        enum_name: Symbol,
        variant: Symbol,
        args: &'arena [Expr<'arena>],
        span: Span,
        id: ExprId,
    },

    Error(Span, ExprId),
}

#[derive(Debug, Clone, PartialEq)]
pub enum BinaryOperator {
    Add, Sub, Mul, Div,
    Eq, NotEq,
    Less, Greater, LessEq, GreaterEq,
    And, Or,
}

#[derive(Debug, Clone, PartialEq)]
pub enum UnaryOperator {
    Not,
    Neg,
}

#[derive(Debug, Clone)]
pub enum Stmt<'arena> {
    Let {
        name: Symbol,
        ty:   Option<Type>,
        value: Expr<'arena>,
        span: Span,
    },
    Return(Expr<'arena>, Span),
    Expr(Expr<'arena>, Span),
    If {
        condition: Expr<'arena>,
        then_body: &'arena [Stmt<'arena>],
        else_branch: Option<&'arena [Stmt<'arena>]>,
        span: Span,
    },
    Assign {
        name: Symbol,
        value: Expr<'arena>,
        span: Span,
    },
    While {
        condition: Expr<'arena>,
        body: &'arena [Stmt<'arena>],
        span: Span,
    },
    For {
        var: Symbol,
        from: Expr<'arena>,
        to: Expr<'arena>,
        body: &'arena [Stmt<'arena>],
        span: Span,
    },
    AssignIndex {
        name: Symbol,
        index: Expr<'arena>,
        value: Expr<'arena>,
        span: Span,
    },
    AssignField {
        object: Symbol,
        field:  Symbol,
        value: Expr<'arena>,
        span: Span,
    },
    AssignDeref {
        target: Expr<'arena>,
        value: Expr<'arena>,
        span: Span,
    },
    Match {
        scrutinee: Expr<'arena>,
        arms: &'arena [(Pattern<'arena>, &'arena [Stmt<'arena>])],
        span: Span,
    },
    Error(Span),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    I32, I64, Bool, Void, Str,
    Array(Box<Type>, usize),
    Struct(Symbol),
    Box(Box<Type>),
    Error,
    Enum(Symbol),
}

#[derive(Debug, Clone)]
pub struct Function<'arena> {
    pub name: Symbol,
    pub params: &'arena [(Symbol, Type)],
    pub return_type: Type,
    pub body: &'arena [Stmt<'arena>],
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: Symbol,
    pub fields: Vec<(Symbol, Type)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub struct EnumDef {
    pub name: Symbol,
    pub variants: Vec<(Symbol, Vec<Type>)>,
    pub span: Span,
}

#[derive(Debug, Clone)]
pub enum Pattern<'arena> {
    Variant { enum_name: Symbol, variant: Symbol, bindings: &'arena [Symbol] },
    Wildcard,
}

#[derive(Debug)]
pub struct Program<'arena> {
    pub functions: Vec<Function<'arena>>,
    pub structs: Vec<StructDef>,
    pub imports:  Vec<&'arena str>,
    pub enums: Vec<EnumDef>,
}

impl<'arena> Expr<'arena> {
    pub fn span(&self) -> Span {
        match self {
            Expr::Integer(_, s, _) => *s,
            Expr::Bool(_, s, _) => *s,
            Expr::StringLiteral(_, s,_) => *s,
            Expr::Identifier(_, s,_) => *s,
            Expr::BinaryOp { span, .. } => *span,
            Expr::UnaryOp { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::Cast { span, .. } => *span,
            Expr::ArrayLiteral(_, s,_) => *s,
            Expr::Index { span, .. } => *span,
            Expr::StructLiteral { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
            Expr::Box { span, .. } => *span,
            Expr::Deref { span, .. } => *span,
            Expr::Error(s, _) => *s,
            Expr::EnumVariant{span, ..} => *span,

        }
    }
    pub fn id(&self) -> ExprId {
        match self {
            Expr::Integer(_, _, id) => *id,
            Expr::Bool(_, _, id) => *id,
            Expr::StringLiteral(_, _, id) => *id,
            Expr::Identifier(_, _, id) => *id,
            Expr::BinaryOp { id, .. } => *id,
            Expr::UnaryOp { id, .. } => *id,
            Expr::Call { id, .. } => *id,
            Expr::Cast { id, .. } => *id,
            Expr::ArrayLiteral(_, _, id) => *id,
            Expr::Index { id, .. } => *id,
            Expr::StructLiteral { id, .. } => *id,
            Expr::FieldAccess { id, .. } => *id,
            Expr::Box { id, .. } => *id,
            Expr::Deref { id, .. } => *id,
            Expr::Error(_, id) => *id,
            Expr::EnumVariant{id, ..} => *id,
        }
    }
}

impl<'arena> Stmt<'arena> {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, ..} => *span,
            Stmt::Return(_, span) => *span,
            Stmt::Expr(_, span) => *span,
            Stmt::If { span, .. } => *span,
            Stmt::Assign { span, .. } => *span,
            Stmt::While { span, .. } => *span,
            Stmt::For { span, .. } => *span,
            Stmt::AssignIndex { span, .. } => *span,
            Stmt::AssignField { span, .. } => *span,
            Stmt::AssignDeref { span, .. } => *span,
            Stmt::Match { span, .. } => *span,
            Stmt::Error(span) => *span,
        }
    }
}

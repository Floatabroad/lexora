use crate::span::Span;
use crate::symbol::Symbol;

#[derive(Debug, Clone)]
pub enum Expr<'arena> {
    Integer(i64, Span),
    Bool(bool, Span),
    StringLiteral(&'arena str, Span),
    Identifier(Symbol, Span),
    BinaryOp {
        left: &'arena Expr<'arena>,
        op : BinaryOperator,
        right: &'arena Expr<'arena>,
        span: Span,
    },
    UnaryOp {
        op :        UnaryOperator,
        operand:    &'arena Expr<'arena>,
        span:       Span,
    },
    Call {
        name: Symbol,
        args: &'arena [Expr<'arena>],
        span: Span,
    },
    Cast {
        expr:   &'arena Expr<'arena>,
        target_type: Type,
        span:        Span,
    },
    ArrayLiteral(&'arena [Expr<'arena>], Span),
    Index {
        array: &'arena Expr<'arena>,
        index: &'arena Expr<'arena>,
        span: Span,
    },
    StructLiteral {
        name: Symbol,
        fields: &'arena [(Symbol, Expr<'arena>)],
        span: Span,
    },
    FieldAccess {
        object: &'arena Expr<'arena>,
        field:  Symbol,
        span: Span,
    },
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
        ty:   Type,
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
}

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    I32, I64, Bool, Void, Str,
    Array(Box<Type>, usize),
    Struct(Symbol),
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

#[derive(Debug)]
pub struct Program<'arena> {
    pub functions: Vec<Function<'arena>>,
    pub structs: Vec<StructDef>,
    pub imports:  Vec<&'arena str>,
}

impl<'arena> Expr<'arena> {
    pub fn span(&self) -> Span {
        match self {
            Expr::Integer(_, s) => *s,
            Expr::Bool(_, s) => *s,
            Expr::StringLiteral(_, s) => *s,
            Expr::Identifier(_, s) => *s,
            Expr::BinaryOp { span, .. } => *span,
            Expr::UnaryOp { span, .. } => *span,
            Expr::Call { span, .. } => *span,
            Expr::Cast { span, .. } => *span,
            Expr::ArrayLiteral(_, s) => *s,
            Expr::Index { span, .. } => *span,
            Expr::StructLiteral { span, .. } => *span,
            Expr::FieldAccess { span, .. } => *span,
        }
    }
}
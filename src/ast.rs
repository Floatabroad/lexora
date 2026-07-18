use crate::span::Span;
use crate::symbol::Symbol;
pub type ExprId = u32;
use std::collections::HashMap;

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
   Try {
       expr: &'arena Expr<'arena>,
       span: Span,
       id: ExprId,
   } ,
    EnumVariant{
        enum_name: Symbol,
        variant: Symbol,
        args: &'arena [Expr<'arena>],
        type_args: Vec<Type>,
        span: Span,
        id: ExprId,
    },
    Match {
        scrutinee: &'arena Expr<'arena>,
        arms: &'arena [(Pattern<'arena>, Block<'arena>)],
        span: Span,
        id: ExprId,
    },
    If{
        condition: &'arena Expr<'arena>,
        then_body: &'arena Block<'arena>,
        else_body: Option<&'arena Block<'arena>>,
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

    Assign {
        name: Symbol,
        value: Expr<'arena>,
        span: Span,
    },
    While {
        condition: Expr<'arena>,
        body: Block<'arena>,
        span: Span,
    },
    For {
        var: Symbol,
        from: Expr<'arena>,
        to: Expr<'arena>,
        body: Block<'arena>,
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

    Error(Span),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Type {
    I32, I64, Bool, Void, Str,
    Array(Box<Type>, usize),
    Struct(Symbol),
    Box(Box<Type>),
    String,
    Error,
    Enum(Symbol, Vec<Type>),
    Param(Symbol),
}

impl Type {
    pub fn substitute(&self, map: &HashMap<Symbol, Type>) -> Type {
        match self {
            Type::Param(p) => map.get(p).cloned().unwrap_or_else(|| self.clone()),
            Type::Array(elem, n) => Type::Array(Box::new(elem.substitute(map)), *n),
            Type::Box(inner) => Type::Box(Box::new(inner.substitute(map))),
            Type::Enum(s, args) => Type::Enum(*s, args.iter().map(|t| t.substitute(map)).collect()),
            other => other.clone(),
        }
    }
    pub fn mangle(&self) -> String {
        match self {
            Type::I32 => "i32".to_string(),
            Type::I64 => "i64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Str => "str".to_string(),
            Type::Void => "void".to_string(),
            Type::String => "string".to_string(),
            Type::Box(inner) => format!("box.{}", inner.mangle()),
            Type::Array(elem, n) => format!("arr{}.{}", n, elem.mangle()),
            Type::Struct(s) => format!("s{}", s.0),
            Type::Enum(s, args) => {
                let mut out = format!("e{}", s.0);
                for a in args {
                    out.push('.');
                    out.push_str(&a.mangle());
                }
                out
            }
            Type::Param(_) | Type::Error => unreachable!("somut olmayan tip mangle edilemez"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Block<'arena> {
    pub stmts: &'arena [Stmt<'arena>],
    pub tail: Option<&'arena Expr<'arena>>,
    pub span: Span,
}


#[derive(Debug, Clone)]
pub struct Function<'arena> {
    pub name: Symbol,
    pub params: &'arena [(Symbol, Type)],
    pub return_type: Type,
    pub body: Block<'arena>,
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
    pub params: Vec<Symbol>,
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
            Expr::Try { span, .. } => *span,
            Expr::Error(s, _) => *s,
            Expr::EnumVariant{span, ..} => *span,
            Expr::Match { span, .. } => *span,
            Expr::If{span, ..} => *span

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
            Expr::Try { id, .. } => *id,
            Expr::Error(_, id) => *id,
            Expr::EnumVariant{id, ..} => *id,
            Expr::Match { id, .. } => *id,
           Expr::If{id, ..} => *id
        }
    }
}

impl<'arena> Stmt<'arena> {
    pub fn span(&self) -> Span {
        match self {
            Stmt::Let { span, ..} => *span,
            Stmt::Return(_, span) => *span,
            Stmt::Expr(_, span) => *span,
           
            Stmt::Assign { span, .. } => *span,
            Stmt::While { span, .. } => *span,
            Stmt::For { span, .. } => *span,
            Stmt::AssignIndex { span, .. } => *span,
            Stmt::AssignField { span, .. } => *span,
            Stmt::AssignDeref { span, .. } => *span,

            Stmt::Error(span) => *span,
        }
    }
}

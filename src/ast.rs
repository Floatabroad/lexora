#[derive(Debug, Clone)]
pub enum Expr {
    Integer(i64),
    Identifier(String),
    BinaryOp{
        left: Box<Expr>,
        op: BinaryOperator,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },
    Cast {
        expr: Box<Expr>,
        target_type: Type,
    },
    Bool(bool),
    UnaryOp {
        op : UnaryOperator,
        operand: Box<Expr>,
    },
    StringLiteral(String),
    ArrayLiteral(Vec<Expr>),
    Index {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    StructLiteral {
        name: String,
        fields: Vec<(String, Expr)>,
    },
    FieldAccess {
        object: Box<Expr>,
        field: String,
    },

}

#[derive(Debug, Clone)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Less,
    Greater,
    And,
    Or,
    LessEq,
    GreaterEq,
}
#[derive(Debug, Clone)]
pub enum UnaryOperator {
    Not,
    Neg,
}
#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        ty: Type,
        value: Expr,
        line: usize,
    },
    Return(Expr, usize),
    Expr(Expr, usize),
    If{
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        line: usize,
    },
    Assign{
        name: String,
        value: Expr,
        line: usize,
    },
    While{
        condition: Expr,
        body: Vec<Stmt>,
        line: usize,
    },
    For {
        var: String,
        from: Expr,
        to: Expr,
        body: Vec<Stmt>,
        line: usize,
    },
    AssignIndex {
        name: String,
        index: Expr,
        value: Expr,
        line: usize,
    },
    AssignField {
        object: String,
        field: String,
        value: Expr,
        line: usize,
    },
}
#[derive(Debug, Clone)]
pub enum Type {
    I32,
    I64,
    Bool,
    Void,
    Str,
    Array(Box<Type>, usize),
    Struct(String),
}
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Vec<Stmt>,
}
#[derive(Debug, Clone)]
pub struct Program {
    pub functions: Vec<Function>,
    pub imports: Vec<String>,
    pub structs: Vec<StructDef>,
}

#[derive(Debug, Clone)]
pub struct StructDef {
    pub name: String,
    pub fields: Vec<(String, Type)>,
}
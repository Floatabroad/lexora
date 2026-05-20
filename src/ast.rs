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

    Bool(bool),
    StringLiteral(String),
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
}
#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        ty: Type,
        value: Expr,
    },
    Return(Expr),
    Expr(Expr),
    If{
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
    },
    Assign{
        name: String,
        value: Expr,
    },
    While{
        condition: Expr,
        body: Vec<Stmt>,
    },

}
#[derive(Debug, Clone)]
pub enum Type {
    I32,
    I64,
    Bool,
    Void,
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
}
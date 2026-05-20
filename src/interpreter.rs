use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

pub struct Interpreter {
    variables: HashMap<String, Value>,
    functions: HashMap<String, Function>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            variables: HashMap::new(),
            functions: HashMap::new(),
        }
    }
    pub fn run_program(&mut self, program: &Program) {
        for func in &program.functions {
            self.functions.insert(func.name.clone(), func.clone());
        }
    }
    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> Value{
        let func = match self.functions.get(name) {
            Some(f) => f.clone(),
            None => panic!("Fonksiyon bulunamadi: '{}'", name),
        };
        let mut saved_vars = self.variables.clone();
        self.variables.clear();

        for ((param_name, _ ), value) in func.params.iter().zip(args.into_iter()) {
            self.variables.insert(param_name.clone(), value);
        }
        let result = self.run_body(&func.body);
        self.variables = saved_vars;
        result
    }
    fn run_body(&mut self, stmts: &[Stmt]) -> Value {
        for stmt in stmts {
            if let Some(val) = self.run_statement(stmt) {
                return val;
            }
        }
        panic!("Fonksiyon deger dondurmedi");
    }
    fn run_statement(&mut self, stmt: &Stmt) -> Option<Value> {
        match stmt {
            Stmt::Let {name, value, ..} => {
                let val = self.eval_expr(value);
                self.variables.insert(name.clone(), val);
                None
            }
            Stmt::Return(expr) => {
                Some(self.eval_expr(expr))
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr);
                None
            }
            Stmt::If { condition, then_body, else_body} => {
                let cond = self.eval_expr(condition);
                let body = match cond {
                    Value::Bool(true) => Some(then_body.clone()),
                    Value::Bool(false) => else_body.clone(),
                    _ => panic!("If kosulu bool olmali"),
                };
                if let Some(stmts) = body {
                    for stmt in &stmts {
                        if let Some(val) = self.run_statement(stmt) {
                            return Some(val);
                        }
                    }
                }
                None
            }
            Stmt::While { condition, body} => {
                loop {
                    let cond = self.eval_expr(condition);
                    match cond {
                        Value::Bool(true) => {
                            for stmt in body {
                                if let Some(val) = self.run_statement(stmt) {
                                    return Some(val);
                                }
                            }
                        }
                        Value::Bool(false) => return None,
                        _ => panic!("While kosulu bool olmali"),
                    }
                }
                None
            }
            Stmt::Assign{name, value} => {
                let val = self.eval_expr(value);
                if !self.variables.contains_key(name) {
                    panic!("Tanimlanmamis degisken: '{}'", name);
                }
                self.variables.insert(name.clone(), val);
                None
            }
        }
    }
    fn eval_expr(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::Integer(n) => Value::Int(*n),
            Expr::Identifier(name) => {
                match self.variables.get(name) {
                    Some(v) => v.clone(),
                    None => panic!("Tanimsiz degisken: '{}'", name),
                }
            }
            Expr::BinaryOp {left, op, right} => {
                let l = self.eval_expr(left);
                let r = self.eval_expr(right);
                match (l, r) {
                    (Value::Int(a), Value::Int(b)) => match op {
                        BinaryOperator::Add => Value::Int(a + b),
                        BinaryOperator::Sub => Value::Int(a - b),
                        BinaryOperator::Mul => Value::Int(a * b),
                        BinaryOperator::Div => Value::Int(a / b),
                        BinaryOperator::Eq => Value::Bool(a == b),
                        BinaryOperator::NotEq => Value::Bool(a != b),
                        BinaryOperator::Less => Value::Bool(a < b),
                        BinaryOperator::Greater => Value::Bool(a > b),
                    },
                    _ => panic!("gecersiz operand tipleri"),
                }
            }
            Expr::Bool(b) => Value::Bool(*b),

            Expr::Call { name, args } => {
                let values: Vec<Value> = args.iter()
                    .map(|a| self.eval_expr(a))
                    .collect();

                if name == "print" {
                    for v in &values {
                        match v {
                            Value::Int(n) => println!("{}", n),
                            Value::Bool(b) => println!("{}", b),
                        }
                    }
                    return Value::Int(0);
                }
                self.call_function(name, values)
            }
        }
    }
}

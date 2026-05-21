use crate::ast::*;
use std::collections::HashMap;

pub struct TypeChecker {
    variables: HashMap<String, Type>,
    functions: HashMap<String, (Vec<Type>, Type)>,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            variables: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    pub fn check_program(&mut self, program: &Program) {
        for func in &program.functions {
            self.functions.insert(
                func.name.clone(),
                (
                    func.params.iter().map(|(_, t)| t.clone()).collect(),
                    func.return_type.clone(),
                ),
            );
        }
        for func in &program.functions {
            self.check_function(func);
        }
    }

    fn check_function(&mut self, func: &Function) {
        self.variables.clear();
        for (name, ty) in &func.params {
            self.variables.insert(name.clone(), ty.clone());
        }
        for stmt in &func.body {
            self.check_statement(stmt, &func.return_type);
        }
    }
    fn check_statement(&mut self, stmt: &Stmt, return_type: &Type) {
        match stmt {
            Stmt::Let {name, ty, value, line } => {
                let value_type = self.check_expr(value);
                if !types_match(ty, &value_type) {
                    panic!("Hata [satır {}]: '{:?}' has type '{:?}' but was expected to have type '{:?}'",line,  name, ty, value_type);
                }
                self.variables.insert(name.clone(), ty.clone());
            }
            Stmt::Return(expr, line) => {
                let expr_type = self.check_expr(expr);
                if !types_match(return_type, &expr_type) {
                   panic!("Hata [satır {}] yanlis donus tipi: beklenen {:?}, bulunan{:?}", line, return_type, expr_type);
                }
            }
            Stmt::If {condition, then_body, else_body, line} => {
                let cond_type = self.check_expr(condition);
                if !types_match(&cond_type, &Type::Bool) {
                    panic!("[Hata satır {}]: if kosulu bool olmali : {:?}",line, cond_type);
                }
                for stmt in then_body {
                    self.check_statement(stmt, return_type);
                }
                if let Some(else_body) = else_body {
                    for stmt in else_body {
                        self.check_statement(stmt, return_type);
                    }
                }
            }
            Stmt::While {condition, body, line} => {
                let cond_type = self.check_expr(condition);
                if !types_match(&cond_type, &Type::Bool) {
                    panic!("[Hata satır {}]: while kosulu bool olmali : {:?}",line,  cond_type);
                }
                for stmt in body {
                    self.check_statement(stmt, return_type);
                }
            }
            Stmt::Assign{name, value, line} => {
                let var_type = match self.variables.get(name) {
                    Some(t) => t.clone(),
                    None => panic!("Hata [satır {}]: unknown variable '{}'", line, name),
                };
                let value_type = self.check_expr(value);
                if !types_match(&var_type, &value_type){
                    panic!("Hata [satır {}]'{}' has type {:?} but was expected to have type {:?}",line,  name, var_type, value_type);
                }
            }
            Stmt::Expr(expr, _) => {
                self.check_expr(expr);
            }
            Stmt::For {var, from, to , body, line } => {
                let from_type = self.check_expr(from);
                let to_type = self.check_expr(to);
                if !types_match(&from_type, &Type::I32) {
                    panic!("Hata [satır {}]: for dongusu from tipi i32 olmali : {:?}",line,  from_type);
                }
                if !types_match(&to_type, &Type::I32) {
                    panic!("Hata [satır {}]: for dongusu to tipi i32 olmali : {:?}",line,  to_type);
                }
                self.variables.insert(var.clone(), Type::I32);
                for stmt in body {
                    self.check_statement(stmt, return_type);
                }
                self.variables.remove(var);
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::Bool(_) => Type::Bool,
            Expr::StringLiteral(_) => Type::I32,

            Expr::Integer(_) => Type::I32,
            Expr::Identifier(name) => {
                match self.variables.get(name){
                    Some(ty) => ty.clone(),
                    None => panic!("unknown variable '{}'", name),
                }
            }
            Expr::BinaryOp {left, op, right} => {
                let left_type = self.check_expr(left);
                let right_type = self.check_expr(right);
                if !types_match(&left_type, &right_type) {
                    panic!(
                        "Tip uyusmazligi: {:?} {:?} {:?} uyusmaz", left_type, op, right_type
                    );
                }
                match op {
                    BinaryOperator::Eq
                    | BinaryOperator::NotEq
                    | BinaryOperator::Less
                    | BinaryOperator::Greater
                    | BinaryOperator::And
                    | BinaryOperator::Or  => Type::Bool,
                    _ => left_type,
                }
            }
            Expr::Call {name, args } => {
                if name == "print" {
                    for arg in args {
                        self.check_expr(arg);
                    }
                    return Type::I32;
                }
                let (param_types, return_type) = match self.functions.get(name) {
                    Some(f) => f.clone(),
                    None => panic!("Tanimsiz fonksiyon: '{}'", name),
                };
                if args.len() != param_types.len() {
                    panic!(
                        "'{}'  fonksiyonu {} arguman bekliyor, {} verildi",
                        name, param_types.len(), args.len()
                    );
                }
                for (arg, expected) in args.iter().zip(param_types.iter()) {
                    let arg_type = self.check_expr(arg);
                    if !types_match(&arg_type, expected) {
                        panic!(
                            "'{}' fonksiyonuna yanlis tip: beklenen {:?}, bulunan {:?}", name, expected, arg_type
                        );
                    }
                }
                return_type
            }
        }
    }
}

fn types_match(a: &Type, b: &Type) -> bool {
    matches!((a, b), (Type::I32, Type::I32) | (Type::Bool, Type::Bool)
    | (Type::I64, Type::I64) | (Type::I64, Type::I32))
}
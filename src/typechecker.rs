use crate::ast::*;
use std::collections::HashMap;
//test
pub struct TypeChecker {
    variables: HashMap<String, Type>,
    functions: HashMap<String, (Vec<Type>, Type)>,
    structs: HashMap<String, Vec<(String, Type)>>,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            variables: HashMap::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
        }
    }

    pub fn check_program(&mut self, program: &Program) {
        for s in &program.structs {
            self.structs.insert(s.name.clone(), s.fields.clone());
        }
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
            Stmt::AssignIndex {name, index, value, line } => {
                let var_type = match self.variables.get(name) {
                    Some(t) => t.clone(),
                    None => panic!("Hata [satır {}]: unknown variable '{}'", line, name),
                };
                let elem_ty = match var_type {
                    Type::Array(ref elem, _) => *elem.clone(),
                    _ => panic!("Hata [satır {}]: '{}' bir array değil", line, name),
                };
                let idx_ty = self.check_expr(index);
                if !types_match(&idx_ty, &Type::I32) {
                    panic!("Array indexi i32 olmali");
                }
                let val_ty = self.check_expr(value);
                if !types_match(&val_ty, &elem_ty) {
                    panic!("Hata [satr {}]: yanlis tip", line);
                }
            }
            Stmt::AssignField {object, field, value, line } => {
                let obj_type = match self.variables.get(object) {
                    Some(t) => t.clone(),
                    None => panic!("Hata [satir {}]: unknown variable '{}'", line, object),
                };
                let struct_name = match &obj_type {
                    Type::Struct(n) => n.clone(),
                    _ => panic! ("Hata [satir {}]: '{}' bir struct degil", line, object),
                };
                let fields = match self.structs.get(&struct_name) {
                    Some(f) => f.clone(),
                    None => panic!("Hata [satir {}]: unknown struct: '{}'", line, struct_name),
                };
                let field_type = match fields.iter().find(|(n, _)| n == field) {
                    Some((_, t)) => t.clone(),
                    None => panic!("Hata [satir {}]: '{}' struct'inda '{}' alani yok", line,struct_name, field),
                };
                let val_type = self.check_expr(value);
                if !types_match(&val_type, &field_type) {
                    panic!("Hata [satir {}]: tip uyusmazligi", line);
                }
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::Bool(_) => Type::Bool,
            Expr::StringLiteral(_) => Type::Str,
            Expr::UnaryOp { op: UnaryOperator::Not, operand } => {
                let ty = self.check_expr(operand);
                if !matches!(ty, Type::Bool) {
                    panic!("not operatoru sadece bool ile kullanılabilir");
                }
                Type::Bool
            }
            Expr::UnaryOp { op: UnaryOperator::Neg, operand } => {
                let ty = self.check_expr(operand);
                if !matches!(ty, Type::I32 | Type::I64) {
                    panic!("negatif operatoru sadece i32 veya i64 ile kullanilabilir");
                }
                ty
            }
            Expr::Cast { expr, target_type } => {
                let from = self.check_expr(expr);
                match (&from, target_type) {
                    (Type::I32, Type::I64) => Type::I64,
                    _ => panic!("geçersiz cast:  {:?} as {:?}", from, target_type),
                }
            }

            Expr::Integer(n) => {
                if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    Type::I64
                }else {
                    Type::I32
                }
            }
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
                    | BinaryOperator::LessEq
                    | BinaryOperator::GreaterEq
                    | BinaryOperator::And
                    | BinaryOperator::Or  => Type::Bool,
                    _ => left_type,
                }
            }
            Expr::ArrayLiteral(elems) => {
                if elems.is_empty() {
                    panic!("Boş array desteklenmiyor");
                }
                let first_ty = self.check_expr(&elems[0]);
                for elem in &elems[1..] {
                    let ty = self.check_expr(elem);
                    if !types_match(&ty, &first_ty) {
                        panic!("Array elemanları aynı tipte olmalı");
                    }
                }
                Type::Array(Box::new(first_ty), elems.len())
            }
            Expr::Index {array, index } => {
                let arr_ty = self.check_expr(array);
                let idx_ty = self.check_expr(index);
                if !types_match(&idx_ty, &Type::I32) {
                    panic!("Array indexi i32 olmalı");
                }
                match arr_ty {
                    Type::Array(elem_ty, _) => *elem_ty,
                    _ => panic!("Index sadece arraylere uygulanabilir"),
                }
            }
            Expr::StructLiteral {name, fields} => {
                let struct_fields = match self.structs.get(name) {
                    Some(f) => f.clone(),
                    None => panic!("Tanimsiz struct: '{}'", name),
                };
                for (field_name, value) in fields {
                    let expected = match struct_fields.iter().find(|(n, _)| n == field_name) {
                        Some((_, t)) => t.clone(),
                        None => panic!("Structda '{}' alani yok", field_name),
                    };
                    let actual = self.check_expr(value);
                    if !types_match(&actual, &expected) {
                        panic!("Structda '{}' alani tipi uyusmuyor: beklenen {:?}, bulunan {:?}", field_name, expected, actual);
                    }
                }
                Type::Struct(name.clone())
            }
            Expr::FieldAccess {object, field} => {
                let obj_type = self.check_expr(object);
                let struct_name = match &obj_type {
                    Type::Struct(n) => n.clone(),
                    _ => panic!("field access sadece struct'larda kullanılabilir."),
                };
                let fields = match self.structs.get(&struct_name) {
                    Some(f) => f.clone(),
                    None => panic!("unknown struct '{}'", struct_name),
                };
                match fields.iter().find(|(n, _)| n == field) {
                    Some((_, ty)) => ty.clone(),
                    None => panic!("'{}' struct'inda '{}' alani yok",struct_name, field),
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
    match (a, b) {
        (Type::I32, Type::I32) | (Type::Bool, Type::Bool) |
        (Type::I64, Type::I64) | (Type::Str, Type::Str) => true,
        (Type::Array(ta, sa), Type::Array(tb, sb)) => sa == sb && types_match(ta, tb),
        (Type::Struct(a), Type::Struct(b)) => a == b,
        _ => false,
    }
}
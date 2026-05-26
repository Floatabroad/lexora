use crate::ast::*;
use crate::symbol::{Symbol, Interner};
use crate::error::LexoraError;
use std::collections::HashMap;

pub struct SymbolTable<T> {
    scopes: Vec<HashMap<Symbol, T>>,
}
impl<T> SymbolTable<T> {
    fn new() -> Self {
        SymbolTable { scopes: Vec::new() }
    }
    fn enter_scope(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn exit_scope(&mut self) {
        self.scopes.pop();
    }
    fn define(&mut self, sym: Symbol, val: T) -> bool {
        let scope = self.scopes.last_mut().unwrap();
        if scope.contains_key(&sym) {
            return false;
        }
        scope.insert(sym, val);
        true
    }
    fn lookup(&self, sym: Symbol) -> Option<&T> {
        self.scopes.iter().rev().find_map(|s| s.get(&sym))
    }
}

pub struct TypeChecker<'i> {
    variables: SymbolTable<Type>,
    functions: HashMap<Symbol, (Vec<Type>, Type)>,
    structs:   HashMap<Symbol, Vec<(Symbol, Type)>>,
    interner: &'i Interner,
}

impl<'i> TypeChecker<'i> {
    pub fn new(interner: &'i Interner) -> Self {
        TypeChecker {
            variables: SymbolTable::new(),
            functions: HashMap::new(),
            structs:   HashMap::new(),
            interner,
        }
    }
    fn resolve(&self, sym:  Symbol) -> String {
        self.interner.resolve(sym).to_string()
    }
    pub fn check_program<'arena>(&mut self, program: &Program<'arena>) -> Result<(), LexoraError> {
        for s in &program.structs {
            self.structs.insert(s.name, s.fields.clone());
        }
        for func in &program.functions {
            let param_types = func.params.iter().map(|(_, t)| t.clone()).collect();
            self.functions.insert(func.name, (param_types, func.return_type.clone()));
        }
        for func in &program.functions {
            self.check_function(func)?;
        }
        Ok(())
    }
    fn check_function<'arena>(&mut self, func: &Function<'arena>) -> Result<(), LexoraError> {
        self.variables.enter_scope();
        for (sym, ty) in func.params.iter() {
            if !self.variables.define(*sym,ty.clone()) {
                return Err(LexoraError::AlreadyDefined {
                    name: self.resolve(*sym),
                    span: func.span,
                });
            }
        }
        for stmt in func.body.iter() {
            self.check_statement(stmt, &func.return_type)?;
        }
        self.variables.exit_scope();
        Ok(())
    }

    fn format_type(&self, ty: &Type) -> String {
        match ty {
            Type::I32 => "i32".to_string(),
            Type::I64 => "i64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Void => "void".to_string(),
            Type::Str => "str".to_string(),
            Type::Array(elem,n) => format!("[{}; {}]",self.format_type(elem),n),
            Type::Struct(s) => self.resolve(*s),
        }
    }
    fn check_statement<'arena>(
        &mut self,
        stmt: &Stmt<'arena>,
        ret_ty: &Type,
    )-> Result<(), LexoraError> {
        match stmt {
            Stmt::Let{name, ty, value, span }=> {
                let val_ty = self.check_expr(value)?;
                if !types_match(ty, &val_ty){
                    return Err(LexoraError::TypeMismatch {
                        expected: self.format_type(ty),
                        found: self.format_type(&val_ty),
                        span: *span,
                    });
                }
                if !self.variables.define(*name, ty.clone()) {
                    return Err(LexoraError::AlreadyDefined {
                        name: self.resolve(*name),
                        span: *span,
                    });
                }
                Ok(())
            }
            Stmt::Assign {name, value, span} => {
                let var_ty = match self.variables.lookup(*name) {
                    Some(t) => t.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: self.resolve(*name),
                        span: *span,
                    }),
                };
                let val_ty = self.check_expr(value)?;
                if !types_match(&var_ty, &val_ty){
                    return Err(LexoraError::TypeMismatch {
                        expected: self.format_type(&var_ty),
                        found:    self.format_type(&val_ty),
                        span: *span,
                    });
                }
                Ok(())
            }
            Stmt::Return(expr, span) => {
                let expr_ty = self.check_expr(expr)?;
                if !types_match(ret_ty, &expr_ty) {
                    return Err(LexoraError::TypeMismatch {
                        expected: self.format_type(ret_ty),
                        found: self.format_type(&expr_ty),
                        span: *span,
                    });
                }
                Ok(())
            }
            Stmt::Expr(expr, _) => {
                self.check_expr(expr)?;
                Ok(())
            }
            Stmt::If {condition, then_body, else_branch, span} =>{
                let cond_ty = self.check_expr(condition)?;
                if !types_match(&cond_ty, &Type::Bool){
                    return  Err(LexoraError::TypeMismatch {
                        expected: "bool".to_string(),
                        found: self.format_type(&cond_ty),
                        span: *span,
                    });
                }
                self.variables.enter_scope();
                for s in then_body.iter() {self.check_statement(s, ret_ty)?;}
                self.variables.exit_scope();
                if let Some(else_stmts) = else_branch {
                    self.variables.enter_scope();
                    for s in else_stmts.iter() {self.check_statement(s, ret_ty)?;}
                    self.variables.exit_scope();
                }
                Ok(())
            }
            Stmt::While {condition, body, span} => {
                let cond_ty = self.check_expr(condition)?;
                if !types_match(&cond_ty, &Type::Bool){
                    return Err(LexoraError::TypeMismatch {
                        expected: "bool".to_string(),
                        found: self.format_type(&cond_ty),
                        span: *span,
                    });
                }
                self.variables.enter_scope();
                for s in body.iter() {self.check_statement(s, ret_ty)?;}
                self.variables.exit_scope();
                Ok(())
            }
            Stmt::For{ var,from, to, body,span} => {
                let from_ty = self.check_expr(from)?;
                let to_ty = self.check_expr(to)?;
                if !types_match(&from_ty, &Type::I32) {
                    return Err(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&from_ty),
                        span: *span,
                    });
                }
                if !types_match(&to_ty, &Type::I32) {
                    return Err(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&to_ty),
                        span: *span,
                    });
                }
                self.variables.enter_scope();
                if !self.variables.define(*var, Type::I32) {
                    return Err(LexoraError::AlreadyDefined {
                        name: self.resolve(*var),
                        span: *span,
                    });
                }
                for s in body.iter() {self.check_statement(s, ret_ty)?;}
                self.variables.exit_scope();
                Ok(())
            }
            Stmt::AssignIndex {name, index, value, span } => {
                let arr_ty = match self.variables.lookup(*name) {
                    Some(t) => t.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: self.resolve(*name),
                        span: *span,
                    }),
                };
                let elem_ty = match arr_ty {
                    Type::Array(elem, _) => *elem,
                    _ => return Err(LexoraError::Custom {
                        message: format!("'{}' bir dizi degil", self.resolve(*name)),
                        span: *span,
                    }),
                };
                let idx_ty = self.check_expr(index)?;
                if !types_match(&idx_ty, &Type::I32) {
                    return Err(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&idx_ty),
                        span: *span,
                    });
                }
                let val_ty = self.check_expr(value)?;
                if !types_match(&elem_ty, &val_ty) {
                    return Err(LexoraError::TypeMismatch {
                        expected: self.format_type(&elem_ty),
                        found: self.format_type(&val_ty),
                        span: *span,
                    });
                }
                Ok(())
            }
            Stmt::AssignField {object, field, value, span} => {
                let obj_ty = match self.variables.lookup(*object) {
                    Some(t) => t.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: self.resolve(*object),
                        span: *span,
                    }),
                };
                let struct_sym = match &obj_ty {
                    Type::Struct(s) => *s,
                    _ => return Err(LexoraError::Custom {
                        message: format!("'{}', bir struct degil",
                        self.resolve(*object)),
                        span: *span,
                    }),
                };
                let fields = match self.structs.get(&struct_sym).cloned(){
                    Some(f) => f,
                    None => return Err(LexoraError::Custom {
                        message: format!("tanımsız bir struct '{}'",
                        self.resolve(struct_sym)),
                        span: *span,
                    }),
                };
                let field_ty = match fields.iter().find(|(n, _)|*n == *field){
                    Some((_, t)) => t.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: self.resolve(*field),
                        span: *span,
                    }),
                };
                let val_ty = self.check_expr(value)?;
                if !types_match(&field_ty, &val_ty) {
                    return Err(LexoraError::TypeMismatch {
                        expected: self.format_type(&field_ty),
                        found: self.format_type(&val_ty),
                        span: *span,
                    });
                }
                Ok(())
            }
        }
    }
    fn check_expr<'arena>(&mut self, expr: &Expr<'arena>) -> Result<Type, LexoraError> {
        match expr{
            Expr::Integer(n, _) => {
                if *n >= i32::MIN as i64 && *n <=i32::MAX as i64{
                    Ok(Type::I32)
                }else {
                    Ok(Type::I64)
                }
            }
            Expr::Bool(_, _)    =>Ok(Type::Bool),
            Expr::StringLiteral(_,_) => Ok(Type::Str),

            Expr::Identifier(sym,span) =>{
                match self.variables.lookup(*sym) {
                    Some(t) => Ok(t.clone()),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: self.resolve(*sym),
                        span: *span,
                    }),
                }
            }
            Expr::BinaryOp {left, op, right, span} => {
                let lt = self.check_expr(left)?;
                let rt = self.check_expr(right)?;
                match op {
                    BinaryOperator::And | BinaryOperator::Or => {
                        if !types_match(&lt, &Type::Bool) || !types_match(&rt, &Type::Bool) {
                            return Err(LexoraError::TypeMismatch {
                                expected: "bool".to_string(),
                                found: if !types_match(&lt, &Type::Bool) {
                                    self.format_type(&lt)
                                } else {
                                    self.format_type(&rt)
                                },
                                span: *span,
                            });
                        }
                        Ok(Type::Bool)
                    }
                    BinaryOperator::Eq | BinaryOperator::NotEq |
                    BinaryOperator::Less | BinaryOperator::Greater|
                    BinaryOperator::LessEq | BinaryOperator::GreaterEq => {
                        if !types_match(&lt, &rt) {
                            return Err(LexoraError::TypeMismatch {
                                expected: self.format_type(&lt),
                                found: self.format_type(&rt),
                                span: *span,
                            });
                        }
                        Ok(Type::Bool)
                    }
                    _ => {
                        if !types_match(&lt, &rt) {
                            return Err(LexoraError::TypeMismatch {
                                expected: self.format_type(&lt),
                                found: self.format_type(&rt),
                                span: *span,
                            });
                        }
                        Ok(lt)
                    }
                }
            }
            Expr::UnaryOp {op, operand, span } => {
                let ty = self.check_expr(operand)?;
                match op {
                    UnaryOperator::Not => {
                        if !types_match(&ty, &Type::Bool) {
                            return Err(LexoraError::TypeMismatch {
                                expected: "bool".to_string(),
                                found: self.format_type(&ty),
                                span: *span,
                            });
                        }
                        Ok(Type::Bool)
                    }
                    UnaryOperator::Neg => {
                        if !types_match(&ty, &Type::I32) && !types_match(&ty,
                        &Type::I64) {
                            return Err(LexoraError::TypeMismatch {
                                expected: "i32 veya i64".to_string(),
                                found: self.format_type(&ty),
                                span: *span,
                            });
                        }
                        Ok(ty)
                    }
                }
            }
            Expr::Cast {expr, target_type, span} => {
                let from_ty = self.check_expr(expr)?;
                match(&from_ty, target_type){
                    (Type::I32, Type::I64) | (Type::I64, Type::I32) =>
                    Ok(target_type.clone()),
                    _ => Err(LexoraError::InvalidCast {
                        from: self.format_type(&from_ty),
                        to: self.format_type(target_type),
                        span: *span,
                    }),
                }
            }
            Expr::Call{name, args, span } => {
                if let Some((param_types, ret_ty)) = self.functions.get(name).cloned() {
                    if args.len() != param_types.len() {
                        return Err(LexoraError::Custom {
                            message: format!("'{}' {} argüman bekliyor, {} verildi", self.resolve(*name), param_types.len(), args.len()),
                            span: *span,
                        });
                    }
                    for (arg, param_ty) in args.iter().zip(param_types.iter()){
                        let arg_ty = self.check_expr(arg)?;
                        if !types_match(&arg_ty, param_ty) {
                            return Err(LexoraError::TypeMismatch {
                                expected: self.format_type(&param_ty),
                                found: self.format_type(&arg_ty),
                                span: *span,
                            });
                        }
                    }
                    Ok(ret_ty)
                }else {
                    let name_str = self.resolve(*name);
                    if name_str == "print" {
                        if args.len() != 1 {
                            return Err(LexoraError::Custom {
                                message: "print bir arguman alir".to_string(),
                                span: *span,
                            });
                        }
                        self.check_expr(&args[0])?;
                        Ok(Type::Void)
                    }else {
                        Err(LexoraError::UndefinedFunction {
                            name: name_str,
                            span: *span,
                        })
                    }
                }
            }
            Expr::ArrayLiteral(elems, span) => {
                if elems.is_empty() {
                    return Err(LexoraError::Custom {
                        message: "bos dizi literal tanımlamaz".to_string(),
                        span: *span,
                    });
                }
                let elem_ty = self.check_expr(&elems[0])?;
                for e in elems.iter().skip(1) {
                    let t = self.check_expr(e)?;
                    if !types_match(&elem_ty, &t) {
                        return Err(LexoraError::TypeMismatch {
                            expected: self.format_type(&elem_ty),
                            found: self.format_type(&t),
                            span: *span,
                        });
                    }
                }
                Ok(Type::Array(Box::new(elem_ty), elems.len()))
            }
            Expr::Index {array, index, span} => {
                let arr_ty = self.check_expr(array)?;
                let idx_ty = self.check_expr(index)?;
                if !types_match(&idx_ty, &Type::I32) {
                    return Err(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&idx_ty),
                        span: *span,
                    });
                }
                match arr_ty {
                    Type::Array(elem, _) => Ok(*elem),
                    _ => Err(LexoraError::Custom {
                        message: "dizi olmayan değere index uygulanamaz".to_string(),
                        span: *span,
                    }),
                }
            }
            Expr::StructLiteral {name, fields, span} => {
                let struct_fields = match self.structs.get(name).cloned() {
                    Some(f) => f,
                    None => return Err(LexoraError::Custom {
                        message: format!("tanimsiz struct '{}'", self.resolve(*name)),
                        span: *span,
                    }),
                };
                for (field_name, field_val) in fields.iter() {
                    let expected_ty = match struct_fields.iter().find(|(n, _)| *n == *field_name) {
                        Some((_, t)) => t.clone(),
                        None => return Err(LexoraError::UndefinedVariable {
                            name: self.resolve(*field_name),
                            span: *span,
                        }),
                    };
                    let actual_ty = self.check_expr(field_val)?;
                    if !types_match(&expected_ty, &actual_ty) {
                        return Err(LexoraError::TypeMismatch {
                            expected: self.format_type(&expected_ty),
                            found: self.format_type(&actual_ty),
                            span: *span,
                        });
                    }
                }
                Ok(Type::Struct(*name))
            }
            Expr::FieldAccess {object, field, span} => {
                let obj_ty = self.check_expr(object)?;
                let struct_sym = match &obj_ty {
                    Type::Struct(s) => *s,
                    _ => return Err(LexoraError::Custom {
                        message: "alan erişimi için struct gerekli".to_string(),
                        span: *span,
                    }),
                };
                let fields = match self.structs.get(&struct_sym).cloned() {
                    Some(f) => f,
                    None => return Err(LexoraError::Custom {
                        message: format!("tanımsız struct '{}'", self.resolve(struct_sym)),
                        span: *span,
                    }),
                };
                match fields.iter().find(|(n, _)| *n == *field) {
                    Some((_, t)) => Ok(t.clone()),
                    None => Err(LexoraError::UndefinedVariable {
                        name: self.resolve(*field),
                        span: *span,
                    }),
                }
            }
        }
    }
}

fn types_match(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::I32, Type::I32) => true,
        (Type::I64, Type::I64) => true,
        (Type::Bool, Type::Bool) => true,
        (Type::Str, Type::Str) => true,
        (Type::Void, Type::Void) => true,
        (Type::Array(t1, n1), Type::Array(t2, n2)) => types_match(t1, t2) && n1 == n2,
        (Type::Struct(s1), Type::Struct(s2)) => s1 == s2,
        _ => false,
    }
}

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
    fn names(&self) -> impl Iterator<Item = Symbol> + '_ {
        self.scopes.iter().rev().flat_map(|s| s.keys().copied())
    }
}

pub struct TypeChecker<'i> {
    variables: SymbolTable<Type>,
    functions: HashMap<Symbol, (Vec<Type>, Type)>,
    structs:   HashMap<Symbol, Vec<(Symbol, Type)>>,
    interner: &'i Interner,
    pub types: HashMap<ExprId, Type>,
    errors: Vec<LexoraError>,
}

impl<'i> TypeChecker<'i> {
    pub fn new(interner: &'i Interner) -> Self {
        TypeChecker {
            variables: SymbolTable::new(),
            functions: HashMap::new(),
            structs:   HashMap::new(),
            interner,
            types: HashMap::new(),
            errors: Vec::new(),
        }
    }
    fn resolve(&self, sym:  Symbol) -> String {
        self.interner.resolve(sym).to_string()
    }
    pub fn check_program<'arena>(&mut self, program: &Program<'arena>) -> Result<(),
        Vec<LexoraError>> {
        for s in &program.structs {
            self.structs.insert(s.name, s.fields.clone());
        }
        for func in &program.functions {
            let param_types = func.params.iter().map(|(_, t)| t.clone()).collect();
            self.functions.insert(func.name, (param_types, func.return_type.clone()));
        }
        for func in &program.functions {
            self.check_function(func);
        }
        if self.errors.is_empty() {
            Ok(())
        } else {
            Err(std::mem::take(&mut self.errors))
        }
    }
    fn nearest_var(&self, target: Symbol) -> Option<String> {
        let name = self.interner.resolve(target);
        crate::suggest::nearest(
            name,
            self.variables.names().map(|sym| self.interner.resolve(sym)),
        )
    }
    fn nearest_fn(&self, target: Symbol) -> Option<String> {
        let name = self.interner.resolve(target);
        crate::suggest::nearest(
            name,
            self.functions
                .keys()
                .map(|sym| self.interner.resolve(*sym))
                .chain(std::iter::once("print")),
        )
    }
    fn nearest_field(&self, target: Symbol, fields: &[(Symbol, Type)]) -> Option<String>
    {
        let name = self.interner.resolve(target);
        crate::suggest::nearest(
            name,
            fields.iter().map(|(n, _)| self.interner.resolve(*n)),
        )
    }
    fn check_function<'arena>(&mut self, func: &Function<'arena>) {
        self.variables.enter_scope();
        for (sym, ty) in func.params.iter() {
            if !self.variables.define(*sym, ty.clone()) {
                self.errors.push(LexoraError::AlreadyDefined {
                    name: self.resolve(*sym),
                    span: func.span,
                });
            }
        }
        for stmt in func.body.iter() {
            self.check_statement(stmt, &func.return_type);
        }
        self.variables.exit_scope();
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
            Type::Error => "<hata>".to_string(),
        }
    }
    fn check_statement<'arena>(&mut self, stmt: &Stmt<'arena>, ret_ty: &Type) {
        match stmt {
            Stmt::Let { name, ty, value, span } => {
                let val_ty = self.check_expr(value);
                let var_ty = match ty {
                    Some(declared) => {
                        if !types_match(declared, &val_ty) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(declared),
                                found: self.format_type(&val_ty),
                                span: *span,
                            });
                        }
                        declared.clone()
                    }
                    None => val_ty,
                };
                if !self.variables.define(*name, var_ty) {
                    self.errors.push(LexoraError::AlreadyDefined {
                        name: self.resolve(*name),
                        span: *span,
                    });
                }
            }
            Stmt::Assign { name, value, span } => {
                let var_ty = match self.variables.lookup(*name) {
                    Some(t) => t.clone(),
                    None => {
                        self.errors.push(LexoraError::UndefinedVariable {
                            name: self.resolve(*name),
                            suggestion: self.nearest_var(*name),
                            span: *span,
                        });
                        Type::Error
                    }
                };
                let val_ty = self.check_expr(value);
                if !types_match(&var_ty, &val_ty) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: self.format_type(&var_ty),
                        found:    self.format_type(&val_ty),
                        span: *span,
                    });
                }
            }
            Stmt::Return(expr, span) => {
                let expr_ty = self.check_expr(expr);
                if !types_match(ret_ty, &expr_ty) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: self.format_type(ret_ty),
                        found: self.format_type(&expr_ty),
                        span: *span,
                    });
                }
            }
            Stmt::Expr(expr, _) => {
                self.check_expr(expr);
            }
            Stmt::If { condition, then_body, else_branch, span } => {
                let cond_ty = self.check_expr(condition);
                if !types_match(&cond_ty, &Type::Bool) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "bool".to_string(),
                        found: self.format_type(&cond_ty),
                        span: *span,
                    });
                }
                self.variables.enter_scope();
                for s in then_body.iter() { self.check_statement(s, ret_ty); }
                self.variables.exit_scope();
                if let Some(else_stmts) = else_branch {
                    self.variables.enter_scope();
                    for s in else_stmts.iter() { self.check_statement(s, ret_ty); }
                    self.variables.exit_scope();
                }
            }
            Stmt::While { condition, body, span } => {
                let cond_ty = self.check_expr(condition);
                if !types_match(&cond_ty, &Type::Bool) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "bool".to_string(),
                        found: self.format_type(&cond_ty),
                        span: *span,
                    });
                }
                self.variables.enter_scope();
                for s in body.iter() { self.check_statement(s, ret_ty); }
                self.variables.exit_scope();
            }
            Stmt::For { var, from, to, body, span } => {
                let from_ty = self.check_expr(from);
                if !types_match(&from_ty, &Type::I32) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&from_ty),
                        span: *span,
                    });
                }
                let to_ty = self.check_expr(to);
                if !types_match(&to_ty, &Type::I32) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&to_ty),
                        span: *span,
                    });
                }
                self.variables.enter_scope();
                if !self.variables.define(*var, Type::I32) {
                    self.errors.push(LexoraError::AlreadyDefined {
                        name: self.resolve(*var),
                        span: *span,
                    });
                }
                for s in body.iter() { self.check_statement(s, ret_ty); }
                self.variables.exit_scope();
            }
            Stmt::AssignIndex { name, index, value, span } => {
                let arr_ty = match self.variables.lookup(*name) {
                    Some(t) => t.clone(),
                    None => {
                        self.errors.push(LexoraError::UndefinedVariable {
                            name: self.resolve(*name),
                            suggestion: self.nearest_var(*name),
                            span: *span,
                        });
                        Type::Error
                    }
                };
                let elem_ty = match arr_ty {
                    Type::Error => Type::Error,
                    Type::Array(elem, _) => *elem,
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: format!("'{}' bir dizi degil", self.resolve(*name)),
                            span: *span,
                        });
                        Type::Error
                    }
                };
                let idx_ty = self.check_expr(index);
                if !types_match(&idx_ty, &Type::I32) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&idx_ty),
                        span: *span,
                    });
                }
                let val_ty = self.check_expr(value);
                if !types_match(&elem_ty, &val_ty) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: self.format_type(&elem_ty),
                        found: self.format_type(&val_ty),
                        span: *span,
                    });
                }
            }
            Stmt::AssignField { object, field, value, span } => {
                let obj_ty = match self.variables.lookup(*object) {
                    Some(t) => t.clone(),
                    None => {
                        self.errors.push(LexoraError::UndefinedVariable {
                            name: self.resolve(*object),
                            suggestion: self.nearest_var(*object),
                            span: *span,
                        });
                        Type::Error
                    }
                };
                let val_ty = self.check_expr(value);
                let struct_sym = match &obj_ty {
                    Type::Error => return,
                    Type::Struct(s) => *s,
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: format!("'{}', bir struct degil",
                                             self.resolve(*object)),
                            span: *span,
                        });
                        return;
                    }
                };
                let fields = match self.structs.get(&struct_sym).cloned() {
                    Some(f) => f,
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: format!("tanimsiz bir struct '{}'",
                                             self.resolve(struct_sym)),
                            span: *span,
                        });
                        return;
                    }
                };
                let field_ty = match fields.iter().find(|(n, _)| *n == *field) {
                    Some((_, t)) => t.clone(),
                    None => {
                        self.errors.push(LexoraError::UndefinedVariable {
                            name: self.resolve(*field),
                            suggestion: self.nearest_field(*field, &fields),
                            span: *span,
                        });
                        return;
                    }
                };
                if !types_match(&field_ty, &val_ty) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: self.format_type(&field_ty),
                        found: self.format_type(&val_ty),
                        span: *span,
                    });
                }
            }
            Stmt::Error(_) => {}
        }
    }
    fn check_expr<'arena>(&mut self, expr: &Expr<'arena>) -> Type {
        let ty = self.check_expr_inner(expr);
        self.types.insert(expr.id(), ty.clone());
        ty
    }
    fn check_expr_inner<'arena>(&mut self, expr: &Expr<'arena>) -> Type {
        match expr {
            Expr::Integer(n, _, _) => {
                if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                    Type::I32
                } else {
                    Type::I64
                }
            }
            Expr::Bool(_, _, _) => Type::Bool,
            Expr::StringLiteral(_, _, _) => Type::Str,

            Expr::Identifier(sym, span, _) => {
                match self.variables.lookup(*sym) {
                    Some(t) => t.clone(),
                    None => {
                        self.errors.push(LexoraError::UndefinedVariable {
                            name: self.resolve(*sym),
                            suggestion: self.nearest_var(*sym),
                            span: *span,
                        });
                        Type::Error
                    }
                }
            }
            Expr::BinaryOp { left, op, right, span, .. } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                match op {
                    BinaryOperator::And | BinaryOperator::Or => {
                        if !types_match(&lt, &Type::Bool) || !types_match(&rt,
                                                                          &Type::Bool) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: "bool".to_string(),
                                found: if !types_match(&lt, &Type::Bool) {
                                    self.format_type(&lt)
                                } else {
                                    self.format_type(&rt)
                                },
                                span: *span,
                            });
                            return Type::Error;
                        }
                        Type::Bool
                    }
                    BinaryOperator::Eq | BinaryOperator::NotEq |
                    BinaryOperator::Less | BinaryOperator::Greater |
                    BinaryOperator::LessEq | BinaryOperator::GreaterEq => {
                        if !types_match(&lt, &rt) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(&lt),
                                found: self.format_type(&rt),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        Type::Bool
                    }
                    _ => {
                        if !types_match(&lt, &rt) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(&lt),
                                found: self.format_type(&rt),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        lt
                    }
                }
            }
            Expr::UnaryOp { op, operand, span, .. } => {
                let ty = self.check_expr(operand);
                match op {
                    UnaryOperator::Not => {
                        if !types_match(&ty, &Type::Bool) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: "bool".to_string(),
                                found: self.format_type(&ty),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        Type::Bool
                    }
                    UnaryOperator::Neg => {
                        if !types_match(&ty, &Type::I32) && !types_match(&ty, &Type::I64)
                        {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: "i32 veya i64".to_string(),
                                found: self.format_type(&ty),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        ty
                    }
                }
            }
            Expr::Cast { expr, target_type, span, .. } => {
                let from_ty = self.check_expr(expr);
                match (&from_ty, target_type) {
                    (Type::Error, _) => Type::Error,
                    (Type::I32, Type::I64) | (Type::I64, Type::I32) =>
                        target_type.clone(),
                    _ => {
                        self.errors.push(LexoraError::InvalidCast {
                            from: self.format_type(&from_ty),
                            to: self.format_type(target_type),
                            span: *span,
                        });
                        Type::Error
                    }
                }
            }
            Expr::Call { name, args, span, .. } => {
                if let Some((param_types, ret_ty)) = self.functions.get(name).cloned() {
                    if args.len() != param_types.len() {
                        self.errors.push(LexoraError::Custom {
                            message: format!("'{}' {} argüman bekliyor, {} verildi",
                                             self.resolve(*name), param_types.len(), args.len()),
                            span: *span,
                        });
                        for arg in args.iter() { self.check_expr(arg); }
                        return ret_ty;
                    }
                    for (arg, param_ty) in args.iter().zip(param_types.iter()) {
                        let arg_ty = self.check_expr(arg);
                        if !types_match(&arg_ty, param_ty) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(&param_ty),
                                found: self.format_type(&arg_ty),
                                span: *span,
                            });
                        }
                    }
                    ret_ty
                } else {
                    let name_str = self.resolve(*name);
                    if name_str == "print" {
                        if args.len() != 1 {
                            self.errors.push(LexoraError::Custom {
                                message: "print bir arguman alir".to_string(),
                                span: *span,
                            });
                        }
                        for arg in args.iter() { self.check_expr(arg); }
                        Type::Void
                    } else {
                        self.errors.push(LexoraError::UndefinedFunction {
                            name: name_str,
                            suggestion: self.nearest_fn(*name),
                            span: *span,
                        });
                        for arg in args.iter() { self.check_expr(arg); }
                        Type::Error
                    }
                }
            }
            Expr::ArrayLiteral(elems, span, _) => {
                if elems.is_empty() {
                    self.errors.push(LexoraError::Custom {
                        message: "bos dizi literal tanımlamaz".to_string(),
                        span: *span,
                    });
                    return Type::Error;
                }
                let elem_ty = self.check_expr(&elems[0]);
                for e in elems.iter().skip(1) {
                    let t = self.check_expr(e);
                    if !types_match(&elem_ty, &t) {
                        self.errors.push(LexoraError::TypeMismatch {
                            expected: self.format_type(&elem_ty),
                            found: self.format_type(&t),
                            span: *span,
                        });
                    }
                }
                Type::Array(Box::new(elem_ty), elems.len())
            }
            Expr::Index { array, index, span, .. } => {
                let arr_ty = self.check_expr(array);
                let idx_ty = self.check_expr(index);
                if !types_match(&idx_ty, &Type::I32) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "i32".to_string(),
                        found: self.format_type(&idx_ty),
                        span: *span,
                    });
                }
                match arr_ty {
                    Type::Error => Type::Error,
                    Type::Array(elem, _) => *elem,
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: "dizi olmayan değere index uygulanamaz".to_string(),
                            span: *span,
                        });
                        Type::Error
                    }
                }
            }
            Expr::StructLiteral { name, fields, span, .. } => {
                let struct_fields = match self.structs.get(name).cloned() {
                    Some(f) => f,
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: format!("tanimsiz struct '{}'",
                                             self.resolve(*name)),
                            span: *span,
                        });
                        for (_, field_val) in fields.iter() { self.check_expr(field_val);
                        }
                        return Type::Error;
                    }
                };
                for (field_name, field_val) in fields.iter() {
                    let actual_ty = self.check_expr(field_val);
                    let expected_ty = match struct_fields.iter().find(|(n, _)| *n ==
                        *field_name) {
                        Some((_, t)) => t.clone(),
                        None => {
                            self.errors.push(LexoraError::UndefinedVariable {
                                name: self.resolve(*field_name),
                                suggestion: self.nearest_field(*field_name,
                                                               &struct_fields),
                                span: *span,
                            });
                            continue;
                        }
                    };
                    if !types_match(&expected_ty, &actual_ty) {
                        self.errors.push(LexoraError::TypeMismatch {
                            expected: self.format_type(&expected_ty),
                            found: self.format_type(&actual_ty),
                            span: *span,
                        });
                    }
                }
                Type::Struct(*name)
            }
            Expr::FieldAccess { object, field, span, .. } => {
                let obj_ty = self.check_expr(object);
                let struct_sym = match &obj_ty {
                    Type::Error => return Type::Error,
                    Type::Struct(s) => *s,
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: "alan erişimi için struct gerekli".to_string(),
                            span: *span,
                        });
                        return Type::Error;
                    }
                };
                let fields = match self.structs.get(&struct_sym).cloned() {
                    Some(f) => f,
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: format!("tanımsız struct '{}'",
                                             self.resolve(struct_sym)),
                            span: *span,
                        });
                        return Type::Error;
                    }
                };
                match fields.iter().find(|(n, _)| *n == *field) {
                    Some((_, t)) => t.clone(),
                    None => {
                        self.errors.push(LexoraError::UndefinedVariable {
                            name: self.resolve(*field),
                            suggestion: self.nearest_field(*field, &fields),
                            span: *span,
                        });
                        Type::Error
                    }
                }
            }
            Expr::Error(_, _) => Type::Error,
        }
    }
}

fn types_match(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Error, _) | (_, Type::Error) => true,
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
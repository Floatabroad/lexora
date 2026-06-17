use crate::ast::*;
use crate::symbol::Symbol;
use crate::error::LexoraError;
use super::types::LlvmType;
use super::value::Value;
use super::builder::IrBuilder;
use std::collections::HashMap;


struct VarTable {
    scopes: Vec<HashMap<Symbol, (LlvmType, Value)>>,
}

impl VarTable {
    fn new() -> Self { VarTable { scopes: Vec::new() }}
    fn enter(&mut self) { self.scopes.push(HashMap::new());}
    fn exit(&mut self) { self.scopes.pop(); }
    fn insert(&mut self, sym: Symbol, val: (LlvmType, Value)){
        self.scopes.last_mut().unwrap().insert(sym, val);
    }
    fn get(&self, sym: &Symbol) -> Option<&(LlvmType, Value)> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(sym) { return Some(v); }
        }
        None
    }
}
pub struct CodeGen<'i> {
    builder: IrBuilder<'i>,
    locals: VarTable,
    functions: HashMap<Symbol, (Vec<LlvmType>, LlvmType)>,
    structs: HashMap<Symbol, Vec<(Symbol, LlvmType)>>,
    current_ret_ty: LlvmType,
    types: &'i HashMap<ExprId, Type>,
}
impl<'i> CodeGen<'i> {
    pub fn new(builder: IrBuilder<'i>, types: &'i HashMap<ExprId, Type>) -> Self {
        CodeGen {
            builder,
            locals: VarTable::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            current_ret_ty: LlvmType::Void,
            types,
        }
    }
    fn ast_type_to_llvm(&self, ty: &Type) -> LlvmType {
        match ty {
            Type::I32 => LlvmType::I32,
            Type::I64 => LlvmType::I64,
            Type::Bool => LlvmType::I1,
            Type::Void => LlvmType::Void,
            Type::Str => LlvmType::Ptr,
            Type::Array(t, n) => LlvmType::Array(Box::new(self.ast_type_to_llvm(t)), *n),
            Type::Struct(s) => LlvmType::Struct(*s),
            Type::Error => unreachable!("Type::Error codegen'e ulasti (errors bos degilse codegen yok)"),
        }
    }
    pub fn gen_program<'arena>(
        &mut self,
        program: &Program<'arena>,
    ) -> Result<String, LexoraError> {
        self.builder.globals.push_str(
            "@.fmt   = private constant [4 x i8] c\"%d\\0A\\00\"\n\
                 @.fmt64 = private constant [6 x i8] c\"%lld\\0A\\00\"\n\
                 @.fmts  = private constant [4 x i8] c\"%s\\0A\\00\"\n\
                 declare i32 @printf(ptr, ...)\n\n"
        );
        self.builder.globals.push_str(
            "@.panicmsg_ovf = private unnamed_addr constant [25 x i8] c\"lexora: integer overflow\\00\"\n\
               declare {i32, i1} @llvm.sadd.with.overflow.i32(i32, i32)\n\
               declare {i32, i1} @llvm.ssub.with.overflow.i32(i32, i32)\n\
               declare {i32, i1} @llvm.smul.with.overflow.i32(i32, i32)\n\
               declare {i64, i1} @llvm.sadd.with.overflow.i64(i64, i64)\n\
               declare {i64, i1} @llvm.ssub.with.overflow.i64(i64, i64)\n\
               declare {i64, i1} @llvm.smul.with.overflow.i64(i64, i64)\n\n"
        );
        self.builder.globals.push_str(
            "declare void @exit(i32)\n\
       @.panicmsg_div = private unnamed_addr constant [25 x i8] c\"lexora: division by zero\\00\"\n\
       @.panicmsg_idx = private unnamed_addr constant [28 x i8] c\"lexora: index out of bounds\\00\"\n\
       define void @lexora_panic(ptr %msg) {\n\
       call i32 (ptr, ...) @printf(ptr @.fmts, ptr %msg)\n\
       call void @exit(i32 1)\n\
       unreachable\n\
       }\n\n"
        );
        for s in &program.structs {
            let field_llvm_tys: Vec<(Symbol, LlvmType)> = s.fields.iter()
                .map(|(sym, ty)| (*sym, self.ast_type_to_llvm(ty)))
                .collect();
            let llvm_tys: Vec<LlvmType> = field_llvm_tys.iter()
                .map(|(_, t)| t.clone())
                .collect();
            self.builder.emit_struct_type(s.name, &llvm_tys);
            self.structs.insert(s.name, field_llvm_tys);
        }
        for func in &program.functions {
            let param_tys = func.params.iter()
                .map(|(_, ty)| self.ast_type_to_llvm(ty))
                .collect();
            let ret_ty = self.ast_type_to_llvm(&func.return_type);
            self.functions.insert(func.name, (param_tys, ret_ty));
        }
        for func in &program.functions {
            self.gen_function(func)?;
        }
        Ok(self.builder.finish())
    }
    fn gen_function<'arena>(&mut self, func: &Function<'arena>) -> Result<(), LexoraError> {
        self.locals = VarTable::new();
        self.locals.enter();

        self.current_ret_ty = self.ast_type_to_llvm(&func.return_type);

        let params_ir: Vec<(Symbol, LlvmType)> = func.params.iter()
            .map(|(sym, ty)| (*sym, self.ast_type_to_llvm(ty)))
            .collect();
        self.builder.emit_function_begin(func.name, &params_ir, &self.current_ret_ty.clone());
        self.builder.emit_label("entry");

        for (sym, llvm_ty) in &params_ir {
            let param_name = self.builder.resolve(*sym).to_string();
            let param_val = Value::Named(format!("%{}", param_name));
            let ptr = self.builder.build_alloca(llvm_ty, "");
            self.builder.build_store(llvm_ty, param_val, ptr.clone());
            self.locals.insert(*sym, (llvm_ty.clone(), ptr));
        }
        for stmt in func.body.iter() { self.gen_statement(stmt)?; }
        if !self.builder.is_terminated() {
            if self.current_ret_ty == LlvmType::Void {
                self.builder.build_ret(&LlvmType::Void, Value::Void);
            } else {
                self.builder.build_unreachable();
            }
        }
        self.locals.exit();
        self.builder.emit_function_end();
        Ok(())
    }


    fn gen_statement<'arena>(&mut self, stmt: &Stmt<'arena>) -> Result<(),
        LexoraError> {
        match stmt {
            Stmt::Return(expr, _) => {
                let val = self.gen_expr(expr)?;
                let ty = self.current_ret_ty.clone();
                self.builder.build_ret(&ty, val);
            }

            Stmt::Expr(expr, _) => {
                self.gen_expr(expr)?;
            }

            Stmt::Let { name, value, .. } => {
                let llvm_ty = self.ast_type_to_llvm(&self.types[&value.id()]);
                let ptr = self.builder.build_alloca(&llvm_ty, "");
                match value {
                    Expr::ArrayLiteral(elems, _, _) => {
                        if let LlvmType::Array(elem_ty, _) = &llvm_ty {
                            for (i, elem) in elems.iter().enumerate() {
                                let elem_val = self.gen_expr(elem)?;
                                let elem_ptr = self.builder.build_gep_array(
                                    elem_ty, elems.len(), ptr.clone(), Value::Const(i
                                        as i64),
                                );
                                self.builder.build_store(elem_ty, elem_val,
                                                         elem_ptr);
                            }
                        }
                    }
                    Expr::StructLiteral { name: struct_name, fields, .. } => {
                        let field_defs =
                            self.structs.get(struct_name).unwrap().clone();
                        for (i, (field_sym, field_llvm_ty)) in
                            field_defs.iter().enumerate() {
                            let field_val = fields.iter()
                                .find(|(n, _)| *n == *field_sym)
                                .map(|(_, v)| self.gen_expr(v))
                                .unwrap()?;
                            let field_ptr = self.builder.build_gep_struct(
                                *struct_name, ptr.clone(), i as u32,
                            );
                            self.builder.build_store(field_llvm_ty, field_val,
                                                     field_ptr);
                        }
                    }
                    _ => {
                        let val = self.gen_expr(value)?;
                        self.builder.build_store(&llvm_ty, val, ptr.clone());
                    }
                }
                self.locals.insert(*name, (llvm_ty, ptr));
            }

            Stmt::Assign { name, value, span } => {
                let (llvm_ty, ptr) = match self.locals.get(name) {
                    Some(v) => v.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: format!("sym#{}", name.0),
                        suggestion: None,
                        span: *span,
                    }),
                };
                let val = self.gen_expr(value)?;
                self.builder.build_store(&llvm_ty, val, ptr);
            }

            Stmt::If { condition, then_body, else_branch, .. } => {
                let cond_val = self.gen_expr(condition)?;
                let then_label = self.builder.fresh_block("then");
                let merge_label = self.builder.fresh_block("merge");

                if let Some(else_stmts) = else_branch {
                    let else_label = self.builder.fresh_block("else");
                    self.builder.build_cond_br(cond_val, &then_label.clone(),
                                               &else_label.clone());

                    self.builder.emit_label(&then_label);
                    self.locals.enter();
                    for s in then_body.iter() { self.gen_statement(s)?; }
                    self.locals.exit();
                    self.builder.build_br(&merge_label);

                    self.builder.emit_label(&else_label);
                    self.locals.enter();
                    for s in else_stmts.iter() { self.gen_statement(s)?; }
                    self.locals.exit();
                    self.builder.build_br(&merge_label);
                } else {
                    self.builder.build_cond_br(cond_val, &then_label.clone(),
                                               &merge_label.clone());

                    self.builder.emit_label(&then_label);
                    self.locals.enter();
                    for s in then_body.iter() { self.gen_statement(s)?; }
                    self.locals.exit();
                    self.builder.build_br(&merge_label);
                }
                self.builder.emit_label(&merge_label);
            }

            Stmt::While { condition, body, .. } => {
                let cond_label = self.builder.fresh_block("while_cond");
                let body_label = self.builder.fresh_block("while_body");
                let end_label = self.builder.fresh_block("while_end");

                self.builder.build_br(&cond_label);
                self.builder.emit_label(&cond_label);
                let cond_val = self.gen_expr(condition)?;
                self.builder.build_cond_br(cond_val, &body_label.clone(),
                                           &end_label.clone());

                self.builder.emit_label(&body_label);
                self.locals.enter();
                for s in body.iter() { self.gen_statement(s)?; }
                self.locals.exit();
                self.builder.build_br(&cond_label);

                self.builder.emit_label(&end_label);
            }

            Stmt::For { var, from, to, body, .. } => {
                let cond_label = self.builder.fresh_block("for_cond");
                let body_label = self.builder.fresh_block("for_body");
                let end_label = self.builder.fresh_block("for_end");

                self.locals.enter();
                let from_val = self.gen_expr(from)?;
                let ptr = self.builder.build_alloca(&LlvmType::I32, "");
                self.builder.build_store(&LlvmType::I32, from_val, ptr.clone());
                self.locals.insert(*var, (LlvmType::I32, ptr.clone()));

                let to_val = self.gen_expr(to)?;
                let to_ptr = self.builder.build_alloca(&LlvmType::I32, "");
                self.builder.build_store(&LlvmType::I32, to_val, to_ptr.clone());

                self.builder.build_br(&cond_label);
                self.builder.emit_label(&cond_label);
                let cur = self.builder.build_load(&LlvmType::I32, ptr.clone());
                let to_cur = self.builder.build_load(&LlvmType::I32, to_ptr);
                let cond = self.builder.build_icmp("slt", &LlvmType::I32, cur,
                                                   to_cur);
                self.builder.build_cond_br(cond, &body_label.clone(),
                                           &end_label.clone());

                self.builder.emit_label(&body_label);
                for s in body.iter() { self.gen_statement(s)?; }
                let cur2 = self.builder.build_load(&LlvmType::I32, ptr.clone());
                let inc_val = self.builder.build_add(&LlvmType::I32, cur2,
                                                     Value::Const(1));
                self.builder.build_store(&LlvmType::I32, inc_val, ptr);
                self.builder.build_br(&cond_label);

                self.builder.emit_label(&end_label);
                self.locals.exit();
            }

            Stmt::AssignIndex { name, index, value, span } => {
                let (arr_ty, arr_ptr) = match self.locals.get(name) {
                    Some(v) => v.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: format!("sym#{}", name.0),
                        suggestion: None,
                        span: *span,
                    }),
                };
                if let LlvmType::Array(elem_ty, size) = arr_ty {
                    let idx_val = self.gen_expr(index)?;
                    let val = self.gen_expr(value)?;
                    let elem_ptr = self.builder.build_checked_gep_array(&elem_ty, size,
                                                                arr_ptr, idx_val);
                    self.builder.build_store(&elem_ty, val, elem_ptr);
                }
            }

            Stmt::AssignField { object, field, value, span } => {
                let (_, obj_ptr) = match self.locals.get(object) {
                    Some(v) => v.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: format!("sym#{}", object.0),
                        suggestion: None,
                        span: *span,
                    }),
                };
                let struct_sym = match self.locals.get(object).unwrap().0.clone() {
                    LlvmType::Struct(s) => s,
                    _ => return Err(LexoraError::Custom {
                        message: "AssignField: struct değil".to_string(),
                        span: *span,
                    }),
                };
                let field_defs = self.structs.get(&struct_sym).unwrap().clone();
                let (idx, (_, field_ty)) = field_defs.iter().enumerate()
                    .find(|(_, (sym, _))| *sym == *field)
                    .unwrap();
                let val = self.gen_expr(value)?;
                let field_ptr = self.builder.build_gep_struct(struct_sym, obj_ptr,
                                                              idx as u32);
                self.builder.build_store(field_ty, val, field_ptr);
            }
            Stmt::Error(_) => return Err(LexoraError::Codegen {
                message: "poison statement codegen'e ulasti".to_string(),
            }),
        }
        Ok(())
    }
    fn gen_expr<'arena>(&mut self, expr: &Expr<'arena>) -> Result<Value, LexoraError> {
        match expr {
            Expr::Integer(n, _, _) => Ok(Value::Const(*n)),
            Expr::Bool(b, _, _) => Ok(Value::Const(if *b { 1 } else { 0 })),
            Expr::StringLiteral(s, _, _) => {
                let (ptr, _) = self.builder.add_string_global(s);
                Ok(ptr)
            }
            Expr::Identifier(sym, span, _) => {
                match self.locals.get(sym) {
                    Some((ty, ptr)) => {
                        let ty = ty.clone();
                        let ptr = ptr.clone();
                        Ok(self.builder.build_load(&ty, ptr))
                    }
                    None => Err(LexoraError::UndefinedVariable {
                        name: format!("sym#{}", sym.0),
                        suggestion: None,
                        span: *span,
                    }),
                }
            }
            Expr::BinaryOp { left, op, right, .. } => {
                let lv = self.gen_expr(left)?;
                let rv = self.gen_expr(right)?;
                let lt = self.expr_llvm_type(left);
                let val = match op {
                    BinaryOperator::Add => self.builder.build_checked_arith("sadd", &lt, lv, rv),
                    BinaryOperator::Sub => self.builder.build_checked_arith("ssub", &lt, lv, rv),
                    BinaryOperator::Mul => self.builder.build_checked_arith("smul", &lt, lv, rv),
                    BinaryOperator::Div => self.builder.build_checked_sdiv(&lt, lv, rv),
                    BinaryOperator::Eq => self.builder.build_icmp("eq", &lt,
                                                                  lv, rv),
                    BinaryOperator::NotEq => self.builder.build_icmp("ne", &lt,
                                                                     lv, rv),
                    BinaryOperator::Less => self.builder.build_icmp("slt", &lt,
                                                                    lv, rv),
                    BinaryOperator::Greater => self.builder.build_icmp("sgt", &lt,
                                                                       lv, rv),
                    BinaryOperator::LessEq => self.builder.build_icmp("sle", &lt,
                                                                      lv, rv),
                    BinaryOperator::GreaterEq => self.builder.build_icmp("sge", &lt,
                                                                         lv, rv),
                    BinaryOperator::And => self.builder.build_and(lv, rv),
                    BinaryOperator::Or => self.builder.build_or(lv, rv),
                };
                Ok(val)
            }
            Expr::UnaryOp { op, operand, .. } => {
                let val = self.gen_expr(operand)?;
                let ty = self.expr_llvm_type(operand);
                let result = match op {
                    UnaryOperator::Not => self.builder.build_not(val),
                    UnaryOperator::Neg => self.builder.build_neg(&ty, val),
                };
                Ok(result)
            }
            Expr::Cast { expr, target_type, .. } => {
                let val = self.gen_expr(expr)?;
                let from_ty = self.expr_llvm_type(expr);
                let to_ty = self.ast_type_to_llvm(target_type);
                let result = match (&from_ty, &to_ty) {
                    (LlvmType::I32, LlvmType::I64) => self.builder.build_sext(val, &from_ty, &to_ty),
                    (LlvmType::I64, LlvmType::I32) => self.builder.build_trunc(val, &from_ty, &to_ty),
                    _ => val,
                };
                Ok(result)
            }
            Expr::Call { name, args, span, .. } => {
                let name_str = self.builder.resolve(*name).to_string();
                if name_str == "print" {
                    let arg = &args[0];
                    let arg_ty = self.expr_llvm_type(arg);
                    let val = self.gen_expr(arg)?;
                    let fmt = match &arg_ty {
                        LlvmType::I64 => "@.fmt64",
                        LlvmType::Ptr => "@.fmts",
                        _ => "@.fmt",
                    };
                    self.builder.output.push_str(&format!(
                        "  call i32 (ptr, ...) @printf(ptr {}, {} {})\n",
                        fmt,
                        arg_ty.to_ir_str(),
                        val.to_ir_str(),
                    ));
                    return Ok(Value::Void);
                }
                let (params_tys, ret_ty) = match self.functions.get(name).cloned() {
                    Some(sig) => sig,
                    None => return Err(LexoraError::UndefinedFunction {
                        name: name_str,
                        suggestion: None,
                        span: *span,
                    }),
                };
                let mut call_args: Vec<(LlvmType, Value)> = Vec::new();
                for (arg, param_ty) in args.iter().zip(params_tys.iter()) {
                    let val = self.gen_expr(arg)?;
                    call_args.push((param_ty.clone(), val));
                }
                Ok(self.builder.build_call(&ret_ty.clone(), *name, &call_args))
            }
            Expr::Index { array, index, span, .. } => {
                let arr_sym = match *array {
                    Expr::Identifier(s, _, _) => s,
                    _ => return Err(LexoraError::Custom {
                        message: "index: dizi adı bekleniyor".to_string(),
                        span: *span,
                    }),
                };
                let (arr_ty, arr_ptr) = match self.locals.get(&arr_sym) {
                    Some(v) => v.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: format!("sym#{}", arr_sym.0),
                        suggestion: None,
                        span: *span,
                    }),
                };
                if let LlvmType::Array(elem_ty, size) = arr_ty {
                    let idx_val = self.gen_expr(index)?;
                    let elem_ptr = self.builder.build_checked_gep_array(&elem_ty, size,
                                                                arr_ptr, idx_val);
                    Ok(self.builder.build_load(&elem_ty, elem_ptr))
                } else {
                    Err(LexoraError::Custom {
                        message: "index: dizi değil".to_string(),
                        span: *span,
                    })
                }
            }
            Expr::FieldAccess { object, field, span, .. } => {
                let obj_sym = match *object {
                    Expr::Identifier(s, _,_) => s,
                    _ => return Err(LexoraError::Custom {
                        message: "alan erişimi: identifier bekleniyor".to_string(),
                        span: *span,
                    }),
                };
                let (obj_ty, obj_ptr) = match self.locals.get(&obj_sym) {
                    Some(v) => v.clone(),
                    None => return Err(LexoraError::UndefinedVariable {
                        name: format!("sym#{}", obj_sym.0),
                        suggestion: None,
                        span: *span,
                    }),
                };
                let struct_sym = match obj_ty {
                    LlvmType::Struct(s) => s,
                    _ => return Err(LexoraError::Custom {
                        message: "alan erişimi: struct değil".to_string(),
                        span: *span,
                    }),
                };
                let field_defs = self.structs.get(&struct_sym).unwrap().clone();
                let (idx, (_, field_ty)) = field_defs.iter().enumerate()
                    .find(|(_, (sym, _))| *sym == *field)
                    .unwrap();
                let field_ptr = self.builder.build_gep_struct(struct_sym, obj_ptr,
                                                              idx as u32);
                Ok(self.builder.build_load(&field_ty.clone(), field_ptr))
            }
            Expr::ArrayLiteral(_, span, _) | Expr::StructLiteral { span, .. } => {
                Err(LexoraError::Custom {
                    message: "literal doğrudan ifade olarak kullanılamaz".to_string(),
                    span: *span,
                })
            }
            Expr::Error(_,_) => Err(LexoraError::Codegen {
                message: "poison expression codegen'e ulasti".to_string(),
            }),
        }
    }
    // fn expr_llvm_type<'arena>(&self, expr: &Expr<'arena>) -> LlvmType {
    //     match expr {
    //         Expr::Integer(n, _) => {
    //             if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 { LlvmType::I32 }
    //             else { LlvmType::I64 }
    //         }
    //         Expr::Bool(_, _)          => LlvmType::I1,
    //         Expr::StringLiteral(_, _) => LlvmType::Ptr,
    //         Expr::Identifier(sym, _)  => {
    //             self.locals.get(sym).map(|(t, _)| t.clone()).unwrap_or(LlvmType::I32)
    //         }
    //         Expr::BinaryOp { op, left, .. } => match op {
    //             BinaryOperator::Eq | BinaryOperator::NotEq  |
    //             BinaryOperator::Less | BinaryOperator::Greater |
    //             BinaryOperator::LessEq | BinaryOperator::GreaterEq |
    //             BinaryOperator::And | BinaryOperator::Or => LlvmType::I1,
    //             _ => self.expr_llvm_type(left),
    //         },
    //         Expr::UnaryOp { op, operand, .. } => match op {
    //             UnaryOperator::Not => LlvmType::I1,
    //             UnaryOperator::Neg => self.expr_llvm_type(operand),
    //         },
    //         Expr::Cast { target_type, .. } => self.ast_type_to_llvm(target_type),
    //         Expr::Call { name, .. } => {
    //             self.functions.get(name).map(|(_, r)|
    //                 r.clone()).unwrap_or(LlvmType::I32)
    //         }
    //         Expr::Index { array, .. } => {
    //             if let Expr::Identifier(sym, _) = *array {
    //                 if let Some((LlvmType::Array(elem, _), _)) = self.locals.get(&sym)
    //                 {
    //                     return *elem.clone();
    //                 }
    //             }
    //             LlvmType::I32
    //         }
    //         Expr::FieldAccess { object, field, .. } => {
    //             if let Expr::Identifier(sym, _) = *object {
    //                 if let Some((LlvmType::Struct(s), _)) = self.locals.get(sym) {
    //                     if let Some(fields) = self.structs.get(s) {
    //                         if let Some((_, ty)) = fields.iter().find(|(n, _)| n ==
    //                             field) {
    //                             return ty.clone();
    //                         }
    //                     }
    //                 }
    //             }
    //             LlvmType::I32
    //         }
    //         Expr::ArrayLiteral(_, _) | Expr::StructLiteral { .. } => LlvmType::Ptr,
    //     }
    // }
    fn expr_llvm_type<'arena>(&self, expr: &Expr<'arena>) -> LlvmType {
        self.ast_type_to_llvm(&self.types[&expr.id()])
    }
}



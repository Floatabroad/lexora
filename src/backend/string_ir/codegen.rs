use crate::ast::*;
use crate::symbol::Symbol;
use crate::error::LexoraError;
use super::types::LlvmType;
use super::value::Value;
use super::builder::IrBuilder;
use std::collections::{HashMap, HashSet};




struct VarTable {
    scopes: Vec<HashMap<Symbol, (LlvmType, Value)>>,
}

#[derive(Clone)]
struct DropLocal {
    sym: Symbol,
    slot: Value,
    flag: Value,
    pointee: Type,
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
    moves: &'i HashSet<ExprId>,
    drop_scopes: Vec<Vec<DropLocal>>,
    enums: HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    instances: &'i HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
}
impl<'i> CodeGen<'i> {
    pub fn new(builder: IrBuilder<'i>, types: &'i HashMap<ExprId, Type>, moves: &'i HashSet<ExprId>, instances: &'i HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>) -> Self {
        CodeGen {
            builder,
            locals: VarTable::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            current_ret_ty: LlvmType::Void,
            types,
            moves,
            drop_scopes: Vec::new(),
            enums: HashMap::new(),
            instances,
        }
    }
    
    fn drop_enter(&mut self) {
        self.drop_scopes.push(Vec::new());
    }
    fn drop_exit(&mut self) {
        if let Some(scope) = self.drop_scopes.pop() {
            if !self.builder.is_terminated() {
                for d in scope.iter().rev(){
                    self.gen_drop(d.slot.clone(), d.flag.clone(), &d.pointee);
                }
            }
        }
    }
    fn free_all_live(&mut self) {
        let scopes: Vec<Vec<DropLocal>> = self.drop_scopes.clone();
        for scope in scopes.iter().rev() {
            for d in scope.iter().rev() {
                self.gen_drop(d.slot.clone(), d.flag.clone(), &d.pointee);
            }
        }
    }
    fn gen_drop(&mut self, slot: Value, flag: Value, ty: &Type) {
        let f = self.builder.build_load(&LlvmType::I1, flag.clone());
        let do_label = self.builder.fresh_block("do_drop");
        let skip_label = self.builder.fresh_block("drop_skip");
        self.builder.build_cond_br(f, &do_label, &skip_label);
        self.builder.emit_label(&do_label);
        match ty {
            Type::Box(inner) => {
                let p = self.builder.build_load(&LlvmType::Ptr, slot);
                self.emit_drop_glue(p, inner);
            }
            Type::Enum(e, args) => {
                let name = Self::inst_name(*e, args);
                self.builder.build_call_drop_enum(&name, slot);
            }
            _ => {}
        }
        self.builder.build_store(&LlvmType::I1, Value::Const(0), flag);
        self.builder.build_br(&skip_label);
        self.builder.emit_label(&skip_label);
    }
    fn emit_drop_glue(&mut self, ptr: Value, pointee: &Type) {
       match pointee {
           Type::Box(inner) => {
               let sub = self.builder.build_load(&LlvmType::Ptr, ptr.clone());
               self.emit_drop_glue(sub, inner);
           }
           Type::Enum(e, args) => {
               let name = Self::inst_name(*e, args);
               self.builder.build_call_drop_enum(&name, ptr.clone());
           }
           _ => {}
       }
        self.builder.build_free(ptr);
    }
    fn emit_enum_glue(&mut self, slot: Value, key: &(Symbol, Vec<Type>)) {
        let name = Self::inst_name(key.0, &key.1);
        let tag_ptr = self.builder.build_gep_enum_tag(&name, slot.clone());
        let tag = self.builder.build_load(&LlvmType::I32, tag_ptr);
        let end = self.builder.fresh_block("edrop_end");
        let variants = self.enums.get(key).unwrap().clone();
        for (variant, ftys) in variants.iter() {
            let owning: Vec<(usize, Type)> = ftys.iter().enumerate()
                .filter_map(|(i, t)| match t {
                    Type::Box(inner) => Some((i, (**inner).clone())),
                    _ => None,
                })
                .collect();
            if owning.is_empty() { continue; }
            let idx = self.variant_tag(key, *variant);
            let case = self.builder.fresh_block("edrop_case");
            let next = self.builder.fresh_block("edrop_next");
            let c = self.builder.build_icmp("eq", &LlvmType::I32, tag.clone(), Value::Const(idx));
            self.builder.build_cond_br(c, &case, &next);
            self.builder.emit_label(&case);
            let payload_ptr = self.builder.build_gep_enum_payload(&name, slot.clone());
            for (i, inner) in owning.iter() {
                let ftpr = self.builder.build_gep_variant_field(&name, *variant, payload_ptr.clone(), *i as u32);
                let boxptr = self.builder.build_load(&LlvmType::Ptr, ftpr);
                self.emit_drop_glue(boxptr, inner);
            }
            self.builder.build_br(&end);
            self.builder.emit_label(&next);
        }
        self.builder.build_br(&end);
        self.builder.emit_label(&end);
    }
    fn gen_drop_function(&mut self, key: &(Symbol, Vec<Type>)) {
        let name = Self::inst_name(key.0, &key.1);
        self.builder.emit_drop_function_begin(&name);
        self.builder.emit_label("entry");
        self.emit_enum_glue(Value::Named("%s".to_string()), key);
        self.builder.build_ret(&LlvmType::Void, Value::Void);
        self.builder.emit_function_end();
    }
    fn inst_name(sym: Symbol, args: &[Type]) -> String {
        if args.is_empty() {
            sym.0.to_string()
        } else {
            let m: Vec<String> = args.iter().map(|t| t.mangle()).collect();
            format!("{}.{}", sym.0, m.join("."))
        }
    }
    fn find_flag(&self, sym: Symbol) -> Option<Value> {
        for scope in self.drop_scopes.iter().rev() {
            for d in scope.iter().rev() {
                if d.sym == sym {
                    return Some(d.flag.clone());
                }
            }
        }
        None
    }
    fn clear_flag_on_move(&mut self, id: ExprId, sym: Symbol) {
        if self.moves.contains(&id) {
            if let Some(flag) = self.find_flag(sym) {
                self.builder.build_store(&LlvmType::I1, Value::Const(0), flag);
            }
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
            Type::Box(_) => LlvmType::Ptr,
            Type::Enum(e, args) => {
                if self.enum_has_payload(&(*e, args.clone())) {
                    LlvmType::Enum(Self::inst_name(*e, args))
                } else {
                    LlvmType::I32
                }
            }
            Type::Error => unreachable!("Type::Error codegen'e ulasti (errors bos degilse codegen yok)"),
            Type::Param(_) => unreachable!("Type::Param codegen'e ulasti (generic enum TC'de reddedilir)"),
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
        self.builder.globals.push_str(
            "declare ptr @malloc(i64)\n\
       declare void @free(ptr)\n\
       @.panicmsg_oom = private unnamed_addr constant [22 x i8] c\"lexora: out of memory\\00\"\n\
       define ptr @lexora_alloc(i64 %size) {\n\
       %p = call ptr @malloc(i64 %size)\n\
       %isnull = icmp eq ptr %p, null\n\
       br i1 %isnull, label %oom, label %allocok\n\
       oom:\n\
       call void @lexora_panic(ptr @.panicmsg_oom)\n\
       unreachable\n\
       allocok:\n\
       ret ptr %p\n\
       }\n\
       define void @lexora_free(ptr %p) {\n\
       call void @free(ptr %p)\n\
       ret void\n\
       }\n\n"
        );
        for e in &program.enums {
            if e.params.is_empty() {
                self.enums.insert((e.name, Vec::new()), e.variants.clone());
            }
        }
        for (key, variants) in self.instances.iter() {
            self.enums.insert(key.clone(), variants.clone());
        }
        let mut inst_keys: Vec<(Symbol, Vec<Type>)> = self.instances.keys().cloned().collect();
        inst_keys.sort_by_key(|(s, a)| Self::inst_name(*s, a));
        let mut enum_keys: Vec<(Symbol, Vec<Type>)> = program.enums.iter()
            .filter(|e| e.params.is_empty())
            .map(|e| (e.name, Vec::new()))
            .collect();
        enum_keys.extend(inst_keys);
        for key in &enum_keys {
            let variants = self.enums.get(key).unwrap().clone();
            let max_slots = variants.iter().map(|(_, tys)| tys.len()).max().unwrap_or(0);
            if max_slots == 0 {
                continue;
            }
            let name = Self::inst_name(key.0, &key.1);
            self.builder.emit_enum_type(&name, max_slots);
            for (variant, tys) in variants.iter() {
                let llvm_tys: Vec<LlvmType> = tys.iter()
                    .map(|t| self.ast_type_to_llvm(t))
                    .collect();
                self.builder.emit_variant_type(&name, *variant, &llvm_tys);
            }
        }
        for key in &enum_keys {
            if self.enum_is_owning(key) {
                self.gen_drop_function(key);
            }
        }
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
    fn enum_has_payload(&self, key: &(Symbol, Vec<Type>)) -> bool {
        self.enums.get(key)
            .map_or(false, |vs| vs.iter().any(|(_, tys)| !tys.is_empty()))
    }
    fn enum_is_owning(&self, key: &(Symbol, Vec<Type>)) -> bool {
        self.enums.get(key).map_or(false, |vs| {
            vs.iter().any(|(_, ftys)| ftys.iter().any(|t| matches!(t, Type::Box(_))))
        })
    }
    fn is_owning_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Box(_) => true,
            Type::Enum(e, args) => self.enum_is_owning(&(*e, args.clone())),
            _ => false,
        }
    }
    fn variant_tag(&self, key: &(Symbol, Vec<Type>), variant: Symbol) -> i64 {
        self.enums.get(key).unwrap()
            .iter().position(|(v, _)| *v == variant).unwrap() as i64
    }

    fn gen_function<'arena>(&mut self, func: &Function<'arena>) -> Result<(), LexoraError> {
        self.locals = VarTable::new();
        self.locals.enter();
        self.drop_enter();

        self.current_ret_ty = self.ast_type_to_llvm(&func.return_type);

        let params_ir: Vec<(Symbol, LlvmType)> = func.params.iter()
            .map(|(sym, ty)| (*sym, self.ast_type_to_llvm(ty)))
            .collect();
        self.builder.emit_function_begin(func.name, &params_ir, &self.current_ret_ty.clone());
        self.builder.emit_label("entry");

        for (sym, ast_ty) in func.params.iter() {
            let llvm_ty = self.ast_type_to_llvm(ast_ty);
            let param_name = self.builder.resolve(*sym).to_string();
            let param_val = Value::Named(format!("%{}", param_name));
            let ptr = self.builder.build_alloca(&llvm_ty, "");
            self.builder.build_store(&llvm_ty, param_val, ptr.clone());
            self.locals.insert(*sym, (llvm_ty, ptr.clone()));
            if self.is_owning_type(ast_ty) {
                let flag = self.builder.build_alloca(&LlvmType::I1, "");
                self.builder.build_store(&LlvmType::I1, Value::Const(1), flag.clone());
                self.drop_scopes.last_mut().unwrap().push(DropLocal{
                    sym: *sym,
                    slot: ptr,
                    flag,
                    pointee: ast_ty.clone(),
                });
            }
        }
        for stmt in func.body.stmts.iter() { self.gen_statement(stmt)?; }
        if let Some(tail) = func.body.tail {
            if !self.builder.is_terminated() {
                let val = self.gen_expr(tail)?;
                self.free_all_live();
                let ty = self.current_ret_ty.clone();
                self.builder.build_ret(&ty, val);
            }
        }
        self.drop_exit();
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
                self.free_all_live();
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
                self.locals.insert(*name, (llvm_ty, ptr.clone()));
                let owning_ty = match self.types.get(&value.id()) {
                    Some(t) if self.is_owning_type(t) => Some(t.clone()),
                    _ => None,
                };
                if let Some(pointee) = owning_ty {
                    let flag = self.builder.build_alloca(&LlvmType::I1, "");
                    self.builder.build_store(&LlvmType::I1, Value::Const(1), flag.clone());
                    self.drop_scopes.last_mut().unwrap().push(DropLocal {
                        sym: *name,
                        slot: ptr,
                        flag,
                        pointee,
                    });
                }
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
                self.drop_enter();
                for s in body.stmts.iter() { self.gen_statement(s)?; }
                if let Some(tail) = body.tail {
                    if !self.builder.is_terminated() {
                        self.gen_expr(tail)?;
                    }
                }                self.drop_exit();
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
                self.locals.enter();
                self.drop_enter();
                for s in body.stmts.iter() { self.gen_statement(s)?; }
                if let Some(tail) = body.tail {
                    if !self.builder.is_terminated() {
                        self.gen_expr(tail)?;
                    }
                }
                self.drop_exit();
                self.locals.exit();
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
            Stmt::AssignDeref {target, value, ..} => {
                let inner = match target {
                    Expr::Deref { target: inner, .. } => inner,
                    _ => return Err(LexoraError::Codegen {
                        message: "ASSignDeref hedefi deref degil".to_string(),
                    }),
                };
                let ptr = self.gen_expr(inner)?;
                let pointee = self.expr_llvm_type(target);
                let val = self.gen_expr(value)?;
                self.builder.build_store(&pointee, val, ptr);
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
            Expr::Identifier(sym, span, id) => {
                match self.locals.get(sym) {
                    Some((ty, ptr)) => {
                        let ty = ty.clone();
                        let ptr = ptr.clone();
                        let loaded = self.builder.build_load(&ty, ptr);
                        self.clear_flag_on_move(*id, *sym);
                        Ok(loaded)
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
            Expr::Box{value, ..} => {
                let pointee = self.expr_llvm_type(value);
                let val = self.gen_expr(value)?;
                Ok(self.builder.build_box(&pointee, val))
            }
            Expr::Deref{ target,id, ..} => {
                let ptr = self.gen_expr(target)?;
                let pointee = self.ast_type_to_llvm(&self.types[&expr.id()]);
               let val = self.builder.build_load(&pointee, ptr.clone());
                if self.moves.contains(id){
                    self.builder.build_free(ptr);
                }
                Ok(val)
            }
            Expr::EnumVariant { variant, args, .. } => {
                let (sym, targs) = match &self.types[&expr.id()] {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => return Err(LexoraError::Codegen {
                        message: "enum ifadesinin tipi enum degil".to_string(),
                    }),
                };
                let key = (sym, targs);
                if !self.enum_has_payload(&key) {
                    return Ok(Value::Const(self.variant_tag(&key, *variant)));
                }
                let name = Self::inst_name(key.0, &key.1);
                let enum_ty = LlvmType::Enum(name.clone());
                let tmp = self.builder.build_alloca(&enum_ty, "");
                let tag = self.variant_tag(&key, *variant);
                let tag_ptr = self.builder.build_gep_enum_tag(&name, tmp.clone());
                self.builder.build_store(&LlvmType::I32, Value::Const(tag), tag_ptr);
                let payload_ptr = self.builder.build_gep_enum_payload(&name, tmp.clone());
                for (i, arg) in args.iter().enumerate() {
                    let field_ty = self.expr_llvm_type(arg);
                    let field_val = self.gen_expr(arg)?;
                    let field_ptr = self.builder.build_gep_variant_field(
                        &name, *variant, payload_ptr.clone(), i as u32,
                    );
                    self.builder.build_store(&field_ty, field_val, field_ptr);
                }
                Ok(self.builder.build_load(&enum_ty, tmp))
            }
            Expr::Match { scrutinee, arms, .. } => {
                let scrut_ty = self.types[&scrutinee.id()].clone();
                let (enum_sym, enum_args) = match &scrut_ty {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => return Err(LexoraError::Codegen {
                        message: "match scrutinee enum degil".to_string(),
                    }),
                };
                let key = (enum_sym, enum_args);
                let name = Self::inst_name(key.0, &key.1);
                let has_payload = self.enum_has_payload(&key);
                let (tag, scrut_ptr) = if has_payload {
                    let enum_ty = LlvmType::Enum(name.clone());
                    let val = self.gen_expr(scrutinee)?;
                    let tmp = self.builder.build_alloca(&enum_ty, "");
                    self.builder.build_store(&enum_ty, val, tmp.clone());
                    let tag_ptr = self.builder.build_gep_enum_tag(&name, tmp.clone());
                    let tag = self.builder.build_load(&LlvmType::I32, tag_ptr);
                    (tag, Some(tmp))
                } else {
                    (self.gen_expr(scrutinee)?, None)
                };
                let match_ty = self.types[&expr.id()].clone();
                let result_slot = if match_ty != Type::Void {
                    let lt = self.ast_type_to_llvm(&match_ty);
                    let slot = self.builder.build_alloca(&lt, "");
                    Some((lt, slot))
                } else {
                    None
                };
                let end_label = self.builder.fresh_block("match_end");
                for (pat, body) in arms.iter() {
                    match pat {
                        Pattern::Variant { variant, bindings, .. } => {
                            let idx = self.variant_tag(&key, *variant);
                            let body_label = self.builder.fresh_block("arm");
                            let next_label = self.builder.fresh_block("arm_next");
                            let c = self.builder.build_icmp("eq", &LlvmType::I32, tag.clone(), Value::Const(idx));
                            self.builder.build_cond_br(c, &body_label, &next_label);

                            self.builder.emit_label(&body_label);
                            self.locals.enter();
                            self.drop_enter();
                            if let Some(ptr) = &scrut_ptr {
                                let payload_ptr = self.builder.build_gep_enum_payload(&name, ptr.clone());
                                let field_tys = self.enums.get(&key).unwrap()
                                    .iter().find(|(v, _)| v == variant).unwrap().1.clone();
                                for (i, b) in bindings.iter().enumerate() {
                                    let fty = self.ast_type_to_llvm(&field_tys[i]);
                                    let fptr = self.builder.build_gep_variant_field(
                                        &name, *variant, payload_ptr.clone(), i as u32,
                                    );
                                    let slot = self.builder.build_alloca(&fty, "");
                                    let loaded = self.builder.build_load(&fty, fptr);
                                    self.builder.build_store(&fty, loaded, slot.clone());
                                    self.locals.insert(*b, (fty, slot.clone()));
                                    if self.is_owning_type(&field_tys[i]) {
                                        let flag = self.builder.build_alloca(&LlvmType::I1, "");
                                        self.builder.build_store(&LlvmType::I1, Value::Const(1), flag.clone());
                                        self.drop_scopes.last_mut().unwrap().push(DropLocal {
                                            sym: *b,
                                            slot,
                                            flag,
                                            pointee: field_tys[i].clone(),
                                        });
                                    }
                                }
                            }
                            for s in body.stmts.iter() { self.gen_statement(s)?; }
                            if let Some(tail) = body.tail {
                                if !self.builder.is_terminated() {
                                    let val = self.gen_expr(tail)?;
                                    if let Some((lt, slot)) = &result_slot {
                                        self.builder.build_store(lt, val, slot.clone());
                                    }
                                }
                            }
                            self.drop_exit();
                            self.locals.exit();
                            self.builder.build_br(&end_label);

                            self.builder.emit_label(&next_label);
                        }
                        Pattern::Wildcard => {
                            self.locals.enter();
                            self.drop_enter();
                            if let Some(ptr) = &scrut_ptr {
                                if self.enum_is_owning(&key) {
                                    self.builder.build_call_drop_enum(&name, ptr.clone());
                                }
                            }
                            for s in body.stmts.iter() { self.gen_statement(s)?; }
                            if let Some(tail) = body.tail {
                                if !self.builder.is_terminated() {
                                    let val = self.gen_expr(tail)?;
                                    if let Some((lt, slot)) = &result_slot {
                                        self.builder.build_store(lt, val, slot.clone());
                                    }
                                }
                            }
                            self.drop_exit();
                            self.locals.exit();
                            self.builder.build_br(&end_label);
                            break;
                        }
                    }
                }
                self.builder.build_br(&end_label);
                self.builder.emit_label(&end_label);
                match result_slot {
                    Some((lt, slot)) => Ok(self.builder.build_load(&lt, slot)),
                    None => Ok(Value::Void),
                }
            }
            Expr::If { condition, then_body, else_body, .. } => {
                let if_ty = self.types[&expr.id()].clone();
                let result_slot = if if_ty != Type::Void {
                    let lt = self.ast_type_to_llvm(&if_ty);
                    let slot = self.builder.build_alloca(&lt, "");
                    Some((lt, slot))
                } else {
                    None
                };
                let cond_val = self.gen_expr(condition)?;
                let then_label = self.builder.fresh_block("then");
                let merge_label = self.builder.fresh_block("merge");
                if let Some(eb) = else_body {
                    let else_label = self.builder.fresh_block("else");
                    self.builder.build_cond_br(cond_val, &then_label, &else_label);

                    self.builder.emit_label(&then_label);
                    self.locals.enter();
                    self.drop_enter();
                    for s in then_body.stmts.iter() { self.gen_statement(s)?; }
                    if let Some(tail) = then_body.tail {
                        if !self.builder.is_terminated() {
                            let val = self.gen_expr(tail)?;
                            if let Some((lt, slot)) = &result_slot {
                                self.builder.build_store(lt, val, slot.clone());
                            }
                        }
                    }
                    self.drop_exit();
                    self.locals.exit();
                    self.builder.build_br(&merge_label);

                    self.builder.emit_label(&else_label);
                    self.locals.enter();
                    self.drop_enter();
                    for s in eb.stmts.iter() { self.gen_statement(s)?; }
                    if let Some(tail) = eb.tail {
                        if !self.builder.is_terminated() {
                            let val = self.gen_expr(tail)?;
                            if let Some((lt, slot)) = &result_slot {
                                self.builder.build_store(lt, val, slot.clone());
                            }
                        }
                    }
                    self.drop_exit();
                    self.locals.exit();
                    self.builder.build_br(&merge_label);
                } else {
                    self.builder.build_cond_br(cond_val, &then_label, &merge_label);

                    self.builder.emit_label(&then_label);
                    self.locals.enter();
                    self.drop_enter();
                    for s in then_body.stmts.iter() { self.gen_statement(s)?; }
                    if let Some(tail) = then_body.tail {
                        if !self.builder.is_terminated() {
                            self.gen_expr(tail)?;
                        }
                    }
                    self.drop_exit();
                    self.locals.exit();
                    self.builder.build_br(&merge_label);
                }
                self.builder.emit_label(&merge_label);
                match result_slot {
                    Some((lt, slot)) => Ok(self.builder.build_load(&lt, slot)),
                    None => Ok(Value::Void),
                }
            }
            Expr::Error(_,_) => Err(LexoraError::Codegen {
                message: "poison expression codegene ulasti".to_string(),
            }),
        }
    }
   
    fn expr_llvm_type<'arena>(&self, expr: &Expr<'arena>) -> LlvmType {
        self.ast_type_to_llvm(&self.types[&expr.id()])
    }
}



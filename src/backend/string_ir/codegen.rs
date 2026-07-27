use super::builder::IrBuilder;
use super::types::LlvmType;
use super::value::Value;
use crate::ast::*;
use crate::error::LexoraError;
use crate::symbol::Symbol;
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
    fn new() -> Self {
        VarTable { scopes: Vec::new() }
    }
    fn enter(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn exit(&mut self) {
        self.scopes.pop();
    }
    fn insert(&mut self, sym: Symbol, val: (LlvmType, Value)) {
        self.scopes.last_mut().unwrap().insert(sym, val);
    }
    fn get(&self, sym: &Symbol) -> Option<&(LlvmType, Value)> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(sym) {
                return Some(v);
            }
        }
        None
    }
}
pub struct CodeGen<'i> {
    builder: IrBuilder<'i>,
    locals: VarTable,
    functions: HashMap<Symbol, (Vec<LlvmType>, LlvmType)>,
    structs: HashMap<Symbol, Vec<(Symbol, LlvmType)>>,
    struct_fields_ast: HashMap<Symbol, Vec<(Symbol, Type)>>,
    current_ret_ty: LlvmType,
    cur_ret_ast: Type,
    types: &'i HashMap<ExprId, Type>,
    moves: &'i HashSet<ExprId>,
    drop_scopes: Vec<Vec<DropLocal>>,
    enums: HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    instances: &'i HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
}
impl<'i> CodeGen<'i> {
    pub fn new(
        builder: IrBuilder<'i>,
        types: &'i HashMap<ExprId, Type>,
        moves: &'i HashSet<ExprId>,
        instances: &'i HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    ) -> Self {
        CodeGen {
            builder,
            locals: VarTable::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            struct_fields_ast: HashMap::new(),
            current_ret_ty: LlvmType::Void,
            cur_ret_ast: Type::Void,
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
                for d in scope.iter().rev() {
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

    fn drop_in_place(&mut self, ptr: Value, ty: &Type) {
        match ty {
            Type::Box(inner) => {
                let p = self.builder.build_load(&LlvmType::Ptr, ptr);
                self.emit_drop_glue(p, inner);
            }
            Type::String => {
                let p = self.builder.build_load(&LlvmType::Ptr, ptr);
                self.builder.build_call_str_free(p);
            }
            Type::Enum(e, args) => {
                let name = Self::inst_name(*e, args);
                self.builder.build_call_drop_enum(&name, ptr);
            }
            Type::Struct(s) => {
                let fields = match self.struct_fields_ast.get(s) {
                    Some(f) => f.clone(),
                    None => return,
                };
                for (i, (_, fty)) in fields.iter().enumerate() {
                    if !self.is_owning_type(fty) {
                        continue;
                    }
                    let fptr = self.builder.build_gep_struct(*s, ptr.clone(), i as u32);
                    self.drop_in_place(fptr, fty);
                }
            }
            Type::Array(elem, n) => {
                if !self.is_owning_type(elem) {
                    return;
                }
                let ety = self.ast_type_to_llvm(elem);
                for i in 0..*n {
                    let eptr =
                        self.builder
                            .build_gep_array(&ety, *n, ptr.clone(), Value::Const(i as i64));
                    self.drop_in_place(eptr, elem);
                }
            }
            _ => {}
        }
    }
    fn gen_drop(&mut self, slot: Value, flag: Value, ty: &Type) {
        let f = self.builder.build_load(&LlvmType::I1, flag.clone());
        let do_label = self.builder.fresh_block("do_drop");
        let skip_label = self.builder.fresh_block("drop_skip");
        self.builder.build_cond_br(f, &do_label, &skip_label);
        self.builder.emit_label(&do_label);
        self.drop_in_place(slot, ty);
        self.builder
            .build_store(&LlvmType::I1, Value::Const(0), flag);
        self.builder.build_br(&skip_label);
        self.builder.emit_label(&skip_label);
    }
    fn emit_drop_glue(&mut self, ptr: Value, pointee: &Type) {
        self.drop_in_place(ptr.clone(), pointee);
        self.builder.build_free(ptr);
    }

    fn emit_enum_glue(&mut self, slot: Value, key: &(Symbol, Vec<Type>)) {
        let name = Self::inst_name(key.0, &key.1);
        let tag_ptr = self.builder.build_gep_enum_tag(&name, slot.clone());
        let tag = self.builder.build_load(&LlvmType::I32, tag_ptr);
        let end = self.builder.fresh_block("edrop_end");
        let variants = self.enums.get(key).unwrap().clone();
        for (variant, ftys) in variants.iter() {
            let owning: Vec<(usize, Type)> = ftys
                .iter()
                .enumerate()
                .filter(|(_, t)| self.is_owning_type(t))
                .map(|(i, t)| (i, t.clone()))
                .collect();
            if owning.is_empty() {
                continue;
            }
            let idx = self.variant_tag(key, *variant);
            let case = self.builder.fresh_block("edrop_case");
            let next = self.builder.fresh_block("edrop_next");
            let c = self
                .builder
                .build_icmp("eq", &LlvmType::I32, tag.clone(), Value::Const(idx));
            self.builder.build_cond_br(c, &case, &next);
            self.builder.emit_label(&case);
            let payload_ptr = self.builder.build_gep_enum_payload(&name, slot.clone());
            for (i, fty) in owning.iter() {
                let ftpr = self.builder.build_gep_variant_field(
                    &name,
                    *variant,
                    payload_ptr.clone(),
                    *i as u32,
                );
                self.drop_in_place(ftpr, fty);
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
        self.builder.emit_entry();
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
                self.builder
                    .build_store(&LlvmType::I1, Value::Const(0), flag);
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
            Type::String => LlvmType::Ptr,
            Type::Struct(s) => LlvmType::Struct(*s),
            Type::Box(_) => LlvmType::Ptr,
            Type::Enum(e, args) => {
                if self.enum_has_payload(&(*e, args.clone())) {
                    LlvmType::Enum(Self::inst_name(*e, args))
                } else {
                    LlvmType::I32
                }
            }
            Type::Error => {
                unreachable!("Type::Error codegen'e ulasti (errors bos degilse codegen yok)")
            }
            Type::Param(_) => {
                unreachable!("Type::Param codegen'e ulasti (generic enum TC'de reddedilir)")
            }
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
                 declare i32 @printf(ptr, ...)\n\n",
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
       }\n\n",
        );
        self.builder.globals.push_str(
            "declare i64 @strlen(ptr)\n\
       declare ptr @strcpy(ptr, ptr)\n\
       define ptr @lexora_str_new(ptr %src) {\n\
       %len = call i64 @strlen(ptr %src)\n\
       %total = add i64 %len, 9\n\
       %blk = call ptr @lexora_alloc(i64 %total)\n\
       store i64 %len, ptr %blk\n\
       %data = getelementptr i8, ptr %blk, i64 8\n\
       %copy = call ptr @strcpy(ptr %data, ptr %src)\n\
       ret ptr %data\n\
       }\n\
       define void @lexora_str_free(ptr %s) {\n\
       %blk = getelementptr i8, ptr %s, i64 -8\n\
       call void @lexora_free(ptr %blk)\n\
       ret void\n\
       }\n\n",
        );
        self.builder.globals.push_str(
            "define ptr @lexora_str_concat(ptr %a, i64 %alen, ptr %b, i64 %blen) {\n\
       %len = add i64 %alen, %blen\n\
       %total = add i64 %len, 9\n\
       %blk = call ptr @lexora_alloc(i64 %total)\n\
       store i64 %len, ptr %blk\n\
       %data = getelementptr i8, ptr %blk, i64 8\n\
       %c1 = call ptr @strcpy(ptr %data, ptr %a)\n\
       %dst = getelementptr i8, ptr %data, i64 %alen\n\
       %c2 = call ptr @strcpy(ptr %dst, ptr %b)\n\
       ret ptr %data\n\
       }\n\n",
        );
        self.builder.globals.push_str(
            "declare i32 @strcmp(ptr, ptr)\n\
       define i1 @lexora_str_eq(ptr %a, i64 %alen, ptr %b, i64 %blen) {\n\
       %nelen = icmp ne i64 %alen, %blen\n\
       br i1 %nelen, label %diff, label %same\n\
       diff:\n\
       ret i1 0\n\
       same:\n\
       %c = call i32 @strcmp(ptr %a, ptr %b)\n\
       %eq = icmp eq i32 %c, 0\n\
       ret i1 %eq\n\
       }\n\n",
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
        let mut enum_keys: Vec<(Symbol, Vec<Type>)> = program
            .enums
            .iter()
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
                let llvm_tys: Vec<LlvmType> =
                    tys.iter().map(|t| self.ast_type_to_llvm(t)).collect();
                self.builder.emit_variant_type(&name, *variant, &llvm_tys);
            }
        }
        for key in &enum_keys {
            if self.enum_is_owning(key) {
                self.gen_drop_function(key);
            }
        }
        for s in &program.structs {
            let field_llvm_tys: Vec<(Symbol, LlvmType)> = s
                .fields
                .iter()
                .map(|(sym, ty)| (*sym, self.ast_type_to_llvm(ty)))
                .collect();
            let llvm_tys: Vec<LlvmType> = field_llvm_tys.iter().map(|(_, t)| t.clone()).collect();
            self.builder.emit_struct_type(s.name, &llvm_tys);
            self.structs.insert(s.name, field_llvm_tys);
            self.struct_fields_ast.insert(s.name, s.fields.clone());
        }
        for func in &program.functions {
            let param_tys = func
                .params
                .iter()
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
        self.enums
            .get(key)
            .map_or(false, |vs| vs.iter().any(|(_, tys)| !tys.is_empty()))
    }
    fn enum_is_owning(&self, key: &(Symbol, Vec<Type>)) -> bool {
        self.enums.get(key).map_or(false, |vs| {
            vs.iter()
                .any(|(_, ftys)| ftys.iter().any(|t| self.is_owning_type(t)))
        })
    }

    fn is_owning_type(&self, ty: &Type) -> bool {
        match ty {
            Type::Box(_) => true,
            Type::String => true,
            Type::Enum(e, args) => self.enum_is_owning(&(*e, args.clone())),
            Type::Struct(s) => self
                .struct_fields_ast
                .get(s)
                .map_or(false, |fs| fs.iter().any(|(_, t)| self.is_owning_type(t))),
            Type::Array(elem, _) => self.is_owning_type(elem),
            _ => false,
        }
    }
    fn variant_tag(&self, key: &(Symbol, Vec<Type>), variant: Symbol) -> i64 {
        self.enums
            .get(key)
            .unwrap()
            .iter()
            .position(|(v, _)| *v == variant)
            .unwrap() as i64
    }
    fn result_variants(&self, key: &(Symbol, Vec<Type>)) -> (Symbol, Type, Symbol, Type) {
        let variants = self.enums.get(key).unwrap();
        let ok = variants
            .iter()
            .find(|(v, _)| self.builder.resolve(*v) == "Ok")
            .unwrap();
        let err = variants
            .iter()
            .find(|(v, _)| self.builder.resolve(*v) == "Err")
            .unwrap();
        (ok.0, ok.1[0].clone(), err.0, err.1[0].clone())
    }
    fn str_len_of<'arena>(&mut self, expr: &Expr<'arena>, val: Value) -> Value {
        match self.types[&expr.id()] {
            Type::String => {
                let blk = self.builder.build_gep_i8(val, -8);
                self.builder.build_load(&LlvmType::I64, blk)
            }
            _ => self.builder.build_call_strlen(val),
        }
    }
    fn gen_str_concat<'arena>(
        &mut self,
        left: &Expr<'arena>,
        right: &Expr<'arena>,
    ) -> Result<Value, LexoraError> {
        let lv = self.gen_expr(left)?;
        let llen = self.str_len_of(left, lv.clone());
        let rv = self.gen_expr(right)?;
        let rlen = self.str_len_of(right, rv.clone());
        let res = self
            .builder
            .build_call_str_concat(lv.clone(), llen, rv.clone(), rlen);
        if matches!(self.types[&left.id()], Type::String) {
            self.builder.build_call_str_free(lv);
        }
        if matches!(self.types[&right.id()], Type::String) {
            self.builder.build_call_str_free(rv);
        }
        Ok(res)
    }

    fn str_temp_free<'arena>(&mut self, expr: &Expr<'arena>, val: Value) {
        if matches!(self.types[&expr.id()], Type::String) && !expr.is_place() {
            self.builder.build_call_str_free(val);
        }
    }
    fn gen_str_cmp<'arena>(
        &mut self,
        left: &Expr<'arena>,
        op: &BinaryOperator,
        right: &Expr<'arena>,
    ) -> Result<Value, LexoraError> {
        let lv = self.gen_expr(left)?;
        let rv = self.gen_expr(right)?;
        let res = match op {
            BinaryOperator::Eq | BinaryOperator::NotEq => {
                let llen = self.str_len_of(left, lv.clone());
                let rlen = self.str_len_of(right, rv.clone());
                let eq = self
                    .builder
                    .build_call_str_eq(lv.clone(), llen, rv.clone(), rlen);
                if *op == BinaryOperator::Eq {
                    eq
                } else {
                    self.builder.build_not(eq)
                }
            }
            _ => {
                let c = self.builder.build_call_strcmp(lv.clone(), rv.clone());
                let pred = match op {
                    BinaryOperator::Less => "slt",
                    BinaryOperator::Greater => "sgt",
                    BinaryOperator::LessEq => "sle",
                    _ => "sge",
                };
                self.builder
                    .build_icmp(pred, &LlvmType::I32, c, Value::Const(0))
            }
        };
        self.str_temp_free(left, lv);
        self.str_temp_free(right, rv);
        Ok(res)
    }

    fn gen_function<'arena>(&mut self, func: &Function<'arena>) -> Result<(), LexoraError> {
        self.locals = VarTable::new();
        self.locals.enter();
        self.drop_enter();

        self.current_ret_ty = self.ast_type_to_llvm(&func.return_type);
        self.cur_ret_ast = func.return_type.clone();
        let params_ir: Vec<(Symbol, LlvmType)> = func
            .params
            .iter()
            .map(|(sym, ty)| (*sym, self.ast_type_to_llvm(ty)))
            .collect();
        self.builder
            .emit_function_begin(func.name, &params_ir, &self.current_ret_ty.clone());
        self.builder.emit_entry();

        for (sym, ast_ty) in func.params.iter() {
            let llvm_ty = self.ast_type_to_llvm(ast_ty);
            let param_name = self.builder.resolve(*sym).to_string();
            let param_val = Value::Named(format!("%{}", param_name));
            let ptr = self.builder.build_entry_alloca(&llvm_ty);
            self.builder.build_store(&llvm_ty, param_val, ptr.clone());
            self.locals.insert(*sym, (llvm_ty, ptr.clone()));
            if self.is_owning_type(ast_ty) {
                let flag = self.builder. build_entry_alloca(&LlvmType::I1);
                self.builder
                    .build_store(&LlvmType::I1, Value::Const(1), flag.clone());
                self.drop_scopes.last_mut().unwrap().push(DropLocal {
                    sym: *sym,
                    slot: ptr,
                    flag,
                    pointee: ast_ty.clone(),
                });
            }
        }
        for stmt in func.body.stmts.iter() {
            self.gen_statement(stmt)?;
        }
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

    fn gen_statement<'arena>(&mut self, stmt: &Stmt<'arena>) -> Result<(), LexoraError> {
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
                let ptr = self.builder.build_entry_alloca(&llvm_ty);
                self.gen_into(value, ptr.clone())?;
                self.locals.insert(*name, (llvm_ty, ptr.clone()));
                let owning_ty = match self.types.get(&value.id()) {
                    Some(t) if self.is_owning_type(t) => Some(t.clone()),
                    _ => None,
                };
                if let Some(pointee) = owning_ty {
                    let flag = self.builder.build_entry_alloca(&LlvmType::I1);
                    self.builder
                        .build_store(&LlvmType::I1, Value::Const(1), flag.clone());
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
                    None => {
                        return Err(LexoraError::UndefinedVariable {
                            name: format!("sym#{}", name.0),
                            suggestion: None,
                            span: *span,
                        });
                    }
                };
                let val = self.gen_expr(value)?;
                self.builder.build_store(&llvm_ty, val, ptr);
            }

            Stmt::While {
                condition, body, ..
            } => {
                let cond_label = self.builder.fresh_block("while_cond");
                let body_label = self.builder.fresh_block("while_body");
                let end_label = self.builder.fresh_block("while_end");

                self.builder.build_br(&cond_label);
                self.builder.emit_label(&cond_label);
                let cond_val = self.gen_expr(condition)?;
                self.builder
                    .build_cond_br(cond_val, &body_label.clone(), &end_label.clone());

                self.builder.emit_label(&body_label);
                self.locals.enter();
                self.drop_enter();
                for s in body.stmts.iter() {
                    self.gen_statement(s)?;
                }
                if let Some(tail) = body.tail {
                    if !self.builder.is_terminated() {
                        self.gen_expr(tail)?;
                    }
                }
                self.drop_exit();
                self.locals.exit();
                self.builder.build_br(&cond_label);

                self.builder.emit_label(&end_label);
            }

            Stmt::For {
                var,
                from,
                to,
                body,
                ..
            } => {
                let cond_label = self.builder.fresh_block("for_cond");
                let body_label = self.builder.fresh_block("for_body");
                let end_label = self.builder.fresh_block("for_end");

                self.locals.enter();
                let from_val = self.gen_expr(from)?;
                let ptr = self.builder.build_entry_alloca(&LlvmType::I32);
                self.builder
                    .build_store(&LlvmType::I32, from_val, ptr.clone());
                self.locals.insert(*var, (LlvmType::I32, ptr.clone()));

                let to_val = self.gen_expr(to)?;
                let to_ptr = self.builder.build_entry_alloca(&LlvmType::I32);
                self.builder
                    .build_store(&LlvmType::I32, to_val, to_ptr.clone());

                self.builder.build_br(&cond_label);
                self.builder.emit_label(&cond_label);
                let cur = self.builder.build_load(&LlvmType::I32, ptr.clone());
                let to_cur = self.builder.build_load(&LlvmType::I32, to_ptr);
                let cond = self.builder.build_icmp("slt", &LlvmType::I32, cur, to_cur);
                self.builder
                    .build_cond_br(cond, &body_label.clone(), &end_label.clone());

                self.builder.emit_label(&body_label);
                self.locals.enter();
                self.drop_enter();
                for s in body.stmts.iter() {
                    self.gen_statement(s)?;
                }
                if let Some(tail) = body.tail {
                    if !self.builder.is_terminated() {
                        self.gen_expr(tail)?;
                    }
                }
                self.drop_exit();
                self.locals.exit();
                let cur2 = self.builder.build_load(&LlvmType::I32, ptr.clone());
                let inc_val = self
                    .builder
                    .build_add(&LlvmType::I32, cur2, Value::Const(1));
                self.builder.build_store(&LlvmType::I32, inc_val, ptr);
                self.builder.build_br(&cond_label);

                self.builder.emit_label(&end_label);
                self.locals.exit();
            }

            Stmt::AssignPlace { target, value, .. } => {
                let ptr = self.gen_place(target)?;
                let ty = self.expr_llvm_type(target);
                let val = self.gen_expr(value)?;
                self.builder.build_store(&ty, val, ptr);
            }

            Stmt::Error(_) => {
                return Err(LexoraError::Codegen {
                    message: "poison statement codegen'e ulasti".to_string(),
                });
            }
        }
        Ok(())
    }
    fn gen_place<'arena>(&mut self, expr: &Expr<'arena>) -> Result<Value, LexoraError> {
        match expr {
            Expr::Identifier(sym, span, _) => match self.locals.get(sym) {
                Some((_, ptr)) => Ok(ptr.clone()),
                None => Err(LexoraError::UndefinedVariable {
                    name: format!("sym#{}", sym.0),
                    suggestion: None,
                    span: *span,
                }),
            },
            Expr::FieldAccess {
                object,
                field,
                span,
                ..
            } => {
                let base = self.gen_place(object)?;
                let struct_sym = match &self.types[&object.id()] {
                    Type::Struct(s) => *s,
                    _ => {
                        return Err(LexoraError::Custom {
                            message: "alan erisimi: struct degil".to_string(),
                            span: *span,
                        });
                    }
                };
                let field_defs = self.structs.get(&struct_sym).unwrap().clone();
                let idx = field_defs
                    .iter()
                    .position(|(sym, _)| *sym == *field)
                    .unwrap();
                Ok(self.builder.build_gep_struct(struct_sym, base, idx as u32))
            }
            Expr::Index {
                array, index, span, ..
            } => {
                let base = self.gen_place(array)?;
                let (elem_ty, size) = match &self.types[&array.id()] {
                    Type::Array(elem, n) => (self.ast_type_to_llvm(elem), *n),
                    _ => {
                        return Err(LexoraError::Custom {
                            message: "index: dizi degil".to_string(),
                            span: *span,
                        });
                    }
                };
                let idx_val = self.gen_expr(index)?;
                Ok(self
                    .builder
                    .build_checked_gep_array(&elem_ty, size, base, idx_val))
            }
            Expr::Deref { target, .. } => self.gen_expr(target),
            other => {
                let ty = self.expr_llvm_type(other);
                let tmp = self.builder.build_entry_alloca(&ty);
                self.gen_into(other, tmp.clone())?;
                Ok(tmp)
            }
        }
    }
    fn gen_into<'arena>(&mut self, expr: &Expr<'arena>, ptr: Value) -> Result<(), LexoraError> {
        match expr {
            Expr::ArrayLiteral(elems, _, _) => {
                let elem_ty = match &self.types[&expr.id()] {
                    Type::Array(elem, _) => self.ast_type_to_llvm(elem),
                    _ => {
                        return Err(LexoraError::Codegen {
                            message: "dizi literalinin tipi dizi degil".to_string(),
                        });
                    }
                };
                for (i, elem) in elems.iter().enumerate() {
                    if elem.is_aggregate_literal() {
                        let eptr = self.builder.build_gep_array(
                            &elem_ty,
                            elems.len(),
                            ptr.clone(),
                            Value::Const(i as i64),
                        );
                        self.gen_into(elem, eptr)?;
                    } else {
                        let val = self.gen_expr(elem)?;
                        let eptr = self.builder.build_gep_array(
                            &elem_ty,
                            elems.len(),
                            ptr.clone(),
                            Value::Const(i as i64),
                        );
                        self.builder.build_store(&elem_ty, val, eptr);
                    }
                }
                Ok(())
            }
            Expr::StructLiteral { name, fields, .. } => {
                let field_defs = self.structs.get(name).unwrap().clone();
                for (i, (fsym, fty)) in field_defs.iter().enumerate() {
                    let fexpr = fields
                        .iter()
                        .find(|(n, _)| *n == *fsym)
                        .map(|(_, v)| v)
                        .unwrap();
                    if fexpr.is_aggregate_literal() {
                        let fptr = self.builder.build_gep_struct(*name, ptr.clone(), i as u32);
                        self.gen_into(fexpr, fptr)?;
                    } else {
                        let val = self.gen_expr(fexpr)?;
                        let fptr = self.builder.build_gep_struct(*name, ptr.clone(), i as u32);
                        self.builder.build_store(fty, val, fptr);
                    }
                }
                Ok(())
            }
            other => {
                let ty = self.expr_llvm_type(other);
                let val = self.gen_expr(other)?;
                self.builder.build_store(&ty, val, ptr);
                Ok(())
            }
        }
    }
    fn gen_expr<'arena>(&mut self, expr: &Expr<'arena>) -> Result<Value, LexoraError> {
        match expr {
            Expr::Integer(n, _, _) => Ok(Value::Const(*n)),
            Expr::Bool(b, _, _) => Ok(Value::Const(if *b { 1 } else { 0 })),
            Expr::StringLiteral(s, _, _) => {
                let (ptr, _) = self.builder.add_string_global(s);
                Ok(ptr)
            }
            Expr::Identifier(sym, span, id) => match self.locals.get(sym) {
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
            },
            Expr::BinaryOp {
                left, op, right, ..
            } => {
                if *op == BinaryOperator::Add && self.types[&expr.id()] == Type::String {
                    return self.gen_str_concat(left, right);
                }
                if is_comparison_op(op)
                    && matches!(self.types[&left.id()], Type::Str | Type::String)
                {
                    return self.gen_str_cmp(left, op, right);
                }
                let lv = self.gen_expr(left)?;
                let rv = self.gen_expr(right)?;
                let lt = self.expr_llvm_type(left);
                let val = match op {
                    BinaryOperator::Add => self.builder.build_checked_arith("sadd", &lt, lv, rv),
                    BinaryOperator::Sub => self.builder.build_checked_arith("ssub", &lt, lv, rv),
                    BinaryOperator::Mul => self.builder.build_checked_arith("smul", &lt, lv, rv),
                    BinaryOperator::Div => self.builder.build_checked_sdiv(&lt, lv, rv),
                    BinaryOperator::Eq => self.builder.build_icmp("eq", &lt, lv, rv),
                    BinaryOperator::NotEq => self.builder.build_icmp("ne", &lt, lv, rv),
                    BinaryOperator::Less => self.builder.build_icmp("slt", &lt, lv, rv),
                    BinaryOperator::Greater => self.builder.build_icmp("sgt", &lt, lv, rv),
                    BinaryOperator::LessEq => self.builder.build_icmp("sle", &lt, lv, rv),
                    BinaryOperator::GreaterEq => self.builder.build_icmp("sge", &lt, lv, rv),
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
            Expr::Cast {
                expr, target_type, ..
            } => {
                let val = self.gen_expr(expr)?;
                let from_ty = self.expr_llvm_type(expr);
                let to_ty = self.ast_type_to_llvm(target_type);
                let result = match (&from_ty, &to_ty) {
                    (LlvmType::I32, LlvmType::I64) => {
                        self.builder.build_sext(val, &from_ty, &to_ty)
                    }
                    (LlvmType::I64, LlvmType::I32) => {
                        self.builder.build_trunc(val, &from_ty, &to_ty)
                    }
                    _ => val,
                };
                Ok(result)
            }
            Expr::Call {
                name, args, span, ..
            } => {
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
                    self.str_temp_free(arg, val);
                    return Ok(Value::Void);
                }

                if name_str == "len" {
                    let arg = &args[0];
                    let val = self.gen_expr(arg)?;
                    let len64 = self.str_len_of(arg, val.clone());
                    self.str_temp_free(arg, val);
                    return Ok(self
                        .builder
                        .build_trunc(len64, &LlvmType::I64, &LlvmType::I32));
                }
                if name_str == "string" {
                    let val = self.gen_expr(&args[0])?;
                    return Ok(self.builder.build_call_str_new(val));
                }
                let (params_tys, ret_ty) = match self.functions.get(name).cloned() {
                    Some(sig) => sig,
                    None => {
                        return Err(LexoraError::UndefinedFunction {
                            name: name_str,
                            suggestion: None,
                            span: *span,
                        });
                    }
                };
                let mut call_args: Vec<(LlvmType, Value)> = Vec::new();
                for (arg, param_ty) in args.iter().zip(params_tys.iter()) {
                    let val = self.gen_expr(arg)?;
                    call_args.push((param_ty.clone(), val));
                }
                Ok(self.builder.build_call(&ret_ty.clone(), *name, &call_args))
            }
            Expr::Index { .. } => {
                let ptr = self.gen_place(expr)?;
                let ty = self.expr_llvm_type(expr);
                Ok(self.builder.build_load(&ty, ptr))
            }
            Expr::FieldAccess { .. } => {
                let ptr = self.gen_place(expr)?;
                let ty = self.expr_llvm_type(expr);
                Ok(self.builder.build_load(&ty, ptr))
            }
            Expr::ArrayLiteral(..) | Expr::StructLiteral { .. } => {
                let ty = self.expr_llvm_type(expr);
                let tmp = self.builder.build_entry_alloca(&ty);
                self.gen_into(expr, tmp.clone())?;
                Ok(self.builder.build_load(&ty, tmp))
            }
            Expr::Box { value, .. } => {
                let pointee = self.expr_llvm_type(value);
                let val = self.gen_expr(value)?;
                Ok(self.builder.build_box(&pointee, val))
            }
            Expr::Deref { target, id, .. } => {
                let ptr = self.gen_expr(target)?;
                let pointee = self.ast_type_to_llvm(&self.types[&expr.id()]);
                let val = self.builder.build_load(&pointee, ptr.clone());
                if self.moves.contains(id) {
                    self.builder.build_free(ptr);
                }
                Ok(val)
            }
            Expr::EnumVariant { variant, args, .. } => {
                let (sym, targs) = match &self.types[&expr.id()] {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => {
                        return Err(LexoraError::Codegen {
                            message: "enum ifadesinin tipi enum degil".to_string(),
                        });
                    }
                };
                let key = (sym, targs);
                if !self.enum_has_payload(&key) {
                    return Ok(Value::Const(self.variant_tag(&key, *variant)));
                }
                let name = Self::inst_name(key.0, &key.1);
                let enum_ty = LlvmType::Enum(name.clone());
                let tmp = self.builder.build_entry_alloca(&enum_ty);
                let tag = self.variant_tag(&key, *variant);
                let tag_ptr = self.builder.build_gep_enum_tag(&name, tmp.clone());
                self.builder
                    .build_store(&LlvmType::I32, Value::Const(tag), tag_ptr);
                let payload_ptr = self.builder.build_gep_enum_payload(&name, tmp.clone());
                for (i, arg) in args.iter().enumerate() {
                    let field_ty = self.expr_llvm_type(arg);
                    let field_val = self.gen_expr(arg)?;
                    let field_ptr = self.builder.build_gep_variant_field(
                        &name,
                        *variant,
                        payload_ptr.clone(),
                        i as u32,
                    );
                    self.builder.build_store(&field_ty, field_val, field_ptr);
                }
                Ok(self.builder.build_load(&enum_ty, tmp))
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let scrut_ty = self.types[&scrutinee.id()].clone();
                let (enum_sym, enum_args) = match &scrut_ty {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => {
                        return Err(LexoraError::Codegen {
                            message: "match scrutinee enum degil".to_string(),
                        });
                    }
                };
                let key = (enum_sym, enum_args);
                let name = Self::inst_name(key.0, &key.1);
                let has_payload = self.enum_has_payload(&key);
                let (tag, scrut_ptr) = if has_payload {
                    let enum_ty = LlvmType::Enum(name.clone());
                    let val = self.gen_expr(scrutinee)?;
                    let tmp = self.builder.build_entry_alloca(&enum_ty);
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
                    let slot = self.builder.build_entry_alloca(&lt);
                    Some((lt, slot))
                } else {
                    None
                };
                let end_label = self.builder.fresh_block("match_end");
                for (pat, body) in arms.iter() {
                    match pat {
                        Pattern::Variant {
                            variant, bindings, ..
                        } => {
                            let idx = self.variant_tag(&key, *variant);
                            let body_label = self.builder.fresh_block("arm");
                            let next_label = self.builder.fresh_block("arm_next");
                            let c = self.builder.build_icmp(
                                "eq",
                                &LlvmType::I32,
                                tag.clone(),
                                Value::Const(idx),
                            );
                            self.builder.build_cond_br(c, &body_label, &next_label);

                            self.builder.emit_label(&body_label);
                            self.locals.enter();
                            self.drop_enter();
                            if let Some(ptr) = &scrut_ptr {
                                let payload_ptr =
                                    self.builder.build_gep_enum_payload(&name, ptr.clone());
                                let field_tys = self
                                    .enums
                                    .get(&key)
                                    .unwrap()
                                    .iter()
                                    .find(|(v, _)| v == variant)
                                    .unwrap()
                                    .1
                                    .clone();
                                for (i, b) in bindings.iter().enumerate() {
                                    let fty = self.ast_type_to_llvm(&field_tys[i]);
                                    let fptr = self.builder.build_gep_variant_field(
                                        &name,
                                        *variant,
                                        payload_ptr.clone(),
                                        i as u32,
                                    );
                                    let slot = self.builder.build_entry_alloca(&fty);
                                    let loaded = self.builder.build_load(&fty, fptr);
                                    self.builder.build_store(&fty, loaded, slot.clone());
                                    self.locals.insert(*b, (fty, slot.clone()));
                                    if self.is_owning_type(&field_tys[i]) {
                                        let flag = self.builder.build_entry_alloca(&LlvmType::I1);
                                        self.builder.build_store(
                                            &LlvmType::I1,
                                            Value::Const(1),
                                            flag.clone(),
                                        );
                                        self.drop_scopes.last_mut().unwrap().push(DropLocal {
                                            sym: *b,
                                            slot,
                                            flag,
                                            pointee: field_tys[i].clone(),
                                        });
                                    }
                                }
                            }
                            for s in body.stmts.iter() {
                                self.gen_statement(s)?;
                            }
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
                            for s in body.stmts.iter() {
                                self.gen_statement(s)?;
                            }
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
            Expr::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                let if_ty = self.types[&expr.id()].clone();
                let result_slot = if if_ty != Type::Void {
                    let lt = self.ast_type_to_llvm(&if_ty);
                    let slot = self.builder.build_entry_alloca(&lt);
                    Some((lt, slot))
                } else {
                    None
                };
                let cond_val = self.gen_expr(condition)?;
                let then_label = self.builder.fresh_block("then");
                let merge_label = self.builder.fresh_block("merge");
                if let Some(eb) = else_body {
                    let else_label = self.builder.fresh_block("else");
                    self.builder
                        .build_cond_br(cond_val, &then_label, &else_label);

                    self.builder.emit_label(&then_label);
                    self.locals.enter();
                    self.drop_enter();
                    for s in then_body.stmts.iter() {
                        self.gen_statement(s)?;
                    }
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
                    for s in eb.stmts.iter() {
                        self.gen_statement(s)?;
                    }
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
                    self.builder
                        .build_cond_br(cond_val, &then_label, &merge_label);

                    self.builder.emit_label(&then_label);
                    self.locals.enter();
                    self.drop_enter();
                    for s in then_body.stmts.iter() {
                        self.gen_statement(s)?;
                    }
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

            Expr::Try { expr: inner, .. } => {
                let (sym, targs) = match &self.types[&inner.id()] {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => {
                        return Err(LexoraError::Codegen {
                            message: "'?' ifadesinin tipi Result degil".to_string(),
                        });
                    }
                };
                let key = (sym, targs);
                let name = Self::inst_name(key.0, &key.1);
                let enum_ty = LlvmType::Enum(name.clone());
                let (ok_sym, ok_fty, err_sym, err_fty) = self.result_variants(&key);
                let ret_key = match &self.cur_ret_ast {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => {
                        return Err(LexoraError::Codegen {
                            message: "'?' kullanan fonksiyonun donus tipi Result degil".to_string(),
                        });
                    }
                };
                let ret_name = Self::inst_name(ret_key.0, &ret_key.1);
                let ret_enum_ty = LlvmType::Enum(ret_name.clone());
                let (_, _, ret_err_sym, _) = self.result_variants(&ret_key);

                let val = self.gen_expr(inner)?;
                let tmp = self.builder.build_entry_alloca(&enum_ty);
                self.builder.build_store(&enum_ty, val, tmp.clone());
                let tag_ptr = self.builder.build_gep_enum_tag(&name, tmp.clone());
                let tag = self.builder.build_load(&LlvmType::I32, tag_ptr);
                let err_tag = self.variant_tag(&key, err_sym);
                let is_err =
                    self.builder
                        .build_icmp("eq", &LlvmType::I32, tag, Value::Const(err_tag));
                let err_label = self.builder.fresh_block("try_err");
                let ok_label = self.builder.fresh_block("try_ok");
                self.builder.build_cond_br(is_err, &err_label, &ok_label);

                self.builder.emit_label(&err_label);
                let e_llvm = self.ast_type_to_llvm(&err_fty);
                let payload_ptr = self.builder.build_gep_enum_payload(&name, tmp.clone());
                let efld_ptr = self
                    .builder
                    .build_gep_variant_field(&name, err_sym, payload_ptr, 0);
                let e_val = self.builder.build_load(&e_llvm, efld_ptr);
                let ret_tmp = self.builder.build_entry_alloca(&ret_enum_ty);
                let ret_tag = self.variant_tag(&ret_key, ret_err_sym);
                let rtag_ptr = self.builder.build_gep_enum_tag(&ret_name, ret_tmp.clone());
                self.builder
                    .build_store(&LlvmType::I32, Value::Const(ret_tag), rtag_ptr);
                let rpayload_ptr = self
                    .builder
                    .build_gep_enum_payload(&ret_name, ret_tmp.clone());
                let rfld_ptr =
                    self.builder
                        .build_gep_variant_field(&ret_name, ret_err_sym, rpayload_ptr, 0);
                self.builder.build_store(&e_llvm, e_val, rfld_ptr);
                let ret_val = self.builder.build_load(&ret_enum_ty, ret_tmp);
                self.free_all_live();
                let rt = self.current_ret_ty.clone();
                self.builder.build_ret(&rt, ret_val);

                self.builder.emit_label(&ok_label);
                let payload_ptr = self.builder.build_gep_enum_payload(&name, tmp);
                let ofld_ptr = self
                    .builder
                    .build_gep_variant_field(&name, ok_sym, payload_ptr, 0);
                let ok_llvm = self.ast_type_to_llvm(&ok_fty);
                Ok(self.builder.build_load(&ok_llvm, ofld_ptr))
            }
            Expr::Error(_, _) => Err(LexoraError::Codegen {
                message: "poison expression codegene ulasti".to_string(),
            }),
        }
    }

    fn expr_llvm_type<'arena>(&self, expr: &Expr<'arena>) -> LlvmType {
        self.ast_type_to_llvm(&self.types[&expr.id()])
    }
}
fn is_comparison_op(op: &BinaryOperator) -> bool {
    matches!(
        op,
        BinaryOperator::Eq
            | BinaryOperator::NotEq
            | BinaryOperator::Less
            | BinaryOperator::Greater
            | BinaryOperator::LessEq
            | BinaryOperator::GreaterEq
    )
}

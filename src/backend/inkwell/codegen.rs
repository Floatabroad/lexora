use crate::ast::*;
use crate::symbol::{Symbol, Interner};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::{Builder, BuilderError};
use inkwell::values::{FunctionValue, PointerValue, BasicValueEnum, BasicMetadataValueEnum, ValueKind,IntValue};
use inkwell::types::{BasicTypeEnum, BasicType, BasicMetadataTypeEnum, StructType};
use inkwell::AddressSpace;
use std::collections::HashMap;
use inkwell::IntPredicate;
use inkwell::targets::{Target, TargetMachine, InitializationConfig, RelocMode, CodeModel, FileType};
use inkwell::OptimizationLevel;
use inkwell::passes::PassBuilderOptions;
use std::path::Path;
use inkwell::intrinsics::Intrinsic;


struct VarTable<'ctx> {
    scopes: Vec<HashMap<Symbol, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>>,
}
impl<'ctx> VarTable<'ctx> {
    fn new() -> Self { VarTable { scopes: Vec::new() } }
    fn enter(&mut self) { self.scopes.push(HashMap::new()); }
    fn exit(&mut self)  { self.scopes.pop(); }
    fn insert(&mut self, sym: Symbol, val: (PointerValue<'ctx>, BasicTypeEnum<'ctx>)) {
        self.scopes.last_mut().unwrap().insert(sym, val);
    }
    fn get(&self, sym: Symbol) -> (PointerValue<'ctx>, BasicTypeEnum<'ctx>) {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(&sym) { return *v; }
        }
        panic!("codegen: tanimsiz degisken (typechecker kacirmali)");
    }
}

pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    interner: &'ctx Interner,

    vars: VarTable<'ctx>,
    structs: HashMap<Symbol, (StructType<'ctx>, Vec<(Symbol, BasicTypeEnum<'ctx>)>)>,
    cur_fn: Option<FunctionValue<'ctx>>,
    opt: u8,
    types: &'ctx HashMap<ExprId, Type>,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(
        context: &'ctx Context,
        interner: &'ctx Interner,
        module_name: &str,
        types: &'ctx HashMap<ExprId, Type>,
        opt: u8,
    ) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        CodeGen {
            context,
            module,
            builder,
            interner,
            vars: VarTable::new(),
            structs: HashMap::new(),
            cur_fn: None,
            opt,
            types,
        }
    }
    fn llvm_type(&self, ty: &Type) -> BasicTypeEnum<'ctx> {
        match ty {
            Type::I32 => self.context.i32_type().into(),
            Type::I64 => self.context.i64_type().into(),
            Type::Bool => self.context.bool_type().into(),
            Type::Str => self.context.ptr_type(AddressSpace::default()).into(),
            Type::Array(elem, n) => {
                let elem_ty = self.llvm_type(elem);
                elem_ty.array_type(*n as u32).into()
            }
            Type::Struct(s) => self.structs.get(s)
                .expect("struct kayitli degil, register_structs once calismali")
                .0.into(),

            Type::Void => panic!("void bir deger tipi olarak kullanilamaz"),
        }
    }
    pub fn compile(&mut self, program: &Program) -> Result<(), BuilderError> {
        self.register_structs(program);
        for func in &program.functions {
            self.declare_functions(func);
        }
        for func in &program.functions{
            self.gen_function(func)?;
        }
        Ok(())
    }
    fn register_structs(&mut self, program: &Program) {
        for s in &program.structs {
            let st = self.context.opaque_struct_type(&format!("struct.{}", s.name.0));
            self.structs.insert(s.name, (st, Vec::new()));
        }
        for s in &program.structs {
            let mut fields = Vec::new();
            let mut fields_tys = Vec::new();
            for (fsym, fty) in &s.fields {
                let lty = self.llvm_type(fty);
                fields.push((*fsym, lty));
                fields_tys.push(lty);
            }
            let st = self.structs.get(&s.name).unwrap().0;
            st.set_body(&fields_tys, false);
            self.structs.get_mut(&s.name).unwrap().1 = fields;
        }
    }
    fn declare_functions(&mut self, func: &Function) -> FunctionValue<'ctx> {
        let param_types: Vec<BasicMetadataTypeEnum>  = func.params.iter()
            .map(|(_, ty)| self.llvm_type(ty).into())
            .collect();

        let fn_type = match &func.return_type {
            Type::Void => self.context.void_type().fn_type(&param_types, false),
            ret => self.llvm_type(ret).fn_type(&param_types, false),
        };

        let name = self.interner.resolve(func.name);
        self.module.get_function(name)
            .unwrap_or_else(|| self.module.add_function(name, fn_type, None))
    }

    fn gen_function(&mut self, func: &Function) -> Result<(), BuilderError> {
        let name = self.interner.resolve(func.name);
        let function = self.module.get_function(name).unwrap();
        self.cur_fn = Some(function);

        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);

        self.vars = VarTable::new();
        self.vars.enter();

        for(i, (sym, ty)) in func.params.iter().enumerate() {
            let llvm_ty = self.llvm_type(ty);
            let pname = self.interner.resolve(*sym);
            let slot = self.entry_alloca(llvm_ty, pname)?;
            let arg = function.get_nth_param(i as u32).unwrap();
            self.builder.build_store(slot, arg)?;
            self.vars.insert(*sym, (slot, llvm_ty));
        }
        for stmt in func.body {
            self.gen_statement(stmt)?;
        }

        let cur = self.builder.get_insert_block().unwrap();
        if cur.get_terminator().is_none() {
            match func.return_type {
                Type::Void => { self.builder.build_return(None)?; }
                _ => { self.builder.build_unreachable()?;}
            }
        }
        self.vars.exit();
        Ok(())
    }
    fn gen_statement(&mut self, stmt: &Stmt) -> Result<(), BuilderError> {
        match stmt {
            Stmt::Let { name, ty, value, .. } => {
                let llvm_ty = self.llvm_type(ty);
                let pname = self.interner.resolve(*name);
                let slot = self.entry_alloca(llvm_ty, pname)?;

                match value {
                    Expr::ArrayLiteral(elems, _, _) => {
                        let array_ty = llvm_ty.into_array_type();
                        let zero = self.context.i32_type().const_zero();
                        for (i, elem) in elems.iter().enumerate() {
                            let elem_val = self.gen_expr(elem)?;
                            let idx = self.context.i32_type().const_int(i as u64,
                                                                        false);
                            let elem_ptr = unsafe {
                                self.builder.build_in_bounds_gep(array_ty, slot, &[zero,
                                    idx], "init_ptr")?
                            };
                            self.builder.build_store(elem_ptr, elem_val)?;
                        }
                    }
                    Expr::StructLiteral { name: struct_name, fields, .. } => {
                        let st = llvm_ty.into_struct_type();
                        let defs = self.structs.get(struct_name).unwrap().1.clone();
                        for (idx, (fsym, _)) in defs.iter().enumerate() {
                            let fexpr = fields.iter()
                                .find(|(n, _)| n == fsym)
                                .map(|(_, v)| v)
                                .unwrap();
                            let fval = self.gen_expr(fexpr)?;
                            let fptr = self.builder.build_struct_gep(st, slot,
                                                                     idx as u32, "fld")?;
                            self.builder.build_store(fptr, fval)?;
                        }
                    }
                    _ => {
                        let val = self.gen_expr(value)?;
                        self.builder.build_store(slot, val)?;
                    }
                }
                self.vars.insert(*name, (slot, llvm_ty));
                Ok(())
            }
            Stmt::Assign {name, value , .. } => {
                let (slot, _ ) = self.vars.get(*name);
                let val = self.gen_expr(value)?;
                self.builder.build_store(slot, val)?;
                Ok(())
            }
            Stmt::Return(expr, _) => {
                let val = self.gen_expr(expr)?;
                self.builder.build_return(Some(&val))?;
                Ok(())
            }
            Stmt::Expr(expr, _) => {
                self.gen_expr(expr)?;
                Ok(())
            }
            Stmt::If { condition, then_body, else_branch, .. } => {
                let cond_val = self.gen_expr(condition)?.into_int_value();
                let function = self.cur_fn.unwrap();

                let then_bb = self.context.append_basic_block(function, "then");

                if let Some(else_stmts)  = *else_branch{
                    let else_bb = self.context.append_basic_block(function, "else_br");
                    let merge_bb  = self.context.append_basic_block(function, "merge");
                    self.builder.build_conditional_branch(cond_val, then_bb, else_bb)?;

                    self.builder.position_at_end(then_bb);
                    self.vars.enter();
                    for stmt in *then_body {self.gen_statement(stmt)?; }
                    self.vars.exit();

                    if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }
                    self.builder.position_at_end(else_bb);
                    self.vars.enter();
                    for stmt in else_stmts {self.gen_statement(stmt)?; }
                    self.vars.exit();

                    if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }
                    self.builder.position_at_end(merge_bb);
                }else {
                    let merge_bb = self.context.append_basic_block(function, "merge");
                    self.builder.build_conditional_branch(cond_val, then_bb, merge_bb)?;

                    self.builder.position_at_end(then_bb);
                    self.vars.enter();
                    for stmt in *then_body{self.gen_statement(stmt)?;}
                    self.vars.exit();
                    if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }
                    self.builder.position_at_end(merge_bb);
                }
                Ok(())
            }
            Stmt::While { condition, body, .. } => {
                let function = self.cur_fn.unwrap();

                let cond_bb = self.context.append_basic_block(function, "while_cond");
                let body_bb = self.context.append_basic_block(function, "while_body");
                let after_bb = self.context.append_basic_block(function, "while_after");

                self.builder.build_unconditional_branch(cond_bb)?;

                self.builder.position_at_end(cond_bb);
                let cond_val = self.gen_expr(condition)?.into_int_value();
                self.builder.build_conditional_branch(cond_val, body_bb, after_bb)?;

                self.builder.position_at_end(body_bb);
                self.vars.enter();
                for stmt in *body {self.gen_statement(stmt)?;}
                self.vars.exit();
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(cond_bb)?;
                }
                self.builder.position_at_end(after_bb);
                Ok(())
            }
            Stmt::For {var, from, to, body, .. } => {
                let function = self.cur_fn.unwrap();
                let i32t: BasicTypeEnum<'ctx> = self.context.i32_type().into();
                let var_name = self.interner.resolve(*var);


                self.vars.enter();
                let slot = self.entry_alloca(i32t, var_name)?;
                let from_val = self.gen_expr(from)?;
                self.builder.build_store(slot, from_val)?;
                self.vars.insert(*var, (slot, i32t));

                let cond_bb = self.context.append_basic_block(function, "for_cond");
                let body_bb = self.context.append_basic_block(function, "for_body");
                let after_bb = self.context.append_basic_block(function, "for_after");

                self.builder.build_unconditional_branch(cond_bb)?;

                self.builder.position_at_end(cond_bb);
                let cur_val = self.builder.build_load(i32t, slot, var_name)?.into_int_value();
                let to_val = self.gen_expr(to)?.into_int_value();
                let cmp = self.builder.build_int_compare(IntPredicate::SLT, cur_val, to_val, "for_cmp")?;
                self.builder.build_conditional_branch(cmp, body_bb, after_bb)?;

                self.builder.position_at_end(body_bb);
                for stmt in *body {self.gen_statement(stmt)?;}

                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    let cur = self.builder.build_load(i32t, slot, var_name)?.into_int_value();
                    let one = self.context.i32_type().const_int(1, false);
                    let next = self.builder.build_int_add(cur, one, "inc")?;
                    self.builder.build_store(slot, next)?;
                    self.builder.build_unconditional_branch(cond_bb)?;
                }
                self.vars.exit();
                self.builder.position_at_end(after_bb);
                Ok(())
            }
            Stmt::AssignIndex { name, index, value, .. } => {
                let (arr_ptr, arr_ty) = self.vars.get(*name);
                let array_ty = arr_ty.into_array_type();

                let idx_val = self.gen_expr(index)?.into_int_value();
                let val = self.gen_expr(value)?;
                self.bounds_check(idx_val, array_ty.len())?;
                let zero = self.context.i32_type().const_zero();
                let elem_ptr = unsafe {
                    self.builder.build_in_bounds_gep(array_ty, arr_ptr, &[zero,
                        idx_val], "elem_ptr")?
                };
                self.builder.build_store(elem_ptr, val)?;
                Ok(())
            }
            Stmt::AssignField { object, field, value, .. } => {
                let (obj_ptr, obj_ty) = self.vars.get(*object);
                let st = obj_ty.into_struct_type();
                let idx = self.field_index(st, *field);
                let val = self.gen_expr(value)?;
                let fptr = self.builder.build_struct_gep(st, obj_ptr, idx, "fld")?;
                self.builder.build_store(fptr, val)?;
                Ok(())
            }
        }
    }
    fn gen_expr(&mut self, expr: &Expr) -> Result<BasicValueEnum<'ctx>, BuilderError> {
        match expr {
            Expr::Integer(n, _, _ ) => {
                let val = if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    self.context.i64_type().const_int(*n as u64, true)
                }else {
                    self.context.i32_type().const_int(*n as u64, true)
                };
                Ok(val.into())
            }
            Expr::Bool(b, _,_) => {
                Ok(self.context.bool_type().const_int(*b as u64, false).into())
            }
            Expr::Identifier(sym, _,_) => {
                let (ptr, pointee_ty) = self.vars.get(*sym);
                let name =self.interner.resolve(*sym);
                Ok(self.builder.build_load(pointee_ty, ptr, name)?)
            }
            Expr::BinaryOp {left, op, right, ..} => {
                let l = self.gen_expr(left)?.into_int_value();
                let r = self.gen_expr(right)?.into_int_value();
                let res = match op {
                    BinaryOperator::Add => self.checked_arith("llvm.sadd.with.overflow", l, r)?,
                    BinaryOperator::Sub => self.checked_arith("llvm.ssub.with.overflow", l, r)?,
                    BinaryOperator::Mul => self.checked_arith("llvm.smul.with.overflow", l, r)?,
                    BinaryOperator::Div => {
                        let function = self.cur_fn.unwrap();
                        let is_zero = self.builder.build_int_compare(
                            IntPredicate::EQ, r, r.get_type().const_zero(), "divz")?;
                        let panic_bb = self.context.append_basic_block(function, "div_panic");
                        let ok_bb = self.context.append_basic_block(function, "div_ok");
                        self.builder.build_conditional_branch(is_zero, panic_bb, ok_bb)?;

                        self.builder.position_at_end(panic_bb);
                        let panic_fn = self.get_or_build_panic()?;
                        let msg = self.fmt_global(".panicmsg_div", "lexora: division by zero")?;
                        self.builder.build_call(panic_fn, &[msg.into()], "")?;
                        self.builder.build_unreachable()?;

                        self.builder.position_at_end(ok_bb);
                        self.builder.build_int_signed_div(l, r, "div")?
                    }
                    BinaryOperator::Eq        =>
                        self.builder.build_int_compare(IntPredicate::EQ,  l, r, "cmp")?,
                    BinaryOperator::NotEq     =>
                        self.builder.build_int_compare(IntPredicate::NE,  l, r, "cmp")?,
                    BinaryOperator::Less      =>
                        self.builder.build_int_compare(IntPredicate::SLT, l, r, "cmp")?,
                    BinaryOperator::Greater   =>
                        self.builder.build_int_compare(IntPredicate::SGT, l, r, "cmp")?,
                    BinaryOperator::LessEq    =>
                        self.builder.build_int_compare(IntPredicate::SLE, l, r, "cmp")?,
                    BinaryOperator::GreaterEq =>
                        self.builder.build_int_compare(IntPredicate::SGE, l, r, "cmp")?,
                    BinaryOperator::And => self.builder.build_and(l, r, "and")?,
                    BinaryOperator::Or  => self.builder.build_or(l, r, "or")?,
                };
                Ok(res.into())
            }
            Expr::Call {name, args, ..} => {
                let fname = self.interner.resolve(*name);

                if fname == "print" {
                    let arg = self.gen_expr(&args[0])?;
                    let printf = self.get_printf();

                    let (fmt_ptr, print_arg): (PointerValue, BasicMetadataValueEnum) =
                        match &self.types[&args[0].id()] {
                            Type::Str => (self.fmt_global(".fmt_s", "%s\n")?, arg.into()),
                            Type::I64 => (self.fmt_global(".fmt_lld", "%lld\n")?,
                                          arg.into()),
                            Type::Bool => {
                                let z = self.builder.build_int_z_extend(
                                    arg.into_int_value(), self.context.i32_type(),
                                    "boolext")?;
                                (self.fmt_global(".fmt_d", "%d\n")?, z.into())
                            }
                            _ => (self.fmt_global(".fmt_d", "%d\n")?, arg.into()),
                        };

                    self.builder.build_call(printf, &[fmt_ptr.into(), print_arg],
                                            "printf_call")?;
                    Ok(self.context.i32_type().const_int(0, false).into())
                } else {
                    let function = self.module.get_function(fname).unwrap();
                    let mut argv: Vec<BasicMetadataValueEnum> = Vec::new();
                    for a in args.iter() {
                        argv.push(self.gen_expr(a)?.into());
                    }
                    let call = self.builder.build_call(function, &argv, "call")?;
                    match call.try_as_basic_value() {
                        ValueKind::Basic(v)       => Ok(v),
                        ValueKind::Instruction(_) => Ok(self.context.i32_type().const_int(0, false).into()),
                    }
                }

            }
            Expr::StringLiteral(s, _,_) => {
                let g = self.builder.build_global_string_ptr(s, ".str")?;
                Ok(g.as_pointer_value().into())
            }
            Expr::Cast{ expr, target_type, ..} => {
                let val = self.gen_expr(expr)?.into_int_value();
                let  from_bits = val.get_type().get_bit_width();

                let target_ty = self.llvm_type(target_type).into_int_type();
                let to_bits = target_ty.get_bit_width();

                let res = if to_bits > from_bits {
                    self.builder.build_int_s_extend(val, target_ty, "sext")?
                } else if to_bits < from_bits {
                    self.builder.build_int_truncate(val, target_ty, "trunc")?
                }else {
                    val
                };
                Ok(res.into())
            }
            Expr::UnaryOp {op, operand, ..} => {
                let val = self.gen_expr(operand)?.into_int_value();
                let res = match op {
                    UnaryOperator::Not => self.builder.build_not(val, "not")?,
                    UnaryOperator::Neg => self.builder.build_int_neg(val, "neg")?,
                };
                Ok(res.into())
            }
            Expr::Index {array, index, ..} => {
                let arr_sym = match array {
                    Expr::Identifier(s, _,_) =>  *s,
                    _ => unreachable!("index hedefi degisken olmali"),
                };
                let (arr_ptr, arr_ty) = self.vars.get(arr_sym);
                let array_ty = arr_ty.into_array_type();
                let elem_ty = array_ty.get_element_type();

                let idx_val = self.gen_expr(index)?.into_int_value();
                self.bounds_check(idx_val, array_ty.len())?;
                let zero = self.context.i32_type().const_zero();
                let elem_ptr = unsafe {
                    self.builder.build_in_bounds_gep(array_ty, arr_ptr, &[zero, idx_val], "elem_ptr")?
                };
                Ok(self.builder.build_load(elem_ty, elem_ptr,"elem")?)
            }
            Expr::FieldAccess { object, field, .. } => {
                let obj_sym = match object {
                    Expr::Identifier(s, _,_) => *s,
                    _ => unreachable!("alan erisimi degisken olmali"),
                };
                let (obj_ptr, obj_ty) = self.vars.get(obj_sym);
                let st = obj_ty.into_struct_type();
                let idx = self.field_index(st, *field);
                let fptr = self.builder.build_struct_gep(st, obj_ptr, idx, "fld")?;
                let fld_ty = st.get_field_type_at_index(idx).unwrap();
                Ok(self.builder.build_load(fld_ty, fptr, "fldval")?)
            }

            _ => todo!("call/cast/unary/index/struct/array/string sonraki adımlarda"),        }
    }
    fn entry_alloca(&self, ty: BasicTypeEnum<'ctx>, name: &str)
        -> Result<PointerValue<'ctx>, BuilderError> {
        let func = self.cur_fn.unwrap();
        let entry = func.get_first_basic_block().unwrap();
        let tmp = self.context.create_builder();
        match entry.get_first_instruction(){
            Some(first) => tmp.position_before(&first),
            None => tmp.position_at_end(entry),
        }
        tmp.build_alloca(ty, name)
    }
    fn get_printf(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("printf") {
            return f;
        }
        let i32t = self.context.i32_type();
        let ptrt = self.context.ptr_type(AddressSpace::default());
        let fn_type = i32t.fn_type(&[ptrt.into()], true);
        self.module.add_function("printf", fn_type, None)
    }
    fn get_or_build_exit(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("exit") {
            return f;
        }
        let void_t = self.context.void_type();
        let i32t = self.context.i32_type();
        let fn_type = void_t.fn_type(&[i32t.into()], false);
        self.module.add_function("exit", fn_type, None)
    }
    fn get_or_build_panic(&self) -> Result<FunctionValue<'ctx>, BuilderError> {
        if let Some(f) = self.module.get_function("lexora_panic") {
            return Ok(f);
        }
        let void_t = self.context.void_type();
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let fn_type = void_t.fn_type(&[ptr_t.into()], false);
        let func = self.module.add_function("lexora_panic", fn_type, None);

        let entry = self.context.append_basic_block(func, "entry");
        let tmp = self.context.create_builder();
        tmp.position_at_end(entry);

        let msg = func.get_nth_param(0).unwrap().into_pointer_value();
        let fmt = tmp.build_global_string_ptr("%s\n", ".panic_fmt")?.as_pointer_value();
        let printf = self.get_printf();
        tmp.build_call(printf, &[fmt.into(), msg.into()], "")?;

        let one = self.context.i32_type().const_int(1, false);
        tmp.build_call(self.get_or_build_exit(), &[one.into()], "")?;
        tmp.build_unreachable()?;
        Ok(func)
    }
    fn bounds_check(&self, idx: IntValue<'ctx>, len: u32) -> Result<(), BuilderError> {
        let function = self.cur_fn.unwrap();
        let size = self.context.i32_type().const_int(len as u64, false);
        let oob = self.builder.build_int_compare(IntPredicate::UGE, idx, size, "oob")?;
        let panic_bb = self.context.append_basic_block(function, "idx_panic");
        let ok_bb = self.context.append_basic_block(function, "idx_ok");
        self.builder.build_conditional_branch(oob, panic_bb, ok_bb)?;
        self.builder.position_at_end(panic_bb);
        let panic_fn = self.get_or_build_panic()?;
        let msg = self.fmt_global(".panicmsg_idx", "lexora: index out of bounds")?;
        self.builder.build_call(panic_fn, &[msg.into()], "")?;
        self.builder.build_unreachable()?;
        self.builder.position_at_end(ok_bb);
        Ok(())
    }

    fn checked_arith(&self, name: &str, l: IntValue<'ctx>, r: IntValue<'ctx>) -> Result<IntValue<'ctx>, BuilderError> {
        let function = self.cur_fn.unwrap();
        let int_ty = l.get_type();
        let intrinsic = Intrinsic::find(name).expect("intrinsic bulunamadi");
        let decl = intrinsic.get_declaration(&self.module, &[int_ty.into()]).expect("intrinsic decl alinamadi");
        let call = self.builder.build_call(decl, &[l.into(), r.into()], "ovf_call")?;
        let agg = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_struct_value(),
            ValueKind::Instruction(_) => unreachable!("overflow intrinsic struct dondurur"),
        };
        let res = self.builder.build_extract_value(agg, 0, "res")?.into_int_value();
        let ovc = self.builder.build_extract_value(agg, 1, "ovc")?.into_int_value();
        let panic_bb = self.context.append_basic_block(function, "ovf_panic");
        let ok_bb = self.context.append_basic_block(function, "ovf_ok");
        self.builder.build_conditional_branch(ovc, panic_bb, ok_bb)?;
        self.builder.position_at_end(panic_bb);
        let panic_fn = self.get_or_build_panic()?;
        let msg = self.fmt_global(".panicmsg_ovf", "lexora: integer overflow")?;
        self.builder.build_call(panic_fn, &[msg.into()], "")?;
        self.builder.build_unreachable()?;
        self.builder.position_at_end(ok_bb);
        Ok(res)
    }
    fn field_index(&self, st: StructType<'ctx>, field: Symbol) -> u32 {
        for (_, (sty, defs)) in self.structs.iter() {
            if *sty == st {
                if let Some(i) = defs.iter().position(|(n, _)| *n == field) {
                    return i as u32;
                }

            }
        }
        panic!("codegen: struct alani bulunamadi");
    }
    fn fmt_global(&self, name: &str, text: &str) -> Result<PointerValue<'ctx>, BuilderError> {
        if let Some(g) = self.module.get_global(name) {
            return Ok(g.as_pointer_value());
        }
        let g = self.builder.build_global_string_ptr(text, name)?;
        Ok(g.as_pointer_value())
    }


    pub fn verify(&self) -> Result<(), String> {
        self.module.verify().map_err(|e| e.to_string())
    }
    fn target_machine(&self) -> Result<TargetMachine, String> {
        Target::initialize_native(&InitializationConfig::default())?;
        let triple = TargetMachine::get_default_triple();
        let target = Target::from_triple(&triple).map_err(|e| e.to_string())?;
        let level = match self.opt {
            0 => OptimizationLevel::None,
            1 => OptimizationLevel::Less,
            2 => OptimizationLevel::Default,
            _ => OptimizationLevel::Aggressive,
        };
        target
            .create_target_machine(
                &triple,
                &TargetMachine::get_host_cpu_name().to_string(),
                &TargetMachine::get_host_cpu_features().to_string(),
                level,
                RelocMode::PIC,
                CodeModel::Default,
            )
            .ok_or_else(|| "TargetMachine olusturulamadi".to_string())
    }
    pub fn optimize(&self) -> Result<(), String> {
        if self.opt == 0 {
            return Ok(());
        }
        let tm = self.target_machine()?;
        self.module.set_triple(&tm.get_triple());
        self.module.set_data_layout(&tm.get_target_data().get_data_layout());
        let opts = PassBuilderOptions::create();
        self.module
            .run_passes(&format!("default<O{}>", self.opt), &tm, opts)
            .map_err(|e| e.to_string())
    }
    pub fn write_ir(&self, path: &Path) -> Result<(), String> {
        self.module.print_to_file(path).map_err(|e| e.to_string())
    }
    pub fn emit_object(&self, path: &Path) -> Result<(), String> {
        let tm = self.target_machine()?;
        tm.write_to_file(&self.module, FileType::Object, path)
            .map_err(|e| e.to_string())
    }
}


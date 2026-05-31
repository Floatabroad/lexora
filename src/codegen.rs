use crate::ast::*;
use crate::symbol::{Symbol, Interner};
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::builder::{Builder, BuilderError};
use inkwell::values::{FunctionValue, PointerValue, BasicValueEnum, BasicMetadataValueEnum, ValueKind};
use inkwell::types::{BasicTypeEnum, BasicType, BasicMetadataTypeEnum};
use inkwell::AddressSpace;
use std::collections::HashMap;
use inkwell::IntPredicate;

pub struct CodeGen<'ctx> {
    context: &'ctx Context,
    module: Module<'ctx>,
    builder: Builder<'ctx>,
    interner: &'ctx Interner,

    vars: HashMap<Symbol, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    cur_fn: Option<FunctionValue<'ctx>>,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(
        context: &'ctx Context,
        interner: &'ctx Interner,
        module_name: &str,
    ) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        CodeGen {
            context,
            module,
            builder,
            interner,
            vars: HashMap::new(),
            cur_fn: None,
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
            Type::Struct(_) => todo!("Struct tipini sonraki adimda ekleyecegiz"),
            Type::Void => panic!("void bir deger tipi olarak kullanilamaz"),
        }
    }
    pub fn compile(&mut self, program: &Program) -> Result<(), BuilderError> {
        for func in &program.functions {
            self.declare_functions(func);
        }
        for func in &program.functions{
            self.gen_function(func)?;
        }
        Ok(())
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

        self.vars.clear();

        for(i, (sym, ty)) in func.params.iter().enumerate() {
            let llvm_ty = self.llvm_type(ty);
            let pname = self.interner.resolve(*sym);
            let slot = self.builder.build_alloca(llvm_ty, pname)?;
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
        Ok(())
    }
    fn gen_statement(&mut self, stmt: &Stmt) -> Result<(), BuilderError> {
        match stmt {
            Stmt::Let {name, ty, value, .. } => {
                // simdilik sadece skalar tipler array struct sonra
                let llvm_ty = self.llvm_type(ty);
                let pname = self.interner.resolve(*name);
                let slot = self.builder.build_alloca(llvm_ty, pname)?;
                let val = self.gen_expr(value)?;
                self.builder.build_store(slot, val)?;
                self.vars.insert(*name, (slot, llvm_ty));
                Ok(())
            }
            Stmt::Assign {name, value , .. } => {
                let (slot, _ ) = self.vars[name];
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
                    for stmt in *then_body {
                        self.gen_statement(stmt)?;
                    }
                    if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }
                    self.builder.position_at_end(else_bb);
                    for stmt in else_stmts {self.gen_statement(stmt)?; }
                    if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }
                    self.builder.position_at_end(merge_bb);
                }else {
                    let merge_bb = self.context.append_basic_block(function, "merge");
                    self.builder.build_conditional_branch(cond_val, then_bb, merge_bb)?;

                    self.builder.position_at_end(then_bb);
                    for stmt in *then_body{self.gen_statement(stmt)?;}
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
                for stmt in *body {self.gen_statement(stmt)?;}
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

                let slot = self.builder.build_alloca(i32t, var_name)?;
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
                self.builder.position_at_end(after_bb);
                Ok(())
            }
            _ => todo!("/assignindex/assignfield")
        }
    }
    fn gen_expr(&mut self, expr: &Expr) -> Result<BasicValueEnum<'ctx>, BuilderError> {
        match expr {
            Expr::Integer(n, _ ) => {
                let val = if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    self.context.i64_type().const_int(*n as u64, true)
                }else {
                    self.context.i32_type().const_int(*n as u64, true)
                };
                Ok(val.into())
            }
            Expr::Bool(b, _) => {
                Ok(self.context.bool_type().const_int(*b as u64, false).into())
            }
            Expr::Identifier(sym, _) => {
                let (ptr, pointee_ty) = self.vars[sym];
                let name =self.interner.resolve(*sym);
                Ok(self.builder.build_load(pointee_ty, ptr, name)?)
            }
            Expr::BinaryOp {left, op, right, ..} => {
                let l = self.gen_expr(left)?.into_int_value();
                let r = self.gen_expr(right)?.into_int_value();
                let res = match op {
                    BinaryOperator::Add => self.builder.build_int_add(l, r, "add")?,
                    BinaryOperator::Sub => self.builder.build_int_sub(l, r, "sub")?,
                    BinaryOperator::Mul => self.builder.build_int_mul(l, r, "mul")?,
                    BinaryOperator::Div => self.builder.build_int_signed_div(l, r, "div")?,
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
                    if arg.is_pointer_value() {
                        (self.fmt_global(".fmt_s", "%s\n")?, arg.into())
                    }else {
                        let iv = arg.into_int_value();
                        match iv.get_type().get_bit_width(){
                            64 => (self.fmt_global(".fmt_lld", "%lld\n")?, iv.into()),
                            1 => {
                                let z = self.builder.build_int_z_extend(
                                    iv, self.context.i32_type(), "boolext")?;
                                (self.fmt_global(".fmt_d", "%d\n")?, z.into())
                            }
                            _ => (self.fmt_global(".fmt_d", "%d\n")?, iv.into()),
                        }
                    };

                    self.builder.build_call(printf, &[fmt_ptr.into(), print_arg], "printf_call")?;
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
            Expr::StringLiteral(s, _) => {
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
            _ => todo!("call/cast/unary/index/struct/array/string sonraki adımlarda"),
        }
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
    fn fmt_global(&self, name: &str, text: &str) -> Result<PointerValue<'ctx>, BuilderError> {
        if let Some(g) = self.module.get_global(name) {
            return Ok(g.as_pointer_value());
        }
        let g = self.builder.build_global_string_ptr(text, name)?;
        Ok(g.as_pointer_value())
    }
    pub fn print_ir(&self) {
        println!("{}", self.module.print_to_string().to_string());
    }
}


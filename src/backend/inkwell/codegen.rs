use crate::ast::*;
use crate::source_map::SourceMap;
use crate::span::Span;
use crate::symbol::{Interner, Symbol};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::OptimizationLevel;
use inkwell::basic_block::BasicBlock;
use inkwell::builder::{Builder, BuilderError};
use inkwell::context::Context;
use inkwell::debug_info::{
    AsDIScope, DICompileUnit, DIFile, DIFlags, DIFlagsConstants, DILocalVariable, DILocation,
    DISubprogram, DIType, DWARFEmissionKind, DWARFSourceLanguage, DebugInfoBuilder,
};
use inkwell::intrinsics::Intrinsic;
use inkwell::llvm_sys::debuginfo::LLVMDIBuilderInsertDeclareRecordAtEnd;
use inkwell::module::FlagBehavior;
use inkwell::module::Module;
use inkwell::passes::PassBuilderOptions;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetData, TargetMachine,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType};
use inkwell::values::{
    AsValueRef, BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue, PointerValue,
    ValueKind,
};
use std::collections::{HashMap, HashSet};
use std::path::Path;
#[derive(Clone)]
struct DropLocal<'ctx> {
    sym: Symbol,
    slot: PointerValue<'ctx>,
    flag: PointerValue<'ctx>,
    pointee: Type,
}

struct VarTable<'ctx> {
    scopes: Vec<HashMap<Symbol, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>>,
}
impl<'ctx> VarTable<'ctx> {
    fn new() -> Self {
        VarTable { scopes: Vec::new() }
    }
    fn enter(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn exit(&mut self) {
        self.scopes.pop();
    }
    fn insert(&mut self, sym: Symbol, val: (PointerValue<'ctx>, BasicTypeEnum<'ctx>)) {
        self.scopes.last_mut().unwrap().insert(sym, val);
    }
    fn get(&self, sym: Symbol) -> (PointerValue<'ctx>, BasicTypeEnum<'ctx>) {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(&sym) {
                return *v;
            }
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
    cur_ret: Option<Type>,
    opt: u8,
    types: &'ctx HashMap<ExprId, Type>,
    sources: &'ctx SourceMap<'ctx>,
    debug: bool,
    dib: Option<DebugInfoBuilder<'ctx>>,
    di_cu: Option<DICompileUnit<'ctx>>,
    di_file: Option<DIFile<'ctx>>,
    di_sp: Option<DISubprogram<'ctx>>,
    target_data: Option<TargetData>,
    struct_fields_ast: HashMap<Symbol, Vec<(Symbol, Type)>>,
    moves: &'ctx HashSet<ExprId>,
    drop_scopes: Vec<Vec<DropLocal<'ctx>>>,
    enums: HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    enum_types: HashMap<(Symbol, Vec<Type>), StructType<'ctx>>,
    variant_types: HashMap<(Symbol, Vec<Type>, Symbol), StructType<'ctx>>,
    instances: &'ctx HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
}

impl<'ctx> CodeGen<'ctx> {
    pub fn new(
        context: &'ctx Context,
        interner: &'ctx Interner,
        module_name: &str,
        types: &'ctx HashMap<ExprId, Type>,
        sources: &'ctx SourceMap<'ctx>,
        opt: u8,
        debug: bool,
        moves: &'ctx HashSet<ExprId>,
        instances: &'ctx HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    ) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();

        let (dib, di_cu, di_file) = if debug {
            let path = std::path::Path::new(sources.entry_name());
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("<girdi>");
            let directory = path.parent().and_then(|s| s.to_str()).unwrap_or(".");

            let dbg_ver = context.i32_type().const_int(3, false);
            module.add_basic_value_flag("Debug Info Version", FlagBehavior::Warning, dbg_ver);

            let (dib, cu) = module.create_debug_info_builder(
                true,
                DWARFSourceLanguage::C,
                filename,
                directory,
                "lexora",
                opt > 0,
                "",
                0,
                "",
                DWARFEmissionKind::Full,
                0,
                false,
                false,
                "",
                "",
            );
            let file = dib.create_file(filename, directory);
            (Some(dib), Some(cu), Some(file))
        } else {
            (None, None, None)
        };
        let target_data = if debug {
            Target::initialize_native(&InitializationConfig::default()).ok();
            let triple = TargetMachine::get_default_triple();
            Target::from_triple(&triple)
                .ok()
                .and_then(|t| {
                    t.create_target_machine(
                        &triple,
                        &TargetMachine::get_host_cpu_name().to_string(),
                        &TargetMachine::get_host_cpu_features().to_string(),
                        OptimizationLevel::None,
                        RelocMode::PIC,
                        CodeModel::Default,
                    )
                })
                .map(|tm| tm.get_target_data())
        } else {
            None
        };
        CodeGen {
            context,
            module,
            builder,
            interner,
            vars: VarTable::new(),
            structs: HashMap::new(),
            cur_fn: None,
            cur_ret: None,
            opt,
            types,
            sources,
            debug,
            dib,
            di_cu,
            di_file,
            di_sp: None,
            target_data,
            struct_fields_ast: HashMap::new(),
            moves,
            drop_scopes: Vec::new(),
            enums: HashMap::new(),
            enum_types: HashMap::new(),
            variant_types: HashMap::new(),
            instances,
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
            Type::Struct(s) => self
                .structs
                .get(s)
                .expect("struct kayitli degil, register_structs once calismali")
                .0
                .into(),
            Type::Box(_) => self.context.ptr_type(AddressSpace::default()).into(),
            Type::String => self.context.ptr_type(AddressSpace::default()).into(),
            Type::Enum(s, args) => {
                if let Some(et) = self.enum_types.get(&(*s, args.clone())) {
                    (*et).into()
                } else {
                    self.context.i32_type().into()
                }
            }
            Type::Void => panic!("void bir deger tipi olarak kullanilamaz"),
            Type::Error => panic!("Type::Error codegen'e ulasti (errors bos degilse codegen yok)"),
            Type::Param(_) => {
                panic!("Type::Param codegen'e ulasti (generic enum TC'de reddedilir)")
            }
        }
    }
    fn di_type(&self, ty: &Type) -> DIType<'ctx> {
        let dib = self.dib.as_ref().unwrap();
        let td = self.target_data.as_ref().unwrap();
        match ty {
            Type::I32 => dib
                .create_basic_type("i32", 32, 0x05, DIFlags::ZERO)
                .unwrap()
                .as_type(),
            Type::I64 => dib
                .create_basic_type("i64", 64, 0x05, DIFlags::ZERO)
                .unwrap()
                .as_type(),
            Type::Bool => dib
                .create_basic_type("bool", 8, 0x02, DIFlags::ZERO)
                .unwrap()
                .as_type(),
            Type::Str => {
                let i8t = dib
                    .create_basic_type("i8", 8, 0x06, DIFlags::ZERO)
                    .unwrap()
                    .as_type();
                dib.create_pointer_type("str", i8t, 64, 0, AddressSpace::default())
                    .as_type()
            }
            Type::Array(elem, n) => {
                let inner = self.di_type(elem);
                let size = td.get_bit_size(&self.llvm_type(ty));
                dib.create_array_type(inner, size, 0, &[0..(*n as i64)])
                    .as_type()
            }
            Type::Struct(sym) => {
                let file = self.di_file.unwrap();
                let scope = self.di_cu.unwrap().as_debug_info_scope();
                let st = self.structs.get(sym).unwrap().0;
                let fields = self.struct_fields_ast.get(sym).unwrap().clone();
                let mut members = Vec::new();
                for (i, (fname, fty)) in fields.iter().enumerate() {
                    let fdi = self.di_type(fty);
                    let fsize = td.get_bit_size(&self.llvm_type(fty));
                    let foffset = td.offset_of_element(&st, i as u32).unwrap_or(0) * 8;
                    let m = dib
                        .create_member_type(
                            scope,
                            self.interner.resolve(*fname),
                            file,
                            0,
                            fsize,
                            0,
                            foffset,
                            DIFlags::ZERO,
                            fdi,
                        )
                        .as_type();
                    members.push(m);
                }
                let total = td.get_bit_size(&self.llvm_type(ty));
                let name = self.interner.resolve(*sym);
                dib.create_struct_type(
                    scope,
                    name,
                    file,
                    0,
                    total,
                    0,
                    DIFlags::ZERO,
                    None,
                    &members,
                    0,
                    None,
                    "",
                )
                .as_type()
            }
            Type::Box(inner) => {
                let pointee = self.di_type(inner);
                dib.create_pointer_type("box", pointee, 64, 0, AddressSpace::default())
                    .as_type()
            }
            Type::String => {
                let i8t = dib
                    .create_basic_type("i8", 8, 0x06, DIFlags::ZERO)
                    .unwrap()
                    .as_type();
                dib.create_pointer_type("String", i8t, 64, 0, AddressSpace::default())
                    .as_type()
            }
            Type::Enum(..) => dib
                .create_basic_type("enum", 32, 0x05, DIFlags::ZERO)
                .unwrap()
                .as_type(),
            Type::Void | Type::Error | Type::Param(_) => {
                unreachable!("debug: void/error tipi di_type'a ulasmaz")
            }
        }
    }
    pub fn compile(&mut self, program: &Program) -> Result<(), BuilderError> {
        self.register_structs(program);
        self.register_enums(program);
        self.gen_drop_functions(program)?;

        for func in &program.functions {
            self.declare_functions(func);
        }
        for func in &program.functions {
            self.gen_function(func)?;
        }
        if let Some(dib) = &self.dib {
            dib.finalize();
        }
        Ok(())
    }
    fn register_structs(&mut self, program: &Program) {
        for s in &program.structs {
            let st = self
                .context
                .opaque_struct_type(&format!("struct.{}", s.name.0));
            self.structs.insert(s.name, (st, Vec::new()));
            self.struct_fields_ast.insert(s.name, s.fields.clone());
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
    fn register_enums(&mut self, program: &Program) {
        for e in &program.enums {
            if e.params.is_empty() {
                self.enums.insert((e.name, Vec::new()), e.variants.clone());
            }
        }
        for (key, variants) in self.instances.iter() {
            self.enums.insert(key.clone(), variants.clone());
        }
        let keys: Vec<(Symbol, Vec<Type>)> = self.enums.keys().cloned().collect();
        for key in keys {
            let variants = self.enums.get(&key).unwrap().clone();
            let max_slots = variants.iter().map(|(_, t)| t.len()).max().unwrap_or(0);
            if max_slots == 0 {
                continue;
            }
            let payload = self.context.i64_type().array_type(max_slots as u32);
            let enum_ty = self
                .context
                .struct_type(&[self.context.i32_type().into(), payload.into()], false);
            self.enum_types.insert(key.clone(), enum_ty);
            for (variant, tys) in variants.iter() {
                let fields: Vec<BasicTypeEnum> = tys.iter().map(|t| self.llvm_type(t)).collect();
                let vt = self.context.struct_type(&fields, false);
                self.variant_types
                    .insert((key.0, key.1.clone(), *variant), vt);
            }
        }
    }
    fn enum_has_payload(&self, key: &(Symbol, Vec<Type>)) -> bool {
        self.enum_types.contains_key(key)
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
    fn variant_tag(&self, key: &(Symbol, Vec<Type>), variant: Symbol) -> u64 {
        self.enums
            .get(key)
            .unwrap()
            .iter()
            .position(|(v, _)| *v == variant)
            .unwrap() as u64
    }
    fn result_variants(&self, key: &(Symbol, Vec<Type>)) -> (Symbol, Type, Symbol, Type) {
        let variants = self.enums.get(key).unwrap();
        let ok = variants
            .iter()
            .find(|(v, _)| self.interner.resolve(*v) == "Ok")
            .unwrap();
        let err = variants
            .iter()
            .find(|(v, _)| self.interner.resolve(*v) == "Err")
            .unwrap();
        (ok.0, ok.1[0].clone(), err.0, err.1[0].clone())
    }
    fn inst_name(sym: Symbol, args: &[Type]) -> String {
        if args.is_empty() {
            sym.0.to_string()
        } else {
            let m: Vec<String> = args.iter().map(|t| t.mangle()).collect();
            format!("{}.{}", sym.0, m.join("."))
        }
    }
    fn declare_functions(&mut self, func: &Function) -> FunctionValue<'ctx> {
        let param_types: Vec<BasicMetadataTypeEnum> = func
            .params
            .iter()
            .map(|(_, ty)| self.llvm_type(ty).into())
            .collect();

        let fn_type = match &func.return_type {
            Type::Void => self.context.void_type().fn_type(&param_types, false),
            ret => self.llvm_type(ret).fn_type(&param_types, false),
        };

        let name = self.interner.resolve(func.name);
        self.module
            .get_function(name)
            .unwrap_or_else(|| self.module.add_function(name, fn_type, None))
    }
    fn set_loc(&self, span: Span) {
        if !self.debug {
            return;
        }
        let dib = match self.dib.as_ref() {
            Some(d) => d,
            None => return,
        };
        let sp = match self.di_sp {
            Some(s) => s,
            None => return,
        };
        if let Some(r) = self.sources.resolve(span) {
            let loc = dib.create_debug_location(
                self.context,
                r.line as u32,
                r.col as u32,
                sp.as_debug_info_scope(),
                None,
            );
            self.builder.set_current_debug_location(loc);
        }
    }
    fn insert_declare(
        &self,
        slot: PointerValue<'ctx>,
        var: DILocalVariable<'ctx>,
        loc: DILocation<'ctx>,
        block: BasicBlock<'ctx>,
    ) {
        let dib = self.dib.as_ref().unwrap();
        let expr = dib.create_expression(vec![]);
        unsafe {
            LLVMDIBuilderInsertDeclareRecordAtEnd(
                dib.as_mut_ptr(),
                slot.as_value_ref(),
                var.as_mut_ptr(),
                expr.as_mut_ptr(),
                loc.as_mut_ptr(),
                block.as_mut_ptr(),
            );
        }
    }
    fn gen_function(&mut self, func: &Function) -> Result<(), BuilderError> {
        let name = self.interner.resolve(func.name);
        let function = self.module.get_function(name).unwrap();
        self.cur_fn = Some(function);
        self.cur_ret = Some(func.return_type.clone());
        let entry = self.context.append_basic_block(function, "entry");
        self.builder.position_at_end(entry);
        self.builder.unset_current_debug_location();

        self.vars = VarTable::new();
        self.vars.enter();
        self.drop_enter();
        if self.debug {
            let dib = self.dib.as_ref().unwrap();
            let file = self.di_file.unwrap();
            let cu = self.di_cu.unwrap();
            let ret_di = match func.return_type {
                Type::Void => None,
                ref t => Some(self.di_type(t)),
            };
            let params_di: Vec<DIType> = func.params.iter().map(|(_, t)| self.di_type(t)).collect();
            let subroutine = dib.create_subroutine_type(file, ret_di, &params_di, DIFlags::ZERO);
            let line = self
                .sources
                .resolve(func.span)
                .map(|r| r.line as u32)
                .unwrap_or(0);
            let sp = dib.create_function(
                cu.as_debug_info_scope(),
                name,
                None,
                file,
                line,
                subroutine,
                false,
                true,
                line,
                DIFlags::ZERO,
                self.opt > 0,
            );
            function.set_subprogram(sp);
            self.di_sp = Some(sp);
        }

        for (i, (sym, ty)) in func.params.iter().enumerate() {
            let llvm_ty = self.llvm_type(ty);
            let pname = self.interner.resolve(*sym);
            let slot = self.entry_alloca(llvm_ty, pname)?;
            let arg = function.get_nth_param(i as u32).unwrap();
            self.builder.build_store(slot, arg)?;
            self.vars.insert(*sym, (slot, llvm_ty));
            if self.is_owning_type(ty) {
                let flag = self.entry_alloca(self.context.bool_type().into(), "dropflag")?;
                self.builder
                    .build_store(flag, self.context.bool_type().const_int(1, false))?;
                self.drop_scopes.last_mut().unwrap().push(DropLocal {
                    sym: *sym,
                    slot,
                    flag,
                    pointee: ty.clone(),
                });
            }
            if self.debug {
                let dib = self.dib.as_ref().unwrap();
                let file = self.di_file.unwrap();
                let sp = self.di_sp.unwrap();
                let line = self
                    .sources
                    .resolve(func.span)
                    .map(|r| r.line as u32)
                    .unwrap_or(0);
                let var = dib.create_parameter_variable(
                    sp.as_debug_info_scope(),
                    pname,
                    (i + 1) as u32,
                    file,
                    line,
                    self.di_type(ty),
                    true,
                    DIFlags::ZERO,
                );
                let loc = dib.create_debug_location(
                    self.context,
                    line,
                    0,
                    sp.as_debug_info_scope(),
                    None,
                );
                let block = self.builder.get_insert_block().unwrap();
                self.insert_declare(slot, var, loc, block);
            }
        }
        for stmt in func.body.stmts {
            self.gen_statement(stmt)?;
        }
        if let Some(tail) = func.body.tail {
            if self
                .builder
                .get_insert_block()
                .unwrap()
                .get_terminator()
                .is_none()
            {
                let val = self.gen_expr(tail)?;
                self.free_all_live()?;
                match func.return_type {
                    Type::Void => {
                        self.builder.build_return(None)?;
                    }
                    _ => {
                        self.builder.build_return(Some(&val))?;
                    }
                }
            }
        }
        self.drop_exit()?;
        let cur = self.builder.get_insert_block().unwrap();
        if cur.get_terminator().is_none() {
            match func.return_type {
                Type::Void => {
                    self.builder.build_return(None)?;
                }
                _ => {
                    self.builder.build_unreachable()?;
                }
            }
        }
        self.vars.exit();
        Ok(())
    }
    fn gen_statement(&mut self, stmt: &Stmt) -> Result<(), BuilderError> {
        self.set_loc(stmt.span());
        match stmt {
            Stmt::Let {
                name, value, span, ..
            } => {
                let llvm_ty = self.llvm_type(&self.types[&value.id()]);
                let pname = self.interner.resolve(*name);
                let slot = self.entry_alloca(llvm_ty, pname)?;

                self.gen_into(value, slot)?;
                self.vars.insert(*name, (slot, llvm_ty));
                let owning_ty = match self.types.get(&value.id()) {
                    Some(t) if self.is_owning_type(t) => Some(t.clone()),
                    _ => None,
                };

                if let Some(pointee) = owning_ty {
                    let flag = self.entry_alloca(self.context.bool_type().into(), "dropflag")?;
                    self.builder
                        .build_store(flag, self.context.bool_type().const_int(1, false))?;
                    self.drop_scopes.last_mut().unwrap().push(DropLocal {
                        sym: *name,
                        slot,
                        flag,
                        pointee,
                    });
                }
                if self.debug {
                    let dib = self.dib.as_ref().unwrap();
                    let file = self.di_file.unwrap();
                    let sp = self.di_sp.unwrap();
                    let dty = &self.types[&value.id()];
                    let line = self
                        .sources
                        .resolve(*span)
                        .map(|r| r.line as u32)
                        .unwrap_or(0);
                    let var = dib.create_auto_variable(
                        sp.as_debug_info_scope(),
                        pname,
                        file,
                        line,
                        self.di_type(dty),
                        true,
                        DIFlags::ZERO,
                        0,
                    );
                    let loc = dib.create_debug_location(
                        self.context,
                        line,
                        0,
                        sp.as_debug_info_scope(),
                        None,
                    );
                    let block = self.builder.get_insert_block().unwrap();
                    self.insert_declare(slot, var, loc, block);
                }

                Ok(())
            }
            Stmt::Assign { name, value, .. } => {
                let (slot, _) = self.vars.get(*name);
                let val = self.gen_expr(value)?;
                self.builder.build_store(slot, val)?;
                Ok(())
            }
            Stmt::Return(expr, _) => {
                let val = self.gen_expr(expr)?;
                self.free_all_live()?;
                self.builder.build_return(Some(&val))?;
                Ok(())
            }
            Stmt::Expr(expr, _) => {
                self.gen_expr(expr)?;
                Ok(())
            }

            Stmt::While {
                condition, body, ..
            } => {
                let function = self.cur_fn.unwrap();

                let cond_bb = self.context.append_basic_block(function, "while_cond");
                let body_bb = self.context.append_basic_block(function, "while_body");
                let after_bb = self.context.append_basic_block(function, "while_after");

                self.builder.build_unconditional_branch(cond_bb)?;

                self.builder.position_at_end(cond_bb);
                let cond_val = self.gen_expr(condition)?.into_int_value();
                self.builder
                    .build_conditional_branch(cond_val, body_bb, after_bb)?;

                self.builder.position_at_end(body_bb);
                self.vars.enter();
                self.drop_enter();
                for stmt in body.stmts {
                    self.gen_statement(stmt)?;
                }
                if let Some(tail) = body.tail {
                    if self
                        .builder
                        .get_insert_block()
                        .unwrap()
                        .get_terminator()
                        .is_none()
                    {
                        self.gen_expr(tail)?;
                    }
                }
                self.drop_exit()?;
                self.vars.exit();
                if self
                    .builder
                    .get_insert_block()
                    .unwrap()
                    .get_terminator()
                    .is_none()
                {
                    self.builder.build_unconditional_branch(cond_bb)?;
                }
                self.builder.position_at_end(after_bb);
                Ok(())
            }
            Stmt::For {
                var,
                from,
                to,
                body,
                ..
            } => {
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
                let cur_val = self
                    .builder
                    .build_load(i32t, slot, var_name)?
                    .into_int_value();
                let to_val = self.gen_expr(to)?.into_int_value();
                let cmp = self.builder.build_int_compare(
                    IntPredicate::SLT,
                    cur_val,
                    to_val,
                    "for_cmp",
                )?;
                self.builder
                    .build_conditional_branch(cmp, body_bb, after_bb)?;

                self.builder.position_at_end(body_bb);
                self.drop_enter();
                for stmt in body.stmts {
                    self.gen_statement(stmt)?;
                }
                if let Some(tail) = body.tail {
                    if self
                        .builder
                        .get_insert_block()
                        .unwrap()
                        .get_terminator()
                        .is_none()
                    {
                        self.gen_expr(tail)?;
                    }
                }
                self.drop_exit()?;
                if self
                    .builder
                    .get_insert_block()
                    .unwrap()
                    .get_terminator()
                    .is_none()
                {
                    let cur = self
                        .builder
                        .build_load(i32t, slot, var_name)?
                        .into_int_value();
                    let one = self.context.i32_type().const_int(1, false);
                    let next = self.builder.build_int_add(cur, one, "inc")?;
                    self.builder.build_store(slot, next)?;
                    self.builder.build_unconditional_branch(cond_bb)?;
                }
                self.vars.exit();
                self.builder.position_at_end(after_bb);
                Ok(())
            }
            Stmt::AssignPlace { target, value, .. } => {
                let ptr = self.gen_place(target)?;
                let val = self.gen_expr(value)?;
                self.builder.build_store(ptr, val)?;
                Ok(())
            }

            Stmt::Error(_) => {
                unreachable!("poison statement codegen'e ulasti (errors bos degilse codegen yok)")
            }
        }
    }
    fn gen_expr(&mut self, expr: &Expr) -> Result<BasicValueEnum<'ctx>, BuilderError> {
        match expr {
            Expr::Integer(n, _, _) => {
                let val = if *n > i32::MAX as i64 || *n < i32::MIN as i64 {
                    self.context.i64_type().const_int(*n as u64, true)
                } else {
                    self.context.i32_type().const_int(*n as u64, true)
                };
                Ok(val.into())
            }
            Expr::Bool(b, _, _) => Ok(self.context.bool_type().const_int(*b as u64, false).into()),
            Expr::Identifier(sym, _, id) => {
                let (ptr, pointee_ty) = self.vars.get(*sym);
                let name = self.interner.resolve(*sym);
                let loaded = self.builder.build_load(pointee_ty, ptr, name)?;
                self.clear_flag_on_move(*id, *sym)?;
                Ok(loaded)
            }
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
                let l = self.gen_expr(left)?.into_int_value();
                let r = self.gen_expr(right)?.into_int_value();
                let res = match op {
                    BinaryOperator::Add => self.checked_arith("llvm.sadd.with.overflow", l, r)?,
                    BinaryOperator::Sub => self.checked_arith("llvm.ssub.with.overflow", l, r)?,
                    BinaryOperator::Mul => self.checked_arith("llvm.smul.with.overflow", l, r)?,
                    BinaryOperator::Div => {
                        let function = self.cur_fn.unwrap();
                        let is_zero = self.builder.build_int_compare(
                            IntPredicate::EQ,
                            r,
                            r.get_type().const_zero(),
                            "divz",
                        )?;
                        let panic_bb = self.context.append_basic_block(function, "div_panic");
                        let ok_bb = self.context.append_basic_block(function, "div_ok");
                        self.builder
                            .build_conditional_branch(is_zero, panic_bb, ok_bb)?;

                        self.builder.position_at_end(panic_bb);
                        let panic_fn = self.get_or_build_panic()?;
                        let msg = self.fmt_global(".panicmsg_div", "lexora: division by zero")?;
                        self.builder.build_call(panic_fn, &[msg.into()], "")?;
                        self.builder.build_unreachable()?;

                        self.builder.position_at_end(ok_bb);
                        let min = l.get_type().const_int(
                            if l.get_type().get_bit_width() == 64 {
                                i64::MIN as u64
                            } else {
                                i32::MIN as i64 as u64
                            },
                            true,
                        );
                        let l_min =
                            self.builder
                                .build_int_compare(IntPredicate::EQ, l, min, "lmin")?;
                        let neg1 = l.get_type().const_all_ones();
                        let r_neg1 =
                            self.builder
                                .build_int_compare(IntPredicate::EQ, r, neg1, "rneg1")?;
                        let both = self.builder.build_and(l_min, r_neg1, "divovf")?;
                        let ovf_bb = self.context.append_basic_block(function, "divovf_panic");
                        let ok2_bb = self.context.append_basic_block(function, "divovf_ok");
                        self.builder.build_conditional_branch(both, ovf_bb, ok2_bb)?;
                        self.builder.position_at_end(ovf_bb);
                        let panic_fn = self.get_or_build_panic()?;
                        let msg = self.fmt_global(".panicmsg_ovf", "lexora: integer overflow")?;
                        self.builder.build_call(panic_fn, &[msg.into()], "")?;
                        self.builder.build_unreachable()?;
                        self.builder.position_at_end(ok2_bb);
                        self.builder.build_int_signed_div(l, r, "div")?
                    }
                    BinaryOperator::Eq => {
                        self.builder
                            .build_int_compare(IntPredicate::EQ, l, r, "cmp")?
                    }
                    BinaryOperator::NotEq => {
                        self.builder
                            .build_int_compare(IntPredicate::NE, l, r, "cmp")?
                    }
                    BinaryOperator::Less => {
                        self.builder
                            .build_int_compare(IntPredicate::SLT, l, r, "cmp")?
                    }
                    BinaryOperator::Greater => {
                        self.builder
                            .build_int_compare(IntPredicate::SGT, l, r, "cmp")?
                    }
                    BinaryOperator::LessEq => {
                        self.builder
                            .build_int_compare(IntPredicate::SLE, l, r, "cmp")?
                    }
                    BinaryOperator::GreaterEq => {
                        self.builder
                            .build_int_compare(IntPredicate::SGE, l, r, "cmp")?
                    }
                    BinaryOperator::And => self.builder.build_and(l, r, "and")?,
                    BinaryOperator::Or => self.builder.build_or(l, r, "or")?,
                };
                Ok(res.into())
            }
            Expr::Call { name, args, .. } => {
                let fname = self.interner.resolve(*name);

                if fname == "print" {
                    let arg = self.gen_expr(&args[0])?;
                    let printf = self.get_printf();

                    let (fmt_ptr, print_arg): (PointerValue, BasicMetadataValueEnum) =
                        match &self.types[&args[0].id()] {
                            Type::Str | Type::String => {
                                (self.fmt_global(".fmt_s", "%s\n")?, arg.into())
                            }
                            Type::I64 => (self.fmt_global(".fmt_lld", "%lld\n")?, arg.into()),
                            Type::Bool => {
                                let z = self.builder.build_int_z_extend(
                                    arg.into_int_value(),
                                    self.context.i32_type(),
                                    "boolext",
                                )?;
                                (self.fmt_global(".fmt_d", "%d\n")?, z.into())
                            }
                            _ => (self.fmt_global(".fmt_d", "%d\n")?, arg.into()),
                        };

                    self.builder
                        .build_call(printf, &[fmt_ptr.into(), print_arg], "printf_call")?;
                    if matches!(self.types[&args[0].id()], Type::String) {
                        self.str_temp_free(&args[0], arg.into_pointer_value())?;
                    }
                    Ok(self.context.i32_type().const_int(0, false).into())
                } else if fname == "len" {
                    let arg = &args[0];
                    let val = self.gen_expr(arg)?.into_pointer_value();
                    let len64 = self.str_len_of(arg, val)?;
                    self.str_temp_free(arg, val)?;
                    Ok(self
                        .builder
                        .build_int_truncate(len64, self.context.i32_type(), "len32")?
                        .into())
                } else if fname == "string" {
                    let arg = self.gen_expr(&args[0])?;
                    let f = self.get_lexora_str_new()?;
                    let call = self.builder.build_call(f, &[arg.into()], "strnew")?;
                    match call.try_as_basic_value() {
                        ValueKind::Basic(v) => Ok(v),
                        ValueKind::Instruction(_) => unreachable!("lexora_str_new ptr dondurur"),
                    }
                } else {
                    let function = self.module.get_function(fname).unwrap();
                    let mut argv: Vec<BasicMetadataValueEnum> = Vec::new();
                    for a in args.iter() {
                        argv.push(self.gen_expr(a)?.into());
                    }
                    let call = self.builder.build_call(function, &argv, "call")?;
                    match call.try_as_basic_value() {
                        ValueKind::Basic(v) => Ok(v),
                        ValueKind::Instruction(_) => {
                            Ok(self.context.i32_type().const_int(0, false).into())
                        }
                    }
                }
            }
            Expr::StringLiteral(s, _, _) => {
                let g = self.builder.build_global_string_ptr(s, ".str")?;
                Ok(g.as_pointer_value().into())
            }
            Expr::Cast {
                expr, target_type, ..
            } => {
                let val = self.gen_expr(expr)?.into_int_value();
                let from_bits = val.get_type().get_bit_width();

                let target_ty = self.llvm_type(target_type).into_int_type();
                let to_bits = target_ty.get_bit_width();

                let res = if to_bits > from_bits {
                    self.builder.build_int_s_extend(val, target_ty, "sext")?
                } else if to_bits < from_bits {
                    self.builder.build_int_truncate(val, target_ty, "trunc")?
                } else {
                    val
                };
                Ok(res.into())
            }
            Expr::UnaryOp { op, operand, .. } => {
                let val = self.gen_expr(operand)?.into_int_value();
                let res = match op {
                    UnaryOperator::Not => self.builder.build_not(val, "not")?,
                    UnaryOperator::Neg => self.builder.build_int_neg(val, "neg")?,
                };
                Ok(res.into())
            }
            Expr::Index { .. } => {
                let ptr = self.gen_place(expr)?;
                let ty = self.llvm_type(&self.types[&expr.id()]);
                Ok(self.builder.build_load(ty, ptr, "elem")?)
            }
            Expr::FieldAccess { .. } => {
                let ptr = self.gen_place(expr)?;
                let ty = self.llvm_type(&self.types[&expr.id()]);
                Ok(self.builder.build_load(ty, ptr, "fldval")?)
            }
            Expr::Box { value, .. } => {
                let pointee = self.llvm_type(&self.types[&value.id()]);
                let val = self.gen_expr(value)?;
                Ok(self.build_box(pointee, val)?.into())
            }
            Expr::Deref { target, id, .. } => {
                let ptr = self.gen_expr(target)?.into_pointer_value();
                let pointee = self.llvm_type(&self.types[&expr.id()]);
                let val = self.builder.build_load(pointee, ptr, "deref")?;
                if self.moves.contains(id) {
                    self.build_free(ptr)?;
                }
                Ok(val)
            }
            Expr::Try { expr: inner, .. } => {
                let (sym, targs) = match &self.types[&inner.id()] {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => unreachable!("'?' ifadesinin tipi Result degil (typechecker kacirmali)"),
                };
                let key = (sym, targs);
                let enum_ty = *self.enum_types.get(&key).unwrap();
                let (ok_sym, ok_fty, err_sym, err_fty) = self.result_variants(&key);
                let ret_key = match self.cur_ret.clone() {
                    Some(Type::Enum(s, a)) => (s, a),
                    _ => unreachable!(
                        "'?' kullanan fonksiyonun donus tipi Result degil (typechecker kacirmali)"
                    ),
                };
                let ret_enum_ty = *self.enum_types.get(&ret_key).unwrap();
                let (_, _, ret_err_sym, _) = self.result_variants(&ret_key);

                let val = self.gen_expr(inner)?;
                let tmp = self.entry_alloca(enum_ty.into(), "tryres")?;
                self.builder.build_store(tmp, val)?;
                let tag_ptr = self.builder.build_struct_gep(enum_ty, tmp, 0, "tag")?;
                let tag = self
                    .builder
                    .build_load(self.context.i32_type(), tag_ptr, "tagval")?
                    .into_int_value();
                let err_tag = self
                    .context
                    .i32_type()
                    .const_int(self.variant_tag(&key, err_sym), false);
                let is_err =
                    self.builder
                        .build_int_compare(IntPredicate::EQ, tag, err_tag, "iserr")?;
                let function = self.cur_fn.unwrap();
                let err_bb = self.context.append_basic_block(function, "try_err");
                let ok_bb = self.context.append_basic_block(function, "try_ok");

                self.builder
                    .build_conditional_branch(is_err, err_bb, ok_bb)?;

                self.builder.position_at_end(err_bb);
                let evt = *self
                    .variant_types
                    .get(&(key.0, key.1.clone(), err_sym))
                    .unwrap();
                let payload_ptr = self.builder.build_struct_gep(enum_ty, tmp, 1, "payload")?;
                let efld = self.builder.build_struct_gep(evt, payload_ptr, 0, "vfld")?;
                let e_val = self
                    .builder
                    .build_load(self.llvm_type(&err_fty), efld, "errval")?;
                let ret_tmp = self.entry_alloca(ret_enum_ty.into(), "tryret")?;
                let rtag = self
                    .context
                    .i32_type()
                    .const_int(self.variant_tag(&ret_key, ret_err_sym), false);
                let rtag_ptr = self
                    .builder
                    .build_struct_gep(ret_enum_ty, ret_tmp, 0, "tag")?;
                self.builder.build_store(rtag_ptr, rtag)?;
                let rvt = *self
                    .variant_types
                    .get(&(ret_key.0, ret_key.1.clone(), ret_err_sym))
                    .unwrap();
                let rpayload = self
                    .builder
                    .build_struct_gep(ret_enum_ty, ret_tmp, 1, "payload")?;
                let rfld = self.builder.build_struct_gep(rvt, rpayload, 0, "vfld")?;
                self.builder.build_store(rfld, e_val)?;
                let ret_val = self.builder.build_load(ret_enum_ty, ret_tmp, "retval")?;
                self.free_all_live()?;
                self.builder.build_return(Some(&ret_val))?;

                self.builder.position_at_end(ok_bb);
                let ovt = *self
                    .variant_types
                    .get(&(key.0, key.1.clone(), ok_sym))
                    .unwrap();
                let payload_ptr = self.builder.build_struct_gep(enum_ty, tmp, 1, "payload")?;
                let ofld = self.builder.build_struct_gep(ovt, payload_ptr, 0, "vfld")?;
                Ok(self
                    .builder
                    .build_load(self.llvm_type(&ok_fty), ofld, "okval")?)
            }
            Expr::Error(_, _) => {
                unreachable!("poison ifade codegen'e ulasti (errors bos degilse codegen yok)")
            }
            Expr::EnumVariant { variant, args, .. } => {
                let (sym, targs) = match &self.types[&expr.id()] {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => unreachable!("enum ifadesinin tipi enum degil (typechecker kacirmali)"),
                };
                let key = (sym, targs);
                if !self.enum_has_payload(&key) {
                    return Ok(self
                        .context
                        .i32_type()
                        .const_int(self.variant_tag(&key, *variant), false)
                        .into());
                }
                let enum_ty = *self.enum_types.get(&key).unwrap();
                let vt = *self
                    .variant_types
                    .get(&(key.0, key.1.clone(), *variant))
                    .unwrap();
                let tmp = self.entry_alloca(enum_ty.into(), "enumtmp")?;
                let tag = self
                    .context
                    .i32_type()
                    .const_int(self.variant_tag(&key, *variant), false);
                let tag_ptr = self.builder.build_struct_gep(enum_ty, tmp, 0, "tag")?;
                self.builder.build_store(tag_ptr, tag)?;
                let payload_ptr = self.builder.build_struct_gep(enum_ty, tmp, 1, "payload")?;
                for (i, arg) in args.iter().enumerate() {
                    let fval = self.gen_expr(arg)?;
                    let fptr = self
                        .builder
                        .build_struct_gep(vt, payload_ptr, i as u32, "vfld")?;
                    self.builder.build_store(fptr, fval)?;
                }
                Ok(self.builder.build_load(enum_ty, tmp, "enumval")?)
            }
            Expr::Match {
                scrutinee, arms, ..
            } => {
                let (enum_sym, enum_args) = match &self.types[&scrutinee.id()] {
                    Type::Enum(s, a) => (*s, a.clone()),
                    _ => unreachable!("match scrutinee enum degil (typechecker kacirmali)"),
                };
                let key = (enum_sym, enum_args);
                let has_payload = self.enum_has_payload(&key);

                let (tag, scrut_ptr) = if has_payload {
                    let enum_ty = *self.enum_types.get(&key).unwrap();
                    let val = self.gen_expr(scrutinee)?;
                    let tmp = self.entry_alloca(enum_ty.into(), "scrut")?;
                    self.builder.build_store(tmp, val)?;
                    let tag_ptr = self.builder.build_struct_gep(enum_ty, tmp, 0, "tag")?;
                    let tag = self
                        .builder
                        .build_load(self.context.i32_type(), tag_ptr, "tagval")?
                        .into_int_value();
                    (tag, Some((tmp, enum_ty)))
                } else {
                    (self.gen_expr(scrutinee)?.into_int_value(), None)
                };
                let match_ty = self.types[&expr.id()].clone();
                let result_slot = if match_ty != Type::Void {
                    let lt = self.llvm_type(&match_ty);
                    Some((lt, self.entry_alloca(lt, "matchval")?))
                } else {
                    None
                };
                let function = self.cur_fn.unwrap();
                let end_bb = self.context.append_basic_block(function, "match_end");
                let i32t = self.context.i32_type();

                let mut arm_blocks: Vec<BasicBlock> = Vec::new();
                let mut cases: Vec<(IntValue, BasicBlock)> = Vec::new();
                let mut default_bb: Option<BasicBlock> = None;
                for (pat, _body) in arms.iter() {
                    let bb = self.context.append_basic_block(function, "arm");
                    arm_blocks.push(bb);
                    match pat {
                        Pattern::Variant { variant, .. } => {
                            let idx = self.variant_tag(&key, *variant);
                            cases.push((i32t.const_int(idx, false), bb));
                        }
                        Pattern::Wildcard => {
                            default_bb = Some(bb);
                        }
                    }
                }
                let default = default_bb.unwrap_or(end_bb);
                self.builder.build_switch(tag, default, &cases)?;

                for ((pat, body), bb) in arms.iter().zip(arm_blocks.iter()) {
                    self.builder.position_at_end(*bb);
                    self.vars.enter();
                    self.drop_enter();
                    if let (
                        Pattern::Variant {
                            variant, bindings, ..
                        },
                        Some((ptr, enum_ty)),
                    ) = (pat, scrut_ptr)
                    {
                        let vt = *self
                            .variant_types
                            .get(&(key.0, key.1.clone(), *variant))
                            .unwrap();
                        let payload_ptr =
                            self.builder.build_struct_gep(enum_ty, ptr, 1, "payload")?;
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
                            let fty = self.llvm_type(&field_tys[i]);
                            let fptr =
                                self.builder
                                    .build_struct_gep(vt, payload_ptr, i as u32, "vfld")?;
                            let loaded = self.builder.build_load(fty, fptr, "bind")?;
                            let bslot = self.entry_alloca(fty, "bindslot")?;
                            self.builder.build_store(bslot, loaded)?;
                            self.vars.insert(*b, (bslot, fty));
                            if self.is_owning_type(&field_tys[i]) {
                                let flag =
                                    self.entry_alloca(self.context.bool_type().into(), "dropflag")?;
                                self.builder.build_store(
                                    flag,
                                    self.context.bool_type().const_int(1, false),
                                )?;
                                self.drop_scopes.last_mut().unwrap().push(DropLocal {
                                    sym: *b,
                                    slot: bslot,
                                    flag,
                                    pointee: field_tys[i].clone(),
                                });
                            }
                        }
                    }
                    if let Pattern::Wildcard = pat {
                        if let Some((ptr, _)) = scrut_ptr {
                            if self.enum_is_owning(&key) {
                                self.builder
                                    .build_call(self.drop_fn(&key), &[ptr.into()], "")?;
                            }
                        }
                    }
                    for s in body.stmts.iter() {
                        self.gen_statement(s)?;
                    }
                    if let Some(tail) = body.tail {
                        if self
                            .builder
                            .get_insert_block()
                            .unwrap()
                            .get_terminator()
                            .is_none()
                        {
                            let val = self.gen_expr(tail)?;
                            if let Some((_, slot)) = &result_slot {
                                self.builder.build_store(*slot, val)?;
                            }
                        }
                    }
                    self.drop_exit()?;
                    self.vars.exit();
                    if self
                        .builder
                        .get_insert_block()
                        .unwrap()
                        .get_terminator()
                        .is_none()
                    {
                        self.builder.build_unconditional_branch(end_bb)?;
                    }
                }
                self.builder.position_at_end(end_bb);
                match result_slot {
                    Some((lt, slot)) => Ok(self.builder.build_load(lt, slot, "matchres")?),
                    None => Ok(self.context.i32_type().const_int(0, false).into()),
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
                    let lt = self.llvm_type(&if_ty);
                    Some((lt, self.entry_alloca(lt, "ifval")?))
                } else {
                    None
                };
                let cond_val = self.gen_expr(condition)?.into_int_value();
                let function = self.cur_fn.unwrap();
                let then_bb = self.context.append_basic_block(function, "then");
                let merge_bb = self.context.append_basic_block(function, "merge");
                if let Some(eb) = else_body {
                    let else_bb = self.context.append_basic_block(function, "else_br");
                    self.builder
                        .build_conditional_branch(cond_val, then_bb, else_bb)?;

                    self.builder.position_at_end(then_bb);
                    self.vars.enter();
                    self.drop_enter();
                    for s in then_body.stmts.iter() {
                        self.gen_statement(s)?;
                    }
                    if let Some(tail) = then_body.tail {
                        if self
                            .builder
                            .get_insert_block()
                            .unwrap()
                            .get_terminator()
                            .is_none()
                        {
                            let val = self.gen_expr(tail)?;
                            if let Some((_, slot)) = &result_slot {
                                self.builder.build_store(*slot, val)?;
                            }
                        }
                    }
                    self.drop_exit()?;
                    self.vars.exit();
                    if self
                        .builder
                        .get_insert_block()
                        .unwrap()
                        .get_terminator()
                        .is_none()
                    {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }

                    self.builder.position_at_end(else_bb);
                    self.vars.enter();
                    self.drop_enter();
                    for s in eb.stmts.iter() {
                        self.gen_statement(s)?;
                    }
                    if let Some(tail) = eb.tail {
                        if self
                            .builder
                            .get_insert_block()
                            .unwrap()
                            .get_terminator()
                            .is_none()
                        {
                            let val = self.gen_expr(tail)?;
                            if let Some((_, slot)) = &result_slot {
                                self.builder.build_store(*slot, val)?;
                            }
                        }
                    }
                    self.drop_exit()?;
                    self.vars.exit();
                    if self
                        .builder
                        .get_insert_block()
                        .unwrap()
                        .get_terminator()
                        .is_none()
                    {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }
                } else {
                    self.builder
                        .build_conditional_branch(cond_val, then_bb, merge_bb)?;

                    self.builder.position_at_end(then_bb);
                    self.vars.enter();
                    self.drop_enter();
                    for s in then_body.stmts.iter() {
                        self.gen_statement(s)?;
                    }
                    if let Some(tail) = then_body.tail {
                        if self
                            .builder
                            .get_insert_block()
                            .unwrap()
                            .get_terminator()
                            .is_none()
                        {
                            self.gen_expr(tail)?;
                        }
                    }
                    self.drop_exit()?;
                    self.vars.exit();
                    if self
                        .builder
                        .get_insert_block()
                        .unwrap()
                        .get_terminator()
                        .is_none()
                    {
                        self.builder.build_unconditional_branch(merge_bb)?;
                    }
                }
                self.builder.position_at_end(merge_bb);
                match result_slot {
                    Some((lt, slot)) => Ok(self.builder.build_load(lt, slot, "ifres")?),
                    None => Ok(self.context.i32_type().const_int(0, false).into()),
                }
            }
            Expr::ArrayLiteral(..) | Expr::StructLiteral { .. } => {
                let ty = self.llvm_type(&self.types[&expr.id()]);
                let tmp = self.entry_alloca(ty, "aggtmp")?;
                self.gen_into(expr, tmp)?;
                Ok(self.builder.build_load(ty, tmp, "aggval")?)
            }
        }
    }
    fn entry_alloca(
        &self,
        ty: BasicTypeEnum<'ctx>,
        name: &str,
    ) -> Result<PointerValue<'ctx>, BuilderError> {
        let func = self.cur_fn.unwrap();
        let entry = func.get_first_basic_block().unwrap();
        let tmp = self.context.create_builder();
        match entry.get_first_instruction() {
            Some(first) => tmp.position_before(&first),
            None => tmp.position_at_end(entry),
        }
        tmp.build_alloca(ty, name)
    }
    fn gen_place(&mut self, expr: &Expr) -> Result<PointerValue<'ctx>, BuilderError> {
        match expr {
            Expr::Identifier(sym, _, _) => Ok(self.vars.get(*sym).0),
            Expr::FieldAccess { object, field, .. } => {
                let base = self.gen_place(object)?;
                let st = self.llvm_type(&self.types[&object.id()]).into_struct_type();
                let idx = self.field_index(st, *field);
                self.builder.build_struct_gep(st, base, idx, "fld")
            }
            Expr::Index { array, index, .. } => {
                let base = self.gen_place(array)?;
                let array_ty = self.llvm_type(&self.types[&array.id()]).into_array_type();
                let idx_val = self.gen_expr(index)?.into_int_value();
                self.bounds_check(idx_val, array_ty.len())?;
                let zero = self.context.i32_type().const_zero();
                unsafe {
                    self.builder
                        .build_in_bounds_gep(array_ty, base, &[zero, idx_val], "elem_ptr")
                }
            }
            Expr::Deref { target, .. } => Ok(self.gen_expr(target)?.into_pointer_value()),
            other => {
                let ty = self.llvm_type(&self.types[&other.id()]);
                let tmp = self.entry_alloca(ty, "placetmp")?;
                self.gen_into(other, tmp)?;
                Ok(tmp)
            }
        }
    }

    fn gen_into(&mut self, expr: &Expr, ptr: PointerValue<'ctx>) -> Result<(), BuilderError> {
        match expr {
            Expr::ArrayLiteral(elems, _, _) => {
                let array_ty = self.llvm_type(&self.types[&expr.id()]).into_array_type();
                let zero = self.context.i32_type().const_zero();
                for (i, elem) in elems.iter().enumerate() {
                    let idx = self.context.i32_type().const_int(i as u64, false);
                    if elem.is_aggregate_literal() {
                        let eptr = unsafe {
                            self.builder
                                .build_in_bounds_gep(array_ty, ptr, &[zero, idx], "init_ptr")?
                        };
                        self.gen_into(elem, eptr)?;
                    } else {
                        let val = self.gen_expr(elem)?;
                        let eptr = unsafe {
                            self.builder
                                .build_in_bounds_gep(array_ty, ptr, &[zero, idx], "init_ptr")?
                        };
                        self.builder.build_store(eptr, val)?;
                    }
                }
                Ok(())
            }
            Expr::StructLiteral { name, fields, .. } => {
                let st = self.structs.get(name).unwrap().0;
                let defs = self.structs.get(name).unwrap().1.clone();
                for (idx, (fsym, _)) in defs.iter().enumerate() {
                    let fexpr = fields
                        .iter()
                        .find(|(n, _)| n == fsym)
                        .map(|(_, v)| v)
                        .unwrap();
                    if fexpr.is_aggregate_literal() {
                        let fptr = self.builder.build_struct_gep(st, ptr, idx as u32, "fld")?;
                        self.gen_into(fexpr, fptr)?;
                    } else {
                        let fval = self.gen_expr(fexpr)?;
                        let fptr = self.builder.build_struct_gep(st, ptr, idx as u32, "fld")?;
                        self.builder.build_store(fptr, fval)?;
                    }
                }
                Ok(())
            }
            other => {
                let val = self.gen_expr(other)?;
                self.builder.build_store(ptr, val)?;
                Ok(())
            }
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
        let fmt = tmp
            .build_global_string_ptr("%s\n", ".panic_fmt")?
            .as_pointer_value();
        let printf = self.get_printf();
        tmp.build_call(printf, &[fmt.into(), msg.into()], "")?;

        let one = self.context.i32_type().const_int(1, false);
        tmp.build_call(self.get_or_build_exit(), &[one.into()], "")?;

        tmp.build_unreachable()?;
        Ok(func)
    }
    fn get_malloc(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("malloc") {
            return f;
        }
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let i64t = self.context.i64_type();
        let fn_type = ptr_t.fn_type(&[i64t.into()], false);
        self.module.add_function("malloc", fn_type, None)
    }
    fn get_free(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("free") {
            return f;
        }
        let void_t = self.context.void_type();
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let fn_type = void_t.fn_type(&[ptr_t.into()], false);
        self.module.add_function("free", fn_type, None)
    }
    fn get_lexora_alloc(&self) -> Result<FunctionValue<'ctx>, BuilderError> {
        if let Some(f) = self.module.get_function("lexora_alloc") {
            return Ok(f);
        }
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let i64t = self.context.i64_type();
        let fn_type = ptr_t.fn_type(&[i64t.into()], false);
        let func = self.module.add_function("lexora_alloc", fn_type, None);

        let entry = self.context.append_basic_block(func, "entry");
        let oom = self.context.append_basic_block(func, "oom");
        let allocok = self.context.append_basic_block(func, "allocok");
        let tmp = self.context.create_builder();

        tmp.position_at_end(entry);
        let size = func.get_nth_param(0).unwrap().into_int_value();
        let call = tmp.build_call(self.get_malloc(), &[size.into()], "p")?;
        let p = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_pointer_value(),
            ValueKind::Instruction(_) => unreachable!("malloc ptr dondurur"),
        };
        let isnull = tmp.build_is_null(p, "isnull")?;
        tmp.build_conditional_branch(isnull, oom, allocok)?;

        tmp.position_at_end(oom);
        let panic_fn = self.get_or_build_panic()?;
        let msg = tmp
            .build_global_string_ptr("lexora: out of memory", ".panicmsg_oom")?
            .as_pointer_value();
        tmp.build_call(panic_fn, &[msg.into()], "")?;
        tmp.build_unreachable()?;

        tmp.position_at_end(allocok);
        tmp.build_return(Some(&p))?;
        Ok(func)
    }
    fn get_lexora_free(&self) -> Result<FunctionValue<'ctx>, BuilderError> {
        if let Some(f) = self.module.get_function("lexora_free") {
            return Ok(f);
        }
        let void_t = self.context.void_type();
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let fn_type = void_t.fn_type(&[ptr_t.into()], false);
        let func = self.module.add_function("lexora_free", fn_type, None);

        let entry = self.context.append_basic_block(func, "entry");
        let tmp = self.context.create_builder();
        tmp.position_at_end(entry);
        let p = func.get_nth_param(0).unwrap().into_pointer_value();
        tmp.build_call(self.get_free(), &[p.into()], "")?;
        tmp.build_return(None)?;
        Ok(func)
    }
    fn get_strlen(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("strlen") {
            return f;
        }
        let i64t = self.context.i64_type();
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        self.module
            .add_function("strlen", i64t.fn_type(&[ptr_t.into()], false), None)
    }
    fn get_strcpy(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("strcpy") {
            return f;
        }
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        self.module.add_function(
            "strcpy",
            ptr_t.fn_type(&[ptr_t.into(), ptr_t.into()], false),
            None,
        )
    }
    fn get_lexora_str_new(&self) -> Result<FunctionValue<'ctx>, BuilderError> {
        if let Some(f) = self.module.get_function("lexora_str_new") {
            return Ok(f);
        }
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let i64t = self.context.i64_type();
        let i8t = self.context.i8_type();
        let func = self.module.add_function(
            "lexora_str_new",
            ptr_t.fn_type(&[ptr_t.into()], false),
            None,
        );
        let entry = self.context.append_basic_block(func, "entry");
        let tmp = self.context.create_builder();
        tmp.position_at_end(entry);
        let src = func.get_nth_param(0).unwrap().into_pointer_value();
        let call = tmp.build_call(self.get_strlen(), &[src.into()], "len")?;
        let len = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_int_value(),
            ValueKind::Instruction(_) => unreachable!("strlen i64 dondurur"),
        };
        let total = tmp.build_int_add(len, i64t.const_int(9, false), "total")?;
        let call = tmp.build_call(self.get_lexora_alloc()?, &[total.into()], "blk")?;
        let blk = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_pointer_value(),
            ValueKind::Instruction(_) => unreachable!("lexora_alloc ptr dondurur"),
        };
        tmp.build_store(blk, len)?;
        let data =
            unsafe { tmp.build_in_bounds_gep(i8t, blk, &[i64t.const_int(8, false)], "data")? };
        tmp.build_call(self.get_strcpy(), &[data.into(), src.into()], "")?;
        tmp.build_return(Some(&data))?;
        Ok(func)
    }
    fn get_lexora_str_free(&self) -> Result<FunctionValue<'ctx>, BuilderError> {
        if let Some(f) = self.module.get_function("lexora_str_free") {
            return Ok(f);
        }
        let void_t = self.context.void_type();
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let i64t = self.context.i64_type();
        let i8t = self.context.i8_type();
        let func = self.module.add_function(
            "lexora_str_free",
            void_t.fn_type(&[ptr_t.into()], false),
            None,
        );
        let entry = self.context.append_basic_block(func, "entry");
        let tmp = self.context.create_builder();
        tmp.position_at_end(entry);
        let s = func.get_nth_param(0).unwrap().into_pointer_value();
        let blk = unsafe {
            tmp.build_in_bounds_gep(i8t, s, &[i64t.const_int((-8i64) as u64, true)], "blk")?
        };
        tmp.build_call(self.get_lexora_free()?, &[blk.into()], "")?;
        tmp.build_return(None)?;
        Ok(func)
    }
    fn get_lexora_str_concat(&self) -> Result<FunctionValue<'ctx>, BuilderError> {
        if let Some(f) = self.module.get_function("lexora_str_concat") {
            return Ok(f);
        }
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let i64t = self.context.i64_type();
        let i8t = self.context.i8_type();
        let func = self.module.add_function(
            "lexora_str_concat",
            ptr_t.fn_type(
                &[ptr_t.into(), i64t.into(), ptr_t.into(), i64t.into()],
                false,
            ),
            None,
        );
        let entry = self.context.append_basic_block(func, "entry");
        let tmp = self.context.create_builder();
        tmp.position_at_end(entry);
        let a = func.get_nth_param(0).unwrap().into_pointer_value();
        let alen = func.get_nth_param(1).unwrap().into_int_value();
        let b = func.get_nth_param(2).unwrap().into_pointer_value();
        let blen = func.get_nth_param(3).unwrap().into_int_value();
        let len = tmp.build_int_add(alen, blen, "len")?;
        let total = tmp.build_int_add(len, i64t.const_int(9, false), "total")?;
        let call = tmp.build_call(self.get_lexora_alloc()?, &[total.into()], "blk")?;
        let blk = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_pointer_value(),
            ValueKind::Instruction(_) => unreachable!("lexora_alloc ptr dondurur"),
        };

        tmp.build_store(blk, len)?;
        let data =
            unsafe { tmp.build_in_bounds_gep(i8t, blk, &[i64t.const_int(8, false)], "data")? };
        tmp.build_call(self.get_strcpy(), &[data.into(), a.into()], "")?;
        let dst = unsafe { tmp.build_in_bounds_gep(i8t, data, &[alen], "dst")? };
        tmp.build_call(self.get_strcpy(), &[dst.into(), b.into()], "")?;
        tmp.build_return(Some(&data))?;
        Ok(func)
    }
    fn get_strcmp(&self) -> FunctionValue<'ctx> {
        if let Some(f) = self.module.get_function("strcmp") {
            return f;
        }
        let i32t = self.context.i32_type();
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        self.module.add_function(
            "strcmp",
            i32t.fn_type(&[ptr_t.into(), ptr_t.into()], false),
            None,
        )
    }
    fn get_lexora_str_eq(&self) -> Result<FunctionValue<'ctx>, BuilderError> {
        if let Some(f) = self.module.get_function("lexora_str_eq") {
            return Ok(f);
        }
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let i64t = self.context.i64_type();
        let i1 = self.context.bool_type();
        let func = self.module.add_function(
            "lexora_str_eq",
            i1.fn_type(
                &[ptr_t.into(), i64t.into(), ptr_t.into(), i64t.into()],
                false,
            ),
            None,
        );
        let entry = self.context.append_basic_block(func, "entry");
        let diff = self.context.append_basic_block(func, "diff");
        let same = self.context.append_basic_block(func, "same");
        let tmp = self.context.create_builder();
        tmp.position_at_end(entry);
        let a = func.get_nth_param(0).unwrap().into_pointer_value();
        let alen = func.get_nth_param(1).unwrap().into_int_value();
        let b = func.get_nth_param(2).unwrap().into_pointer_value();
        let blen = func.get_nth_param(3).unwrap().into_int_value();
        let ne = tmp.build_int_compare(IntPredicate::NE, alen, blen, "nelen")?;
        tmp.build_conditional_branch(ne, diff, same)?;

        tmp.position_at_end(diff);
        tmp.build_return(Some(&i1.const_zero()))?;

        tmp.position_at_end(same);
        let call = tmp.build_call(self.get_strcmp(), &[a.into(), b.into()], "c")?;
        let c = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_int_value(),
            ValueKind::Instruction(_) => unreachable!("strcmp i32 dondurur"),
        };
        let eq = tmp.build_int_compare(
            IntPredicate::EQ,
            c,
            self.context.i32_type().const_zero(),
            "eq",
        )?;
        tmp.build_return(Some(&eq))?;
        Ok(func)
    }
    fn str_len_of(
        &mut self,
        expr: &Expr,
        val: PointerValue<'ctx>,
    ) -> Result<IntValue<'ctx>, BuilderError> {
        let i64t = self.context.i64_type();
        let i8t = self.context.i8_type();
        match self.types[&expr.id()] {
            Type::String => {
                let blk = unsafe {
                    self.builder.build_in_bounds_gep(
                        i8t,
                        val,
                        &[i64t.const_int((-8i64) as u64, true)],
                        "lenblk",
                    )?
                };
                Ok(self.builder.build_load(i64t, blk, "len")?.into_int_value())
            }
            _ => {
                let call = self
                    .builder
                    .build_call(self.get_strlen(), &[val.into()], "len")?;
                match call.try_as_basic_value() {
                    ValueKind::Basic(v) => Ok(v.into_int_value()),
                    ValueKind::Instruction(_) => unreachable!("strlen i64 dondurur"),
                }
            }
        }
    }
    fn gen_str_concat(
        &mut self,
        left: &Expr,
        right: &Expr,
    ) -> Result<BasicValueEnum<'ctx>, BuilderError> {
        let lv = self.gen_expr(left)?.into_pointer_value();
        let llen = self.str_len_of(left, lv)?;
        let rv = self.gen_expr(right)?.into_pointer_value();
        let rlen = self.str_len_of(right, rv)?;
        let f = self.get_lexora_str_concat()?;
        let call = self.builder.build_call(
            f,
            &[lv.into(), llen.into(), rv.into(), rlen.into()],
            "concat",
        )?;
        let res = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v,
            ValueKind::Instruction(_) => unreachable!("lexora_str_concat ptr dondurur"),
        };
        if matches!(self.types[&left.id()], Type::String) {
            self.builder
                .build_call(self.get_lexora_str_free()?, &[lv.into()], "")?;
        }
        if matches!(self.types[&right.id()], Type::String) {
            self.builder
                .build_call(self.get_lexora_str_free()?, &[rv.into()], "")?;
        }
        Ok(res)
    }

    fn str_temp_free(&mut self, expr: &Expr, val: PointerValue<'ctx>) -> Result<(), BuilderError> {
        if matches!(self.types[&expr.id()], Type::String) && !expr.is_place() {
            self.builder
                .build_call(self.get_lexora_str_free()?, &[val.into()], "")?;
        }
        Ok(())
    }
    fn gen_str_cmp(
        &mut self,
        left: &Expr,
        op: &BinaryOperator,
        right: &Expr,
    ) -> Result<BasicValueEnum<'ctx>, BuilderError> {
        let lv = self.gen_expr(left)?.into_pointer_value();
        let rv = self.gen_expr(right)?.into_pointer_value();
        let res = match op {
            BinaryOperator::Eq | BinaryOperator::NotEq => {
                let llen = self.str_len_of(left, lv)?;
                let rlen = self.str_len_of(right, rv)?;
                let f = self.get_lexora_str_eq()?;
                let call = self.builder.build_call(
                    f,
                    &[lv.into(), llen.into(), rv.into(), rlen.into()],
                    "streq",
                )?;
                let eq = match call.try_as_basic_value() {
                    ValueKind::Basic(v) => v.into_int_value(),
                    ValueKind::Instruction(_) => unreachable!("lexora_str_eq i1 dondurur"),
                };
                if *op == BinaryOperator::Eq {
                    eq
                } else {
                    self.builder.build_not(eq, "strne")?
                }
            }
            _ => {
                let call = self.builder.build_call(
                    self.get_strcmp(),
                    &[lv.into(), rv.into()],
                    "strcmp",
                )?;
                let c = match call.try_as_basic_value() {
                    ValueKind::Basic(v) => v.into_int_value(),
                    ValueKind::Instruction(_) => unreachable!("strcmp i32 dondurur"),
                };
                let pred = match op {
                    BinaryOperator::Less => IntPredicate::SLT,
                    BinaryOperator::Greater => IntPredicate::SGT,
                    BinaryOperator::LessEq => IntPredicate::SLE,
                    _ => IntPredicate::SGE,
                };
                self.builder.build_int_compare(
                    pred,
                    c,
                    self.context.i32_type().const_zero(),
                    "strord",
                )?
            }
        };
        self.str_temp_free(left, lv)?;
        self.str_temp_free(right, rv)?;
        Ok(res.into())
    }

    fn build_box(
        &self,
        pointee: BasicTypeEnum<'ctx>,
        val: BasicValueEnum<'ctx>,
    ) -> Result<PointerValue<'ctx>, BuilderError> {
        let size = pointee.size_of().expect("box icin sized tip lazim");
        let alloc = self.get_lexora_alloc()?;
        let call = self.builder.build_call(alloc, &[size.into()], "box_raw")?;
        let raw = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_pointer_value(),
            ValueKind::Instruction(_) => unreachable!("lexora_alloc ptr dondurur"),
        };
        self.builder.build_store(raw, val)?;
        Ok(raw)
    }
    fn build_free(&self, ptr: PointerValue<'ctx>) -> Result<(), BuilderError> {
        let free = self.get_lexora_free()?;
        self.builder.build_call(free, &[ptr.into()], "")?;
        Ok(())
    }

    fn drop_enter(&mut self) {
        self.drop_scopes.push(Vec::new());
    }
    fn drop_exit(&mut self) -> Result<(), BuilderError> {
        if let Some(scope) = self.drop_scopes.pop() {
            let terminated = self
                .builder
                .get_insert_block()
                .map(|b| b.get_terminator().is_some())
                .unwrap_or(true);
            if !terminated {
                for d in scope.iter().rev() {
                    self.build_drop(d.slot, d.flag, &d.pointee)?;
                }
            }
        }
        Ok(())
    }
    fn free_all_live(&mut self) -> Result<(), BuilderError> {
        let scopes: Vec<Vec<DropLocal>> = self.drop_scopes.clone();
        for scope in scopes.iter().rev() {
            for d in scope.iter().rev() {
                self.build_drop(d.slot, d.flag, &d.pointee)?;
            }
        }
        Ok(())
    }
    fn find_flag(&self, sym: Symbol) -> Option<PointerValue<'ctx>> {
        for scope in self.drop_scopes.iter().rev() {
            for d in scope.iter().rev() {
                if d.sym == sym {
                    return Some(d.flag);
                }
            }
        }
        None
    }
    fn clear_flag_on_move(&mut self, id: ExprId, sym: Symbol) -> Result<(), BuilderError> {
        if self.moves.contains(&id) {
            if let Some(flag) = self.find_flag(sym) {
                self.builder
                    .build_store(flag, self.context.bool_type().const_zero())?;
            }
        }
        Ok(())
    }
    fn build_drop(
        &self,
        slot: PointerValue<'ctx>,
        flag: PointerValue<'ctx>,
        ty: &Type,
    ) -> Result<(), BuilderError> {
        let function = self.cur_fn.unwrap();
        let i1 = self.context.bool_type();
        let f = self
            .builder
            .build_load(i1, flag, "dropflag")?
            .into_int_value();
        let do_bb = self.context.append_basic_block(function, "drop_do");
        let skip_bb = self.context.append_basic_block(function, "drop_skip");
        self.builder.build_conditional_branch(f, do_bb, skip_bb)?;

        self.builder.position_at_end(do_bb);
        self.drop_in_place(slot, ty)?;
        self.builder.build_store(flag, i1.const_zero())?;
        self.builder.build_unconditional_branch(skip_bb)?;
        self.builder.position_at_end(skip_bb);
        Ok(())
    }
    fn drop_in_place(&self, ptr: PointerValue<'ctx>, ty: &Type) -> Result<(), BuilderError> {
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        match ty {
            Type::Box(inner) => {
                let p = self
                    .builder
                    .build_load(ptr_t, ptr, "boxptr")?
                    .into_pointer_value();
                self.emit_drop_glue(p, inner)?;
            }
            Type::String => {
                let p = self
                    .builder
                    .build_load(ptr_t, ptr, "strptr")?
                    .into_pointer_value();
                self.builder
                    .build_call(self.get_lexora_str_free()?, &[p.into()], "")?;
            }
            Type::Enum(e, args) => {
                self.builder
                    .build_call(self.drop_fn(&(*e, args.clone())), &[ptr.into()], "")?;
            }
            Type::Struct(s) => {
                let st = self.structs.get(s).unwrap().0;
                let fields = self.struct_fields_ast.get(s).unwrap().clone();
                for (i, (_, fty)) in fields.iter().enumerate() {
                    if !self.is_owning_type(fty) {
                        continue;
                    }
                    let fptr = self.builder.build_struct_gep(st, ptr, i as u32, "dropfld")?;
                    self.drop_in_place(fptr, fty)?;
                }
            }
            Type::Array(elem, n) => {
                if !self.is_owning_type(elem) {
                    return Ok(());
                }
                let arr_ty = self.llvm_type(ty).into_array_type();
                let zero = self.context.i32_type().const_zero();
                for i in 0..*n {
                    let idx = self.context.i32_type().const_int(i as u64, false);
                    let eptr = unsafe {
                        self.builder
                            .build_in_bounds_gep(arr_ty, ptr, &[zero, idx], "dropelem")?
                    };
                    self.drop_in_place(eptr, elem)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
    fn emit_drop_glue(&self, ptr: PointerValue<'ctx>, pointee: &Type) -> Result<(), BuilderError> {
        self.drop_in_place(ptr, pointee)?;
        self.build_free(ptr)?;
        Ok(())
    }
    fn emit_enum_glue(
        &self,
        slot: PointerValue<'ctx>,
        key: &(Symbol, Vec<Type>),
    ) -> Result<(), BuilderError> {
        let function = self.cur_fn.unwrap();
        let enum_ty = *self.enum_types.get(key).unwrap();
        let i32t = self.context.i32_type();
        let tag_ptr = self.builder.build_struct_gep(enum_ty, slot, 0, "tag")?;
        let tag = self
            .builder
            .build_load(i32t, tag_ptr, "tagval")?
            .into_int_value();

        let end_bb = self.context.append_basic_block(function, "edrop_end");
        let variants = self.enums.get(key).unwrap().clone();

        let mut cases: Vec<(IntValue, BasicBlock)> = Vec::new();
        let mut case_data: Vec<(Symbol, Vec<(usize, Type)>, BasicBlock)> = Vec::new();
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
            let bb = self.context.append_basic_block(function, "edrop_case");
            cases.push((i32t.const_int(idx, false), bb));
            case_data.push((*variant, owning, bb));
        }
        self.builder.build_switch(tag, end_bb, &cases)?;

        for (variant, owning, bb) in case_data.iter() {
            self.builder.position_at_end(*bb);
            let vt = *self
                .variant_types
                .get(&(key.0, key.1.clone(), *variant))
                .unwrap();
            let payload_ptr = self.builder.build_struct_gep(enum_ty, slot, 1, "payload")?;
            for (i, fty) in owning.iter() {
                let fptr = self
                    .builder
                    .build_struct_gep(vt, payload_ptr, *i as u32, "vfld")?;
                self.drop_in_place(fptr, fty)?;
            }
            self.builder.build_unconditional_branch(end_bb)?;
        }
        self.builder.position_at_end(end_bb);
        Ok(())
    }
    fn gen_drop_functions(&mut self, program: &Program) -> Result<(), BuilderError> {
        let void_t = self.context.void_type();
        let ptr_t = self.context.ptr_type(AddressSpace::default());
        let mut keys: Vec<(Symbol, Vec<Type>)> = program
            .enums
            .iter()
            .filter(|e| e.params.is_empty())
            .map(|e| (e.name, Vec::new()))
            .collect();
        let mut inst_keys: Vec<(Symbol, Vec<Type>)> = self.instances.keys().cloned().collect();
        inst_keys.sort_by_key(|(s, a)| Self::inst_name(*s, a));
        keys.extend(inst_keys);
        for key in &keys {
            if self.enum_is_owning(key) {
                let fn_type = void_t.fn_type(&[ptr_t.into()], false);
                self.module.add_function(
                    &format!("drop.enum.{}", Self::inst_name(key.0, &key.1)),
                    fn_type,
                    None,
                );
            }
        }
        for key in &keys {
            if self.enum_is_owning(key) {
                let func = self.drop_fn(key);
                let entry = self.context.append_basic_block(func, "entry");
                self.builder.position_at_end(entry);
                self.cur_fn = Some(func);
                let slot = func.get_nth_param(0).unwrap().into_pointer_value();
                self.emit_enum_glue(slot, key)?;
                self.builder.build_return(None)?;
            }
        }
        self.cur_fn = None;
        Ok(())
    }

    fn drop_fn(&self, key: &(Symbol, Vec<Type>)) -> FunctionValue<'ctx> {
        self.module
            .get_function(&format!("drop.enum.{}", Self::inst_name(key.0, &key.1)))
            .unwrap()
    }
    fn bounds_check(&self, idx: IntValue<'ctx>, len: u32) -> Result<(), BuilderError> {
        let function = self.cur_fn.unwrap();
        let size = self.context.i32_type().const_int(len as u64, false);
        let oob = self
            .builder
            .build_int_compare(IntPredicate::UGE, idx, size, "oob")?;
        let panic_bb = self.context.append_basic_block(function, "idx_panic");
        let ok_bb = self.context.append_basic_block(function, "idx_ok");
        self.builder
            .build_conditional_branch(oob, panic_bb, ok_bb)?;
        self.builder.position_at_end(panic_bb);
        let panic_fn = self.get_or_build_panic()?;
        let msg = self.fmt_global(".panicmsg_idx", "lexora: index out of bounds")?;
        self.builder.build_call(panic_fn, &[msg.into()], "")?;
        self.builder.build_unreachable()?;
        self.builder.position_at_end(ok_bb);
        Ok(())
    }

    fn checked_arith(
        &self,
        name: &str,
        l: IntValue<'ctx>,
        r: IntValue<'ctx>,
    ) -> Result<IntValue<'ctx>, BuilderError> {
        let function = self.cur_fn.unwrap();
        let int_ty = l.get_type();
        let intrinsic = Intrinsic::find(name).expect("intrinsic bulunamadi");
        let decl = intrinsic
            .get_declaration(&self.module, &[int_ty.into()])
            .expect("intrinsic decl alinamadi");
        let call = self
            .builder
            .build_call(decl, &[l.into(), r.into()], "ovf_call")?;
        let agg = match call.try_as_basic_value() {
            ValueKind::Basic(v) => v.into_struct_value(),
            ValueKind::Instruction(_) => unreachable!("overflow intrinsic struct dondurur"),
        };
        let res = self
            .builder
            .build_extract_value(agg, 0, "res")?
            .into_int_value();
        let ovc = self
            .builder
            .build_extract_value(agg, 1, "ovc")?
            .into_int_value();
        let panic_bb = self.context.append_basic_block(function, "ovf_panic");
        let ok_bb = self.context.append_basic_block(function, "ovf_ok");
        self.builder
            .build_conditional_branch(ovc, panic_bb, ok_bb)?;
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
        self.module
            .set_data_layout(&tm.get_target_data().get_data_layout());
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

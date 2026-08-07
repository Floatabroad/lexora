use crate::ast::*;
use crate::error::LexoraError;
use crate::span::Span;
use crate::symbol::{Interner, Symbol};
use std::collections::{HashMap, HashSet};

const MAX_ARRAY_ELEMS: usize = 16384;

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
    structs: HashMap<Symbol, Vec<(Symbol, Type)>>,
    interner: &'i Interner,
    pub types: HashMap<ExprId, Type>,
    enums: HashMap<Symbol, (Vec<Symbol>, Vec<(Symbol, Vec<Type>)>)>,
    pub instances: HashMap<(Symbol, Vec<Type>), Vec<(Symbol, Vec<Type>)>>,
    fn_defs: HashMap<Symbol, (Vec<Symbol>, Vec<Type>, Type)>,
    pub fn_instances: HashMap<(Symbol, Vec<Type>), (Vec<Type>, Type)>,
    methods: HashMap<(Symbol, Symbol), (Vec<Type>, Type)>,
    pub call_targs: HashMap<ExprId, Vec<Type>>,
    errors: Vec<LexoraError>,
    loop_vars: Vec<Symbol>,
    cur_ret: Type,
}

impl<'i> TypeChecker<'i> {
    pub fn new(interner: &'i Interner) -> Self {
        TypeChecker {
            variables: SymbolTable::new(),
            functions: HashMap::new(),
            structs: HashMap::new(),
            interner,
            types: HashMap::new(),
            enums: HashMap::new(),
            instances: HashMap::new(),
            fn_defs: HashMap::new(),
            fn_instances: HashMap::new(),
            methods: HashMap::new(),
            call_targs: HashMap::new(),
            errors: Vec::new(),
            loop_vars: Vec::new(),
            cur_ret: Type::Void,
        }
    }
    fn resolve(&self, sym: Symbol) -> String {
        self.interner.resolve(sym).to_string()
    }
    pub fn check_program<'arena>(
        &mut self,
        program: &Program<'arena>,
    ) -> Result<(), Vec<LexoraError>> {
        let mut type_names: HashMap<Symbol, ()> = HashMap::new();
        for (name, span) in program
            .structs
            .iter()
            .map(|s| (s.name, s.span))
            .chain(program.enums.iter().map(|e| (e.name, e.span)))
        {
            if type_names.insert(name, ()).is_some() {
                self.errors.push(LexoraError::AlreadyDefined {
                    name: self.resolve(name),
                    span,
                });
            }
        }
        let mut fn_names: HashMap<Symbol, ()> = HashMap::new();

        for func in &program.functions {
            if fn_names.insert(func.name, ()).is_some() {
                self.errors.push(LexoraError::AlreadyDefined {
                    name: self.resolve(func.name),
                    span: func.span,
                });
            }
        }
        match program
            .functions
            .iter()
            .find(|f| self.resolve(f.name) == "main")
        {
            Some(m) => {
                if !m.params.is_empty() || !matches!(m.return_type, Type::I32) {
                    self.errors.push(LexoraError::Custom {
                        message: "'main' imzasi 'fn main() -> i32' olmali".to_string(),
                        span: m.span,
                    });
                }
            }
            None => self.errors.push(LexoraError::Custom {
                message: "'main' fonksiyonu bulunamadi; giris noktasi 'fn main() -> i32' olmali"
                    .to_string(),
                span: Span::default(),
            }),
        }
        for e in &program.enums {
            self.enums
                .entry(e.name)
                .or_insert_with(|| (e.params.clone(), e.variants.clone()));
        }
        for s in &program.structs {
            self.structs
                .entry(s.name)
                .or_insert_with(|| s.fields.clone());
        }
        for e in &program.enums {
            let generic = !e.params.is_empty();
            let mut seen_variants: HashMap<Symbol, ()> = HashMap::new();
            for (v, _) in &e.variants {
                if seen_variants.insert(*v, ()).is_some() {
                    self.errors.push(LexoraError::AlreadyDefined {
                        name: format!("{}::{}", self.resolve(e.name), self.resolve(*v)),
                        span: e.span,
                    });
                }
            }
            for (vi, (v, ftys)) in e.variants.iter().enumerate() {
                for (fi, t) in ftys.iter().enumerate() {
                    let sp = e
                        .variant_field_spans
                        .get(vi)
                        .and_then(|v| v.get(fi))
                        .copied()
                        .unwrap_or(e.span);
                    self.validate_type(t, sp);
                    let ok = match t {
                        Type::I32
                        | Type::I64
                        | Type::F32
                        | Type::F64
                        | Type::Bool
                        | Type::Str
                        | Type::String
                        | Type::Box(_) => true,
                        Type::Param(_) => generic,
                        _ => false,
                    };
                    if !ok {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'{}::{}' alani icin desteklenmeyen tip: {} — enum alani skaler (i32/i64/f32/f64/bool/str/String) veya Box<T> olabilir",
                                self.resolve(e.name), self.resolve(*v), self.format_type(t)
                            ),
                            span: sp,
                        });
                    }
                }
            }
        }
        for s in &program.structs {
            let mut seen_fields: HashMap<Symbol, ()> = HashMap::new();
            for (f, _) in &s.fields {
                if seen_fields.insert(*f, ()).is_some() {
                    self.errors.push(LexoraError::AlreadyDefined {
                        name: format!("{}.{}", self.resolve(s.name), self.resolve(*f)),
                        span: s.span,
                    });
                }
            }
            for (i, (_, t)) in s.fields.iter().enumerate() {
                let sp = s.field_spans.get(i).copied().unwrap_or(s.span);
                self.validate_type(t, sp);
            }
        }

        for s in &program.structs {
            for (i, (f, t)) in s.fields.iter().enumerate() {
                let sp = s.field_spans.get(i).copied().unwrap_or(s.span);
                let mut seen: HashSet<Symbol> = HashSet::from([s.name]);
                if self.struct_cycle(s.name, t, &mut seen) {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}.{}' alani struct'i kendine dahil ediyor (sonsuz boyut); dolayim icin Box<{}> kullanin",
                            self.resolve(s.name), self.resolve(*f), self.resolve(s.name)
                        ),
                        span: sp,
                    });
                    continue;
                }
                let mut seen: HashSet<Symbol> = HashSet::from([s.name]);
                if self.drop_cycle(s.name, t, &mut seen) {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}.{}' alani struct'i Box/dizi dolayimiyla kendine dahil ediyor; boyle bir degerin tabani olamaz ve drop kodu sonsuz aciliyor — zinciri sonlandirmak icin araya bir enum koyun, ornegin Opt<Box<{}>>",
                            self.resolve(s.name), self.resolve(*f), self.resolve(s.name)
                        ),
                        span: sp,
                    });
                }
            }
        }
        for func in &program.functions {
            let fname = self.resolve(func.name);
            if matches!(fname.as_str(), "print" | "string" | "len") {
                self.errors.push(LexoraError::Custom {
                    message: format!(
                        "'{}' yerlesik bir fonksiyon; ayni isimde tanim yapilamaz",
                        fname
                    ),
                    span: func.span,
                });
            }
            if matches!(
                fname.as_str(),
                "printf"
                    | "exit"
                    | "malloc"
                    | "free"
                    | "strlen"
                    | "strcpy"
                    | "strcmp"
                    | "lexora_panic"
                    | "lexora_alloc"
                    | "lexora_free"
                    | "lexora_str_new"
                    | "lexora_str_free"
                    | "lexora_str_concat"
                    | "lexora_str_eq"
            ) {
                self.errors.push(LexoraError::Custom {
                    message: format!(
                        "'{}' Lexora runtime'ina ait bir sembol; ayni isimde tanim yapilamaz",
                        fname
                    ),
                    span: func.span,
                });
            }
            if self.structs.contains_key(&func.name) || self.enums.contains_key(&func.name) {
                self.errors.push(LexoraError::Custom {
                     message: format!(
                        "'{}' bir tip adi; ayni isimde fonksiyon tanimlanamaz (uretilen sembol isimleri carpisir)", fname
                     ),
                    span:func.span,
                });
            }
            let param_types: Vec<Type> = func
                .params
                .iter()
                .enumerate()
                .map(|(i, (_, t))| {
                    let sp = func.param_spans.get(i).copied().unwrap_or(func.span);
                    if self.validate_type(t, sp) {
                        t.clone()
                    } else {
                        Type::Error
                    }
                })
                .collect();
            let ret_ty = if self.validate_type(&func.return_type, func.ret_span) {
                func.return_type.clone()
            } else {
                Type::Error
            };
            if func.type_params.is_empty() {
                self.functions
                    .entry(func.name)
                    .or_insert_with(|| (param_types, ret_ty));
            } else {
                self.fn_defs
                    .entry(func.name)
                    .or_insert_with(|| (func.type_params.clone(), param_types, ret_ty));
            }
        }
        for imp in &program.impls {
            let tname = self.resolve(imp.type_name);
            let is_struct = self.structs.contains_key(&imp.type_name);
            let generic_enum = match self.enums.get(&imp.type_name) {
                Some((p, _)) => Some(!p.is_empty()),
                None => None,
            };
            if !is_struct && generic_enum.is_none() {
                self.errors.push(LexoraError::Custom {
                    message: format!(
                        "tanimsiz tip '{}'; impl yalnizca tanimli struct ve enum'lara yazilabilir",
                        tname
                    ),
                    span: imp.type_span,
                });
                continue;
            }
            if generic_enum == Some(true) {
                self.errors.push(LexoraError::Custom {
                    message: format!("'{}' generic; generic tipe impl henuz yazilamaz", tname),
                    span: imp.type_span,
                });
                continue;
            }
            for m in &imp.methods {
                if !m.type_params.is_empty() {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}.{}' kendi tip parametresini alamaz; metot generic'leri henuz yok",
                            tname,
                            self.resolve(m.name)
                        ),
                        span: m.span,
                    });
                    continue;
                }
                if !m.self_param {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}.{}' ilk parametresi 'self' olmali; statik metotlar henuz yok",
                            tname,
                            self.resolve(m.name)
                        ),
                        span: m.span,
                    });
                    continue;
                }
                let param_types: Vec<Type> = m
                    .params
                    .iter()
                    .enumerate()
                    .map(|(i, (_, t))| {
                        if i == 0 {
                            return t.clone();
                        }
                        let sp = m.param_spans.get(i).copied().unwrap_or(m.span);
                        if self.validate_type(t, sp) {
                            t.clone()
                        } else {
                            Type::Error
                        }
                    })
                    .collect();
                let ret_ty = if self.validate_type(&m.return_type, m.ret_span) {
                    m.return_type.clone()
                } else {
                    Type::Error
                };
                if self
                    .methods
                    .insert((imp.type_name, m.name), (param_types, ret_ty))
                    .is_some()
                {
                    self.errors.push(LexoraError::AlreadyDefined {
                        name: format!("{}.{}", tname, self.resolve(m.name)),
                        span: m.span,
                    });
                }
            }
        }
        for func in &program.functions {
            self.check_function(func, None);
        }
        for imp in &program.impls {
            for m in &imp.methods {
                self.check_function(m, Some(imp.type_name));
            }
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
                .chain(self.fn_defs.keys())
                .map(|sym| self.interner.resolve(*sym))
                .chain(std::iter::once("print"))
                .chain(std::iter::once("string"))
                .chain(std::iter::once("len")),
        )
    }
    fn nearest_method(&self, ty: Symbol, target: Symbol) -> Option<String> {
        let name = self.interner.resolve(target);
        let cands: Vec<&str> = self
            .methods
            .keys()
            .filter(|(t, _)| *t == ty)
            .map(|(_, m)| self.interner.resolve(*m))
            .collect();
        crate::suggest::nearest(name, cands)
    }
    fn nearest_field(&self, target: Symbol, fields: &[(Symbol, Type)]) -> Option<String> {
        let name = self.interner.resolve(target);
        crate::suggest::nearest(name, fields.iter().map(|(n, _)| self.interner.resolve(*n)))
    }
    fn enum_variants_for(&self, sym: Symbol, args: &[Type]) -> Option<Vec<(Symbol, Vec<Type>)>> {
        let (params, variants) = self.enums.get(&sym)?;
        if params.is_empty() || args.is_empty() {
            return Some(variants.clone());
        }
        let map: HashMap<Symbol, Type> = params.iter().copied().zip(args.iter().cloned()).collect();
        Some(
            variants
                .iter()
                .map(|(v, ftys)| (*v, ftys.iter().map(|t| t.substitute(&map)).collect()))
                .collect(),
        )
    }
    fn result_parts(&self, sym: Symbol, args: &[Type]) -> Option<(Type, Type)> {
        let variants = self.enum_variants_for(sym, args)?;
        if variants.len() != 2 {
            return None;
        }
        let ok = variants
            .iter()
            .find(|(v, _)| self.resolve(*v) == "Ok")?
            .1
            .clone();
        let err = variants
            .iter()
            .find(|(v, _)| self.resolve(*v) == "Err")?
            .1
            .clone();
        if ok.len() != 1 || err.len() != 1 {
            return None;
        }
        Some((ok[0].clone(), err[0].clone()))
    }

    fn drop_cycle(&self, root: Symbol, ty: &Type, seen: &mut HashSet<Symbol>) -> bool {
        match ty {
            Type::Struct(s) => {
                if *s == root {
                    return true;
                }
                if !seen.insert(*s) {
                    return false;
                }
                match self.structs.get(s) {
                    Some(fields) => fields.iter().any(|(_, t)| self.drop_cycle(root, t, seen)),
                    None => false,
                }
            }
            Type::Box(inner) => self.drop_cycle(root, inner, seen),
            Type::Array(elem, _) => self.drop_cycle(root, elem, seen),
            _ => false,
        }
    }
    fn struct_cycle(&self, root: Symbol, ty: &Type, seen: &mut HashSet<Symbol>) -> bool {
        match ty {
            Type::Struct(s) => {
                if *s == root {
                    return true;
                }
                if !seen.insert(*s) {
                    return false;
                }
                match self.structs.get(s) {
                    Some(fields) => fields.iter().any(|(_, t)| self.struct_cycle(root, t, seen)),
                    None => false,
                }
            }
            Type::Array(elem, _) => self.struct_cycle(root, elem, seen),
            _ => false,
        }
    }
    fn type_arg_arity_error(&mut self, name: Symbol, given: usize, expected: usize, span: Span) {
        let message = if expected == 0 {
            format!("'{}' generic degil, tip argumani almaz", self.resolve(name))
        } else {
            format!(
                "'{}' {} tip parametresi bekliyor, {} verildi",
                self.resolve(name),
                expected,
                given
            )
        };
        self.errors.push(LexoraError::Custom { message, span });
    }
    fn validate_type(&mut self, ty: &Type, span: Span) -> bool {
        match ty {
            Type::Array(elem, _) => {
                let total = array_elems(ty);
                if total.map_or(true, |t| t > MAX_ARRAY_ELEMS) {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "dizi cok buyuk: toplam {} eleman (sinir {}) — absurt boyutta dizi derleyiciyi cokertiyor, bu bir politika siniri",
                            match total {
                                Some(t) => t.to_string(),
                                None => "asiri".to_string(),
                            },
                            MAX_ARRAY_ELEMS
                        ),
                        span,
                    });
                    return false;
                }
                self.validate_type(elem, span)
            }
            Type::Box(inner) => self.validate_type(inner, span),
            Type::Struct(s) => {
                if self.structs.contains_key(s) {
                    return true;
                }
                if self.enums.contains_key(s) {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}' bir enum ama tip pozisyonunda struct sanildi; enum, tip olarak kullanilmadan once ve ayni dosyada tanimli olmali",
                            self.resolve(*s)
                        ),
                        span,
                    });
                    return false;
                }
                let name = self.resolve(*s);
                let suggestion = {
                    let names: Vec<&str> = self
                        .structs
                        .keys()
                        .chain(self.enums.keys())
                        .map(|k| self.interner.resolve(*k))
                        .collect();
                    crate::suggest::nearest(&name, names)
                };
                let message = match suggestion {
                    Some(near) => {
                        format!(
                            "tanimsiz tip '{}'; bunu mu demek istediniz: '{}'?",
                            name, near
                        )
                    }

                    None => format!("tanimsiz tip '{}'", name),
                };
                self.errors.push(LexoraError::Custom { message, span });
                false
            }
            Type::Enum(sym, args) => {
                let mut ok = true;
                for a in args {
                    if !self.validate_type(a, span) {
                        ok = false;
                    }
                }
                let params_len = match self.enums.get(sym) {
                    Some((p, _)) => p.len(),
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'{}' bir enum degil; tip argumani yalnizca generic enum alir",
                                self.resolve(*sym)
                            ),
                            span,
                        });
                        return false;
                    }
                };
                if args.len() != params_len {
                    self.type_arg_arity_error(*sym, args.len(), params_len, span);
                    return false;
                }
                if ok && !args.is_empty() && args.iter().all(type_is_concrete) {
                    self.check_instantiation(*sym, args, span);
                }
                ok
            }
            _ => true,
        }
    }
    fn check_instantiation(&mut self, sym: Symbol, args: &[Type], span: Span) {
        let key = (sym, args.to_vec());
        if self.instances.contains_key(&key) {
            return;
        }
        let variants = match self.enum_variants_for(sym, args) {
            Some(v) => v,
            None => return,
        };
        self.instances.insert(key, variants.clone());
        let inst_name = self.format_type(&Type::Enum(sym, args.to_vec()));
        for (v, ftys) in &variants {
            for t in ftys {
                match t {
                    Type::I32 | Type::I64 |Type::F32| Type::F64| Type::Bool | Type::Str | Type::String | Type::Box(_) => {}

                    _ => self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}::{}' alani icin desteklenmeyen tip: {} — enum alani skaler (i32/i64/f32/f64/bool/str/String) veya Box<T> olabilir",
                            inst_name, self.resolve(*v), self.format_type(t)
                        ),
                        span,
                    }),
                }
                self.validate_type(t, span);
            }
        }
    }
    fn check_fn_instantiation(
        &mut self,
        name: Symbol,
        targs: Vec<Type>,
        ptys: &[Type],
        rty: &Type,
        span: Span,
    ) -> bool {
        let key = (name, targs);
        if self.fn_instances.contains_key(&key) {
            return true;
        }
        let mut ok = true;
        for t in ptys {
            if !self.validate_type(t, span) {
                ok = false;
            }
        }
        if !self.validate_type(rty, span) {
            ok = false;
        }
        if !ok {
            return false;
        }
        self.fn_instances.insert(key, (ptys.to_vec(), rty.clone()));
        true
    }
    fn check_function<'arena>(&mut self, func: &Function<'arena>, owner: Option<Symbol>) {
        let (param_tys, ret_ty) = match owner {
            Some(t) => match self.methods.get(&(t, func.name)) {
                Some((p, r)) => (p.clone(), r.clone()),
                None => (
                    func.params.iter().map(|(_, t)| t.clone()).collect(),
                    func.return_type.clone(),
                ),
            },

            None => match self.functions.get(&func.name) {
                Some((p, r)) => (p.clone(), r.clone()),
                None => match self.fn_defs.get(&func.name) {
                    Some((_, p, r)) => (p.clone(), r.clone()),
                    None => (
                        func.params.iter().map(|(_, t)| t.clone()).collect(),
                        func.return_type.clone(),
                    ),
                },
            },
        };
        self.cur_ret = ret_ty.clone();
        self.variables.enter_scope();
        for (i, (sym, _)) in func.params.iter().enumerate() {
            let ty = param_tys.get(i).cloned().unwrap_or(Type::Error);
            if !self.variables.define(*sym, ty) {
                self.errors.push(LexoraError::AlreadyDefined {
                    name: self.resolve(*sym),
                    span: func.span,
                });
            }
        }
        for stmt in func.body.stmts.iter() {
            self.check_statement(stmt, &ret_ty);
        }
        if let Some(tail) = func.body.tail {
            let tail_ty = self.check_expr_ex(tail, Some(&ret_ty));
            if !types_match(&ret_ty, &tail_ty) {
                self.errors.push(LexoraError::TypeMismatch {
                    expected: self.format_type(&ret_ty),
                    found: self.format_type(&tail_ty),
                    span: tail.span(),
                });
            }
        }
        if !matches!(func.return_type, Type::Void) && !block_always_returns(&func.body) {
            self.errors.push(LexoraError::Custom {
                message: format!(
                    "'{}' fonksiyonu {} donduruyor ama tum yollar deger dondurmuyor",
                    self.resolve(func.name),
                    self.format_type(&func.return_type)
                ),
                span: func.span,
            });
        }
        self.variables.exit_scope();
    }

    fn format_type(&self, ty: &Type) -> String {
        match ty {
            Type::I32 => "i32".to_string(),
            Type::I64 => "i64".to_string(),
            Type::F32 => "f32".to_string(),
            Type::F64 => "f64".to_string(),
            Type::Bool => "bool".to_string(),
            Type::Void => "void".to_string(),
            Type::Str => "str".to_string(),
            Type::Array(elem, n) => format!("[{}; {}]", self.format_type(elem), n),
            Type::Struct(s) => self.resolve(*s),
            Type::Box(inner) => format!("Box<{}>", self.format_type(inner)),
            Type::String => "String".to_string(),
            Type::Enum(s, args) => {
                if args.is_empty() {
                    self.resolve(*s)
                } else {
                    let a: Vec<String> = args.iter().map(|t| self.format_type(t)).collect();
                    format!("{}<{}>", self.resolve(*s), a.join(", "))
                }
            }
            Type::Param(p) => self.resolve(*p),
            Type::Error => "<hata>".to_string(),
        }
    }
    fn check_statement<'arena>(&mut self, stmt: &Stmt<'arena>, ret_ty: &Type) {
        match stmt {
            Stmt::Let {
                name,
                ty,
                ty_span,
                value,
                span,
            } => {
                let val_ty = match ty {
                    Some(declared) => self.check_expr_ex(value, Some(declared)),
                    None => self.check_expr(value),
                };
                let var_ty = match ty {
                    Some(declared) => {
                        if !self.validate_type(declared, ty_span.unwrap_or(*span)) {
                            Type::Error
                        } else {
                            if !types_match(declared, &val_ty) {
                                self.errors.push(LexoraError::TypeMismatch {
                                    expected: self.format_type(declared),
                                    found: self.format_type(&val_ty),
                                    span: *span,
                                });
                            }
                            declared.clone()
                        }
                    }
                    None => {
                        self.validate_type(&val_ty, *span);
                        val_ty
                    }
                };
                if matches!(var_ty, Type::Void) {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}' void deger ile baglanamaz; void bir deger tipi degil",
                            self.resolve(*name)
                        ),
                        span: *span,
                    });
                }
                let bagli = self.variables.define(*name, var_ty);
                if !bagli && self.resolve(*name) != "_" {
                    self.errors.push(LexoraError::AlreadyDefined {
                        name: self.resolve(*name),
                        span: *span,
                    });
                }
            }
            Stmt::Assign { name, value, span } => {
                if self.loop_vars.contains(name) {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "dongu degiskeni '{}' degistirilemez",
                            self.resolve(*name)
                        ),
                        span: *span,
                    });
                }
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
                let val_ty = self.check_expr_ex(value, Some(&var_ty));
                if !types_match(&var_ty, &val_ty) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: self.format_type(&var_ty),
                        found: self.format_type(&val_ty),
                        span: *span,
                    });
                }
            }
            Stmt::Return(expr, span) => {
                let expr_ty = self.check_expr_ex(expr, Some(ret_ty));
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

            Stmt::While {
                condition,
                body,
                span,
            } => {
                let cond_ty = self.check_expr(condition);
                if !types_match(&cond_ty, &Type::Bool) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "bool".to_string(),
                        found: self.format_type(&cond_ty),
                        span: *span,
                    });
                }
                self.variables.enter_scope();
                for s in body.stmts.iter() {
                    self.check_statement(s, ret_ty);
                }
                if let Some(t) = body.tail {
                    let tail_ty = self.check_expr(t);
                    if !types_match(&tail_ty, &Type::Void) {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "dongu govdesi deger uretemez (bulunan {})",
                                self.format_type(&tail_ty)
                            ),
                            span: t.span(),
                        });
                    }
                }
                self.variables.exit_scope();
            }
            Stmt::For {
                var,
                from,
                to,
                body,
                span,
            } => {
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
                self.loop_vars.push(*var);
                for s in body.stmts.iter() {
                    self.check_statement(s, ret_ty);
                }
                if let Some(t) = body.tail {
                    let tail_ty = self.check_expr(t);
                    if !types_match(&tail_ty, &Type::Void) {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "dongu govdesi deger uretemez (bulunan {})",
                                self.format_type(&tail_ty)
                            ),
                            span: t.span(),
                        });
                    }
                }
                self.loop_vars.pop();
                self.variables.exit_scope();
            }
            Stmt::AssignPlace {
                target,
                value,
                span,
            } => {
                let target_ty = self.check_expr(target);
                let val_ty = self.check_expr_ex(value, Some(&target_ty));
                if !types_match(&target_ty, &val_ty) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: self.format_type(&target_ty),
                        found: self.format_type(&val_ty),
                        span: *span,
                    });
                }
            }
            Stmt::Error(_) => {}
        }
    }
    fn check_expr<'arena>(&mut self, expr: &Expr<'arena>) -> Type {
        self.check_expr_ex(expr, None)
    }
    fn check_expr_ex<'arena>(&mut self, expr: &Expr<'arena>, expected: Option<&Type>) -> Type {
        let ty = self.check_expr_inner(expr, expected);
        self.types.insert(expr.id(), ty.clone());
        ty
    }
    fn check_expr_inner<'arena>(&mut self, expr: &Expr<'arena>, expected: Option<&Type>) -> Type {
        match expr {
            Expr::Integer(n, _, _) => {
                if *n >= i32::MIN as i64 && *n <= i32::MAX as i64 {
                    Type::I32
                } else {
                    Type::I64
                }
            }
            Expr::Float(_, _, _) => Type::F64,
            Expr::Bool(_, _, _) => Type::Bool,
            Expr::StringLiteral(_, _, _) => Type::Str,

            Expr::Identifier(sym, span, _) => {
                if self.resolve(*sym) == "_" {
                    self.errors.push(LexoraError::Custom {
                        message: "'_' bir deger degil; yalnizca desende ve baglama hedefinde kullanilabilir".to_string(),
                        span: *span,
                    });
                    return Type::Error;
                }
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
            Expr::BinaryOp {
                left,
                op,
                right,
                span,
                ..
            } => {
                let lt = self.check_expr(left);
                let rt = self.check_expr(right);
                match op {
                    BinaryOperator::And | BinaryOperator::Or => {
                        if !types_match(&lt, &Type::Bool) || !types_match(&rt, &Type::Bool) {
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
                    BinaryOperator::Eq
                    | BinaryOperator::NotEq
                    | BinaryOperator::Less
                    | BinaryOperator::Greater
                    | BinaryOperator::LessEq
                    | BinaryOperator::GreaterEq => {
                        if matches!(lt, Type::Error) || matches!(rt, Type::Error) {
                            return Type::Error;
                        }
                        if is_stringy(&lt) && is_stringy(&rt) {
                            return Type::Bool;
                        }
                        if !types_match(&lt, &rt) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(&lt),
                                found: self.format_type(&rt),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        let ok = match op {
                            BinaryOperator::Eq | BinaryOperator::NotEq => {
                                matches!(
                                    lt,
                                    Type::I32 | Type::I64 | Type::Bool | Type::F32 | Type::F64
                                )
                            }
                            _ => matches!(lt, Type::I32 | Type::I64 | Type::F32 | Type::F64),
                        };
                        if !ok {
                            let msg = match op {
                                BinaryOperator::Eq | BinaryOperator::NotEq => format!(
                                    "'==' / '!=' bu tipte kullanilamaz: {} — esitlik sayisal, bool veya string tiplerinde olur",
                                    self.format_type(&lt)
                                ),
                                _ => format!(
                                    "siralama karsilastirmasi sayisal veya string tip gerektirir, bulunan {}",
                                    self.format_type(&lt)
                                ),
                            };
                            self.errors.push(LexoraError::Custom {
                                message: msg,
                                span: *span,
                            });
                            return Type::Error;
                        }
                        Type::Bool
                    }
                    _ => {
                        if matches!(lt, Type::Error) || matches!(rt, Type::Error) {
                            return Type::Error;
                        }
                        if *op == BinaryOperator::Add {
                            match (&lt, &rt) {
                                (Type::String, Type::String)
                                | (Type::String, Type::Str)
                                | (Type::Str, Type::String) => return Type::String,
                                (Type::Str, Type::Str) => {
                                    self.errors.push(LexoraError::Custom {
                                        message: "iki str sabiti '+' ile birlestirilemez; once string() ile String'e cevirin".to_string(),
                                        span: *span,
                                    });
                                    return Type::Error;
                                }
                                _ => {}
                            }
                        }
                        if !types_match(&lt, &rt) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(&lt),
                                found: self.format_type(&rt),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        if !matches!(lt, Type::I32 | Type::I64 | Type::F32 | Type::F64) {
                            self.errors.push(LexoraError::Custom {
                                message: format!(
                                    "aritmetik islem sayisal tip gerektirir (i32/i64/f32/f64), bulunan {}",
                                    self.format_type(&lt)
                                ),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        lt
                    }
                }
            }
            Expr::UnaryOp {
                op, operand, span, ..
            } => {
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
                        if !matches!(
                            ty,
                            Type::Error | Type::I32 | Type::I64 | Type::F32 | Type::F64
                        ) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: "i32, i64, f32 veya f64".to_string(),
                                found: self.format_type(&ty),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        ty
                    }
                }
            }
            Expr::Cast {
                expr,
                target_type,
                span,
                ..
            } => {
                self.validate_type(target_type, *span);
                let from_ty = self.check_expr(expr);
                match (&from_ty, target_type) {
                    (Type::Error, _) => Type::Error,
                    (Type::I32, Type::I64) | (Type::I64, Type::I32) => target_type.clone(),
                    (Type::F32, Type::F64) | (Type::F64, Type::F32) => target_type.clone(),
                    (Type::I32 | Type::I64, Type::F32 | Type::F64) => target_type.clone(),
                    (Type::F32 | Type::F64, Type::I32 | Type::I64) => target_type.clone(),
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
            Expr::Call {
                name,
                args,
                type_args,
                span,
                id,
            } => {
                if let Some((tparams, def_ptys, def_ret)) = self.fn_defs.get(name).cloned() {
                    if !type_args.is_empty() && type_args.len() != tparams.len() {
                        self.type_arg_arity_error(*name, type_args.len(), tparams.len(), *span);
                        for arg in args.iter() {
                            self.check_expr(arg);
                        }
                        return Type::Error;
                    }
                    if args.len() != def_ptys.len() {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'{}' {} argüman bekliyor, {} verildi",
                                self.resolve(*name),
                                def_ptys.len(),
                                args.len()
                            ),
                            span: *span,
                        });
                        for arg in args.iter() {
                            self.check_expr(arg);
                        }
                        return Type::Error;
                    }

                    let arg_tys: Vec<Type> = args.iter().map(|a| self.check_expr(a)).collect();
                    let targs: Vec<Type> = if !type_args.is_empty() {
                        type_args.clone()
                    } else {
                        let mut map: HashMap<Symbol, Type> = HashMap::new();
                        if let Some(exp) = expected {
                            unify(&def_ret, exp, &mut map);
                        }
                        for (pty, aty) in def_ptys.iter().zip(arg_tys.iter()) {
                            unify(pty, aty, &mut map);
                        }
                        let mut resolved: Vec<Type> = Vec::new();
                        for p in &tparams {
                            match map.get(p) {
                                Some(t) => resolved.push(t.clone()),
                                None => {
                                    if arg_tys.iter().any(|t| matches!(t, Type::Error)) {
                                        return Type::Error;
                                    }
                                    self.errors.push(LexoraError::Custom {
                                        message: format!(
                                            "'{}' icin '{}' tip parametresi cikarilamiyor; turbofish kullanin: {}::<T>(...)",
                                            self.resolve(*name),
                                            self.resolve(*p),
                                            self.resolve(*name)
                                        ),
                                        span: *span,
                                    });
                                    return Type::Error;
                                }
                            }
                        }
                        resolved
                    };
                    let mut ok = true;
                    for t in &targs {
                        if !self.validate_type(t, *span) {
                            ok = false;
                        } else if !type_is_concrete(t) {
                            self.errors.push(LexoraError::Custom {
                                message: format!(
                                    "'{}' somut olmayan tip argumaniyla cagrilamiyor: {} — generic fonksiyon icinden generic cagri henuz desteklenmiyor",
                                    self.resolve(*name),
                                    self.format_type(t)
                                ),
                                span: *span,
                            });
                            ok = false;
                        }
                    }
                    if !ok {
                        return Type::Error;
                    }
                    let map: HashMap<Symbol, Type> =
                        tparams.iter().copied().zip(targs.iter().cloned()).collect();
                    let ptys: Vec<Type> = def_ptys.iter().map(|t| t.substitute(&map)).collect();
                    let rty = def_ret.substitute(&map);
                    let sig_ok =
                        self.check_fn_instantiation(*name, targs.clone(), &ptys, &rty, *span);
                    for ((arg, aty), pty) in args.iter().zip(arg_tys.iter()).zip(ptys.iter()) {
                        if !types_match(aty, pty) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(pty),
                                found: self.format_type(aty),
                                span: arg.span(),
                            });
                        }
                    }
                    if !sig_ok {
                        return Type::Error;
                    }
                    self.call_targs.insert(*id, targs);
                    return rty;
                }
                let bilinen = self.functions.contains_key(name)
                    || matches!(self.resolve(*name).as_str(), "print" | "string" | "len");
                if !type_args.is_empty() && bilinen {
                    self.type_arg_arity_error(*name, type_args.len(), 0, *span);
                }
                if let Some((param_types, ret_ty)) = self.functions.get(name).cloned() {
                    if args.len() != param_types.len() {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'{}' {} argüman bekliyor, {} verildi",
                                self.resolve(*name),
                                param_types.len(),
                                args.len()
                            ),
                            span: *span,
                        });
                        for arg in args.iter() {
                            self.check_expr(arg);
                        }
                        return ret_ty;
                    }
                    for (arg, param_ty) in args.iter().zip(param_types.iter()) {
                        let arg_ty = self.check_expr_ex(arg, Some(param_ty));
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
                        for arg in args.iter() {
                            let aty = self.check_expr(arg);
                            if !matches!(
                                aty,
                                Type::Error
                                    | Type::I32
                                    | Type::I64
                                    | Type::Bool
                                    | Type::Str
                                    | Type::String
                                    | Type::F32
                                    | Type::F64
                            ) {
                                self.errors.push(LexoraError::Custom {
                                    message: format!(
                                        "print bu tipi yazdiramaz: {} — yazdirilabilir tipler i32/i64/f32/f64/bool/str/String",
                                        self.format_type(&aty)
                                    ),
                                    span: arg.span(),
                                });
                            }
                        }
                        Type::Void
                    } else if name_str == "string" {
                        if args.len() != 1 {
                            self.errors.push(LexoraError::Custom {
                                message: "string bir str arguman alir".to_string(),
                                span: *span,
                            });
                            for arg in args.iter() {
                                self.check_expr(arg);
                            }
                            return Type::String;
                        }
                        let arg_ty = self.check_expr(&args[0]);
                        if !matches!(arg_ty, Type::Error) && !types_match(&arg_ty, &Type::Str) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: "str".to_string(),
                                found: self.format_type(&arg_ty),
                                span: args[0].span(),
                            });
                        }
                        Type::String
                    } else if name_str == "len" {
                        if args.len() != 1 {
                            self.errors.push(LexoraError::Custom {
                                message: "len bir str veya String arguman alir".to_string(),
                                span: *span,
                            });
                            for arg in args.iter() {
                                self.check_expr(arg);
                            }
                            return Type::I32;
                        }
                        let arg_ty = self.check_expr(&args[0]);
                        if !matches!(arg_ty, Type::Error) && !is_stringy(&arg_ty) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: "String veya str".to_string(),
                                found: self.format_type(&arg_ty),
                                span: args[0].span(),
                            });
                        }
                        Type::I32
                    } else {
                        self.errors.push(LexoraError::UndefinedFunction {
                            name: name_str,
                            suggestion: self.nearest_fn(*name),
                            span: *span,
                        });
                        for arg in args.iter() {
                            self.check_expr(arg);
                        }
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
            Expr::Index {
                array, index, span, ..
            } => {
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
            Expr::StructLiteral {
                name, fields, span, ..
            } => {
                let struct_fields = match self.structs.get(name).cloned() {
                    Some(f) => f,
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: format!("tanimsiz struct '{}'", self.resolve(*name)),
                            span: *span,
                        });
                        for (_, field_val) in fields.iter() {
                            self.check_expr(field_val);
                        }
                        return Type::Error;
                    }
                };
                let mut seen: Vec<Symbol> = Vec::new();
                for (field_name, field_val) in fields.iter() {
                    let actual_ty = self.check_expr(field_val);
                    let expected_ty = match struct_fields.iter().find(|(n, _)| *n == *field_name) {
                        Some((_, t)) => t.clone(),
                        None => {
                            self.errors.push(LexoraError::UndefinedVariable {
                                name: self.resolve(*field_name),
                                suggestion: self.nearest_field(*field_name, &struct_fields),
                                span: *span,
                            });
                            continue;
                        }
                    };
                    if seen.contains(field_name) {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'{}' alani birden fazla kez verildi",
                                self.resolve(*field_name)
                            ),
                            span: *span,
                        });
                    }
                    seen.push(*field_name);
                    if !types_match(&expected_ty, &actual_ty) {
                        self.errors.push(LexoraError::TypeMismatch {
                            expected: self.format_type(&expected_ty),
                            found: self.format_type(&actual_ty),
                            span: *span,
                        });
                    }
                }
                let missing: Vec<String> = struct_fields
                    .iter()
                    .filter(|(n, _)| !seen.contains(n))
                    .map(|(n, _)| format!("'{}'", self.resolve(*n)))
                    .collect();
                if !missing.is_empty() {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}' struct'i icin eksik alan: {}",
                            self.resolve(*name),
                            missing.join(", ")
                        ),
                        span: *span,
                    });
                }
                Type::Struct(*name)
            }
            Expr::FieldAccess {
                object,
                field,
                span,
                ..
            } => {
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
                            message: format!("tanımsız struct '{}'", self.resolve(struct_sym)),
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
            Expr::Box { value, .. } => {
                let inner = self.check_expr(value);
                Type::Box(Box::new(inner))
            }
            Expr::Deref { target, span, .. } => {
                let target_ty = self.check_expr(target);
                match target_ty {
                    Type::Error => Type::Error,
                    Type::Box(inner) => *inner,
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "Box olmayan deger deref edilemez: {}",
                                self.format_type(&target_ty)
                            ),
                            span: *span,
                        });
                        Type::Error
                    }
                }
            }
            Expr::EnumVariant {
                enum_name,
                variant,
                args,
                type_args,
                span,
                ..
            } => {
                let (params, variants) = match self.enums.get(enum_name).cloned() {
                    Some(pv) => pv,
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: format!("tanimsiz enum '{}'", self.resolve(*enum_name)),
                            span: *span,
                        });
                        for a in args.iter() {
                            self.check_expr(a);
                        }
                        return Type::Error;
                    }
                };
                let def_field_tys = match variants.iter().find(|(v, _)| v == variant) {
                    Some((_, tys)) => tys.clone(),
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'{}' enum'unda '{}' varyanti yok",
                                self.resolve(*enum_name),
                                self.resolve(*variant)
                            ),
                            span: *span,
                        });
                        for a in args.iter() {
                            self.check_expr(a);
                        }
                        return Type::Error;
                    }
                };

                if params.is_empty() && !type_args.is_empty() {
                    self.type_arg_arity_error(*enum_name, type_args.len(), 0, *span);
                }
                if !params.is_empty() && !type_args.is_empty() && type_args.len() != params.len() {
                    self.type_arg_arity_error(*enum_name, type_args.len(), params.len(), *span);
                    for a in args.iter() {
                        self.check_expr(a);
                    }
                    return Type::Error;
                }

                if args.len() != def_field_tys.len() {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}::{}' {} alan bekliyor, {} verildi",
                            self.resolve(*enum_name),
                            self.resolve(*variant),
                            def_field_tys.len(),
                            args.len()
                        ),
                        span: *span,
                    });
                }
                let arg_tys: Vec<Type> = args.iter().map(|a| self.check_expr(a)).collect();
                let targs: Vec<Type> = if params.is_empty() {
                    Vec::new()
                } else if !type_args.is_empty() {
                    type_args.clone()
                } else {
                    let mut map: HashMap<Symbol, Type> = HashMap::new();
                    if let Some(Type::Enum(esym, eargs)) = expected {
                        if *esym == *enum_name && eargs.len() == params.len() {
                            for (p, a) in params.iter().zip(eargs.iter()) {
                                if type_is_concrete(a) {
                                    map.insert(*p, a.clone());
                                }
                            }
                        }
                    }
                    for (fty, aty) in def_field_tys.iter().zip(arg_tys.iter()) {
                        unify(fty, aty, &mut map);
                    }
                    let mut resolved: Vec<Type> = Vec::new();
                    for p in &params {
                        match map.get(p) {
                            Some(t) => resolved.push(t.clone()),
                            None => {
                                if arg_tys.iter().any(|t| matches!(t, Type::Error)) {
                                    return Type::Error;
                                }
                                let msg = if args.is_empty() {
                                    format!(
                                        "'{}::{}' icin tip cikarimi yapacak arguman yok; turbofish kullanin: {}::<T>::{}",
                                        self.resolve(*enum_name),
                                        self.resolve(*variant),
                                        self.resolve(*enum_name),
                                        self.resolve(*variant)
                                    )
                                } else {
                                    format!(
                                        "'{}::{}' icin '{}' tip parametresi cikarilamiyor; turbofish kullanin",
                                        self.resolve(*enum_name),
                                        self.resolve(*variant),
                                        self.resolve(*p)
                                    )
                                };
                                self.errors.push(LexoraError::Custom {
                                    message: msg,
                                    span: *span,
                                });
                                return Type::Error;
                            }
                        }
                    }
                    resolved
                };
                let concrete_ftys: Vec<Type> = if targs.is_empty() {
                    def_field_tys
                } else {
                    let map: HashMap<Symbol, Type> =
                        params.iter().copied().zip(targs.iter().cloned()).collect();
                    def_field_tys.iter().map(|t| t.substitute(&map)).collect()
                };
                for ((arg, aty), fty) in args.iter().zip(arg_tys.iter()).zip(concrete_ftys.iter()) {
                    if !types_match(aty, fty) {
                        self.errors.push(LexoraError::TypeMismatch {
                            expected: self.format_type(fty),
                            found: self.format_type(aty),
                            span: arg.span(),
                        });
                    }
                }
                let result = Type::Enum(*enum_name, targs);
                self.validate_type(&result, *span);
                result
            }
            Expr::Match {
                scrutinee,
                arms,
                span,
                ..
            } => {
                let scrut_ty = self.check_expr(scrutinee);
                let (enum_sym, enum_args) = match &scrut_ty {
                    Type::Error => (None, Vec::new()),
                    Type::Enum(s, a) => (Some(*s), a.clone()),
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "match yalnizca enum uzerinde olur, bulunan {}",
                                self.format_type(&scrut_ty)
                            ),
                            span: *span,
                        });
                        (None, Vec::new())
                    }
                };

                let all_variants: Option<Vec<(Symbol, Vec<Type>)>> =
                    enum_sym.and_then(|e| self.enum_variants_for(e, &enum_args));
                let ret = self.cur_ret.clone();
                let mut covered: Vec<Symbol> = Vec::new();
                let mut has_wildcard = false;
                let mut match_ty: Option<Type> = None;
                for (pat, body) in arms.iter() {
                    let pat_span = pat.span();
                    if has_wildcard {
                        self.errors.push(LexoraError::Custom {
                            message: "bu kol asla eslesmez: onceki '_' tum durumlari kapsiyor"
                                .to_string(),
                            span: pat_span,
                        });
                    }
                    let mut binds: Vec<(Symbol, Type)> = Vec::new();
                    match pat {
                        Pattern::Wildcard(_) => has_wildcard = true,
                        Pattern::Variant {
                            enum_name,
                            variant,
                            bindings,
                            ..
                        } => {
                            let mut seen_binds: Vec<Symbol> = Vec::new();
                            for b in bindings.iter() {
                                if self.resolve(*b) == "_" {
                                    continue;
                                }
                                if seen_binds.contains(b) {
                                    self.errors.push(LexoraError::Custom {
                                        message: format!(
                                            "'{}' bu desende birden fazla kez baglaniyor",
                                            self.resolve(*b)
                                        ),
                                        span: pat_span,
                                    });
                                }
                                seen_binds.push(*b);
                            }
                            if let (Some(e), Some(vars)) = (enum_sym, &all_variants) {
                                if *enum_name != e {
                                    self.errors.push(LexoraError::Custom {
                                        message: format!("desen enum'u '{}', scrutinee enum'u '{}' ile uyusmuyor",
                                                         self.resolve(*enum_name), self.resolve(e)),
                                        span: pat_span,
                                    });
                                } else if let Some((_, ftys)) =
                                    vars.iter().find(|(v, _)| v == variant)
                                {
                                    if covered.contains(variant) {
                                        self.errors.push(LexoraError::Custom {
                                            message: format!(
                                                "'{}::{}' zaten onceki bir kolda kapsandi",
                                                self.resolve(e),
                                                self.resolve(*variant)
                                            ),
                                            span: pat_span,
                                        });
                                    }
                                    covered.push(*variant);
                                    if bindings.len() != ftys.len() {
                                        self.errors.push(LexoraError::Custom {
                                            message: format!(
                                                "'{}::{}' {} alan baglar, {} desen verildi",
                                                self.resolve(e),
                                                self.resolve(*variant),
                                                ftys.len(),
                                                bindings.len()
                                            ),
                                            span: pat_span,
                                        });
                                    }
                                    for (b, t) in bindings.iter().zip(ftys.iter()) {
                                        binds.push((*b, t.clone()));
                                    }
                                } else {
                                    self.errors.push(LexoraError::Custom {
                                        message: format!(
                                            "'{}' enum'unda '{}' varyanti yok",
                                            self.resolve(e),
                                            self.resolve(*variant)
                                        ),
                                        span: pat_span,
                                    });
                                }
                            }
                        }
                    }
                    self.variables.enter_scope();
                    for (b, t) in binds {
                        self.variables.define(b, t);
                    }
                    for s in body.stmts.iter() {
                        self.check_statement(s, &ret);
                    }
                    let arm_ty = match body.tail {
                        Some(t) => self.check_expr_ex(t, expected),
                        None => Type::Void,
                    };
                    self.variables.exit_scope();
                    match &match_ty {
                        None => match_ty = Some(arm_ty),
                        Some(prev) => {
                            if !types_match(prev, &arm_ty) {
                                self.errors.push(LexoraError::TypeMismatch {
                                    expected: self.format_type(prev),
                                    found: self.format_type(&arm_ty),
                                    span: body.span,
                                });
                            }
                        }
                    }
                }
                if let (Some(vars), false) = (&all_variants, has_wildcard) {
                    for (v, _) in vars {
                        if !covered.contains(v) {
                            self.errors.push(LexoraError::Custom {
                                message: format!(
                                    "eksik match: '{}::{}' kapsanmadi",
                                    self.resolve(enum_sym.unwrap()),
                                    self.resolve(*v)
                                ),
                                span: *span,
                            });
                        }
                    }
                }
                match_ty.unwrap_or(Type::Void)
            }
            Expr::If {
                condition,
                then_body,
                else_body,
                span,
                ..
            } => {
                let cond_ty = self.check_expr(condition);
                if !types_match(&cond_ty, &Type::Bool) {
                    self.errors.push(LexoraError::TypeMismatch {
                        expected: "bool".to_string(),
                        found: self.format_type(&cond_ty),
                        span: condition.span(),
                    });
                }
                let ret = self.cur_ret.clone();
                self.variables.enter_scope();
                for s in then_body.stmts.iter() {
                    self.check_statement(s, &ret);
                }
                let then_ty = match then_body.tail {
                    Some(t) => self.check_expr_ex(t, expected),
                    None => Type::Void,
                };
                self.variables.exit_scope();
                match else_body {
                    Some(eb) => {
                        self.variables.enter_scope();
                        for s in eb.stmts.iter() {
                            self.check_statement(s, &ret);
                        }
                        let else_ty = match eb.tail {
                            Some(t) => self.check_expr_ex(t, expected),
                            None => Type::Void,
                        };
                        self.variables.exit_scope();
                        if !types_match(&then_ty, &else_ty) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(&then_ty),
                                found: self.format_type(&else_ty),
                                span: eb.span,
                            });
                        }
                        then_ty
                    }
                    None => {
                        if !types_match(&then_ty, &Type::Void) {
                            self.errors.push(LexoraError::Custom {
                                message: format!(
                                    "else'siz if deger uretemez (then kolu {} uretiyor)",
                                    self.format_type(&then_ty)
                                ),
                                span: *span,
                            });
                            return Type::Error;
                        }
                        Type::Void
                    }
                }
            }
            Expr::Try {
                expr: inner, span, ..
            } => {
                let inner_ty = self.check_expr(inner);
                if matches!(inner_ty, Type::Error) {
                    return Type::Error;
                }
                let (sym, args) = match &inner_ty {
                    Type::Enum(s, a) if self.resolve(*s) == "Result" => (*s, a.clone()),
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'?' yalnizca Result uzerinde kullanilabilir, bulunan {}",
                                self.format_type(&inner_ty)
                            ),
                            span: *span,
                        });
                        return Type::Error;
                    }
                };
                let (ok_ty, err_ty) = match self.result_parts(sym, &args) {
                    Some(p) => p,
                    None => {
                        self.errors.push(LexoraError::Custom {
                            message: "'?' icin Result iki varyantli olmali: Ok(T) ve Err(E)"
                                .to_string(),
                            span: *span,
                        });
                        return Type::Error;
                    }
                };
                let ret_err = match &self.cur_ret {
                    Type::Enum(rs, ra) if *rs == sym => {
                        let ra = ra.clone();
                        self.result_parts(sym, &ra).map(|(_, e)| e)
                    }
                    _ => None,
                };
                match ret_err {
                    Some(re) => {
                        if !types_match(&err_ty, &re) {
                            self.errors.push(LexoraError::TypeMismatch {
                                expected: self.format_type(&re),
                                found: self.format_type(&err_ty),
                                span: *span,
                            });
                            return Type::Error;
                        }

                        ok_ty
                    }
                    None => {
                        let ret_s = self.format_type(&self.cur_ret);
                        self.errors.push(LexoraError::Custom {
                            message: format!("'?' yalnizca Result donduren fonksiyonda kullanilabilir (donus tipi {})", ret_s),
                            span: *span,
                        });
                        return Type::Error;
                    }
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
                span,
                ..
            } => {
                let recv_ty = self.check_expr(receiver);
                let type_sym = match &recv_ty {
                    Type::Error => {
                        for a in args.iter() {
                            self.check_expr(a);
                        }
                        return Type::Error;
                    }
                    Type::Struct(s) => *s,
                    Type::Enum(s, a) if a.is_empty() => *s,
                    _ => {
                        self.errors.push(LexoraError::Custom {
                            message: format!(
                                "'{}' tipinin metodu yok; metotlar yalnizca struct ve non-generic enum'lara yazilir",
                                self.format_type(&recv_ty)
                            ),
                            span: receiver.span(),
                        });
                        for a in args.iter() {
                            self.check_expr(a);
                        }
                        return Type::Error;
                    }
                };
                let sig = match self.methods.get(&(type_sym, *method)).cloned() {
                    Some(s) => s,
                    None => {
                        let suggestion = self.nearest_method(type_sym, *method);
                        let msg = match suggestion {
                            Some(near) => format!(
                                "'{}' tipinde '{}' metodu yok; bunu mu demek istediniz: '{}'?",
                                self.resolve(type_sym),
                                self.resolve(*method),
                                near
                            ),
                            None => format!(
                                "'{}' tipinde '{}' metodu yok",
                                self.resolve(type_sym),
                                self.resolve(*method)
                            ),
                        };
                        self.errors.push(LexoraError::Custom {
                            message: msg,
                            span: *span,
                        });
                        for a in args.iter() {
                            self.check_expr(a);
                        }
                        return Type::Error;
                    }
                };
                let expected = sig.0.len().saturating_sub(1);
                if args.len() != expected {
                    self.errors.push(LexoraError::Custom {
                        message: format!(
                            "'{}.{}' {} arguman bekliyor, {} verildi",
                            self.resolve(type_sym),
                            self.resolve(*method),
                            expected,
                            args.len()
                        ),
                        span: *span,
                    });
                    for a in args.iter() {
                        self.check_expr(a);
                    }
                    return sig.1;
                }
                for (arg, pty) in args.iter().zip(sig.0.iter().skip(1)) {
                    let aty = self.check_expr_ex(arg, Some(pty));
                    if !types_match(&aty, pty) {
                        self.errors.push(LexoraError::TypeMismatch {
                            expected: self.format_type(pty),
                            found: self.format_type(&aty),
                            span: arg.span(),
                        });
                    }
                }

                sig.1
            }

            Expr::Error(_, _) => Type::Error,
        }
    }
}

fn is_stringy(ty: &Type) -> bool {
    matches!(ty, Type::Str | Type::String)
}

fn types_match(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::Error, _) | (_, Type::Error) => true,
        (Type::I32, Type::I32) => true,
        (Type::I64, Type::I64) => true,
        (Type::F32, Type::F32) => true,
        (Type::F64, Type::F64) => true,
        (Type::Bool, Type::Bool) => true,
        (Type::Str, Type::Str) => true,
        (Type::Void, Type::Void) => true,
        (Type::Array(t1, n1), Type::Array(t2, n2)) => types_match(t1, t2) && n1 == n2,
        (Type::Struct(s1), Type::Struct(s2)) => s1 == s2,
        (Type::Box(t1), Type::Box(t2)) => types_match(t1, t2),
        (Type::String, Type::String) => true,
        (Type::Enum(s1, a1), Type::Enum(s2, a2)) => {
            s1 == s2
                && a1.len() == a2.len()
                && a1.iter().zip(a2.iter()).all(|(x, y)| types_match(x, y))
        }
        (Type::Param(p1), Type::Param(p2)) => p1 == p2,
        _ => false,
    }
}
fn block_always_returns(block: &Block) -> bool {
    if block.tail.is_some() {
        return true;
    }
    block.stmts.iter().any(stmt_always_returns)
}

fn stmt_always_returns(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::Return(..) => true,
        Stmt::Expr(e, _) => expr_always_returns(e),
        _ => false,
    }
}

fn expr_always_returns(expr: &Expr) -> bool {
    match expr {
        Expr::If {
            then_body,
            else_body: Some(eb),
            ..
        } => block_always_returns(then_body) && block_always_returns(eb),
        Expr::Match { arms, .. } => {
            !arms.is_empty() && arms.iter().all(|(_, b)| block_always_returns(b))
        }
        _ => false,
    }
}
fn type_is_concrete(ty: &Type) -> bool {
    match ty {
        Type::Param(_) => false,
        Type::Array(elem, _) => type_is_concrete(elem),
        Type::Box(inner) => type_is_concrete(inner),
        Type::Enum(_, args) => args.iter().all(type_is_concrete),
        _ => true,
    }
}
fn array_elems(ty: &Type) -> Option<usize> {
    match ty {
        Type::Array(elem, n) => array_elems(elem).and_then(|inner| n.checked_mul(inner)),
        _ => Some(1),
    }
}
fn unify(def_ty: &Type, concrete: &Type, map: &mut HashMap<Symbol, Type>) {
    match (def_ty, concrete) {
        (Type::Param(p), t) => {
            if !matches!(t, Type::Error) && !map.contains_key(p) {
                map.insert(*p, t.clone());
            }
        }
        (Type::Array(a, _), Type::Array(b, _)) => unify(a, b, map),
        (Type::Box(a), Type::Box(b)) => unify(a, b, map),
        (Type::Enum(s1, a1), Type::Enum(s2, a2)) => {
            if s1 == s2 {
                for (x, y) in a1.iter().zip(a2.iter()) {
                    unify(x, y, map);
                }
            }
        }
        _ => {}
    }
}

use crate::ast::*;
use crate::error::LexoraError;
use crate::span::Span;
use crate::symbol::{Interner, Symbol};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy, PartialEq, Eq)]
enum MoveState {
    Owned,
    Moved,
    MaybeMoved,
}

#[derive(Clone,  PartialEq, Eq)]
enum BindState {
    Copy,
    Owning{
        paths: Vec<Vec<u32>>,
        states: Vec<MoveState>,
    },
}

pub struct MoveChecker<'a> {
    types: &'a HashMap<ExprId, Type>,
    interner: &'a Interner,
    scopes: Vec<HashMap<Symbol, BindState>>,
    moves: HashMap<ExprId, Vec<u32>>,
    errors: Vec<LexoraError>,
    cur_self: Option<Symbol>,
    structs: HashMap<Symbol, Vec<(Symbol, Type)>>,
    enums: HashMap<Symbol, (Vec<Symbol>, Vec<(Symbol, Vec<Type>)>)>,
}

impl<'a> MoveChecker<'a> {
    pub fn new(types: &'a HashMap<ExprId, Type>, interner: &'a Interner) -> Self {
        MoveChecker {
            types,
            interner,
            scopes: Vec::new(),
            moves: HashMap::new(),
            errors: Vec::new(),
            cur_self: None,
            structs: HashMap::new(),
            enums: HashMap::new(),
        }
    }
    pub fn check(mut self, program: &Program) -> (HashMap<ExprId, Vec<u32>>, Vec<LexoraError>) {
        for e in &program.enums {
            self.enums
                .insert(e.name, (e.params.clone(), e.variants.clone()));
        }
        for s in &program.structs {
            self.structs.insert(s.name, s.fields.clone());
        }
        for func in &program.functions {
            self.check_function(func);
        }
        for imp in &program.impls {
            for m in &imp.methods {
                self.check_function(m);
            }
        }
        (self.moves, self.errors)
    }
    fn enter(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn exit(&mut self) {
        self.scopes.pop();
    }
    fn bind(&mut self, sym: Symbol, ty: Option<&Type>) {
        let state = match ty {
            Some(t) if self.is_owning(t) => {
                let mut paths: Vec<Vec<u32>> = Vec::new();
                self.owning_nodes(t, &mut Vec::new(), &mut paths);
                let states = vec![MoveState::Owned; paths.len()];
                BindState::Owning { paths, states }
            }
            _ => BindState::Copy,
        };
        self.scopes.last_mut().unwrap().insert(sym, state);
    }

    fn lookup(&self, sym: Symbol) -> Option<BindState> {
        self.scopes.iter().rev().find_map(|s| s.get(&sym)).cloned()
    }

    fn lookup_mut(&mut self, sym: Symbol) -> Option<&mut BindState> {
        self.scopes.iter_mut().rev().find_map(|s| s.get_mut(&sym))
    }
    fn variants_for(&self, sym: Symbol, args: &[Type]) -> Option<Vec<(Symbol, Vec<Type>)>> {
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
    fn is_owning(&self, ty: &Type) -> bool {
        match ty {
            Type::Box(_) => true,
            Type::String => true,
            Type::Param(_) => true,
            Type::Enum(e, args) => self.variants_for(*e, args).map_or(false, |vs| {
                vs.iter()
                    .any(|(_, ftys)| ftys.iter().any(|t| self.is_owning(t)))
            }),
            Type::Struct(s) => self
                .structs
                .get(s)
                .map_or(false, |fs| fs.iter().any(|(_, t)| self.is_owning(t))),
            Type::Array(elem, _) => self.is_owning(elem),
            _ => false,
        }
    }
    fn owning_nodes(&self, ty: &Type, prefix: &mut Vec<u32>, out: &mut Vec<Vec<u32>>) {
        if !self.is_owning(ty) {
            return;
        }
        out.push(prefix.clone());
        if let Type::Struct(s) = ty {
            let fields = match self.structs.get(s) {
                Some(f) => f.clone(),
                None => return,
            };
            for (i, (_, fty)) in fields.iter().enumerate() {
                prefix.push(i as u32);
                self.owning_nodes(fty, prefix, out);
                prefix.pop();
            }
        }
    }
    fn place_path(&self, expr: &Expr) -> Option<(Symbol, Vec<u32>)> {
        match expr {
            Expr::Identifier(sym, _, _) => Some((*sym, Vec::new())),
            Expr::FieldAccess { object, field, .. } => {
                let (sym, mut path) = self.place_path(object)?;
                let sty = match self.types.get(&object.id())? {
                    Type::Struct(s) => *s,
                    _ => return None,
                };
                let idx = self
                    .structs
                    .get(&sty)?
                    .iter()
                    .position(|(n, _)| n == field)?;
                path.push(idx as u32);
                Some((sym, path))
            }
            _ => None,
        }
    }
    fn nodes_under(&self, sym: Symbol, path: &[u32]) -> Vec<usize> {
        match self.lookup(sym) {
            Some(BindState::Owning { paths, .. }) => paths
                .iter()
                .enumerate()
                .filter(|(_, p)| p.starts_with(path))
                .map(|(i, _)| i)
                .collect(),
            _ => Vec::new(),
        }
    }
    fn all_owned(&self, sym: Symbol) -> bool {
        matches!(
            self.lookup(sym),
            Some(BindState::Owning { ref states, .. }) if states.iter().all(|s| *s == MoveState::Owned)
        )
    }
    fn check_path_live(&mut self, sym: Symbol, path: &[u32], span: Span, consuming: bool) {
        let idx = self.nodes_under(sym, path);
        if idx.is_empty() {
            return;
        }
        let states: Vec<MoveState> = match self.lookup(sym) {
            Some(BindState::Owning { states, .. }) => idx.iter().map(|i| states[*i]).collect(),
            _ => return,
        };
        if states.iter().any(|s| *s == MoveState::MaybeMoved) {
            self.errors.push(LexoraError::Custom {
                message: "kosullu tasinmis olabilecek deger kullanildi".to_string(),
                span,
            });
        } else if states.iter().all(|s| *s == MoveState::Moved) {
            let message = if consuming {
                "tasinmis deger tekrar kullanildi"
            } else {
                "tasinmis deger kullanildi"
            };
            self.errors.push(LexoraError::Custom {
                message: message.to_string(),
                span,
            });
        } else if states.iter().any(|s| *s == MoveState::Moved) {
            self.errors.push(LexoraError::Custom {
                message: "kismen tasinmis deger kullanildi; alanlari ayri ayri kullanin"
                    .to_string(),
                span,
            });
        }
    }
    fn mark_path_moved(&mut self, sym: Symbol, path: &[u32]) {
        let idx = self.nodes_under(sym, path);
        if let Some(BindState::Owning { states, .. }) = self.lookup_mut(sym) {
            for i in idx {
                states[i] = MoveState::Moved;
            }
        }
    }
    fn mark_path_live(&mut self, sym: Symbol, path: &[u32]) {
        let idx = self.nodes_under(sym, path);
        if let Some(BindState::Owning { states, .. }) = self.lookup_mut(sym) {
            for i in idx {
                states[i] = MoveState::Owned;
            }
        }
    }

    fn check_assign_target(&mut self, sym: Symbol, path: &[u32], span: Span) {
        let bad = match self.lookup(sym) {
            Some(BindState::Owning { paths, states }) => {
                paths.iter().zip(states.iter()).any(|(p, s)| {
                    p.len() < path.len() && path.starts_with(p) && *s != MoveState::Owned
                })
            }
            _ => false,
        };
        if bad {
            self.errors.push(LexoraError::Custom {
                message: "tasinmis degerin bir parcasina atama yapilamaz; once butun olarak yeniden atayin".to_string(),
                span,
            });
        }
    }
    fn expr_is_owning(&self, expr: &Expr) -> bool {
        self.types
            .get(&expr.id())
            .map_or(false, |t| self.is_owning(t))
    }

    fn check_place_base(&mut self, base: &Expr) {
        if base.is_place() {
            return;
        }
        if self.expr_is_owning(base) {
            self.errors.push(LexoraError::Custom {
                message: "gecici deger uzerinden alan/eleman/deref erisimi yapilamaz; once bir degiskene baglayin".to_string(),
                span: base.span(),
            });
        }
    }

    fn check_function(&mut self, func: &Function) {
        self.enter();
        self.cur_self = if func.self_param {
            func.params.first().map(|(s, _)| *s)
        } else {
            None
        };
        for (i, (sym, ty)) in func.params.iter().enumerate() {
            if i == 0 && func.self_param {
                self.bind(*sym, None);
            } else {
                self.bind(*sym, Some(ty));
            }
        }
        for stmt in func.body.stmts.iter() {
            self.check_stmt(stmt);
        }
        if let Some(tail) = func.body.tail {
            self.consume_expr(tail);
        }
        self.exit();
        self.cur_self = None;
    }
    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, .. } => {
                self.consume_expr(value);
                let vt = self.types.get(&value.id()).cloned();
                self.bind(*name, vt.as_ref());
            }
            Stmt::Assign { name, value, .. } => {
                self.consume_expr(value);
                self.mark_path_live(*name, &[]);
            }
            Stmt::Return(expr, _) => self.consume_expr(expr),
            Stmt::Expr(expr, span) => {
                if self.expr_is_owning(expr) {
                    self.errors.push(LexoraError::Custom {
                        message: "owning deger bir degiskene baglanmali (sahipsiz kalamaz)"
                            .to_string(),
                        span: *span,
                    });
                }
                self.consume_expr(expr);
            }

            Stmt::While {
                condition,
                body,
                span,
            } => {
                self.consume_expr(condition);
                self.check_loop(body, *span, None);
            }
            Stmt::For {
                var,
                from,
                to,
                body,
                span,
            } => {
                self.consume_expr(from);
                self.consume_expr(to);
                self.check_loop(body, *span, Some(*var));
            }
            Stmt::AssignPlace { target, value, span } => {
                match self.place_path(target) {
                    Some((sym, path))
                    if matches!(self.lookup(sym), Some(BindState::Owning { .. })) =>
                        {
                            self.check_assign_target(sym, &path, *span);
                            self.consume_expr(value);
                            self.mark_path_live(sym, &path);
                        }
                    _ => {
                        self.read_place(target);
                        self.consume_expr(value);
                    }
                }
            }
            Stmt::Error(_) => {}
        }
    }
    fn check_loop(&mut self, body: &Block, span: Span, loop_var: Option<Symbol>) {
        let entry = self.scopes.clone();
        self.enter();
        if let Some(v) = loop_var {
            self.bind(v, None);
        }
        for s in body.stmts.iter() {
            self.check_stmt(s);
        }
        if let Some(t) = body.tail {
            self.consume_expr(t);
        }
        self.exit();
        let mut moved_outer = false;
        for (i, scope) in entry.iter().enumerate() {
            for (sym, st) in scope.iter() {
                if let BindState::Owning { states, .. } = st {
                    if states.iter().all(|s| *s == MoveState::Owned) {
                        if let Some(BindState::Owning { states: s2, .. }) =
                            self.scopes.get(i).and_then(|m| m.get(sym))
                        {
                            if s2.iter().any(|s| *s != MoveState::Owned) {
                                moved_outer = true;
                            }
                        }
                    }
                }
            }
        }
        if moved_outer {
            self.errors.push(LexoraError::Custom {
                message: "dongu govdesinde owning deger disari tasinamaz".to_string(),
                span,
            });
        }
        self.scopes = entry;
    }
    fn consume_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Identifier(sym, span, id) => {
                if Some(*sym) == self.cur_self && self.expr_is_owning(expr) {
                    self.errors.push(LexoraError::Custom {
                        message: "'self' tasinamaz (sahipli alicida deger olarak kacamaz); yerinde odunc alin".to_string(),
                        span: *span,
                    });
                }
                if matches!(self.lookup(*sym), Some(BindState::Owning { .. })) {
                    self.check_path_live(*sym, &[], *span, true);
                    self.mark_path_moved(*sym, &[]);
                    self.moves.insert(*id, Vec::new());
                }
            }
            Expr::Box { value, .. } => self.consume_expr(value),
            Expr::Deref { target, span, id } => {
                if self.expr_is_owning(expr) {
                    if matches!(target, Expr::Identifier(..)) {
                        self.consume_expr(target);
                        self.moves.insert(*id, Vec::new());
                    } else {
                        self.errors.push(LexoraError::Custom{
                            message: "Box icinden owning deger deref ile tasinamaz (yalnizca *degisken formunda tasinabilir)".to_string(),
                            span: *span,
                        });
                        self.read_place(target);
                    }
                } else {
                    self.check_place_base(target);
                    self.read_place(target);
                }
            }
            Expr::Call { name, args, .. } => {
                let fname = self.interner.resolve(*name);
                if fname == "print" || fname == "len" {
                    for arg in args.iter() {
                        if self.expr_is_owning(arg)
                            && !arg.is_place()
                            && !matches!(self.types.get(&arg.id()), Some(Type::String))
                        {
                            self.errors.push(LexoraError::Custom{
                                message: "owning gecici deger odunc alan cagriya verilemez; once bir degiskene baglayin".to_string(),
                                span: arg.span(),
                            });
                        }
                        self.read_place(arg);
                    }
                } else {
                    for arg in args.iter() {
                        self.consume_expr(arg);
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
                if is_comparison(op) {
                    let mut ls: HashSet<Symbol> = HashSet::new();
                    let mut rs: HashSet<Symbol> = HashSet::new();
                    collect_idents(left, &mut ls);
                    collect_idents(right, &mut rs);
                    let shared: Vec<Symbol> = ls
                        .intersection(&rs)
                        .copied()
                        .filter(|s| self.all_owned(*s))
                        .collect();
                    self.read_place(left);
                    self.read_place(right);
                    for s in shared {
                        if !self.all_owned(s) {
                            self.errors.push(LexoraError::Custom {
                                message: "karsilastirmanin iki tarafi ayni owning degeri kullaniyor ve biri onu tasiyor; once bir degiskene baglayin".to_string(),
                                span: *span,
                            });
                        }
                    }
                } else if matches!(op, BinaryOperator::And | BinaryOperator::Or) {
                    self.consume_expr(left);
                    let entry = self.scopes.clone();
                    self.consume_expr(right);
                    let after = self.scopes.clone();
                    self.scopes = join_scopes(&after, &entry);
                } else {
                    self.consume_expr(left);
                    self.consume_expr(right);
                }
            }

            Expr::UnaryOp { operand, .. } => self.consume_expr(operand),
            Expr::Cast { expr, .. } => self.consume_expr(expr),
            Expr::Index {
                array, index, span, ..
            } => {
                if self.expr_is_owning(expr) {
                    self.errors.push(LexoraError::Custom {
                        message: "owning eleman diziden tasinamaz; yerinde odunc alin".to_string(),
                        span: *span,
                    });
                }
                self.check_place_base(array);
                self.read_place(array);
                self.consume_expr(index);
            }
            Expr::FieldAccess { object, span, id,.. } => {
                self.check_place_base(object);
                if !self.expr_is_owning(expr) {
                    self.read_place(object);
                    return;
                }
                match self.place_path(expr) {
                    Some((sym, path))
                    if matches!(self.lookup(sym), Some(BindState::Owning { .. })) =>
                        {
                            self.check_path_live(sym, &path, *span, true);
                            self.mark_path_moved(sym, &path);
                            self.moves.insert(*id, path);
                        }
                    Some((sym, _)) if Some(sym) == self.cur_self => {
                        self.errors.push(LexoraError::Custom {
                            message: "'self' alanlari tasinamaz (odunc alinan alicidan cikamaz); yerinde odunc alin".to_string(),
                            span: *span,
                        });
                        self.read_place(object);
                    }
                    _ => {
                        if object.is_place() {
                            self.errors.push(LexoraError::Custom {
                                message: "owning alan yalnizca yerel bir degiskenin alan zincirinden tasinabilir; dizi elemani ve Box icerigi yerinde odunc alinir".to_string(),
                                span: *span,
                            });
                        }
                        self.read_place(object);
                    }
                }
            }
            Expr::ArrayLiteral(elems, _, _) => {
                for e in elems.iter() {
                    self.consume_expr(e);
                }
            }
            Expr::StructLiteral { fields, .. } => {
                for (_, v) in fields.iter() {
                    self.consume_expr(v);
                }
            }
            Expr::EnumVariant { args, .. } => {
                for a in args.iter() {
                    self.consume_expr(a);
                }
            }
            Expr::Try { expr, .. } => self.consume_expr(expr),
            Expr::Match {
                scrutinee, arms, ..
            } => {
                self.consume_expr(scrutinee);
                let scrut_args = match self.types.get(&scrutinee.id()) {
                    Some(Type::Enum(_, a)) => a.clone(),
                    _ => Vec::new(),
                };
                let entry = self.scopes.clone();
                let mut joined: Option<Vec<HashMap<Symbol, BindState>>> = None;
                for (pat, body) in arms.iter() {
                    self.scopes = entry.clone();
                    self.enter();
                    if let Pattern::Variant {
                        enum_name,
                        variant,
                        bindings,
                        ..
                    } = pat
                    {
                        let ftys = self
                            .variants_for(*enum_name, &scrut_args)
                            .and_then(|vs| {
                                vs.iter()
                                    .find(|(v, _)| v == variant)
                                    .map(|(_, t)| t.clone())
                            })
                            .unwrap_or_default();
                        for (i, b) in bindings.iter().enumerate() {
                            self.bind(*b, ftys.get(i));
                        }
                    }
                    for s in body.stmts.iter() {
                        self.check_stmt(s);
                    }
                    if let Some(t) = body.tail {
                        self.consume_expr(t);
                    }
                    self.exit();
                    let after = self.scopes.clone();
                    joined = Some(match joined {
                        None => after,
                        Some(j) => join_scopes(&j, &after),
                    });
                }
                if let Some(j) = joined {
                    self.scopes = j;
                }
            }
            Expr::If {
                condition,
                then_body,
                else_body,
                ..
            } => {
                self.consume_expr(condition);
                let entry = self.scopes.clone();
                self.enter();
                for s in then_body.stmts.iter() {
                    self.check_stmt(s);
                }
                if let Some(t) = then_body.tail {
                    self.consume_expr(t);
                }
                self.exit();
                let after_then = self.scopes.clone();
                self.scopes = entry.clone();
                if let Some(eb) = else_body {
                    self.enter();
                    for s in eb.stmts.iter() {
                        self.check_stmt(s);
                    }
                    if let Some(t) = eb.tail {
                        self.consume_expr(t);
                    }
                    self.exit();
                }
                let after_else = self.scopes.clone();
                self.scopes = join_scopes(&after_then, &after_else);
            }
            Expr::MethodCall { receiver, args, .. } => {
                self.check_place_base(receiver);
                self.read_place(receiver);
                for a in args.iter() {
                    self.consume_expr(a);
                }
            }
            Expr::Integer(..)
            | Expr::Float(..)
            | Expr::Bool(..)
            | Expr::StringLiteral(..)
            | Expr::Error(..) => {}
        }
    }
    fn read_place(&mut self, expr: &Expr) {
        match expr {
            Expr::Identifier(sym, span, _) => {
                self.check_path_live(*sym, &[], *span, false);
            }
            Expr::Deref { target, .. } => {
                self.check_place_base(target);
                self.read_place(target)
            }
            Expr::Index { array, index, .. } => {
                self.check_place_base(array);
                self.read_place(array);
                self.consume_expr(index);
            }
            Expr::FieldAccess { object, span, .. } => {
                self.check_place_base(object);
                match self.place_path(expr) {
                    Some((sym, path)) => self.check_path_live(sym, &path, *span, false),
                    None => self.read_place(object),
                }
            }
            other => self.consume_expr(other),
        }
    }
}

fn is_comparison(op: &BinaryOperator) -> bool {
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

fn join_state(a: &BindState, b: &BindState) -> BindState {
    match (a, b) {
        (BindState::Owning { paths, states: sa }, BindState::Owning { states: sb, .. })
        if sa.len() == sb.len() =>
            {
                BindState::Owning {
                    paths: paths.clone(),
                    states: sa
                        .iter()
                        .zip(sb.iter())
                        .map(|(x, y)| {
                            if x == y {
                                *x
                            } else {
                                MoveState::MaybeMoved
                            }
                        })
                        .collect(),
                }
            }
        _ => a.clone(),
    }
}

fn join_scopes(
    a: &[HashMap<Symbol, BindState>],
    b: &[HashMap<Symbol, BindState>],
) -> Vec<HashMap<Symbol, BindState>> {
    a.iter()
        .zip(b.iter())
        .map(|(ma, mb)| {
            ma.iter()
                .map(|(sym, sa)| {
                    let sb = mb.get(sym).unwrap_or(sa);
                    (*sym, join_state(sa, sb))
                })
                .collect()
        })
        .collect()
}

fn collect_idents(expr: &Expr, out: &mut HashSet<Symbol>) {
    match expr {
        Expr::Identifier(sym, _, _) => {
            out.insert(*sym);
        }
        Expr::BinaryOp { left, right, .. } => {
            collect_idents(left, out);
            collect_idents(right, out);
        }
        Expr::UnaryOp { operand, .. } => collect_idents(operand, out),
        Expr::Cast { expr, .. } => collect_idents(expr, out),
        Expr::Box { value, .. } => collect_idents(value, out),
        Expr::Deref { target, .. } => collect_idents(target, out),
        Expr::Try { expr, .. } => collect_idents(expr, out),
        Expr::FieldAccess { object, .. } => collect_idents(object, out),
        Expr::Index { array, index, .. } => {
            collect_idents(array, out);
            collect_idents(index, out);
        }
        Expr::Call { args, .. } | Expr::EnumVariant { args, .. } => {
            for a in args.iter() {
                collect_idents(a, out);
            }
        }
        Expr::ArrayLiteral(elems, _, _) => {
            for e in elems.iter() {
                collect_idents(e, out);
            }
        }
        Expr::StructLiteral { fields, .. } => {
            for (_, v) in fields.iter() {
                collect_idents(v, out);
            }
        }
        Expr::Match {
            scrutinee, arms, ..
        } => {
            collect_idents(scrutinee, out);
            for (_, b) in arms.iter() {
                collect_block_idents(b, out);
            }
        }
        Expr::If {
            condition,
            then_body,
            else_body,
            ..
        } => {
            collect_idents(condition, out);
            collect_block_idents(then_body, out);
            if let Some(eb) = else_body {
                collect_block_idents(eb, out);
            }
        }
        Expr::MethodCall { receiver, args, .. } => {
            collect_idents(receiver, out);
            for a in args.iter() {
                collect_idents(a, out);
            }
        }
        Expr::Integer(..)
        | Expr::Float(..)
        | Expr::Bool(..)
        | Expr::StringLiteral(..)
        | Expr::Error(..) => {}
    }
}

fn collect_block_idents(block: &Block, out: &mut HashSet<Symbol>) {
    for s in block.stmts.iter() {
        match s {
            Stmt::Let { value, .. }
            | Stmt::Return(value, _)
            | Stmt::Expr(value, _)
            | Stmt::Assign { value, .. } => collect_idents(value, out),
            Stmt::AssignPlace { target, value, .. } => {
                collect_idents(target, out);
                collect_idents(value, out);
            }

            Stmt::While {
                condition, body, ..
            } => {
                collect_idents(condition, out);
                collect_block_idents(body, out);
            }
            Stmt::For { from, to, body, .. } => {
                collect_idents(from, out);
                collect_idents(to, out);
                collect_block_idents(body, out);
            }
            Stmt::Error(_) => {}
        }
    }
    if let Some(t) = block.tail {
        collect_idents(t, out);
    }
}

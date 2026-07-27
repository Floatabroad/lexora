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

#[derive(Clone, Copy, PartialEq, Eq)]
enum BindState {
    Copy,
    Owning(MoveState),
}

pub struct MoveChecker<'a> {
    types: &'a HashMap<ExprId, Type>,
    interner: &'a Interner,
    scopes: Vec<HashMap<Symbol, BindState>>,
    moves: HashSet<ExprId>,
    errors: Vec<LexoraError>,
    structs: HashMap<Symbol, Vec<(Symbol, Type)>>,
    enums: HashMap<Symbol, (Vec<Symbol>, Vec<(Symbol, Vec<Type>)>)>,
}

impl<'a> MoveChecker<'a> {
    pub fn new(types: &'a HashMap<ExprId, Type>, interner: &'a Interner) -> Self {
        MoveChecker {
            types,
            interner,
            scopes: Vec::new(),
            moves: HashSet::new(),
            errors: Vec::new(),
            structs: HashMap::new(),
            enums: HashMap::new(),
        }
    }
    pub fn check(mut self, program: &Program) -> (HashSet<ExprId>, Vec<LexoraError>) {
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
        (self.moves, self.errors)
    }
    fn enter(&mut self) {
        self.scopes.push(HashMap::new());
    }
    fn exit(&mut self) {
        self.scopes.pop();
    }
    fn bind(&mut self, sym: Symbol, owning: bool) {
        let state = if owning {
            BindState::Owning(MoveState::Owned)
        } else {
            BindState::Copy
        };
        self.scopes.last_mut().unwrap().insert(sym, state);
    }
    fn lookup(&self, sym: Symbol) -> Option<BindState> {
        self.scopes.iter().rev().find_map(|s| s.get(&sym)).copied()
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
    fn expr_is_owning(&self, expr: &Expr) -> bool {
        self.types
            .get(&expr.id())
            .map_or(false, |t| self.is_owning(t))
    }
    fn check_place_base(&mut self, base: &Expr) {
        if base.is_place() {
            return;
        }
        if self.expr_is_owning(base) || matches!(base, Expr::Deref { .. }) {
            self.errors.push(LexoraError::Custom {
                message: "gecici deger uzerinden alan/eleman erisimi yapilamaz; once bir degiskene baglayin".to_string(),
                span: base.span(),
            });
        }
    }
    fn check_function(&mut self, func: &Function) {
        self.enter();
        for (sym, ty) in func.params.iter() {
            self.bind(*sym, self.is_owning(ty));
        }
        for stmt in func.body.stmts.iter() {
            self.check_stmt(stmt);
        }
        if let Some(tail) = func.body.tail {
            self.consume_expr(tail);
        }
        self.exit();
    }
    fn check_stmt(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Let { name, value, .. } => {
                self.consume_expr(value);
                let owning = self.expr_is_owning(value);
                self.bind(*name, owning);
            }
            Stmt::Assign { name, value, span } => {
                self.consume_expr(value);
                if matches!(self.lookup(*name), Some(BindState::Owning(_))) {
                    self.errors.push(LexoraError::Custom {
                        message: "owning bir degiskene tekrar atama henuz desteklenmiyor"
                            .to_string(),
                        span: *span,
                    });
                }
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
            Stmt::AssignPlace {
                target,
                value,
                span,
            } => {
                if self.expr_is_owning(target) {
                    self.errors.push(LexoraError::Custom {
                        message: "owning hedefe yeniden atama henuz desteklenmiyor (eski deger sizardi)".to_string(),
                        span: *span,
                    });
                }
                self.read_place(target);
                self.consume_expr(value);
            }
            Stmt::Error(_) => {}
        }
    }
    fn check_loop(&mut self, body: &Block, span: Span, loop_var: Option<Symbol>) {
        let entry = self.scopes.clone();
        self.enter();
        if let Some(v) = loop_var {
            self.bind(v, false);
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
                if let BindState::Owning(MoveState::Owned) = st {
                    if let Some(BindState::Owning(s2)) =
                        self.scopes.get(i).and_then(|m| m.get(sym)).copied()
                    {
                        if s2 != MoveState::Owned {
                            moved_outer = true;
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
                let prev = match self.lookup_mut(*sym) {
                    Some(BindState::Owning(state)) => {
                        let prev = *state;
                        *state = MoveState::Moved;
                        Some(prev)
                    }
                    _ => None,
                };
                if let Some(prev) = prev {
                    self.moves.insert(*id);
                    match prev {
                        MoveState::Owned => {}
                        MoveState::Moved => self.errors.push(LexoraError::Custom {
                            message: "tasinmis deger tekrar kullanildi".to_string(),
                            span: *span,
                        }),
                        MoveState::MaybeMoved => self.errors.push(LexoraError::Custom {
                            message: "kosullu tasinmis olabilecek deger kullanildi".to_string(),
                            span: *span,
                        }),
                    }
                }
            }
            Expr::Box { value, .. } => self.consume_expr(value),
            Expr::Deref { target, span, id } => {
                if self.expr_is_owning(expr) {
                    if matches!(target, Expr::Identifier(..)) {
                        self.consume_expr(target);
                        self.moves.insert(*id);
                    } else {
                        self.errors.push(LexoraError::Custom{
                            message: "Box icinden owning deger deref ile tasinamaz (yalnizca *degisken formunda tasinabilir)".to_string(),
                            span: *span,
                        });
                        self.read_place(target);
                    }
                } else {
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
                        .filter(|s| {
                            matches!(self.lookup(*s), Some(BindState::Owning(MoveState::Owned)))
                        })
                        .collect();
                    self.read_place(left);
                    self.read_place(right);
                    for s in shared {
                        if !matches!(self.lookup(s), Some(BindState::Owning(MoveState::Owned))) {
                            self.errors.push(LexoraError::Custom {
                                message: "karsilastirmanin iki tarafi ayni owning degeri kullaniyor ve biri onu tasiyor; once bir degiskene baglayin".to_string(),
                                span: *span,
                            });
                        }
                    }
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
            Expr::FieldAccess { object, span, .. } => {
                if self.expr_is_owning(expr) {
                    self.errors.push(LexoraError::Custom {
                        message: "owning alan struct'tan tasinamaz; yerinde odunc alin".to_string(),
                        span: *span,
                    });
                }
                self.check_place_base(object);
                self.read_place(object)
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
                            let owning = ftys.get(i).map_or(false, |t| self.is_owning(t));
                            self.bind(*b, owning);
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
            Expr::Integer(..) | Expr::Bool(..) | Expr::StringLiteral(..) | Expr::Error(..) => {}
        }
    }
    fn read_place(&mut self, expr: &Expr) {
        match expr {
            Expr::Identifier(sym, span, _) => {
                if let Some(BindState::Owning(state)) = self.lookup(*sym) {
                    match state {
                        MoveState::Owned => {}
                        MoveState::Moved => self.errors.push(LexoraError::Custom {
                            message: "tasinmis deger kullanildi".to_string(),
                            span: *span,
                        }),
                        MoveState::MaybeMoved => self.errors.push(LexoraError::Custom {
                            message: "kosullu tasinmis olabilecek deger kullanildi".to_string(),
                            span: *span,
                        }),
                    }
                }
            }
            Expr::Deref { target, .. } => self.read_place(target),
            Expr::Index { array, index, .. } => {
                self.check_place_base(array);
                self.read_place(array);
                self.consume_expr(index);
            }
            Expr::FieldAccess { object, .. } => {
                self.check_place_base(object);
                self.read_place(object)
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

fn join_state(a: BindState, b: BindState) -> BindState {
    match (a, b) {
        (BindState::Owning(x), BindState::Owning(y)) => {
            if x == y {
                BindState::Owning(x)
            } else {
                BindState::Owning(MoveState::MaybeMoved)
            }
        }
        _ => a,
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
                    let sb = mb.get(sym).copied().unwrap_or(*sa);
                    (*sym, join_state(*sa, sb))
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

        Expr::Integer(..) | Expr::Bool(..) | Expr::StringLiteral(..) | Expr::Error(..) => {}
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

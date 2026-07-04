use crate::ast::*;
use crate::error::LexoraError;
use crate::span::Span;
use crate::symbol::Symbol;
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
    scopes: Vec<HashMap<Symbol, BindState>>,
    moves: HashSet<ExprId>,
    errors: Vec<LexoraError>,
   enums: HashMap<Symbol, Vec<(Symbol, Vec<Type>)>>,
}

impl<'a> MoveChecker<'a> {
    pub fn new(types: &'a HashMap<ExprId, Type>) -> Self {
        MoveChecker {
            types,
            scopes: Vec::new(),
            moves: HashSet::new(),
            errors: Vec::new(),
            enums: HashMap::new(),
        }
    }
    pub fn check(mut self, program: &Program) -> (HashSet<ExprId>, Vec<LexoraError>) {
        for e in &program.enums {
            self.enums.insert(e.name, e.variants.clone());
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
    fn is_owning(&self, ty: &Type) -> bool {
        match ty {
            Type::Box(_) => true,
            Type::Enum(e) => self.enums.get(e).map_or(false, |vs|{
                vs.iter().any(|(_, ftys)| ftys.iter().any(|t| self.is_owning(t)))
            }),
            _ => false
        }
    }
    fn expr_is_owning(&self, expr: &Expr) -> bool {
        self.types.get(&expr.id()).map_or(false, |t| self.is_owning(t))
    }
    fn check_function(&mut self, func: &Function) {
        self.enter();
        for (sym, ty) in func.params.iter(){
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
            Stmt::Let {name, value, ..} =>{
                self.consume_expr(value);
                let owning = self.expr_is_owning(value);
                self.bind(*name, owning);
            }
            Stmt::Assign{name, value, span} => {
                self.consume_expr(value);
                if matches!(self.lookup(*name), Some(BindState::Owning(_))) {
                    self.errors.push(LexoraError::Custom{
                        message: "owning bir degiskene tekrar atama henuz desteklenmiyor".to_string(),
                        span: *span
                    });
                }
            }
            Stmt::Return(expr, _) => self.consume_expr(expr),
            Stmt::Expr(expr, span) => {
                if self.expr_is_owning(expr) {
                    self.errors.push(LexoraError::Custom{
                        message: "owning deger bir degiskene baglanmali (sahipsiz kalamaz)".to_string(),
                        span: *span
                    });
                }
                self.consume_expr(expr);
            }
          
            Stmt::While {condition, body, span} => {
                self.consume_expr(condition);
                self.check_loop(body, *span, None);
            }
            Stmt::For {var, from, to, body, span} =>{
                self.consume_expr(from);
                self.consume_expr(to);
                self.check_loop(body, *span, Some(*var));
            }
            Stmt::AssignIndex {index, value, ..} => {
                self.consume_expr(index);
                self.consume_expr(value);
            }
            Stmt::AssignField {value, ..} => self.consume_expr(value),
            Stmt::AssignDeref {target, value, span} => {
                if self.expr_is_owning(target) {
                    self.errors.push(LexoraError::Custom{
                        message: "owning pointee'ye deref-atama henuz desteklenmiyor (eski deger sizardi)".to_string(),
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
        for(i, scope) in entry.iter().enumerate() {
            for (sym, st) in scope.iter(){
                if let BindState::Owning(MoveState::Owned) = st {
                    if let Some(BindState::Owning(s2)) = self.scopes.get(i).and_then(|m| m.get(sym)).copied(){
                        if s2 != MoveState::Owned {
                            moved_outer = true;
                        }
                    }
                }
            }
        }
        if moved_outer {
            self.errors.push(LexoraError::Custom{
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
                        MoveState::Moved => self.errors.push(LexoraError::Custom{
                            message: "tasinmis deger tekrar kullanildi".to_string(),
                            span: *span,
                        }),
                        MoveState::MaybeMoved => self.errors.push(LexoraError::Custom{
                            message: "kosullu tasinmis olabilecek deger kullanildi".to_string(),
                            span: *span,
                        }),
                    }
                }
            }
            Expr::Box{value,..} =>self.consume_expr(value),
            Expr::Deref{target, span, id} => {
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
                }else{
                    self.read_place(target);
                }
            }
            Expr::Call{args,..} =>{
                for arg in args.iter() {
                    self.consume_expr(arg);
                }
            }
            Expr::BinaryOp {left, right, ..} => {
                self.consume_expr(left);
                self.consume_expr(right);
            }
            Expr::UnaryOp {operand,..} =>self.consume_expr(operand),
            Expr::Cast{expr,..} => self.consume_expr(expr),
            Expr::Index{array, index, ..} =>{
                self.read_place(array);
                self.consume_expr(index);
            }
            Expr::FieldAccess {object,..} => self.read_place(object),
            Expr::ArrayLiteral(elems, _,_) => {
                for e in elems.iter() {
                    self.consume_expr(e);
                }
            }
            Expr::StructLiteral {fields, ..} => {
                for(_, v) in fields.iter() {
                    self.consume_expr(v);
                }
            }
            Expr::EnumVariant { args, .. } => {
                for a in args.iter() { self.consume_expr(a); }
            }
            Expr::Match { scrutinee, arms, .. } => {
                self.consume_expr(scrutinee);
                let entry = self.scopes.clone();
                let mut joined: Option<Vec<HashMap<Symbol, BindState>>> = None;
                for (pat, body) in arms.iter() {
                    self.scopes = entry.clone();
                    self.enter();
                    if let Pattern::Variant { enum_name, variant, bindings } = pat {
                        let ftys = self.enums.get(enum_name)
                            .and_then(|vs| vs.iter().find(|(v, _)| v == variant))
                            .map(|(_, t)| t.clone())
                            .unwrap_or_default();
                        for (i, b) in bindings.iter().enumerate() {
                            let owning = ftys.get(i).map_or(false, |t| self.is_owning(t));
                            self.bind(*b, owning);
                        }
                    }
                    for s in body.stmts.iter() { self.check_stmt(s); }
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
            Expr::If { condition, then_body, else_body, .. } => {
                self.consume_expr(condition);
                let entry = self.scopes.clone();
                self.enter();
                for s in then_body.stmts.iter() { self.check_stmt(s); }
                if let Some(t) = then_body.tail {
                    self.consume_expr(t);
                }
                self.exit();
                let after_then = self.scopes.clone();
                self.scopes = entry.clone();
                if let Some(eb) = else_body {
                    self.enter();
                    for s in eb.stmts.iter() { self.check_stmt(s); }
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
        match expr{
            Expr::Identifier(sym, span, _) => {
                if let Some(BindState::Owning(state)) = self.lookup(*sym) {
                    match state {
                        MoveState::Owned => {}
                        MoveState::Moved => self.errors.push(LexoraError::Custom{
                            message: "tasinmis deger kullanildi".to_string(),
                            span: *span,
                        }),
                        MoveState::MaybeMoved => self.errors.push(LexoraError::Custom{
                            message: "kosullu tasinmis olabilecek deger kullanildi".to_string(),
                            span: *span,
                        })
                    }
                }
            }
            Expr::Deref{target, ..} => self.read_place(target),
            Expr::Index{array, index, ..} =>{
                self.read_place(array);
                self.consume_expr(index);
            }
            Expr::FieldAccess {object,..} => self.read_place(object),
            other => self.consume_expr(other),
        }
    }
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
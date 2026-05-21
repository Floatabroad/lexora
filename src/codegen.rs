use crate::ast::*;
use std::fmt::Write;
use std::collections::HashMap;
pub struct CodeGen {
    output: String,
    globals: String,
    next_temp: usize,
    next_block: usize,
    next_str: usize,
    locals: HashMap<String, Type>,
    fn_types: HashMap<String, Type>,
}

impl CodeGen {
    pub fn new() -> Self {
        CodeGen {
            output: String::new(),
            globals: String::new(),
            next_temp: 0,
            next_block : 0,
            next_str: 0,
            locals: HashMap::new(),
            fn_types: HashMap::new(),
        }
    }
    fn fresh_temp(&mut self) -> String {
        let t = format!("%t{}", self.next_temp);
        self.next_temp += 1;
        t
    }
    fn expr_llvm_type(&self, expr: &Expr) -> &'static str {
        match expr {
            Expr::Integer(_) => "i32",
            Expr::Bool(_) => "i1",
            Expr::Identifier(name) => {
                if let Some(ty) = self.locals.get(name) {
                    Self::ty_to_llvm(ty)
                } else {
                    "i32"
                }
            }
            Expr::BinaryOp {op, ..} => match op {
                BinaryOperator::Eq | BinaryOperator::NotEq |
                BinaryOperator::Less | BinaryOperator::Greater |
                BinaryOperator::And | BinaryOperator::Or => "i1",
                _ => "i32",
            },
            Expr::Call {name, ..} => {
                self.fn_types.get(name.as_str())
                    .map(|t| Self::ty_to_llvm(t))
                    .unwrap_or("i32")
            }
            Expr::StringLiteral(_) => "ptr",
        }
    }
    fn ty_to_llvm(t: &Type) -> &'static str {
        match t {
            Type::I32 => "i32",
            Type::I64 => "i64",
            Type::Bool => "i1",
            Type::Void => "void",
        }
    }
    pub fn generate(&mut self, program: &Program) -> String{
        let mut result = String::new();
        writeln!(result, "@.fmt = private constant [4 x i8] c\"%d\\0A\\00\"").unwrap();
        writeln!(result, "@.fmt64 = private constant [6 x i8] c\"%lld\\0A\\00\"").unwrap();
        writeln!(result, "declare i32 @printf(ptr, ...)\n").unwrap();
        for func in &program.functions {
            self.fn_types.insert(func.name.clone(), func.return_type.clone());
        }
        for func in &program.functions {
            self.gen_function(func);
        }
        result.push_str(&self.globals);
        result.push_str(&self.output);
        result
    }
    fn gen_function(&mut self, func: &Function) {
        self.next_temp = 0;
        self.next_block = 0;
        self.locals.clear();

        let params: Vec<String> = func.params.iter()
            .map(|(name, ty)| format!("{} %{}", Self::ty_to_llvm(ty), name))
            .collect();
        writeln!(
            self.output,
            "define {} @{}({}) {{",
            Self::ty_to_llvm(&func.return_type),
            func.name,
            params.join(", ")
        ).unwrap();

        writeln!(self.output, "entry:").unwrap();

        for stmt in &func.body {
            self.gen_statement(stmt);
        }
        if matches!(func.return_type, Type::Void) {
            writeln!(self.output, "  ret void").unwrap();
        }
        writeln!(self.output, "}}\n").unwrap();
    }

    fn gen_statement(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Return(expr, _) => {
                let val = self.gen_expr(expr);
                writeln!(self.output, "  ret i32 {}", val).unwrap();
            }

            Stmt::Expr(expr, _) => {
                self.gen_expr(expr);
            }
            Stmt::Let {name , ty, value, .. } => {
                let val = self.gen_expr(value);
                let llvm_ty = Self::ty_to_llvm(ty);
                writeln!(self.output, "  %{}.addr = alloca {}", name, llvm_ty).unwrap();
                writeln!(self.output, "  store {} {}, ptr %{}.addr", llvm_ty, val, name).unwrap();
                self.locals.insert(name.clone(), ty.clone());
            }
            Stmt::Assign {name, value, ..} => {
                let val = self.gen_expr(value);
                let llvm_ty = match self.locals.get(name) {
                    Some(ty) => Self::ty_to_llvm(ty),
                    None => "i32",
                };
                writeln!(self.output, "  store {} {}, ptr %{}.addr", llvm_ty, val, name).unwrap();
            }
            Stmt::If { condition, then_body, else_body, .. } => {
                let cond_val = self.gen_expr(condition);
                let id = self.next_block;
                self.next_block += 1;
                let then_label = format!("then{}", id);
                let merge_label = format!("merge{}", id);

                if let Some(else_stmts) = else_body {
                    let else_label = format!("else{}", id);
                    writeln!(self.output, "  br i1 {}, label %{}, label %{}", cond_val, then_label, else_label).unwrap();

                    writeln!(self.output, "{}:", then_label).unwrap();
                    for s in then_body { self.gen_statement(s); }
                    if !matches!(then_body.last(),
  Some(Stmt::Return(_, _))) {
                        writeln!(self.output, "  br label %{}",
                                 merge_label).unwrap();
                    }




                    writeln!(self.output, "{}:", else_label).unwrap();
                    for s in else_stmts { self.gen_statement(s); }
                    if !matches!(else_stmts.last(), Some(Stmt::Return(_, _))) {
                        writeln!(self.output, "  br label %{}", merge_label).unwrap();
                    }

                    writeln!(self.output, "{}:", merge_label).unwrap();
                } else {
                    writeln!(self.output, "  br i1 {}, label %{}, label %{}", cond_val, then_label, merge_label).unwrap();

                    writeln!(self.output, "{}:", then_label).unwrap();
                    for s in then_body { self.gen_statement(s); }
                    if !matches!(then_body.last(), Some(Stmt::Return(_, _))) {
                        writeln!(self.output, "  br label %{}", merge_label).unwrap();
                    }

                    writeln!(self.output, "{}:", merge_label).unwrap();
                }
            }
            Stmt::While {condition, body, ..} => {
                let id = self.next_block;
                self.next_block += 1;
                let cond_label = format!("loop_cond{}", id);
                let body_label = format!("loop_body{}", id);
                let end_label = format!("loop_end{}", id);

                writeln!(self.output, "  br label %{}", cond_label).unwrap();
                writeln!(self.output, "{}:", cond_label).unwrap();
                let cond_val = self.gen_expr(condition);
                writeln!(self.output, "  br i1 {}, label %{}, label %{}", cond_val, body_label, end_label).unwrap();

                writeln!(self.output, "{}:", body_label).unwrap();
                for s in body { self.gen_statement(s); }
                writeln!(self.output, "  br label %{}", cond_label).unwrap();

                writeln!(self.output, "{}:", end_label).unwrap();
            }

            Stmt::For {var, from, to, body, ..} => {
                let id = self.next_block;
                self.next_block += 1;
                let cond_label = format!("for_cond{}", id);
                let body_label = format!("for_body{}", id);
                let end_label = format!("for_end{}", id);

                let from_val = self.gen_expr(from);
                writeln!(self.output, "  %{}.addr = alloca i32", var).unwrap();
                writeln!(self.output, "  store i32 {}, ptr %{}.addr", from_val, var).unwrap();
                self.locals.insert(var.clone(), Type::I32);

                writeln!(self.output, "  br label %{}", cond_label).unwrap();
                writeln!(self.output, "{}:", cond_label).unwrap();

                let cur = self.fresh_temp();

                writeln!(self.output, "  {} = load i32, ptr %{}.addr", cur, var).unwrap();
                let to_val = self.gen_expr(to);
                let cond = self.fresh_temp();
                writeln!(self.output, "  {} = icmp slt i32 {}, {}", cond, cur, to_val).unwrap();
                writeln!(self.output, "  br i1 {}, label %{}, label %{}", cond, body_label, end_label).unwrap();

                writeln!(self.output, "{}:", body_label).unwrap();
                for s in body { self.gen_statement(s); }

                let inc_val = self.fresh_temp();
                let cur2 = self.fresh_temp();
                writeln!(self.output, "  {} = load i32, ptr %{}.addr", cur2, var).unwrap();
                writeln!(self.output, "  {} = add i32 {}, 1", inc_val, cur2).unwrap();
                writeln!(self.output, "  store i32 {}, ptr %{}.addr", inc_val, var).unwrap();
                writeln!(self.output, "  br label %{}", cond_label).unwrap();

                writeln!(self.output, "{}:", end_label).unwrap();
            }

        }
    }

    fn gen_expr(&mut self, expr: &Expr) -> String {
        match expr {
            Expr::Integer(n) => n.to_string(),
            Expr::BinaryOp  { left, op, right } => {
                let l = self.gen_expr(left);
                let r = self.gen_expr(right);
                let result = self.fresh_temp();

                let instr = match op {
                    BinaryOperator::Add     => "add",
                    BinaryOperator::Sub     => "sub",
                    BinaryOperator::Mul     => "mul",
                    BinaryOperator::Div     => "sdiv",
                    BinaryOperator::Eq      => "icmp eq",
                    BinaryOperator::NotEq   => "icmp ne",
                    BinaryOperator::Less    => "icmp slt",
                    BinaryOperator::Greater => "icmp sgt",
                    BinaryOperator::And     => "and",
                    BinaryOperator::Or      => "or",
                };
               // writeln!(self.output, "  {} = {} i32 {}, {}", result, instr, l, r).unwrap();
                let ty = match op {
                    BinaryOperator::And | BinaryOperator::Or => "i1", _ => "i32",
                };
                writeln!(self.output, "  {} = {} {} {}, {}", result, instr,ty, l, r).unwrap();
                result
            }
            Expr::Bool(b) => {
                if *b { "1".to_string() } else { "0".to_string() }
            }
            Expr::Identifier(name) => {
                if let Some(ty) = self.locals.get(name) {
                    let llvm_ty = Self::ty_to_llvm(ty);
                    let result = self.fresh_temp();
                    writeln!(self.output, "  {} = load {}, ptr %{}.addr", result, llvm_ty, name).unwrap();
                    result
                } else {
                    format!("%{}", name)
                }
            }
            Expr::Call{name, args} => {
                if name == "print" {
                    for arg in args {
                        let result = self.fresh_temp();
                        match arg {
                            Expr::StringLiteral(s) => {
                                let id = self.next_str;
                                self.next_str += 1;
                                let len = s.len() + 2;
                                writeln!(self.globals, "@str{} = private constant [{} x i8] c\"{}\\0A\\00\"", id, len, s).unwrap();
                                writeln!(self.output, "  {} = call i32 (ptr, ...) @printf(ptr @str{})", result, id).unwrap();
                            }
                            _ => {
                                let ty = self.expr_llvm_type(arg);
                                let val = self.gen_expr(arg);
                                if ty == "i64" {
                                    writeln!(self.output, "  {} = call i32 (ptr, ...) @printf(ptr @.fmt64, i64 {})", result, val).unwrap();
                                }else {
                                    writeln!(self.output, "  {} = call i32 (ptr, ...) @printf(ptr @.fmt, i32 {})", result, val).unwrap();
                                }
                            }
                        }
                    }
                    return "0".to_string();
                }
                let arg_values: Vec<String> = args.iter()
                    .map(|a| self.gen_expr(a))
                    .collect();

                let arg_str = arg_values.iter()
                    .map(|v| format!("i32 {}", v))
                    .collect::<Vec<_>>()
                    .join(", ");

                let ret_ty = self.fn_types.get(name)
                    .map(|t| Self::ty_to_llvm(t))
                    .unwrap_or("i32");

                if ret_ty == "void" {
                    writeln!(self.output, "  call void @{}({})", name, arg_str).unwrap();
                    "0".to_string()
                }else {
                    let result = self.fresh_temp();
                    writeln!(self.output, "  {} = call {} @{}({})", result, ret_ty, name, arg_str).unwrap();
                    result
                }
            }
            Expr::StringLiteral(_) => panic!("StringLiteral sadece print icinde kullanılabilir"),
        }
    }
}
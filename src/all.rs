#[derive(Debug, Clone)]
pub enum Expr {
    Integer(i64),
    Identifier(String),
    BinaryOp{
        left: Box<Expr>,
        op: BinaryOperator,
        right: Box<Expr>,
    },
    Call {
        name: String,
        args: Vec<Expr>,
    },

    Bool(bool),
    StringLiteral(String),
}

#[derive(Debug, Clone)]
pub enum BinaryOperator {
    Add,
    Sub,
    Mul,
    Div,
    Eq,
    NotEq,
    Less,
    Greater,
    And,
    Or,
}
#[derive(Debug, Clone)]
pub enum Stmt {
    Let {
        name: String,
        ty: Type,
        value: Expr,
        line: usize,
    },
    Return(Expr, usize),
    Expr(Expr, usize),
    If{
        condition: Expr,
        then_body: Vec<Stmt>,
        else_body: Option<Vec<Stmt>>,
        line: usize,
    },
    Assign{
        name: String,
        value: Expr,
        line: usize,
    },
    While{
        condition: Expr,
        body: Vec<Stmt>,
        line: usize,
    },
    For {
        var: String,
        from: Expr,
        to: Expr,
        body: Vec<Stmt>,
        line: usize,
    }

}
#[derive(Debug, Clone)]
pub enum Type {
    I32,
    I64,
    Bool,
    Void,
}
#[derive(Debug, Clone)]
pub struct Function {
    pub name: String,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Vec<Stmt>,
}
#[derive(Debug, Clone)]
pub struct Program {
    pub functions: Vec<Function>,
}use crate::ast::*;
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
}use crate::ast::*;
use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum Value {
    Int(i64),
    Bool(bool),
}

pub struct Interpreter {
    variables: HashMap<String, Value>,
    functions: HashMap<String, Function>,
}

impl Interpreter {
    pub fn new() -> Self {
        Interpreter {
            variables: HashMap::new(),
            functions: HashMap::new(),
        }
    }
    pub fn run_program(&mut self, program: &Program) {
        for func in &program.functions {
            self.functions.insert(func.name.clone(), func.clone());
        }
    }
    pub fn call_function(&mut self, name: &str, args: Vec<Value>) -> Value{
        let func = match self.functions.get(name) {
            Some(f) => f.clone(),
            None => panic!("Fonksiyon bulunamadi: '{}'", name),
        };
        let mut saved_vars = self.variables.clone();
        self.variables.clear();

        for ((param_name, _ ), value) in func.params.iter().zip(args.into_iter()) {
            self.variables.insert(param_name.clone(), value);
        }
        let result = self.run_body(&func.body);
        self.variables = saved_vars;
        result
    }
    fn run_body(&mut self, stmts: &[Stmt]) -> Value {
        for stmt in stmts {
            if let Some(val) = self.run_statement(stmt) {
                return val;
            }
        }
        panic!("Fonksiyon deger dondurmedi");
    }
    fn run_statement(&mut self, stmt: &Stmt) -> Option<Value> {
        match stmt {
            Stmt::Let {name, value, ..} => {
                let val = self.eval_expr(value);
                self.variables.insert(name.clone(), val);
                None
            }
            Stmt::Return(expr) => {
                Some(self.eval_expr(expr))
            }
            Stmt::Expr(expr) => {
                self.eval_expr(expr);
                None
            }
            Stmt::If { condition, then_body, else_body} => {
                let cond = self.eval_expr(condition);
                let body = match cond {
                    Value::Bool(true) => Some(then_body.clone()),
                    Value::Bool(false) => else_body.clone(),
                    _ => panic!("If kosulu bool olmali"),
                };
                if let Some(stmts) = body {
                    for stmt in &stmts {
                        if let Some(val) = self.run_statement(stmt) {
                            return Some(val);
                        }
                    }
                }
                None
            }
            Stmt::While { condition, body} => {
                loop {
                    let cond = self.eval_expr(condition);
                    match cond {
                        Value::Bool(true) => {
                            for stmt in body {
                                if let Some(val) = self.run_statement(stmt) {
                                    return Some(val);
                                }
                            }
                        }
                        Value::Bool(false) => return None,
                        _ => panic!("While kosulu bool olmali"),
                    }
                }
                None
            }
            Stmt::Assign{name, value} => {
                let val = self.eval_expr(value);
                if !self.variables.contains_key(name) {
                    panic!("Tanimlanmamis degisken: '{}'", name);
                }
                self.variables.insert(name.clone(), val);
                None
            }
        }
    }
    fn eval_expr(&mut self, expr: &Expr) -> Value {
        match expr {
            Expr::Integer(n) => Value::Int(*n),
            Expr::Identifier(name) => {
                match self.variables.get(name) {
                    Some(v) => v.clone(),
                    None => panic!("Tanimsiz degisken: '{}'", name),
                }
            }
            Expr::BinaryOp {left, op, right} => {
                let l = self.eval_expr(left);
                let r = self.eval_expr(right);
                match (l, r) {
                    (Value::Int(a), Value::Int(b)) => match op {
                        BinaryOperator::Add => Value::Int(a + b),
                        BinaryOperator::Sub => Value::Int(a - b),
                        BinaryOperator::Mul => Value::Int(a * b),
                        BinaryOperator::Div => Value::Int(a / b),
                        BinaryOperator::Eq => Value::Bool(a == b),
                        BinaryOperator::NotEq => Value::Bool(a != b),
                        BinaryOperator::Less => Value::Bool(a < b),
                        BinaryOperator::Greater => Value::Bool(a > b),
                    },
                    _ => panic!("gecersiz operand tipleri"),
                }
            }
            Expr::Bool(b) => Value::Bool(*b),

            Expr::Call { name, args } => {
                let values: Vec<Value> = args.iter()
                    .map(|a| self.eval_expr(a))
                    .collect();

                if name == "print" {
                    for v in &values {
                        match v {
                            Value::Int(n) => println!("{}", n),
                            Value::Bool(b) => println!("{}", b),
                        }
                    }
                    return Value::Int(0);
                }
                self.call_function(name, values)
            }
        }
    }
}
use std::thread::sleep;

#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    //literals
    Integer(i64),
    Identifier(String),
    StringLiteral(String),

    //Keywords
    Let, 
    Fn, 
    Return, 
    If,
    Else,
    True,
    False,
    While,
    For,
    In,
    DotDot, // ..
    And,
    Or,
    
    //Types
    I32,
    I64,
    Bool,
    Void,

    //Operators
    Plus,
    Minus,
    Star,
    Slash,
    Equals,
    EqualsEquals,
    Bang,
    BangEquals,
    Less,
    Greater,

    //Delimiter,
    Semicolon,
    Colon,
    Comma,
    Arrow, // ->
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,

    //Special
    Eof,
}

pub struct Lexer {
    input: Vec<char>,
    pos: usize,
    pub line: usize,
}


impl Lexer {
    pub fn new(source: &str) -> Self {
        Lexer{
            input: source.chars().collect(),
            pos: 0,
            line: 1,
        }
    }
    fn current(&self) -> char {
        if self.pos < self.input.len() {
            self.input[self.pos]
        } else {
            '\0'
        }
    }
    fn peek(&self) -> char {
        if self.pos + 1 < self.input.len() {
            self.input[self.pos + 1]
        }  else {
            '\0'
        }
    }
    fn advance(&mut self){
        if self.pos < self.input.len() && self.input[self.pos] == '\n' {
            self.line += 1;
        }
        self.pos += 1;
    }
    pub fn next_token(&mut self) -> Token {
        //whitespace atla
        while self.current().is_whitespace() {
            self.advance();
        }
        let ch = self.current();

        match ch {
            '\0' => Token::Eof,
            '+' => { self.advance(); Token::Plus }
            '-' => {
                if self.peek() == '>' {
                    self.advance();
                    self.advance();
                    Token::Arrow
                } else {
                    self.advance();
                    Token::Minus
                }
            }
            '*' => { self.advance(); Token::Star }
            '/' => { self.advance(); Token::Slash }
            '.' => {
                if self.peek() == '.' {
                    self.advance();
                    self.advance();
                    Token::DotDot
                } else {
                    panic!("Unexpected character: .");
                }
            }
            ';' => { self.advance(); Token::Semicolon }
            ':' => { self.advance(); Token::Colon }
            ',' => { self.advance(); Token::Comma }
            '(' => { self.advance(); Token::LeftParen}
            ')' => { self.advance(); Token::RightParen}
            '{' => { self.advance(); Token::LeftBrace}
            '}' => { self.advance(); Token::RightBrace}
            '<' => { self.advance(); Token::Less}
            '>' => { self.advance(); Token::Greater}
            '=' => {
                if self.peek() == '=' {
                    self.advance();
                    self.advance();
                    Token::EqualsEquals
                } else {
                    self.advance();
                    Token::Equals
                }
            }
            '!' => {
                if self.peek() == '=' {
                    self.advance();
                    self.advance();
                    Token::BangEquals
                } else {
                    self.advance();
                    Token::Bang
                }
            }
            '0'..='9' => self.read_integer(),
            '"' => {
                self.advance();
                let mut s = String::new();
                while self.current() != '"' && self.current() != '\0' {
                    s.push(self.current());
                    self.advance();
                }
                if self.current() == '"' { self.advance();}
                Token::StringLiteral(s)
            }
            'a'..='z' | 'A'..='Z' | '_' => self.read_identifier(),
            _ => panic!("Unexpected character: {}", ch),
        }
    }
    fn read_integer(&mut self) -> Token {
        let mut number = String::new();
        while self.current().is_ascii_digit() {
            number.push(self.current());
            self.advance();
        }
        let value: i64 = number.parse().unwrap();
        Token::Integer(value)
    }
    fn read_identifier(&mut self) -> Token {
        let mut ident = String::new();
         while self.current().is_alphanumeric() || self.current() == '_' {
            ident.push(self.current());
            self.advance();
         }
        match ident.as_str() {
            "for" => Token::For,
            "in" => Token::In,
            "let" => Token::Let,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "i32" => Token::I32,
            "i64" => Token::I64,
            "bool" => Token::Bool,
            "void" => Token::Void,
            "true" => Token::True,
            "false" => Token::False,
            "while" => Token::While,
            "and" => Token::And,
            "or" => Token::Or,
            _ => Token::Identifier(ident),
        }
    }
}mod lexer;
mod ast;
mod parser;
mod typechecker;
mod codegen;

//use std::os::unix::raw::off_t;
use lexer::Lexer;
use parser::Parser;
use typechecker::TypeChecker;
use codegen::CodeGen;
use std::env;
use std::fs;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Kullanim: lexora <dosya.lx>");
        std::process::exit(1);
    }
    let source = fs::read_to_string(&args[1])
        .unwrap_or_else(|_| panic!("Dosya okunamadi: {}", args[1]));
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(|| {
        let lexer = Lexer::new(&source);
        let mut parser = Parser::new(lexer);
        let program = parser.parse_program();

        let mut checker = TypeChecker::new();
        checker.check_program(&program);

        let mut codegen = CodeGen::new();
        codegen.generate(&program)
    });

    match result {
        Ok(ir) => {
            fs::write("output.ll", &ir).unwrap();
            println!("LLVM IR yazildi: output.ll");
            println!("---");
            println!("{}", ir);
        }
        Err(e) => {
            if let Some(msg) = e.downcast_ref::<String>() {
                eprintln!("{}", msg);
            } else if let Some(msg) = e.downcast_ref::<&str>() {
                eprintln!("{}", msg);
            } else {
                eprintln!("Bilinmeyen hata");
            }
            std::process::exit(1);
        }
    }
}use crate::lexer::{Lexer, Token};
use crate::ast::*;

pub struct Parser {
    lexer: Lexer,
    current: Token,
    current_line: usize,
}

impl Parser {
    pub fn new(mut lexer: Lexer) -> Self {
        let current = lexer.next_token();
        Parser { lexer, current, current_line: 1 }
    }
    fn advance(&mut self) -> Token {
        let prev = self.current.clone();
        self.current_line = self.lexer.line;
        self.current = self.lexer.next_token();
        prev
    }
    fn expect(&mut self, expected: Token) -> Token {
        if self.current == expected {
            self.advance()
        }else {
            panic!("Hata [satır {}]: Beklenen: {:?}, Bulunan: {:?}",self.current_line, expected, self.current);
        }
    }
    fn parse_type(&mut self) -> Type {
        match self.current.clone() {
            Token::I32 => { self.advance(); Type::I32 }
            Token::I64 => { self.advance(); Type::I64 }
            Token::Bool => { self.advance(); Type::Bool }
            Token::Void => { self.advance(); Type::Void }
            _ => panic!("Beklenen tip, bulunan: {:?}", self.current),
        }
    }
    pub fn parse_program(&mut self) -> Program {
        let mut functions = Vec::new();
        while self.current != Token::Eof {
            functions.push(self.parse_function());
        }
        Program { functions }
    }
    fn parse_function(&mut self) -> Function {
        self.expect(Token::Fn);

        let name = match self.advance() {
            Token::Identifier(n) => n,
            _ => panic!("Fonksiyon ismi bekleniyor"),
        };
        self.expect(Token::LeftParen);
        let mut params = Vec::new();

        while self.current != Token::RightParen {
            let param_name = match self.advance() {
                Token::Identifier(n) => n,
                _ => panic!("Parametre ismi bekleniyor"),
        };
            self.expect(Token::Colon);
            let param_type = self.parse_type();
            params.push((param_name, param_type));
            if self.current == Token::Comma {
                self.advance();
            }
        }
        self.expect(Token::RightParen);
        self.expect(Token::Arrow);
        let return_type = self.parse_type();
        self.expect(Token::LeftBrace);

        let mut body = Vec::new();
        while self.current != Token::RightBrace {
            body.push(self.parse_statement());
        }
        self.expect(Token::RightBrace);
        Function { name, params, return_type, body }
    }

    fn parse_statement(&mut self) -> Stmt {
        match self.current.clone() {
            Token::Let => {
                self.advance();
                let name = match self.advance() {
                    Token::Identifier(n) => n,
                    _ => panic!("Degisken ismi bekleniyor"),
                };
                self.expect(Token::Colon);
                let ty = self.parse_type();
                self.expect(Token::Equals);
                let value = self.parse_expr();
                self.expect(Token::Semicolon);
                Stmt::Let { name, ty, value, line: self.current_line }
            }
            Token::Return => {
                self.advance();
                let value = self.parse_expr();
                self.expect(Token::Semicolon);
                Stmt::Return(value, self.current_line)
            }
            Token::If => {
                self.advance();
                let condition = self.parse_expr();
                self.expect(Token::LeftBrace);
                let mut then_body = Vec::new();
                while self.current != Token::RightBrace {
                    then_body.push(self.parse_statement());
                }
                self.expect(Token::RightBrace);
                let else_body = if self.current == Token::Else {
                    self.advance();
                    if self.current == Token::If {
                        Some(vec![self.parse_statement()])
                    }else {
                        self.expect(Token::LeftBrace);
                        let mut body = Vec::new();
                        while self.current != Token::RightBrace {
                            body.push(self.parse_statement());
                        }
                        self.expect(Token::RightBrace);
                        Some(body)
                    }
                }else {
                    None
                };
                Stmt::If {condition, then_body, else_body, line: self.current_line}
            }
            Token::While => {
                self.advance();
                let condition = self.parse_expr();
                self.expect(Token::LeftBrace);
                let mut body = Vec::new();
                while self.current != Token::RightBrace {
                    body.push(self.parse_statement());
                }
                self.expect(Token::RightBrace);
                Stmt::While {condition, body, line: self.current_line}
            }
            Token::For => {
                let line = self.current_line;
                self.advance();
                let var = match self.advance() {
                    Token::Identifier(n) => n,
                    _ => panic!("Hata [satır {}]: for döngüsünde değişken ismi bekleniyor", self.current_line),
                };
                self.expect(Token::In);
                let from = self.parse_primary();
                self.expect(Token::DotDot);
                let to = self.parse_primary();
                self.expect(Token::LeftBrace);
                let mut body = Vec::new();
                while self.current != Token::RightBrace {
                    body.push(self.parse_statement());
                }
                self.expect(Token::RightBrace);
                Stmt::For {var, from, to, body, line}

            }

            Token::Identifier(_) => {
              let name = match self.advance() {
                  Token::Identifier(n) => n,
                  _ => unreachable!(),
                };

                if self.current == Token::Equals {
                    self.advance();
                    let value  = self.parse_expr();
                    self.expect(Token::Semicolon);
                    Stmt::Assign{name, value, line: self.current_line}
                }else if self.current == Token::LeftParen {
                    self.advance();
                    let mut args = Vec::new();
                    while self.current != Token::RightParen {
                        args.push(self.parse_expr());
                        if self.current == Token::Comma {
                            self.advance();
                        }
                    }
                    self.expect(Token::RightParen);
                    self.expect(Token::Semicolon);
                    Stmt::Expr(Expr::Call { name, args }, self.current_line)
                }else {
                    panic!("unexpected token: {:?}", self.current);
                }
            },
            _ => {
                let expr = self.parse_expr();
                self.expect(Token::Semicolon);
                Stmt::Expr(expr, self.current_line)
            }
        }
    }
    fn parse_expr(&mut self) -> Expr {
        let mut left = self.parse_primary();

        loop {
            let op = match self.current {
                Token::Plus         => BinaryOperator::Add,
                Token::Minus        => BinaryOperator::Sub,
                Token::Star         => BinaryOperator::Mul,
                Token::Slash        => BinaryOperator::Div,
                Token::EqualsEquals => BinaryOperator::Eq,
                Token::BangEquals   => BinaryOperator::NotEq,
                Token::Less         => BinaryOperator::Less,
                Token::Greater      => BinaryOperator::Greater,
                Token::And          => BinaryOperator::And,
                Token::Or           => BinaryOperator::Or,
                _ => break,
            };
            self.advance();
            let right = self.parse_primary();
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left
    }
    fn parse_primary(&mut self) -> Expr {
        match self.current.clone() {
            Token::Integer(n) => { self.advance(); Expr::Integer(n) }
            Token::Identifier(name) => {
                self.advance();
                if self.current == Token::LeftParen {
                    self.advance();
                    let mut args = Vec::new();
                    while self.current != Token::RightParen {
                        args.push(self.parse_expr());
                        if self.current == Token::Comma {
                            self.advance();
                        }
                    }
                    self.expect(Token::RightParen);
                    Expr::Call  { name, args }
                } else {
                    Expr::Identifier(name)
                }
            }
            Token::True => { self.advance(); Expr::Bool(true) }
            Token::False => { self.advance(); Expr::Bool(false) }
            Token::StringLiteral(s) => { self.advance(); Expr::StringLiteral(s) }
            _ => panic!("Hata [satır {}]: Beklenmedik token: {:?}", self.current_line, self.current),
        }
    }
}use crate::ast::*;
use std::collections::HashMap;

pub struct TypeChecker {
    variables: HashMap<String, Type>,
    functions: HashMap<String, (Vec<Type>, Type)>,
}

impl TypeChecker {
    pub fn new() -> Self {
        TypeChecker {
            variables: HashMap::new(),
            functions: HashMap::new(),
        }
    }

    pub fn check_program(&mut self, program: &Program) {
        for func in &program.functions {
            self.functions.insert(
                func.name.clone(),
                (
                    func.params.iter().map(|(_, t)| t.clone()).collect(),
                    func.return_type.clone(),
                ),
            );
        }
        for func in &program.functions {
            self.check_function(func);
        }
    }

    fn check_function(&mut self, func: &Function) {
        self.variables.clear();
        for (name, ty) in &func.params {
            self.variables.insert(name.clone(), ty.clone());
        }
        for stmt in &func.body {
            self.check_statement(stmt, &func.return_type);
        }
    }
    fn check_statement(&mut self, stmt: &Stmt, return_type: &Type) {
        match stmt {
            Stmt::Let {name, ty, value, line } => {
                let value_type = self.check_expr(value);
                if !types_match(ty, &value_type) {
                    panic!("Hata [satır {}]: '{:?}' has type '{:?}' but was expected to have type '{:?}'",line,  name, ty, value_type);
                }
                self.variables.insert(name.clone(), ty.clone());
            }
            Stmt::Return(expr, line) => {
                let expr_type = self.check_expr(expr);
                if !types_match(return_type, &expr_type) {
                   panic!("Hata [satır {}] yanlis donus tipi: beklenen {:?}, bulunan{:?}", line, return_type, expr_type);
                }
            }
            Stmt::If {condition, then_body, else_body, line} => {
                let cond_type = self.check_expr(condition);
                if !types_match(&cond_type, &Type::Bool) {
                    panic!("[Hata satır {}]: if kosulu bool olmali : {:?}",line, cond_type);
                }
                for stmt in then_body {
                    self.check_statement(stmt, return_type);
                }
                if let Some(else_body) = else_body {
                    for stmt in else_body {
                        self.check_statement(stmt, return_type);
                    }
                }
            }
            Stmt::While {condition, body, line} => {
                let cond_type = self.check_expr(condition);
                if !types_match(&cond_type, &Type::Bool) {
                    panic!("[Hata satır {}]: while kosulu bool olmali : {:?}",line,  cond_type);
                }
                for stmt in body {
                    self.check_statement(stmt, return_type);
                }
            }
            Stmt::Assign{name, value, line} => {
                let var_type = match self.variables.get(name) {
                    Some(t) => t.clone(),
                    None => panic!("Hata [satır {}]: unknown variable '{}'", line, name),
                };
                let value_type = self.check_expr(value);
                if !types_match(&var_type, &value_type){
                    panic!("Hata [satır {}]'{}' has type {:?} but was expected to have type {:?}",line,  name, var_type, value_type);
                }
            }
            Stmt::Expr(expr, _) => {
                self.check_expr(expr);
            }
            Stmt::For {var, from, to , body, line } => {
                let from_type = self.check_expr(from);
                let to_type = self.check_expr(to);
                if !types_match(&from_type, &Type::I32) {
                    panic!("Hata [satır {}]: for dongusu from tipi i32 olmali : {:?}",line,  from_type);
                }
                if !types_match(&to_type, &Type::I32) {
                    panic!("Hata [satır {}]: for dongusu to tipi i32 olmali : {:?}",line,  to_type);
                }
                self.variables.insert(var.clone(), Type::I32);
                for stmt in body {
                    self.check_statement(stmt, return_type);
                }
                self.variables.remove(var);
            }
        }
    }
    fn check_expr(&mut self, expr: &Expr) -> Type {
        match expr {
            Expr::Bool(_) => Type::Bool,
            Expr::StringLiteral(_) => Type::I32,

            Expr::Integer(_) => Type::I32,
            Expr::Identifier(name) => {
                match self.variables.get(name){
                    Some(ty) => ty.clone(),
                    None => panic!("unknown variable '{}'", name),
                }
            }
            Expr::BinaryOp {left, op, right} => {
                let left_type = self.check_expr(left);
                let right_type = self.check_expr(right);
                if !types_match(&left_type, &right_type) {
                    panic!(
                        "Tip uyusmazligi: {:?} {:?} {:?} uyusmaz", left_type, op, right_type
                    );
                }
                match op {
                    BinaryOperator::Eq
                    | BinaryOperator::NotEq
                    | BinaryOperator::Less
                    | BinaryOperator::Greater
                    | BinaryOperator::And
                    | BinaryOperator::Or  => Type::Bool,
                    _ => left_type,
                }
            }
            Expr::Call {name, args } => {
                if name == "print" {
                    for arg in args {
                        self.check_expr(arg);
                    }
                    return Type::I32;
                }
                let (param_types, return_type) = match self.functions.get(name) {
                    Some(f) => f.clone(),
                    None => panic!("Tanimsiz fonksiyon: '{}'", name),
                };
                if args.len() != param_types.len() {
                    panic!(
                        "'{}'  fonksiyonu {} arguman bekliyor, {} verildi",
                        name, param_types.len(), args.len()
                    );
                }
                for (arg, expected) in args.iter().zip(param_types.iter()) {
                    let arg_type = self.check_expr(arg);
                    if !types_match(&arg_type, expected) {
                        panic!(
                            "'{}' fonksiyonuna yanlis tip: beklenen {:?}, bulunan {:?}", name, expected, arg_type
                        );
                    }
                }
                return_type
            }
        }
    }
}

fn types_match(a: &Type, b: &Type) -> bool {
    matches!((a, b), (Type::I32, Type::I32) | (Type::Bool, Type::Bool)
    | (Type::I64, Type::I64) | (Type::I64, Type::I32))
}
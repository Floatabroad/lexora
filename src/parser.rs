use bumpalo::Bump;
use std::mem;
use crate::lexer::{Lexer, Token, SpannedToken};
use crate::ast::*;
use crate::ast::Expr::Identifier;
use crate::symbol::{Symbol, Interner};
use crate::span::Span;
use crate::error::LexoraError;
use std::collections::HashSet;


pub struct Parser<'src, 'arena> {
    lexer:                Lexer<'src>,
    current:              SpannedToken<'src>,
    peek:                 SpannedToken<'src>,
    arena:                &'arena Bump,
    pub interner:         Interner,
    allow_struct_literal: bool,
    pub next_expr_id: ExprId,
    enum_names:           HashSet<Symbol>,
    type_params: Vec<Symbol>,
    errors:               Vec<LexoraError>,
}

impl<'src, 'arena> Parser<'src, 'arena> {
   pub fn new(lexer: Lexer<'src>, arena: &'arena Bump) -> Self {
       Self::with_interner(lexer, arena, Interner::new(), 0, )
   }
    pub fn with_interner(
        lexer: Lexer<'src>,
        arena: &'arena Bump,
        interner: Interner,
        start_id: ExprId,
    ) -> Self {
        let mut parser = Parser{
            lexer,
            current: SpannedToken{ token: Token::Eof, span: Span::default()},
            peek: SpannedToken{token: Token::Eof, span: Span::default()},
            arena,
            interner,
            allow_struct_literal: true,
            next_expr_id: start_id,
            enum_names: HashSet::new(),
            type_params: Vec::new(),
            errors: Vec::new(),

        };
        parser.current = parser.pull();
        parser.peek = parser.pull();
        parser
    }
    fn next_id(&mut self) -> ExprId {
        let id = self.next_expr_id;
        self.next_expr_id += 1;
        id
    }
    fn pull(&mut self) -> SpannedToken<'src> {
       loop {
           let t = self.lexer.next_token();
           if t.token != Token::Error {
               return t;
           }
       }
    }
    fn advance(&mut self) -> SpannedToken<'src>{
        let next       = self.pull();
        let prev_peek                = mem::replace(&mut self.peek, next);
        mem::replace(&mut self.current, prev_peek)
    }

    fn expect(&mut self, expected: Token<'src>) -> Result<SpannedToken<'src>, LexoraError> {
        if self.current.token == expected {
            Ok(self.advance())
        } else {
            Err(LexoraError::UnexpectedToken{
                expected: format!("{:?}", expected),
                found:    format!("{:?}", self.current.token),
                span:     self.current.span,
            })
        }
    }

    fn synchronize(&mut self) {
        let was_semi = self.current.token == Token::Semicolon;
        self.advance();
        if was_semi{
            return;
        }
        while self.current.token != Token::Eof{
            match self.current.token {
                Token::Fn | Token::Struct | Token::Import | Token::RightBrace => return,
                Token::Semicolon => {
                    self.advance();
                    return;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }
   fn parse_symbol(&mut self) -> Result<Symbol, LexoraError> {
       let sym = if let Token::Identifier(s) = self.current.token {
           self.interner.intern(s)
       }else {
           return Err(LexoraError::UnexpectedToken {
               expected: "identifier".to_string(),
               found:    format!("{:?}", self.current.token),
               span:     self.current.span,
           });
       };
       self.advance();
       Ok(sym)
   }
    fn parse_type(&mut self) -> Result<Type, LexoraError> {
        match self.current.token.clone() {
            Token::I32 => {self.advance(); Ok(Type::I32)}
            Token::I64 => { self.advance(); Ok(Type::I64) }
            Token::Bool => { self.advance(); Ok(Type::Bool) }
            Token::Str => {self.advance(); Ok(Type::Str)}
            Token::Void => {self.advance(); Ok(Type::Void)}
            Token::Identifier("Box") => {
                self.advance();
                self.expect(Token::Less)?;
                let inner = self.parse_type()?;
                self.expect(Token::Greater)?;
                Ok(Type::Box(Box::new(inner)))
            }
            Token::LeftBracket => {
                self.advance();
                let elem_ty = self.parse_type()?;
                self.expect(Token::Semicolon)?;
                let size = match self.current.token {
                    Token::Integer(n) => {
                        let s = n as usize;
                        self.advance();
                        s
                    }
                    _ => return Err(LexoraError::UnexpectedToken {
                        expected: "array boyutu".to_string(),
                        found:    format!("{:?}", self.current.token),
                        span:     self.current.span,
                    }),
                };
                self.expect(Token::RightBracket)?;
                Ok(Type::Array(Box::new(elem_ty), size))
            }
            Token::Identifier(_) => {
                let sym = self.parse_symbol()?;
                if self.type_params.contains(&sym) {
                    Ok(Type::Param(sym))
                } else if self.enum_names.contains(&sym) {
                    let args = self.parse_type_args()?;
                    Ok(Type::Enum(sym, args))
                } else {
                    Ok(Type::Struct(sym))
                }
            }
            _ => Err(LexoraError::UnexpectedToken {
                expected: "tip(i32, i64, bool, void, str, [T;N], Struct".to_string(),
                found:    format!("{:?}", self.current.token),
                span:     self.current.span,
            }),

        }
    }
    fn parse_type_args(&mut self) -> Result<Vec<Type>, LexoraError> {
        let mut args = Vec::new();
        if self.current.token != Token::Less {
            return Ok(args);
        }
        self.advance();
        while self.current.token != Token::Greater {
            args.push(self.parse_type()?);
            if self.current.token == Token::Comma {
                self.advance();
            }
        }
        self.expect(Token::Greater)?;
        Ok(args)
    }
    pub fn parse_program(&mut self) -> Program<'arena> {
        let mut functions = Vec::new();
        let mut structs   = Vec::new();
        let mut imports: Vec<&'arena str> = Vec::new();
        let mut enums = Vec::new();
        while self.current.token != Token::Eof {
           self.type_params.clear();
            let result = match self.current.token.clone() {
                Token::Struct => self.parse_struct().map(|s| structs.push(s)),
                Token::Import => self.parse_import().map(|p| imports.push(p)),
                Token::Fn => self.parse_function().map(|f| functions.push(f)),
                Token::Enum => self.parse_enum().map(|e| enums.push(e)),
                _ =>  Err(LexoraError::UnexpectedToken {
                    expected: "struct, fn, import".to_string(),
                    found: format!("{:?}", self.current.token),
                    span: self.current.span,
                }),
            };
            if let Err(e) = result {
                self.errors.push(e);
                self.synchronize();
            }
        }

        self.errors.extend(self.lexer.take_errors());
        Program{ functions, structs, imports, enums}
    }
    pub fn take_errors(&mut self) -> Vec<LexoraError> {
        std::mem::take(&mut self.errors)
    }

    fn parse_struct(&mut self) -> Result<StructDef, LexoraError> {
        let start = self.current.span;
        self.expect(Token::Struct)?;
        let name = self.parse_symbol()?;
        self.expect(Token::LeftBrace)?;

        let mut fields: Vec<(Symbol, Type)> = Vec::new();
        while self.current.token != Token::RightBrace {
            let field_name = self.parse_symbol()?;
            self.expect(Token::Colon)?;
            let field_type = self.parse_type()?;
            fields.push((field_name, field_type));
            if self.current.token == Token::Comma {
                self.advance();
            }
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        Ok(StructDef { name, fields, span: start.merge(end) })
    }

    fn parse_enum(&mut self) -> Result<EnumDef, LexoraError> {
        let start = self.current.span;
        self.expect(Token::Enum)?;
        let name = self.parse_symbol()?;
        self.enum_names.insert(name);
        if self.current.token == Token::Less {
            self.advance();
            while self.current.token != Token::Greater {
                let p = self.parse_symbol()?;
                self.type_params.push(p);
                if self.current.token == Token::Comma {
                    self.advance();
                }
            }
            self.expect(Token::Greater)?;
        }
        self.expect(Token::LeftBrace)?;
        let mut variants = Vec::new();
        while self.current.token != Token::RightBrace {
            let v = self.parse_symbol()?;
            let mut fields = Vec::new();
            if self.current.token == Token::LeftParen {
                self.advance();
                while self.current.token != Token::RightParen {
                    fields.push(self.parse_type()?);
                    if self.current.token == Token::Comma {
                        self.advance();
                    }
                }
                self.expect(Token::RightParen)?;
            }
            variants.push((v, fields));
            if self.current.token == Token::Comma {
                self.advance();
            }
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        Ok(EnumDef { name, params: mem::take(&mut self.type_params), variants, span: start.merge(end) })
    }

    fn parse_value_block(&mut self) -> Result<Block<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::LeftBrace)?;
        let mut stmts: Vec<Stmt<'arena>> = Vec::new();
        let mut tail: Option<&'arena Expr<'arena>> = None;
        while !matches!(self.current.token, Token::RightBrace | Token::Eof | Token::Fn | Token::Struct | Token::Import) {
            let starts_stmt = matches!(self.current.token,
                Token::Let | Token::Return | Token::While | Token::For);
            let result = if starts_stmt {
                self.parse_statement().map(|s| stmts.push(s))
            } else {
                let stmt_start = self.current.span;
                self.parse_expr().and_then(|expr| {
                    if self.current.token == Token::Equals {
                        self.advance();
                        let value = self.parse_expr()?;
                        let end = self.current.span;
                        self.expect(Token::Semicolon)?;
                        let stmt = self.assign_stmt(expr, value, stmt_start.merge(end))?;
                        stmts.push(stmt);
                        Ok(())
                    } else if self.current.token == Token::RightBrace {
                        if Self::expr_is_valueless(&expr) {
                            let end = expr.span();
                            stmts.push(Stmt::Expr(expr, stmt_start.merge(end)));
                        } else {
                            tail = Some(self.arena.alloc(expr));
                        }
                        Ok(())
                    } else if matches!(expr, Expr::Match { .. } | Expr::If { .. }) {
                        let end = expr.span();
                        if self.current.token == Token::Semicolon {
                            self.advance();
                        }
                        stmts.push(Stmt::Expr(expr, stmt_start.merge(end)));
                        Ok(())
                    } else {
                        let end = self.current.span;
                        self.expect(Token::Semicolon)
                            .map(|_| stmts.push(Stmt::Expr(expr, stmt_start.merge(end))))
                    }
                })
            };
            if let Err(e) = result {
                let span = e.span().unwrap_or(self.current.span);
                self.errors.push(e);
                self.synchronize();
                stmts.push(Stmt::Error(span));
            }
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        let stmts = self.arena.alloc_slice_fill_iter(stmts.into_iter());
        Ok(Block { stmts, tail, span: start.merge(end) })
    }
    fn assign_stmt(&mut self, target: Expr<'arena>, value: Expr<'arena>, span: Span) -> Result<Stmt<'arena>, LexoraError> {
        match target {
            Expr::Identifier(name, _, _) =>
                Ok(Stmt::Assign { name, value, span }),
            Expr::Index { array: &Expr::Identifier(name, _, _), index, .. } =>
                Ok(Stmt::AssignIndex { name, index: index.clone(), value, span }),
            Expr::FieldAccess { object: &Expr::Identifier(object, _, _), field, .. } =>
                Ok(Stmt::AssignField { object, field, value, span }),
            Expr::Deref { .. } =>
                Ok(Stmt::AssignDeref { target, value, span }),
            other => Err(LexoraError::Custom {
                message: "gecersiz atama hedefi".to_string(),
                span: other.span(),
            }),
        }
    }
    fn expr_is_valueless(expr: &Expr) -> bool {
        match expr {
            Expr::Match { arms, .. } => arms.iter().all(|(_, b)| Self::block_is_valueless(b)),
            Expr::If { then_body, else_body, .. } => match else_body {
                None => true,
                Some(eb) => Self::block_is_valueless(then_body) && Self::block_is_valueless(eb),
            },
            _ => false,
        }
    }
    fn block_is_valueless(block: &Block) -> bool {
        match block.tail {
            None => true,
            Some(t) => Self::expr_is_valueless(t),
        }
    }
    fn parse_import(&mut self) -> Result<&'arena str, LexoraError> {
        self.expect(Token::Import)?;
        let path = if let Token::StringLiteral(s) = self.current.token {
            let p = self.arena.alloc_str(s);
            self.advance();
            p
        } else {
            return Err(LexoraError::UnexpectedToken {
                expected: "dosya yolu".to_string(),
                found:    format!("{:?}", self.current.token),
                span:     self.current.span,
            });
        };
        self.expect(Token::Semicolon)?;
        Ok(path)
    }
    fn parse_statement(&mut self) -> Result<Stmt<'arena>, LexoraError>{

        let start = self.current.span;
        match self.current.token.clone() {
            Token::Let => {
                self.advance();
                let name = self.parse_symbol()?;
                let ty = if self.current.token == Token::Colon {
                    self.advance();
                    Some(self.parse_type()?)
                }else {
                    None
                };
                self.expect(Token::Equals)?;
                let value = self.parse_expr()?;
                let end = self.current.span;
                self.expect(Token::Semicolon)?;
                Ok(Stmt::Let { name, value, ty, span: start.merge(end) })
            }
            Token::Return => {
                self.advance();
                let value = self.parse_expr()?;
                let end = self.current.span;
                self.expect(Token::Semicolon)?;
                Ok(Stmt::Return(value, start.merge(end)))
            }

            Token::While => self.parse_while(),
            Token::For  => self.parse_for(),
            _ => Err(LexoraError::UnexpectedToken {
                expected: "statement".to_string(),
                found: format!("{:?}", self.current.token),
                span: start,
            }),
        }
    }

    fn parse_if_expr(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::If)?;
        self.allow_struct_literal = false;
        let condition = self.parse_expr()?;
        self.allow_struct_literal = true;
        let then_body = self.parse_value_block()?;
        let mut end = then_body.span;
        let else_body = if self.current.token == Token::Else {
            self.advance();
            let blk = if self.current.token == Token::If {
                let else_if = self.parse_if_expr()?;
                let espan = else_if.span();
                Block { stmts: &[], tail: Some(self.arena.alloc(else_if)), span: espan }
            } else {
                self.parse_value_block()?
            };
            end = blk.span;
            Some(&*self.arena.alloc(blk))
        } else {
            None
        };
        Ok(Expr::If {
            condition: self.arena.alloc(condition),
            then_body: self.arena.alloc(then_body),
            else_body,
            span: start.merge(end),
            id: self.next_id(),
        })
    }

    fn parse_while(&mut self) -> Result<Stmt<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::While)?;
        self.allow_struct_literal = false;
        let condition = self.parse_expr()?;
        self.allow_struct_literal = true;
        let body = self.parse_value_block()?;
        let span = start.merge(body.span);
        Ok(Stmt::While { condition, body, span })
    }

    fn parse_for(&mut self) -> Result<Stmt<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::For)?;
        let var = self.parse_symbol()?;
        self.expect(Token::In)?;
        self.allow_struct_literal = false;
        let from = self.parse_expr()?;
        self.expect(Token::DotDot)?;
        let to = self.parse_expr()?;
        self.allow_struct_literal = true;
        let body = self.parse_value_block()?;
        let span = start.merge(body.span);
        Ok(Stmt::For { var, from, to, body, span })
    }
    fn parse_match_expr(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::Match)?;
        self.allow_struct_literal = false;
        let scrutinee = self.parse_expr()?;
        self.allow_struct_literal = true;
        self.expect(Token::LeftBrace)?;
        let mut arms: Vec<(Pattern<'arena>, Block<'arena>)> = Vec::new();
        while self.current.token != Token::RightBrace && self.current.token != Token::Eof {
            let pat = self.parse_pattern()?;
            self.expect(Token::FatArrow)?;
            let body = if self.current.token == Token::LeftBrace {
                self.parse_value_block()?
            } else {
                let expr = self.parse_expr()?;
                let espan = expr.span();
                Block { stmts: &[], tail: Some(self.arena.alloc(expr)), span: espan }
            };
            arms.push((pat, body));
            if self.current.token == Token::Comma {
                self.advance();
            }
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        let arms = self.arena.alloc_slice_fill_iter(arms.into_iter());
        Ok(Expr::Match {
            scrutinee: self.arena.alloc(scrutinee),
            arms,
            span: start.merge(end),
            id: self.next_id(),
        })
    }
    fn parse_pattern(&mut self) -> Result<Pattern<'arena>, LexoraError> {
        if matches!(self.current.token, Token::Identifier("_")) {
            self.advance();
            return Ok(Pattern::Wildcard);
        }
        let enum_name = self.parse_symbol()?;
        self.expect(Token::ColonColon)?;
        let variant = self.parse_symbol()?;
        let mut bindings: Vec<Symbol> = Vec::new();
        if self.current.token == Token::LeftParen {
            self.advance();
            while self.current.token != Token::RightParen {
                bindings.push(self.parse_symbol()?);
                if self.current.token == Token::Comma {
                    self.advance();
                }
            }
            self.expect(Token::RightParen)?;
        }
        let bindings = self.arena.alloc_slice_fill_iter(bindings.into_iter());
        Ok(Pattern::Variant { enum_name, variant, bindings })
    }

    pub fn parse_expr(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.parse_or()
    }
    fn parse_or(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        let mut left = self.parse_and()?;
        while self.current.token == Token::Or {
            self.advance();
            let right = self.parse_and()?;
            let span = start.merge(right.span());
            let id = self.next_id();
            left = Expr::BinaryOp {
                left: self.arena.alloc(left),
                op:   BinaryOperator::Or,
                right: self.arena.alloc(right),
                span,
                id,
            };
        }
        Ok(left)
    }
    fn parse_and(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        let mut left = self.parse_comparison()?;
        while self.current.token == Token::And {
            self.advance();
            let right = self.parse_comparison()?;
            let span = start.merge(right.span());
            let id = self.next_id();
            left = Expr::BinaryOp {
                left: self.arena.alloc(left),
                op:   BinaryOperator::And,
                right: self.arena.alloc(right),
                span,
                id,
            };
        }
        Ok(left)
    }


    fn parse_comparison(&mut self) -> Result<Expr<'arena>, LexoraError>{
        let start = self.current.span;
        let left = self.parse_additive()?;
        let op = match self.current.token {
            Token::EqualsEquals => BinaryOperator::Eq,
            Token::BangEquals   => BinaryOperator::NotEq,
            Token::Less         => BinaryOperator::Less,
            Token::Greater      => BinaryOperator::Greater,
            Token::LessEq       => BinaryOperator::LessEq,
            Token::GreaterEq    => BinaryOperator::GreaterEq,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_additive()?;
        let span = start.merge(right.span());
        let id = self.next_id();
        Ok(Expr::BinaryOp { left: self.arena.alloc(left), op, right: self.arena.alloc(right), span, id})
    }
    fn parse_additive(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        let mut left = self.parse_multiplicative()?;
        loop {
            let op = match self.current.token {
                Token::Plus => BinaryOperator::Add,
                Token::Minus => BinaryOperator::Sub,
                _ =>  break,
            };
            self.advance();
            let right = self.parse_multiplicative()?;
            let span = start.merge(right.span());
            let id = self.next_id();
            left = Expr::BinaryOp {
                left: self.arena.alloc(left),
                op,
                right: self.arena.alloc(right),
                span,
                id,
            };
        }
        Ok(left)
    }

    fn parse_multiplicative(&mut self) -> Result<Expr<'arena>, LexoraError>{
        let start = self.current.span;
        let mut left = self.parse_cast()?;
        loop {
            let op = match self.current.token {
                Token::Star => BinaryOperator::Mul,
                Token::Slash    => BinaryOperator::Div,
                _               => break,
            };
            self.advance();
            let right = self.parse_cast()?;
            let span = start.merge(right.span());
            let id = self.next_id();
            left = Expr::BinaryOp { left: self.arena.alloc(left), op, right: self.arena.alloc(right), span, id};
        }
        Ok(left)
    }
    fn parse_cast(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let mut expr = self.parse_unary()?;
        while self.current.token == Token::As {
            let start = expr.span();
            self.advance();
            let ty_span = self.current.span;
            let target_type = self.parse_type()?;
            let id = self.next_id();
            expr = Expr::Cast {
                expr: self.arena.alloc(expr),
                target_type,
                span: start.merge(ty_span),
                id,
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        match self.current.token {
            Token::Not => {
                self.advance();
                let operand = self.parse_unary()?;
                let span = start.merge(operand.span());
                let id = self.next_id();
                Ok(Expr::UnaryOp { op: UnaryOperator::Not, operand: self.arena.alloc(operand), span, id })
            }
            Token::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                let span = start.merge(operand.span());
                let id = self.next_id();
                Ok(Expr::UnaryOp { op: UnaryOperator::Neg, operand: self.arena.alloc(operand), span, id })
            }
            Token::Box => {
                self.advance();
                let value = self.parse_unary()?;
                let span = start.merge(value.span());
                let id = self.next_id();
                Ok(Expr::Box { value: self.arena.alloc(value), span, id })
            }
            Token::Star => {
                self.advance();
                let target = self.parse_unary()?;
                let span = start.merge(target.span());
                let id = self.next_id();
                Ok(Expr::Deref { target: self.arena.alloc(target), span, id })
            }
            _ => self.parse_postfix(),
        }
    }
    fn parse_postfix(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.current.token {
                Token::LeftBracket => {
                    let start = expr.span();
                    self.advance();
                    let index = self.parse_expr()?;
                    let end = self.current.span;
                    self.expect(Token::RightBracket)?;
                    let id = self.next_id();
                    expr = Expr::Index {
                        array:  self.arena.alloc(expr),
                        index:  self.arena.alloc(index),
                        span:   start.merge(end),
                        id,
                    };
                }
                Token::Dot => {
                    let start = expr.span();
                    self.advance();
                    let end = self.current.span;
                    let field = self.parse_symbol()?;
                    let id = self.next_id();
                    expr = Expr::FieldAccess {
                        object: self.arena.alloc(expr),
                        field,
                        span: start.merge(end),
                        id,
                    };
                }
                _ => break,
            }
        }
        Ok(expr)
    }
    fn parse_primary(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        match self.current.token.clone() {
            Token::Integer(n) => {
                self.advance();
                Ok(Expr::Integer(n, start, self.next_id()))
            }
            Token::True => {
                self.advance();
                Ok(Expr::Bool(true, start, self.next_id()))
            }
            Token::False => {
                self.advance();
                Ok(Expr::Bool(false, start, self.next_id()))
            }
            Token::StringLiteral(s) => {
                let s = self.arena.alloc_str(s);
                self.advance();
                Ok(Expr::StringLiteral(s, start, self.next_id()))
            }
            Token::LeftParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(Token::RightParen)?;
                Ok(expr)
            }
            Token::LeftBracket => {
                self.advance();
                let mut elems: Vec<Expr<'arena>> = Vec::new();
                while self.current.token != Token::RightBracket {
                    if matches!(self.current.token, Token::Semicolon | Token::RightBrace
                | Token::RightParen | Token::Eof) {
                        break;
                    }
                    elems.push(self.parse_expr()?);
                    if self.current.token == Token::Comma {
                        self.advance();
                    }
                }
                    let end = self.current.span;
                    self.expect(Token::RightBracket)?;
                    let elems = self.arena.alloc_slice_fill_iter(elems.into_iter());
                    Ok(Expr::ArrayLiteral(elems, start.merge(end), self.next_id()))
                }
            Token::Match => self.parse_match_expr(),
           Token::If => self.parse_if_expr(),
            Token::Identifier(_) => {
                let name = self.parse_symbol()?;
                if self.current.token == Token::ColonColon {
                    self.advance();
                    let type_args = if self.current.token == Token::Less {
                        let targs = self.parse_type_args()?;
                        self.expect(Token::ColonColon)?;
                        targs
                    } else {
                        Vec::new()
                    };
                    let mut var_span = self.current.span;
                    let variant = self.parse_symbol()?;
                    let mut args: Vec<Expr<'arena>> = Vec::new();
                    if self.current.token == Token::LeftParen {
                        self.advance();
                        while self.current.token != Token::RightParen {
                            if matches!(self.current.token, Token::Semicolon | Token::RightBrace | Token::Eof) {
                                break;
                            }
                            args.push(self.parse_expr()?);
                            if self.current.token == Token::Comma {
                                self.advance();
                            }
                        }
                        var_span = self.current.span;
                        self.expect(Token::RightParen)?;
                    }
                    let args = self.arena.alloc_slice_fill_iter(args.into_iter());
                    return Ok(Expr::EnumVariant {
                        enum_name: name,
                        variant,
                        args,
                        type_args,
                        span: start.merge(var_span),
                        id: self.next_id(),
                    });
                }
                    if self.current.token == Token::LeftParen {
                        self.advance();
                        let mut args: Vec<Expr<'arena>> = Vec::new();
                        while self.current.token != Token::RightParen {
                            if matches!(self.current.token, Token::Semicolon |
      Token::RightBrace | Token::RightBracket | Token::Eof) {
                                break;
                            }
                            args.push(self.parse_expr()?);
                            if self.current.token == Token::Comma {
                                self.advance();
                            }
                        }
                        let end = self.current.span;
                        self.expect(Token::RightParen)?;
                        let args = self.arena.alloc_slice_fill_iter(args.into_iter());
                        Ok(Expr::Call { name, args, span: start.merge(end), id: self.next_id() })
                    } else if self.allow_struct_literal && self.current.token == Token::LeftBrace {
                        self.advance();
                        let mut fields: Vec<(Symbol, Expr<'arena>)> = Vec::new();
                        while self.current.token != Token::RightBrace {
                            let field_name = self.parse_symbol()?;
                            self.expect(Token::Colon)?;
                            let field_val = self.parse_expr()?;
                            fields.push((field_name, field_val));
                            if self.current.token == Token::Comma {
                                self.advance();
                            }
                        }
                        let end = self.current.span;
                        self.expect(Token::RightBrace)?;
                        let fields = self.arena.alloc_slice_fill_iter(fields.into_iter());
                        Ok(Expr::StructLiteral { name, fields, span: start.merge(end), id: self.next_id() })
                    } else {
                        Ok(Identifier(name, start, self.next_id()))
                    }
                }
                _ => {
                    self.errors.push(LexoraError::UnexpectedToken {
                        expected: "expression".to_string(),
                        found: format!("{:?}", self.current.token),
                        span: start,
                    });
                    if !matches!(
                        self.current.token,
                        Token::Semicolon | Token::RightBrace | Token::RightParen | Token::RightBracket| Token::Comma
                        | Token::Eof
                    ) {
                        self.advance();
                    }
                    Ok(Expr::Error(start, self.next_id()))
                }
            }
        }
        fn parse_function(&mut self) -> Result<Function<'arena>, LexoraError> {
            let start = self.current.span;
            self.expect(Token::Fn)?;
            let name = self.parse_symbol()?;
            self.expect(Token::LeftParen)?;

            let mut params: Vec<(Symbol, Type)> = Vec::new();
            while self.current.token != Token::RightParen {
                let param_name = self.parse_symbol()?;
                self.expect(Token::Colon)?;
                let param_type = self.parse_type()?;
                params.push((param_name, param_type));
                if self.current.token == Token::Comma {
                    self.advance();
                }
            }
            self.expect(Token::RightParen)?;
            self.expect(Token::Arrow)?;
            let return_type = self.parse_type()?;

            let body = self.parse_value_block()?;
            let params = self.arena.alloc_slice_fill_iter(params.into_iter());
            let span = start.merge(body.span);

            Ok(Function { name, params, return_type, body, span })
        }

}
use bumpalo::Bump;
use std::mem;
use crate::lexer::{Lexer, Token, SpannedToken};
use crate::ast::*;
use crate::ast::Expr::Identifier;
use crate::symbol::{Symbol, Interner};
use crate::span::Span;
use crate::error::LexoraError;


pub struct Parser<'src, 'arena> {
    lexer:                Lexer<'src>,
    current:              SpannedToken<'src>,
    peek:                 SpannedToken<'src>,
    arena:                &'arena Bump,
    pub interner:         Interner,
    allow_struct_literal: bool,
    pub next_expr_id: ExprId,
    errors:               Vec<LexoraError>,
}

impl<'src, 'arena> Parser<'src, 'arena> {
   pub fn new(lexer: Lexer<'src>, arena: &'arena Bump) -> Self {
       Self::with_interner(lexer, arena, Interner::new(), 0)
   }
    pub fn with_interner(
        mut lexer: Lexer<'src>,
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
                Ok(Type::Struct(sym))
            }
            _ => Err(LexoraError::UnexpectedToken {
                expected: "tip(i32, i64, bool, void, str, [T;N], Struct".to_string(),
                found:    format!("{:?}", self.current.token),
                span:     self.current.span,
            }),

        }
    }

    pub fn parse_program(&mut self) -> Program<'arena> {
        let mut functions = Vec::new();
        let mut structs   = Vec::new();
        let mut imports: Vec<&'arena str> = Vec::new();

        while self.current.token != Token::Eof {
            let result = match self.current.token.clone() {
                Token::Struct => self.parse_struct().map(|s| structs.push(s)),
                Token::Import => self.parse_import().map(|p| imports.push(p)),
                Token::Fn => self.parse_function().map(|f| functions.push(f)),
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
        Program{ functions, structs, imports}
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
    fn parse_block(&mut self) -> Result<(&'arena [Stmt<'arena>], Span), LexoraError> {
        self.expect(Token::LeftBrace)?;
        let mut stmts: Vec<Stmt<'arena>> = Vec::new();
        while !matches!(self.current.token, Token::RightBrace | Token::Eof | Token::Fn | Token::Struct | Token::Import) {
            match self.parse_statement(){
                Ok(s) => stmts.push(s),
                Err(e) => {
                  let span = e.span().unwrap_or(self.current.span);
                    self.errors.push(e);
                    self.synchronize();
                    stmts.push(Stmt::Error(span));
                }
            }
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        let body = self.arena.alloc_slice_fill_iter(stmts.into_iter());
        Ok((body, end))
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
            Token::If   => self.parse_if(),
            Token::While => self.parse_while(),
            Token::For  => self.parse_for(),
            Token::Identifier(_) => {
                match self.peek.token.clone() {
                    Token::Equals => {
                        let name = self.parse_symbol()?;
                        self.expect(Token::Equals)?;
                        let value = self.parse_expr()?;
                        let end = self.current.span;
                        self.expect(Token::Semicolon)?;
                        Ok(Stmt::Assign { name, value, span: start.merge(end) })
                    }
                    Token::LeftBracket => {
                        let name = self.parse_symbol()?;
                        self.expect(Token::LeftBracket)?;
                        let index = self.parse_expr()?;
                        self.expect(Token::RightBracket)?;
                        self.expect(Token::Equals)?;
                        let value = self.parse_expr()?;
                        let end = self.current.span;
                        self.expect(Token::Semicolon)?;
                        Ok(Stmt::AssignIndex { name, index, value, span: start.merge(end) })
                    }
                    Token::Dot => {
                        let object = self.parse_symbol()?;
                        self.expect(Token::Dot)?;
                        let field = self.parse_symbol()?;
                        self.expect(Token::Equals)?;
                        let value = self.parse_expr()?;
                        let end = self.current.span;
                        self.expect(Token::Semicolon)?;
                        Ok(Stmt::AssignField { object, field, value, span: start.merge(end) })
                    }
                    _ => {
                        let expr = self.parse_expr()?;
                        let end = self.current.span;
                        self.expect(Token::Semicolon)?;
                        Ok(Stmt::Expr(expr, start.merge(end)))
                    }
                }
            }
            _ => {
                let expr = self.parse_expr()?;
                let end = self.current.span;
                self.expect(Token::Semicolon)?;
                Ok(Stmt::Expr(expr, start.merge(end)))
            }
        }
    }

    fn parse_if(&mut self) -> Result<Stmt<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::If)?;

        self.allow_struct_literal = false;
        let condition = self.parse_expr()?;
        self.allow_struct_literal = true;

        let (then_body, end) = self.parse_block()?;

        let else_branch = if self.current.token == Token::Else {
            self.advance();
            if self.current.token == Token::If {
                let else_if = self.parse_if()?;
                Some(self.arena.alloc_slice_fill_iter(std::iter::once(else_if)) as &[Stmt<'arena>])
            } else {
               let (else_body, _) = self.parse_block()?;
                Some(else_body)
            }
        } else {
            None
        };
        Ok(Stmt::If { condition, then_body, else_branch, span: start.merge(end) })
    }

    fn parse_while(&mut self) -> Result<Stmt<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::While)?;

        self.allow_struct_literal = false;
        let condition = self.parse_expr()?;
        self.allow_struct_literal = true;

        let (body, end) = self.parse_block()?;
        Ok(Stmt::While { condition, body, span: start.merge(end) })
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


        let (body, end) = self.parse_block()?;
        Ok(Stmt::For { var, from, to, body, span: start.merge(end) })
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
                Token::Identifier(_) => {
                    let name = self.parse_symbol()?;
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

            let (body, end) = self.parse_block()?;
            let params = self.arena.alloc_slice_fill_iter(params.into_iter());

            Ok(Function { name, params, return_type, body, span: start.merge(end) })
        }

}
use crate::ast::Expr::Identifier;
use crate::ast::*;
use crate::error::LexoraError;
use crate::lexer::{Lexer, SpannedToken, Token};
use crate::span::Span;
use crate::symbol::{Interner, Symbol};
use bumpalo::Bump;
use std::collections::HashSet;
use std::mem;

const MAX_EXPR_DEPTH: u32 = 128;
pub struct Parser<'src, 'arena> {
    lexer: Lexer<'src>,
    current: SpannedToken<'src>,
    peek: SpannedToken<'src>,
    arena: &'arena Bump,
    pub interner: Interner,
    allow_struct_literal: bool,
    pub next_expr_id: ExprId,
    enum_names: HashSet<Symbol>,
    type_params: Vec<Symbol>,
    depth: u32,
    last_span: Span,
    errors: Vec<LexoraError>,
}

impl<'src, 'arena> Parser<'src, 'arena> {
    pub fn new(lexer: Lexer<'src>, arena: &'arena Bump) -> Self {
        Self::with_interner(lexer, arena, Interner::new(), 0)
    }
    pub fn with_interner(
        lexer: Lexer<'src>,
        arena: &'arena Bump,
        interner: Interner,
        start_id: ExprId,
    ) -> Self {
        let mut parser = Parser {
            lexer,
            current: SpannedToken {
                token: Token::Eof,
                span: Span::default(),
            },
            peek: SpannedToken {
                token: Token::Eof,
                span: Span::default(),
            },
            arena,
            interner,
            allow_struct_literal: true,
            next_expr_id: start_id,
            enum_names: HashSet::new(),
            type_params: Vec::new(),
            depth: 0,
            last_span: Span::default(),
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
    fn advance(&mut self) -> SpannedToken<'src> {
        let next = self.pull();
        let prev_peek = mem::replace(&mut self.peek, next);
        let out = mem::replace(&mut self.current, prev_peek);
        self.last_span = out.span;
        out
    }

    fn expect(&mut self, expected: Token<'src>) -> Result<SpannedToken<'src>, LexoraError> {
        if self.current.token == expected {
            Ok(self.advance())
        } else {
            Err(LexoraError::UnexpectedToken {
                expected: format!("{:?}", expected),
                found: format!("{:?}", self.current.token),
                span: self.current.span,
            })
        }
    }
    fn with_struct_literal<T>(
        &mut self,
        allow: bool,
        f: impl FnOnce(&mut Self) -> Result<T, LexoraError>,
    ) -> Result<T, LexoraError> {
        let prev = self.allow_struct_literal;
        self.allow_struct_literal = allow;
        let out = f(self);
        self.allow_struct_literal = prev;
        out
    }
    fn depth_guard<T>(
        &mut self,
        f: impl FnOnce(&mut Self) -> Result<T, LexoraError>,
    ) -> Result<T, LexoraError> {
        let prev = self.depth;
        let out = f(self);
        self.depth = prev;
        out
    }

    fn depth_bump(&mut self, what: &str) -> Result<(), LexoraError> {
        self.depth += 1;
        if self.depth > MAX_EXPR_DEPTH {
            return Err(LexoraError::Custom {
                message: format!("{} cok derin ic ice (sinir {})", what, MAX_EXPR_DEPTH),
                span: self.current.span,
            });
        }
        Ok(())
    }
    fn arg_list_stopper(tok: &Token) -> bool {
        matches!(
            tok,
            Token::Semicolon
                | Token::RightBrace
                | Token::RightParen
                | Token::RightBracket
                | Token::Eof
        )
    }

    fn is_expr_stopper(tok: &Token) -> bool {
        matches!(tok, Token::Comma) || Self::arg_list_stopper(tok)
    }
    fn synchronize(&mut self) {
        let was_semi = self.current.token == Token::Semicolon;
        self.advance();
        if was_semi {
            return;
        }
        while self.current.token != Token::Eof {
            match self.current.token {
                Token::Fn
                | Token::Struct
                | Token::Import
                | Token::Enum
                | Token::Impl
                | Token::RightBrace => {
                    return;
                }
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
        } else {
            return Err(LexoraError::UnexpectedToken {
                expected: "identifier".to_string(),
                found: format!("{:?}", self.current.token),
                span: self.current.span,
            });
        };
        self.advance();
        Ok(sym)
    }
    fn parse_type_spanned(&mut self) -> Result<(Type, Span), LexoraError> {
        let start = self.current.span;
        let ty = self.parse_type()?;
        Ok((ty, start.merge(self.last_span)))
    }
    fn parse_type(&mut self) -> Result<Type, LexoraError> {
        self.depth_guard(|p| {
            p.depth_bump("tip")?;
            p.parse_type_inner()
        })
    }
    fn parse_type_inner(&mut self) -> Result<Type, LexoraError> {
        match self.current.token.clone() {
            Token::I32 => {
                self.advance();
                Ok(Type::I32)
            }
            Token::I64 => {
                self.advance();
                Ok(Type::I64)
            }
            Token::F32 => {
                self.advance();
                Ok(Type::F32)
            }
            Token::F64 => {
                self.advance();
                Ok(Type::F64)
            }
            Token::Bool => {
                self.advance();
                Ok(Type::Bool)
            }
            Token::Str => {
                self.advance();
                Ok(Type::Str)
            }
            Token::Void => {
                self.advance();
                Ok(Type::Void)
            }
            Token::Identifier("Box") => {
                self.advance();
                self.expect(Token::Less)?;
                let inner = self.parse_type()?;
                self.expect(Token::Greater)?;
                Ok(Type::Box(Box::new(inner)))
            }
            Token::Identifier("String") => {
                self.advance();
                Ok(Type::String)
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
                    _ => {
                        return Err(LexoraError::UnexpectedToken {
                            expected: "array boyutu".to_string(),
                            found: format!("{:?}", self.current.token),
                            span: self.current.span,
                        });
                    }
                };
                self.expect(Token::RightBracket)?;
                Ok(Type::Array(Box::new(elem_ty), size))
            }
            Token::Identifier(_) => {
                let sym = self.parse_symbol()?;
                if self.type_params.contains(&sym) {
                    Ok(Type::Param(sym))
                } else if self.enum_names.contains(&sym) || self.current.token == Token::Less {
                    let args = self.parse_type_args()?;
                    Ok(Type::Enum(sym, args))
                } else {
                    Ok(Type::Struct(sym))
                }
            }
            _ => Err(LexoraError::UnexpectedToken {
                expected: "tip (i32, i64, f32, f64, bool, void, str, [T;N], Struct, Enum)"
                    .to_string(),
                found: format!("{:?}", self.current.token),
                span: self.current.span,
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
        let mut structs = Vec::new();
        let mut imports: Vec<&'arena str> = Vec::new();
        let mut enums = Vec::new();
        let mut impls = Vec::new();
        while self.current.token != Token::Eof {
            self.type_params.clear();
            let result = match self.current.token.clone() {
                Token::Struct => self.parse_struct().map(|s| structs.push(s)),
                Token::Import => self.parse_import().map(|p| imports.push(p)),
                Token::Fn => self.parse_function(None).map(|f| functions.push(f)),
                Token::Enum => self.parse_enum().map(|e| enums.push(e)),
                Token::Impl => self.parse_impl().map(|i| impls.push(i)),
                _ => Err(LexoraError::UnexpectedToken {
                    expected: "struct, fn, enum, impl, import".to_string(),
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
        Program {
            functions,
            structs,
            imports,
            enums,
            impls,
        }
    }
    pub fn take_errors(&mut self) -> Vec<LexoraError> {
        std::mem::take(&mut self.errors)
    }

    pub fn seed_enum_names(&mut self, names: HashSet<Symbol>) {
        self.enum_names.extend(names);
    }
    fn parse_struct(&mut self) -> Result<StructDef, LexoraError> {
        let start = self.current.span;
        self.expect(Token::Struct)?;
        let name = self.parse_symbol()?;
        self.expect(Token::LeftBrace)?;

        let mut fields: Vec<(Symbol, Type)> = Vec::new();
        let mut field_spans: Vec<Span> = Vec::new();
        while self.current.token != Token::RightBrace {
            let field_name = self.parse_symbol()?;
            self.expect(Token::Colon)?;
            let (field_type, ty_span) = self.parse_type_spanned()?;
            fields.push((field_name, field_type));
            field_spans.push(ty_span);
            if self.current.token == Token::Comma {
                self.advance();
            }
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        Ok(StructDef {
            name,
            fields,
            field_spans,
            span: start.merge(end),
        })
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
        let mut variant_field_spans: Vec<Vec<Span>> = Vec::new();
        while self.current.token != Token::RightBrace {
            let v = self.parse_symbol()?;
            let mut fields = Vec::new();
            let mut spans: Vec<Span> = Vec::new();
            if self.current.token == Token::LeftParen {
                self.advance();
                while self.current.token != Token::RightParen {
                    let (t, sp) = self.parse_type_spanned()?;
                    fields.push(t);
                    spans.push(sp);
                    if self.current.token == Token::Comma {
                        self.advance();
                    }
                }
                self.expect(Token::RightParen)?;
            }
            variants.push((v, fields));
            variant_field_spans.push(spans);
            if self.current.token == Token::Comma {
                self.advance();
            }
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        Ok(EnumDef {
            name,
            params: mem::take(&mut self.type_params),
            variants,
            variant_field_spans,
            span: start.merge(end),
        })
    }
    fn parse_value_block(&mut self) -> Result<Block<'arena>, LexoraError> {
        self.depth_guard(|p| {
            p.depth_bump("blok")?;
            p.with_struct_literal(true, |q| q.parse_value_block_inner())
        })
    }

    fn parse_value_block_inner(&mut self) -> Result<Block<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::LeftBrace)?;
        let mut stmts: Vec<Stmt<'arena>> = Vec::new();
        let mut tail: Option<&'arena Expr<'arena>> = None;
        while !matches!(
            self.current.token,
            Token::RightBrace
                | Token::Eof
                | Token::Fn
                | Token::Struct
                | Token::Import
                | Token::Enum
                | Token::Impl
        ) {
            let starts_stmt = matches!(
                self.current.token,
                Token::Let | Token::Return | Token::While | Token::For
            );
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
        Ok(Block {
            stmts,
            tail,
            span: start.merge(end),
        })
    }
    fn assign_stmt(
        &mut self,
        target: Expr<'arena>,
        value: Expr<'arena>,
        span: Span,
    ) -> Result<Stmt<'arena>, LexoraError> {
        match target {
            Expr::Identifier(name, _, _) => Ok(Stmt::Assign { name, value, span }),
            Expr::Index { .. } | Expr::FieldAccess { .. } | Expr::Deref { .. }
                if target.is_place() =>
            {
                Ok(Stmt::AssignPlace {
                    target,
                    value,
                    span,
                })
            }
            other => Err(LexoraError::Custom {
                message: "gecersiz atama hedefi".to_string(),
                span: other.span(),
            }),
        }
    }
    fn expr_is_valueless(expr: &Expr) -> bool {
        match expr {
            Expr::Match { arms, .. } => arms.iter().all(|(_, b)| Self::block_is_valueless(b)),
            Expr::If {
                then_body,
                else_body,
                ..
            } => match else_body {
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
                found: format!("{:?}", self.current.token),
                span: self.current.span,
            });
        };
        self.expect(Token::Semicolon)?;
        Ok(path)
    }
    fn parse_statement(&mut self) -> Result<Stmt<'arena>, LexoraError> {
        let start = self.current.span;
        match self.current.token.clone() {
            Token::Let => {
                self.advance();
                let name = self.parse_symbol()?;
                let (ty, ty_span) = if self.current.token == Token::Colon {
                    self.advance();
                    let (t, sp) = self.parse_type_spanned()?;
                    (Some(t), Some(sp))
                } else {
                    (None, None)
                };
                self.expect(Token::Equals)?;
                let value = self.parse_expr()?;
                let end = self.current.span;
                self.expect(Token::Semicolon)?;
                Ok(Stmt::Let {
                    name,
                    value,
                    ty,
                    ty_span,
                    span: start.merge(end),
                })
            }
            Token::Return => {
                self.advance();
                let value = self.parse_expr()?;
                let end = self.current.span;
                self.expect(Token::Semicolon)?;
                Ok(Stmt::Return(value, start.merge(end)))
            }

            Token::While => self.parse_while(),
            Token::For => self.parse_for(),
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
        let condition = self.with_struct_literal(false, |p| p.parse_expr())?;
        let then_body = self.parse_value_block()?;
        let mut end = then_body.span;
        let else_body = if self.current.token == Token::Else {
            self.advance();
            let blk = if self.current.token == Token::If {
                let else_if = self.parse_if_expr()?;
                let espan = else_if.span();
                Block {
                    stmts: &[],
                    tail: Some(self.arena.alloc(else_if)),
                    span: espan,
                }
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
        let condition = self.with_struct_literal(false, |p| p.parse_expr())?;
        let body = self.parse_value_block()?;
        let span = start.merge(body.span);
        Ok(Stmt::While {
            condition,
            body,
            span,
        })
    }

    fn parse_for(&mut self) -> Result<Stmt<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::For)?;
        let var = self.parse_symbol()?;
        self.expect(Token::In)?;
        let from = self.with_struct_literal(false, |p| p.parse_expr())?;
        self.expect(Token::DotDot)?;
        let to = self.with_struct_literal(false, |p| p.parse_expr())?;
        let body = self.parse_value_block()?;
        let span = start.merge(body.span);
        Ok(Stmt::For {
            var,
            from,
            to,
            body,
            span,
        })
    }
    fn parse_match_expr(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::Match)?;
        let scrutinee = self.with_struct_literal(false, |p| p.parse_expr())?;
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
                Block {
                    stmts: &[],
                    tail: Some(self.arena.alloc(expr)),
                    span: espan,
                }
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
        let start = self.current.span;
        if matches!(self.current.token, Token::Identifier("_")) {
            self.advance();
            return Ok(Pattern::Wildcard(start));
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
        Ok(Pattern::Variant {
            enum_name,
            variant,
            bindings,
            span: start.merge(self.last_span),
        })
    }
    pub fn parse_expr(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.depth_guard(|p| {
            p.depth_bump("ifade")?;
            p.parse_or()
        })
    }

    fn parse_or(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.depth_guard(|p| {
            let start = p.current.span;
            let mut left = p.parse_and()?;
            while p.current.token == Token::Or {
                p.advance();
                p.depth_bump("ifade")?;
                let right = p.parse_and()?;
                let span = start.merge(right.span());
                let id = p.next_id();
                left = Expr::BinaryOp {
                    left: p.arena.alloc(left),
                    op: BinaryOperator::Or,
                    right: p.arena.alloc(right),
                    span,
                    id,
                };
            }
            Ok(left)
        })
    }
    fn parse_and(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.depth_guard(|p| {
            let start = p.current.span;
            let mut left = p.parse_comparison()?;
            while p.current.token == Token::And {
                p.advance();
                p.depth_bump("ifade")?;
                let right = p.parse_comparison()?;
                let span = start.merge(right.span());
                let id = p.next_id();
                left = Expr::BinaryOp {
                    left: p.arena.alloc(left),
                    op: BinaryOperator::And,
                    right: p.arena.alloc(right),
                    span,
                    id,
                };
            }
            Ok(left)
        })
    }

    fn parse_comparison(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        let left = self.parse_additive()?;
        let op = match self.current.token {
            Token::EqualsEquals => BinaryOperator::Eq,
            Token::BangEquals => BinaryOperator::NotEq,
            Token::Less => BinaryOperator::Less,
            Token::Greater => BinaryOperator::Greater,
            Token::LessEq => BinaryOperator::LessEq,
            Token::GreaterEq => BinaryOperator::GreaterEq,
            _ => return Ok(left),
        };
        self.advance();
        let right = self.parse_additive()?;
        let span = start.merge(right.span());
        let id = self.next_id();
        Ok(Expr::BinaryOp {
            left: self.arena.alloc(left),
            op,
            right: self.arena.alloc(right),
            span,
            id,
        })
    }
    fn parse_additive(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.depth_guard(|p| {
            let start = p.current.span;
            let mut left = p.parse_multiplicative()?;
            loop {
                let op = match p.current.token {
                    Token::Plus => BinaryOperator::Add,
                    Token::Minus => BinaryOperator::Sub,
                    _ => break,
                };
                p.advance();
                p.depth_bump("ifade")?;
                let right = p.parse_multiplicative()?;
                let span = start.merge(right.span());
                let id = p.next_id();
                left = Expr::BinaryOp {
                    left: p.arena.alloc(left),
                    op,
                    right: p.arena.alloc(right),
                    span,
                    id,
                };
            }
            Ok(left)
        })
    }

    fn parse_multiplicative(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.depth_guard(|p| {
            let start = p.current.span;
            let mut left = p.parse_cast()?;
            loop {
                let op = match p.current.token {
                    Token::Star => BinaryOperator::Mul,
                    Token::Slash => BinaryOperator::Div,
                    _ => break,
                };
                p.advance();
                p.depth_bump("ifade")?;
                let right = p.parse_cast()?;
                let span = start.merge(right.span());
                let id = p.next_id();
                left = Expr::BinaryOp {
                    left: p.arena.alloc(left),
                    op,
                    right: p.arena.alloc(right),
                    span,
                    id,
                };
            }
            Ok(left)
        })
    }
    fn parse_cast(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.depth_guard(|p| {
            let mut expr = p.parse_unary()?;
            while p.current.token == Token::As {
                p.depth_bump("ifade")?;
                let start = expr.span();
                p.advance();
                let ty_span = p.current.span;
                let target_type = p.parse_type()?;
                let id = p.next_id();
                expr = Expr::Cast {
                    expr: p.arena.alloc(expr),
                    target_type,
                    span: start.merge(ty_span),
                    id,
                };
            }
            Ok(expr)
        })
    }

    fn parse_unary(&mut self) -> Result<Expr<'arena>, LexoraError> {
        if !matches!(
            self.current.token,
            Token::Not | Token::Minus | Token::Box | Token::Star
        ) {
            return self.parse_postfix();
        }
        self.depth_guard(|p| {
            p.depth_bump("ifade")?;
            p.parse_prefix()
        })
    }

    fn parse_prefix(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        match self.current.token {
            Token::Not => {
                self.advance();
                let operand = self.parse_unary()?;
                let span = start.merge(operand.span());
                let id = self.next_id();
                Ok(Expr::UnaryOp {
                    op: UnaryOperator::Not,
                    operand: self.arena.alloc(operand),
                    span,
                    id,
                })
            }
            Token::Minus => {
                self.advance();
                let operand = self.parse_unary()?;
                let span = start.merge(operand.span());
                let id = self.next_id();
                Ok(Expr::UnaryOp {
                    op: UnaryOperator::Neg,
                    operand: self.arena.alloc(operand),
                    span,
                    id,
                })
            }
            Token::Box => {
                self.advance();
                let value = self.parse_unary()?;
                let span = start.merge(value.span());
                let id = self.next_id();
                Ok(Expr::Box {
                    value: self.arena.alloc(value),
                    span,
                    id,
                })
            }
            Token::Star => {
                self.advance();
                let target = self.parse_unary()?;
                let span = start.merge(target.span());
                let id = self.next_id();
                Ok(Expr::Deref {
                    target: self.arena.alloc(target),
                    span,
                    id,
                })
            }
            _ => self.parse_postfix(),
        }
    }
    fn parse_postfix(&mut self) -> Result<Expr<'arena>, LexoraError> {
        self.depth_guard(|p| {
            let mut expr = p.parse_primary()?;
            loop {
                if !matches!(
                    p.current.token,
                    Token::LeftBracket | Token::Dot | Token::Question
                ) {
                    break;
                }
                p.depth_bump("ifade")?;
                match p.current.token {
                    Token::LeftBracket => {
                        let start = expr.span();
                        p.advance();
                        let index = p.with_struct_literal(true, |q| q.parse_expr())?;
                        let end = p.current.span;
                        p.expect(Token::RightBracket)?;
                        let id = p.next_id();
                        expr = Expr::Index {
                            array: p.arena.alloc(expr),
                            index: p.arena.alloc(index),
                            span: start.merge(end),
                            id,
                        };
                    }
                    Token::Dot => {
                        let start = expr.span();
                        p.advance();
                        let end = p.current.span;
                        let field = p.parse_symbol()?;
                        if p.current.token == Token::LeftParen {
                            p.advance();
                            let mut args: Vec<Expr<'arena>> = Vec::new();
                            while p.current.token != Token::RightParen {
                                if Self::arg_list_stopper(&p.current.token) {
                                    break;
                                }
                                args.push(p.with_struct_literal(true, |q| q.parse_expr())?);
                                if p.current.token == Token::Comma {
                                    p.advance();
                                }
                            }
                            let cend = p.current.span;
                            p.expect(Token::RightParen)?;
                            let args = p.arena.alloc_slice_fill_iter(args.into_iter());
                            let id = p.next_id();
                            expr = Expr::MethodCall {
                                receiver: p.arena.alloc(expr),
                                method: field,
                                args,
                                span: start.merge(cend),
                                id,
                            };
                        } else {
                            let id = p.next_id();
                            expr = Expr::FieldAccess {
                                object: p.arena.alloc(expr),
                                field,
                                span: start.merge(end),
                                id,
                            };
                        }
                    }

                    Token::Question => {
                        let start = expr.span();
                        let end = p.current.span;
                        p.advance();
                        expr = Expr::Try {
                            expr: p.arena.alloc(expr),
                            span: start.merge(end),
                            id: p.next_id(),
                        };
                    }
                    _ => break,
                }
            }
            Ok(expr)
        })
    }

    fn parse_primary(&mut self) -> Result<Expr<'arena>, LexoraError> {
        let start = self.current.span;
        match self.current.token.clone() {
            Token::Integer(n) => {
                self.advance();
                Ok(Expr::Integer(n, start, self.next_id()))
            }
            Token::Float(v) => {
                self.advance();
                Ok(Expr::Float(v, start, self.next_id()))
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
                let expr = self.with_struct_literal(true, |p| p.parse_expr())?;
                self.expect(Token::RightParen)?;
                Ok(expr)
            }
            Token::LeftBracket => {
                self.advance();
                let mut elems: Vec<Expr<'arena>> = Vec::new();
                while self.current.token != Token::RightBracket {
                    if Self::arg_list_stopper(&self.current.token) {
                        break;
                    }
                    elems.push(self.with_struct_literal(true, |p| p.parse_expr())?);
                    if self.current.token == Token::Comma {
                        self.advance();
                    }
                }
                let end = self.current.span;
                self.expect(Token::RightBracket)?;
                let elems = self.arena.alloc_slice_fill_iter(elems.into_iter());
                Ok(Expr::ArrayLiteral(elems, start.merge(end), self.next_id()))
            }
            Token::SelfKw => {
                self.advance();
                let sym = self.interner.intern("self");
                Ok(Expr::Identifier(sym, start, self.next_id()))
            }
            Token::Match => self.parse_match_expr(),
            Token::If => self.parse_if_expr(),
            Token::Identifier(_) => {
                let name = self.parse_symbol()?;
                if self.current.token == Token::ColonColon {
                    self.advance();
                    let type_args = if self.current.token == Token::Less {
                        self.parse_type_args()?
                    } else {
                        Vec::new()
                    };
                    if self.current.token == Token::LeftParen {
                        self.advance();
                        let mut args: Vec<Expr<'arena>> = Vec::new();
                        while self.current.token != Token::RightParen {
                            if Self::arg_list_stopper(&self.current.token) {
                                break;
                            }
                            args.push(self.with_struct_literal(true, |p| p.parse_expr())?);
                            if self.current.token == Token::Comma {
                                self.advance();
                            }
                        }
                        let end = self.current.span;
                        self.expect(Token::RightParen)?;
                        let args = self.arena.alloc_slice_fill_iter(args.into_iter());
                        return Ok(Expr::Call {
                            name,
                            args,
                            type_args,
                            span: start.merge(end),
                            id: self.next_id(),
                        });
                    }
                    if !type_args.is_empty() {
                        self.expect(Token::ColonColon)?;
                    }
                    let mut var_span = self.current.span;
                    let variant = self.parse_symbol()?;
                    let mut args: Vec<Expr<'arena>> = Vec::new();

                    if self.current.token == Token::LeftParen {
                        self.advance();
                        while self.current.token != Token::RightParen {
                            if Self::arg_list_stopper(&self.current.token) {
                                break;
                            }
                            args.push(self.with_struct_literal(true, |p| p.parse_expr())?);
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
                        if Self::arg_list_stopper(&self.current.token) {
                            break;
                        }
                        args.push(self.with_struct_literal(true, |p| p.parse_expr())?);
                        if self.current.token == Token::Comma {
                            self.advance();
                        }
                    }
                    let end = self.current.span;
                    self.expect(Token::RightParen)?;
                    let args = self.arena.alloc_slice_fill_iter(args.into_iter());
                    Ok(Expr::Call {
                        name,
                        args,
                        type_args: Vec::new(),
                        span: start.merge(end),
                        id: self.next_id(),
                    })
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
                    Ok(Expr::StructLiteral {
                        name,
                        fields,
                        span: start.merge(end),
                        id: self.next_id(),
                    })
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
                if !Self::is_expr_stopper(&self.current.token) {
                    self.advance();
                }
                Ok(Expr::Error(start, self.next_id()))
            }
        }
    }
    fn parse_function(
        &mut self,
        self_type: Option<Symbol>,
    ) -> Result<Function<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::Fn)?;
        let name = self.parse_symbol()?;
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
        self.expect(Token::LeftParen)?;

        let mut params: Vec<(Symbol, Type)> = Vec::new();
        let mut param_spans: Vec<Span> = Vec::new();
        let mut self_param = false;
        if let Some(recv) = self_type {
            if self.current.token == Token::SelfKw {
                let sp = self.current.span;
                self.advance();
                let ty = if self.enum_names.contains(&recv) {
                    Type::Enum(recv, Vec::new())
                } else {
                    Type::Struct(recv)
                };
                let sym = self.interner.intern("self");
                params.push((sym, ty));
                param_spans.push(sp);
                self_param = true;
                if self.current.token == Token::Comma {
                    self.advance();
                }
            }
        }
        while self.current.token != Token::RightParen {
            let param_name = self.parse_symbol()?;
            self.expect(Token::Colon)?;
            let (param_type, ty_span) = self.parse_type_spanned()?;
            params.push((param_name, param_type));
            param_spans.push(ty_span);
            if self.current.token == Token::Comma {
                self.advance();
            }
        }
        self.expect(Token::RightParen)?;
        self.expect(Token::Arrow)?;
        let (return_type, ret_span) = self.parse_type_spanned()?;

        let body = self.parse_value_block()?;
        let params = self.arena.alloc_slice_fill_iter(params.into_iter());
        let param_spans = self.arena.alloc_slice_fill_iter(param_spans.into_iter());
        let span = start.merge(body.span);

        Ok(Function {
            name,
            type_params: mem::take(&mut self.type_params),
            params,
            param_spans,
            return_type,
            ret_span,
            body,
            span,
            self_param,
        })
    }

    fn parse_impl(&mut self) -> Result<ImplBlock<'arena>, LexoraError> {
        let start = self.current.span;
        self.expect(Token::Impl)?;
        let type_span = self.current.span;
        let type_name = self.parse_symbol()?;
        self.expect(Token::LeftBrace)?;
        let mut methods: Vec<Function<'arena>> = Vec::new();
        while self.current.token != Token::RightBrace {
            if matches!(
                self.current.token,
                Token::Eof | Token::Struct | Token::Import | Token::Enum | Token::Impl
            ) {
                break;
            }
            self.type_params.clear();
            methods.push(self.parse_function(Some(type_name))?);
        }
        let end = self.current.span;
        self.expect(Token::RightBrace)?;
        Ok(ImplBlock {
            type_name,
            type_span,
            methods,
            span: start.merge(end),
        })
    }
}

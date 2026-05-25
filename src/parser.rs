use bumpalo::Bump;
use std::mem;
use crate::lexer::{Lexer, Token, SpannedToken};
use crate::ast::*;
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
}

impl<'src, 'arena> Parser<'src, 'arena> {
    pub fn new(mut lexer: Lexer<'src>, arena: &'arena Bump) ->Result<Self, LexoraError> {
        let current = lexer.next_token()?;
        let peek = lexer.next_token()?;
        Ok(Parser {
            lexer,
            current,
            peek,
            arena,
            interner: Interner::new(),
            allow_struct_literal: true,
        })
    }
    fn advance(&mut self) -> Result<SpannedToken<'src>, LexoraError>{
        let next       = self.lexer.next_token()?;
        let prev_peek                = mem::replace(&mut self.peek, next);
        Ok(mem::replace(&mut self.current, prev_peek))
    }

    fn expect(&mut self, expected: Token<'src>) -> Result<SpannedToken<'src>, LexoraError> {
        if self.current.token == expected {
            self.advance()
        } else {
            Err(LexoraError::UnexpectedToken{
                expected: format!("{:?}", expected),
                found:    format!("{:?}", self.current.token),
                span:     self.current.span,
            })
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
       self.advance()?;
       Ok(sym)
   }
}
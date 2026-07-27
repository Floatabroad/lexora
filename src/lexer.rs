use crate::span::Span;
use crate::error::LexoraError;


#[derive(Debug, Clone, PartialEq)]
pub enum Token<'src> {
    Integer(i64),
    Identifier(&'src str),
    StringLiteral(&'src str),

    Let, Fn, Return, If, Else,
    True, False, While, For, In,
    And, Or, Not, As, Import, Struct, Box, Enum, Match,

    I32, I64, Bool, Void, Str,

    Plus, Minus, Star, Slash,
    Equals, EqualsEquals,
    Bang, BangEquals,
    Less, Greater, LessEq, GreaterEq,
    FatArrow,

    Semicolon, Colon, Comma,
    Arrow, Dot, DotDot,
    LeftParen, RightParen,
    LeftBrace, RightBrace,
    LeftBracket, RightBracket,
    ColonColon,Question,

    Eof,
    Error,
}
#[derive(Debug, Clone)]
pub struct SpannedToken<'src> {
    pub token: Token<'src>,
    pub span: Span,
}


pub struct Lexer<'src> {
    source:   &'src str,
    pos:      usize,
    pub line: usize,
    base: u32,
    errors: Vec<LexoraError>,
}

impl<'src> Lexer<'src> {
    pub fn new(source: &'src str, base: u32) -> Self {

        Lexer { source, pos: 0, line: 1, base, errors: Vec::new()}
    }
    pub fn take_errors(&mut self) -> Vec<LexoraError> {
        std::mem::take(&mut self.errors)
    }
    fn span(&self, start: u32, end: u32) -> Span {
        Span::new(self.base + start, self.base + end)
    }
    fn current(&self) -> u8 {
        if self.pos < self.source.len() {
            self.source.as_bytes()[self.pos]
        }else {
            0
        }
    }
   
    fn advance(&mut self) {
        if self.pos < self.source.len() {
            if self.source.as_bytes()[self.pos] == b'\n' {
                self.line += 1;
            }
            self.pos += 1;
        }
    }
    fn skip_whitespace(&mut self) {
        loop {
            while self.pos < self.source.len()
                && (self.current() == b' '
                || self.current() == b'\t'
                || self.current() == b'\r'
                || self.current() == b'\n') {
                self.advance();
            }
            if self.current() == b'/'
                && self.pos + 1 < self.source.len()
                && self.source.as_bytes()[self.pos + 1] == b'/' {
                while self.pos < self.source.len() && self.current() != b'\n' {
                    self.advance();
                }
            } else {
                return;
            }
        }
    }
    pub fn next_token(&mut self) -> SpannedToken<'src> {
        self.skip_whitespace();
        let start = self.pos as u32;

        if self.pos >= self.source.len() {
            return SpannedToken {
                token: Token::Eof,
                span: self.span(start, start),
            };
        }
        let ch = self.current();
        let token = match ch {
            b'+' => {self.advance(); Token::Plus}
            b'-' => {
                self.advance();
                if self.current() == b'>' {self.advance(); Token::Arrow}
                else {Token::Minus}
            }
            b'*' => {self.advance(); Token::Star}
            b'/' => {self.advance(); Token::Slash}
            b';' => {self.advance(); Token::Semicolon}
            b':' => {
                self.advance();
                if self.current() == b':' { self.advance(); Token::ColonColon }
                else { Token::Colon }
            }
            b',' => {self.advance(); Token::Comma}
            b'(' => {self.advance(); Token::LeftParen}
            b')' => {self.advance(); Token::RightParen}
            b'{' => {self.advance(); Token::LeftBrace}
            b'}' => {self.advance(); Token::RightBrace}
            b'[' => {self.advance(); Token::LeftBracket}
            b']' => {self.advance(); Token::RightBracket}
            b'.' => {
                self.advance();
                if self.current() == b'.' {self.advance();Token::DotDot}
                else {Token::Dot}
            }
            b'<' => {
                self.advance();
                if self.current() == b'=' {self.advance(); Token::LessEq}
                else {Token::Less}
            }
            b'>' => {
                self.advance();
                if self.current() == b'=' {self.advance(); Token::GreaterEq}
                else {Token::Greater}
            }
            b'=' => {
                self.advance();
                if self.current() == b'=' { self.advance(); Token::EqualsEquals }
                else if self.current() == b'>' { self.advance(); Token::FatArrow }
                else { Token::Equals }
            }
            b'!' => {
                self.advance();
                if self.current() == b'=' {self.advance(); Token::BangEquals}
                else {Token::Bang}
            }
           b'?' => {self.advance(); Token::Question}
            b'0'..=b'9' => self.read_integer(),
            b'"'        => self.read_string(start),
            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.read_identifier(),
            _ => {
                let c = self.source[self.pos..].chars().next().unwrap();
                self.errors.push(LexoraError::Custom {
                    message: format!("beklenmedik karakter: '{}'", c),
                    span: self.span(start, start + c.len_utf8() as u32),
                });
                self.pos += c.len_utf8();
                Token::Error
            }
        };
        let end = self.pos as u32;
        SpannedToken { token, span: self.span(start, end) }
    }

    fn read_integer(&mut self) -> Token<'src> {
        let start = self.pos;
        while self.pos < self.source.len()
            && self.source.as_bytes()[self.pos].is_ascii_digit() {
            self.advance();
        }
        let s = &self.source[start..self.pos];
        match s.parse::<i64>() {
            Ok(value) => Token::Integer(value),
            Err(_) => {
                self.errors.push(LexoraError::Custom {
                    message: format!("tam sayi literali cok buyuk (i64 siniri asildi): {}", s),
                    span: self.span(start as u32, self.pos as u32),
                });
                Token::Integer(0)
            }
        }
    }
    fn read_string(&mut self, start: u32) -> Token<'src> {
        self.advance();
        let content_start = self.pos;
        while self.pos < self.source.len() && self.current() != b'"' {
            self.advance();
        }
        if self.pos >= self.source.len() {
            self.errors.push(LexoraError::Custom {
                message: "Kapatilmamis string".to_string(),
                span: self.span(start, self.pos as u32),
            });
            return Token::Error;
        }
        let content = &self.source[content_start..self.pos];
        self.advance();
        Token::StringLiteral(content)
    }
    fn read_identifier(&mut self) -> Token<'src> {
        let start = self.pos;
        while self.pos < self.source.len()
            && (self.source.as_bytes()[self.pos].is_ascii_alphanumeric()
            || self.source.as_bytes()[self.pos] == b'_') {
            self.advance();
        }
        let ident = &self.source[start..self.pos];
        match ident {
            "let" => Token::Let,
            "fn" => Token::Fn,
            "return" => Token::Return,
            "if" => Token::If,
            "else" => Token::Else,
            "true" => Token::True,
            "false" => Token::False,
            "while" => Token::While,
            "for" => Token::For,
            "in" => Token::In,
            "and" => Token::And,
            "or" => Token::Or,
            "not" => Token::Not,
            "as" => Token::As,
            "import" => Token::Import,
            "struct" => Token::Struct,
            "box" => Token::Box,
            "i32" => Token::I32,
            "i64" => Token::I64,
            "bool" => Token::Bool,
            "void" => Token::Void,
            "str" => Token::Str,
            "enum" => Token::Enum,
            "match" => Token::Match,
            _ => Token::Identifier(ident),
        }
    }
}


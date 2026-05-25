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
    Not,
    As,
    Import,
    Struct,

    //Types
    I32,
    I64,
    Bool,
    Void,
    Str,

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
    LessEq,
    GreaterEq,

    //Delimiter,
    Semicolon,
    Colon,
    Comma,
    Arrow, // ->
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Dot,

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
                    self.advance();
                    Token::Dot
                }
            }
            ';' => { self.advance(); Token::Semicolon }
            ':' => { self.advance(); Token::Colon }
            ',' => { self.advance(); Token::Comma }
            '(' => { self.advance(); Token::LeftParen}
            ')' => { self.advance(); Token::RightParen}
            '{' => { self.advance(); Token::LeftBrace}
            '}' => { self.advance(); Token::RightBrace}
            '[' => { self.advance(); Token::LeftBracket}
            ']' => { self.advance(); Token::RightBracket}
            '<' => {
                if self.peek() == '=' {
                    self.advance(); self.advance();
                    Token::LessEq
                } else {
                    self.advance();
                    Token::Less
                }
            }
            '>' => {
                if self.peek() == '=' {
                    self.advance(); self.advance();
                    Token::GreaterEq
                } else {
                    self.advance();
                    Token::Greater
                }
            }
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
            "not" => Token::Not,
            "str" => Token::Str,
            "as" => Token::As,
            "import" => Token::Import,
            "struct" => Token::Struct,
            _ => Token::Identifier(ident),
        }
    }
}
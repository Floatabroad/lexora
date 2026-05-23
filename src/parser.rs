use crate::lexer::{Lexer, Token};
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
            Token::Str => { self.advance(); Type::Str }
            _ => panic!("Beklenen tip, bulunan: {:?}", self.current),
        }
    }
    pub fn parse_program(&mut self) -> Program {
        let mut functions = Vec::new();
        let mut imports = Vec::new();
        while self.current != Token::Eof {
            if self.current == Token::Import {
                self.advance();
                let path = match self.advance() {
                    Token::StringLiteral(s) => s,
                    _ => panic!("import sonrası dosya yolu bekleniyor"),
                };
                self.expect(Token::Semicolon);
                imports.push(path);
            } else {
                functions.push(self.parse_function());
            }
        }
        Program { functions, imports }
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
        let mut left = self.parse_comparison();
        loop {
            let op = match self.current {
                Token::And => BinaryOperator::And,
                Token::Or => BinaryOperator::Or,
                _ => break,

            };
            self.advance();
            let right = self.parse_comparison();
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left
    }
    fn parse_comparison(&mut self) -> Expr {
        let mut left = self.parse_additive();
        loop {
            let op = match self.current{
                Token::EqualsEquals  => BinaryOperator::Eq,
                Token::BangEquals    => BinaryOperator::NotEq,
                Token::Less          => BinaryOperator::Less,
                Token::Greater       => BinaryOperator::Greater,
                Token::LessEq        => BinaryOperator::LessEq,
                Token::GreaterEq     => BinaryOperator::GreaterEq,
                _ => break,
            };
            self.advance();
            let right = self.parse_additive();
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_additive(&mut self) -> Expr {
        let mut left = self.parse_multiplicative();
        loop {
            let op = match self.current {
                Token::Plus => BinaryOperator::Add,
                Token::Minus => BinaryOperator::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_multiplicative();
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left
    }

    fn parse_multiplicative(&mut self) -> Expr {
        let mut left = self.parse_cast();
        loop {
            let op = match self.current {
                Token::Star => BinaryOperator::Mul,
                Token::Slash => BinaryOperator::Div,
                _ => break,
            };
            self.advance();
            let right = self.parse_cast();
            left = Expr::BinaryOp {
                left: Box::new(left),
                op,
                right: Box::new(right),
            };
        }
        left

    }
    fn parse_cast(&mut self) -> Expr {
        let expr = self.parse_primary();
        if self.current == Token::As {
            self.advance();
            let target_type = self.parse_type();
            Expr::Cast { expr: Box::new(expr), target_type }
        }else {
            expr
        }
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
            Token::Not => {
                self.advance();
                let operand = self.parse_comparison();
                Expr::UnaryOp { op: UnaryOperator::Not, operand: Box::new(operand) }
            }
            Token::Minus => {
                self.advance();
                let operand = self.parse_primary();
                Expr::UnaryOp { op: UnaryOperator::Neg, operand: Box::new(operand) }
            }
            Token::True => { self.advance(); Expr::Bool(true) }
            Token::False => { self.advance(); Expr::Bool(false) }
            Token::StringLiteral(s) => { self.advance(); Expr::StringLiteral(s) }
            Token::LeftParen => {
                self.advance();
                let expr = self.parse_expr();
                self.expect(Token::RightParen);
                expr
            }
            _ => panic!("Hata [satır {}]: Beklenmedik token: {:?}", self.current_line, self.current),
        }
    }
}
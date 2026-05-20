use crate::lexer::{Lexer, Token};
use crate::ast::*;

pub struct Parser {
    lexer: Lexer,
    current: Token,
}

impl Parser {
    pub fn new(mut lexer: Lexer) -> Self {
        let current = lexer.next_token();
        Parser { lexer, current }
    }
    fn advance(&mut self) -> Token {
        let prev = self.current.clone();
        self.current = self.lexer.next_token();
        prev
    }
    fn expect(&mut self, expected: Token) -> Token {
        if self.current == expected {
            self.advance()
        }else {
            panic!("Beklenen: {:?}, Bulunan: {:?}", expected, self.current);
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
                Stmt::Let { name, ty, value }
            }
            Token::Return => {
                self.advance();
                let value = self.parse_expr();
                self.expect(Token::Semicolon);
                Stmt::Return(value)
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
                Stmt::If {condition, then_body, else_body}
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
                Stmt::While {condition, body}
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
                    Stmt::Assign{name, value}
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
                    Stmt::Expr(Expr::Call { name, args })
                }else {
                    panic!("unexpected token: {:?}", self.current);
                }
            },
            _ => {
                let expr = self.parse_expr();
                self.expect(Token::Semicolon);
                Stmt::Expr(expr)
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
            _ => panic!("Beklenmedik token: {:?}", self.current),
        }
    }
}
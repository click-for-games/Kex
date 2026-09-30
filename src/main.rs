use std::collections::HashMap;
use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::{Command, exit};

// Color definitions for dramatic output
const COLOR_RED: &str = "\x1b[31;1m";
const COLOR_GREEN: &str = "\x1b[32;1m";
const COLOR_YELLOW: &str = "\x1b[33;1m";
const COLOR_CYAN: &str = "\x1b[36;1m";
const COLOR_RESET: &str = "\x1b[0m";

fn kex_error(phase: &str, line: usize, msg: &str) {
    eprintln!(
        "{}Kex {} Error{} [Line {}]: {}",
        COLOR_RED, phase, COLOR_RESET, line, msg
    );
    exit(1);
}

// ============================================================================
// MODULE 1: VALUE & DATA TYPES
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum KexValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Array(Vec<KexValue>),
    Null,
}

impl KexValue {
    pub fn to_string(&self) -> String {
        match self {
            KexValue::Number(n) => n.to_string(),
            KexValue::String(s) => s.clone(),
            KexValue::Boolean(b) => b.to_string(),
            KexValue::Array(arr) => {
                let items: Vec<String> = arr.iter().map(|v| v.to_string()).collect();
                format!("[{}]", items.join(", "))
            }
            KexValue::Null => "null".to_string(),
        }
    }
}

// ============================================================================
// MODULE 2: LEXER & TOKENS
// ============================================================================

#[derive(Debug, PartialEq, Clone)]
pub enum TokenKind {
    Let, If, Else, While, Fn, Return, KxOut, KxIn, Len, TypeCast, SysCall,
    Identifier(String), Number(f64), StringLit(String), BoolLit(bool),
    Assign, Plus, Minus, Star, Slash, Percent, Equal, NotEqual,
    Less, Greater, LessEq, GreaterEq, And, Or, Not,
    LParen, RParen, LBrace, RBrace, LBracket, RBracket, Semicolon, Comma, EOF,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
}

pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer { input, pos: 0, line: 1 }
    }

    pub fn tokenize(&mut self) -> Vec<Token> {
        let mut tokens = Vec::new();
        let chars: Vec<char> = self.input.chars().collect();

        while self.pos < chars.len() {
            let ch = chars[self.pos];

            if ch == '\n' {
                self.line += 1;
                self.pos += 1;
                continue;
            }

            if ch.is_whitespace() {
                self.pos += 1;
                continue;
            }

            let current_line = self.line;

            match ch {
                ';' => tokens.push(Token { kind: TokenKind::Semicolon, line: current_line }),
                ',' => tokens.push(Token { kind: TokenKind::Comma, line: current_line }),
                '(' => tokens.push(Token { kind: TokenKind::LParen, line: current_line }),
                ')' => tokens.push(Token { kind: TokenKind::RParen, line: current_line }),
                '{' => tokens.push(Token { kind: TokenKind::LBrace, line: current_line }),
                '}' => tokens.push(Token { kind: TokenKind::RBrace, line: current_line }),
                '[' => tokens.push(Token { kind: TokenKind::LBracket, line: current_line }),
                ']' => tokens.push(Token { kind: TokenKind::RBracket, line: current_line }),
                '+' => tokens.push(Token { kind: TokenKind::Plus, line: current_line }),
                '-' => tokens.push(Token { kind: TokenKind::Minus, line: current_line }),
                '*' => tokens.push(Token { kind: TokenKind::Star, line: current_line }),
                '/' => {
                    if self.peek_next(&chars) == '/' {
                        while self.pos < chars.len() && chars[self.pos] != '\n' {
                            self.pos += 1;
                        }
                        continue;
                    } else if self.peek_next(&chars) == '*' {
                        self.pos += 2;
                        while self.pos + 1 < chars.len()
                            && !(chars[self.pos] == '*' && chars[self.pos + 1] == '/')
                        {
                            if chars[self.pos] == '\n' { self.line += 1; }
                            self.pos += 1;
                        }
                        self.pos += 2;
                        continue;
                    } else {
                        tokens.push(Token { kind: TokenKind::Slash, line: current_line });
                    }
                }
                '%' => tokens.push(Token { kind: TokenKind::Percent, line: current_line }),
                '=' => {
                    if self.peek_next(&chars) == '=' {
                        self.pos += 1;
                        tokens.push(Token { kind: TokenKind::Equal, line: current_line });
                    } else {
                        tokens.push(Token { kind: TokenKind::Assign, line: current_line });
                    }
                }
                '!' => {
                    if self.peek_next(&chars) == '=' {
                        self.pos += 1;
                        tokens.push(Token { kind: TokenKind::NotEqual, line: current_line });
                    } else {
                        tokens.push(Token { kind: TokenKind::Not, line: current_line });
                    }
                }
                '<' => {
                    if self.peek_next(&chars) == '=' {
                        self.pos += 1;
                        tokens.push(Token { kind: TokenKind::LessEq, line: current_line });
                    } else {
                        tokens.push(Token { kind: TokenKind::Less, line: current_line });
                    }
                }
                '>' => {
                    if self.peek_next(&chars) == '=' {
                        self.pos += 1;
                        tokens.push(Token { kind: TokenKind::GreaterEq, line: current_line });
                    } else {
                        tokens.push(Token { kind: TokenKind::Greater, line: current_line });
                    }
                }
                '&' if self.peek_next(&chars) == '&' => {
                    self.pos += 1;
                    tokens.push(Token { kind: TokenKind::And, line: current_line });
                }
                '|' if self.peek_next(&chars) == '|' => {
                    self.pos += 1;
                    tokens.push(Token { kind: TokenKind::Or, line: current_line });
                }
                '\'' | '"' => {
                    let quote = ch;
                    self.pos += 1;
                    let start = self.pos;
                    while self.pos < chars.len() && chars[self.pos] != quote {
                        if chars[self.pos] == '\n' { self.line += 1; }
                        self.pos += 1;
                    }
                    let s: String = chars[start..self.pos].iter().collect();
                    tokens.push(Token { kind: TokenKind::StringLit(s), line: current_line });
                }
                c if c.is_ascii_digit() => {
                    let start = self.pos;
                    while self.pos < chars.len()
                        && (chars[self.pos].is_ascii_digit() || chars[self.pos] == '.')
                    {
                        self.pos += 1;
                    }
                    let s: String = chars[start..self.pos].iter().collect();
                    tokens.push(Token { kind: TokenKind::Number(s.parse().unwrap_or(0.0)), line: current_line });
                    continue;
                }
                c if c.is_alphabetic() || c == '_' => {
                    let start = self.pos;
                    while self.pos < chars.len()
                        && (chars[self.pos].is_alphanumeric() || chars[self.pos] == '_')
                    {
                        self.pos += 1;
                    }
                    let ident: String = chars[start..self.pos].iter().collect();
                    let kind = match ident.as_str() {
                        "let" => TokenKind::Let,
                        "if" => TokenKind::If,
                        "else" => TokenKind::Else,
                        "while" => TokenKind::While,
                        "fn" => TokenKind::Fn,
                        "return" => TokenKind::Return,
                        "kxout" => TokenKind::KxOut,
                        "kxin" => TokenKind::KxIn,
                        "len" => TokenKind::Len,
                        "cast" => TokenKind::TypeCast,
                        "syscall" => TokenKind::SysCall,
                        "true" => TokenKind::BoolLit(true),
                        "false" => TokenKind::BoolLit(false),
                        _ => TokenKind::Identifier(ident),
                    };
                    tokens.push(Token { kind, line: current_line });
                    continue;
                }
                unknown => {
                    kex_error("Lexer", current_line, &format!("Unexpected character '{}'", unknown));
                }
            }
            self.pos += 1;
        }
        tokens.push(Token { kind: TokenKind::EOF, line: self.line });
        tokens
    }

    fn peek_next(&self, chars: &[char]) -> char {
        if self.pos + 1 < chars.len() { chars[self.pos + 1] } else { '\0' }
    }
}

// ============================================================================
// MODULE 3: AST & PARSER
// ============================================================================

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(KexValue),
    Var(String),
    ArrayLit(Vec<Expr>),
    Index(Box<Expr>, Box<Expr>),
    Binary(Box<Expr>, String, Box<Expr>),
    Unary(String, Box<Expr>),
    Call(String, Vec<Expr>),
    KxIn,
    Len(Box<Expr>),
    TypeCast(Box<Expr>, String),
}

#[derive(Debug, Clone)]
pub enum Statement {
    VarDecl(String, Expr),
    Assign(String, Expr),
    ArraySet(String, Expr, Expr),
    KxOut(Expr),
    SysCall(Expr),
    If(Expr, Vec<Statement>, Vec<Statement>),
    While(Expr, Vec<Statement>),
    FnDecl(String, Vec<String>, Vec<Statement>),
    Return(Expr),
    Expr(Expr),
}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> Token {
        self.tokens.get(self.pos).cloned().unwrap_or(Token { kind: TokenKind::EOF, line: 0 })
    }

    fn consume(&mut self) -> Token {
        let tok = self.peek();
        self.pos += 1;
        tok
    }

    pub fn parse(&mut self) -> Vec<Statement> {
        let mut stmts = Vec::new();
        while self.peek().kind != TokenKind::EOF {
            stmts.push(self.parse_statement());
        }
        stmts
    }

    fn parse_statement(&mut self) -> Statement {
        let tok = self.peek();
        match tok.kind {
            TokenKind::Let => {
                self.consume();
                if let TokenKind::Identifier(name) = self.consume().kind {
                    let mut init = Expr::Literal(KexValue::Null);
                    if self.peek().kind == TokenKind::Assign {
                        self.consume();
                        init = self.parse_expr();
                    }
                    if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                    Statement::VarDecl(name, init)
                } else {
                    kex_error("Syntax", tok.line, "Expected identifier after 'let'");
                    unreachable!()
                }
            }
            TokenKind::KxOut => {
                self.consume();
                let expr = self.parse_expr();
                if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                Statement::KxOut(expr)
            }
            TokenKind::SysCall => {
                self.consume();
                let expr = self.parse_expr();
                if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                Statement::SysCall(expr)
            }
            TokenKind::If => {
                self.consume();
                let cond = self.parse_expr();
                let then_branch = self.parse_block();
                let mut else_branch = Vec::new();
                if self.peek().kind == TokenKind::Else {
                    self.consume();
                    else_branch = self.parse_block();
                }
                Statement::If(cond, then_branch, else_branch)
            }
            TokenKind::While => {
                self.consume();
                let cond = self.parse_expr();
                let body = self.parse_block();
                Statement::While(cond, body)
            }
            TokenKind::Fn => {
                self.consume();
                if let TokenKind::Identifier(name) = self.consume().kind {
                    self.expect(TokenKind::LParen);
                    let mut params = Vec::new();
                    if self.peek().kind != TokenKind::RParen {
                        loop {
                            if let TokenKind::Identifier(p) = self.consume().kind {
                                params.push(p);
                            }
                            if self.peek().kind == TokenKind::Comma {
                                self.consume();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(TokenKind::RParen);
                    let body = self.parse_block();
                    Statement::FnDecl(name, params, body)
                } else {
                    kex_error("Syntax", tok.line, "Expected function identifier after 'fn'");
                    unreachable!()
                }
            }
            TokenKind::Return => {
                self.consume();
                let expr = self.parse_expr();
                if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                Statement::Return(expr)
            }
            TokenKind::Identifier(ref name) => {
                let id_name = name.clone();
                if self.pos + 1 < self.tokens.len() && self.tokens[self.pos + 1].kind == TokenKind::Assign {
                    self.consume();
                    self.consume();
                    let val = self.parse_expr();
                    if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                    Statement::Assign(id_name, val)
                } else if self.pos + 1 < self.tokens.len() && self.tokens[self.pos + 1].kind == TokenKind::LBracket {
                    self.consume();
                    self.consume();
                    let idx = self.parse_expr();
                    self.expect(TokenKind::RBracket);
                    self.expect(TokenKind::Assign);
                    let val = self.parse_expr();
                    if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                    Statement::ArraySet(id_name, idx, val)
                } else {
                    let expr = self.parse_expr();
                    if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                    Statement::Expr(expr)
                }
            }
            _ => {
                let expr = self.parse_expr();
                if self.peek().kind == TokenKind::Semicolon { self.consume(); }
                Statement::Expr(expr)
            }
        }
    }

    fn parse_block(&mut self) -> Vec<Statement> {
        self.expect(TokenKind::LBrace);
        let mut stmts = Vec::new();
        while self.peek().kind != TokenKind::RBrace && self.peek().kind != TokenKind::EOF {
            stmts.push(self.parse_statement());
        }
        self.expect(TokenKind::RBrace);
        stmts
    }

    fn parse_expr(&mut self) -> Expr { self.parse_logical_or() }

    fn parse_logical_or(&mut self) -> Expr {
        let mut left = self.parse_logical_and();
        while self.peek().kind == TokenKind::Or {
            self.consume();
            let right = self.parse_logical_and();
            left = Expr::Binary(Box::new(left), "||".to_string(), Box::new(right));
        }
        left
    }

    fn parse_logical_and(&mut self) -> Expr {
        let mut left = self.parse_equality();
        while self.peek().kind == TokenKind::And {
            self.consume();
            let right = self.parse_equality();
            left = Expr::Binary(Box::new(left), "&&".to_string(), Box::new(right));
        }
        left
    }

    fn parse_equality(&mut self) -> Expr {
        let mut left = self.parse_comparison();
        while matches!(self.peek().kind, TokenKind::Equal | TokenKind::NotEqual) {
            let op = match self.consume().kind {
                TokenKind::Equal => "==".to_string(),
                TokenKind::NotEqual => "!=".to_string(),
                _ => unreachable!(),
            };
            let right = self.parse_comparison();
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_comparison(&mut self) -> Expr {
        let mut left = self.parse_term();
        while matches!(
            self.peek().kind,
            TokenKind::Less | TokenKind::Greater | TokenKind::LessEq | TokenKind::GreaterEq
        ) {
            let op = match self.consume().kind {
                TokenKind::Less => "<".to_string(),
                TokenKind::Greater => ">".to_string(),
                TokenKind::LessEq => "<=".to_string(),
                TokenKind::GreaterEq => ">=".to_string(),
                _ => unreachable!(),
            };
            let right = self.parse_term();
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_term(&mut self) -> Expr {
        let mut left = self.parse_factor();
        while matches!(self.peek().kind, TokenKind::Plus | TokenKind::Minus) {
            let op = match self.consume().kind {
                TokenKind::Plus => "+".to_string(),
                TokenKind::Minus => "-".to_string(),
                _ => unreachable!(),
            };
            let right = self.parse_factor();
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_factor(&mut self) -> Expr {
        let mut left = self.parse_primary();
        while matches!(self.peek().kind, TokenKind::Star | TokenKind::Slash | TokenKind::Percent) {
            let op = match self.consume().kind {
                TokenKind::Star => "*".to_string(),
                TokenKind::Slash => "/".to_string(),
                TokenKind::Percent => "%".to_string(),
                _ => unreachable!(),
            };
            let right = self.parse_primary();
            left = Expr::Binary(Box::new(left), op, Box::new(right));
        }
        left
    }

    fn parse_primary(&mut self) -> Expr {
        let tok = self.consume();
        match tok.kind {
            TokenKind::Number(n) => Expr::Literal(KexValue::Number(n)),
            TokenKind::StringLit(s) => Expr::Literal(KexValue::String(s)),
            TokenKind::BoolLit(b) => Expr::Literal(KexValue::Boolean(b)),
            TokenKind::KxIn => Expr::KxIn,
            TokenKind::Len => {
                self.expect(TokenKind::LParen);
                let expr = self.parse_expr();
                self.expect(TokenKind::RParen);
                Expr::Len(Box::new(expr))
            }
            TokenKind::TypeCast => {
                self.expect(TokenKind::LParen);
                let expr = self.parse_expr();
                self.expect(TokenKind::Comma);
                if let TokenKind::StringLit(target) = self.consume().kind {
                    self.expect(TokenKind::RParen);
                    Expr::TypeCast(Box::new(expr), target)
                } else {
                    kex_error("Syntax", tok.line, "Expected target string type literal in cast()");
                    unreachable!()
                }
            }
            TokenKind::Identifier(id) => {
                if self.peek().kind == TokenKind::LParen {
                    self.consume();
                    let mut args = Vec::new();
                    if self.peek().kind != TokenKind::RParen {
                        loop {
                            args.push(self.parse_expr());
                            if self.peek().kind == TokenKind::Comma {
                                self.consume();
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(TokenKind::RParen);
                    Expr::Call(id, args)
                } else if self.peek().kind == TokenKind::LBracket {
                    self.consume();
                    let idx = self.parse_expr();
                    self.expect(TokenKind::RBracket);
                    Expr::Index(Box::new(Expr::Var(id)), Box::new(idx))
                } else {
                    Expr::Var(id)
                }
            }
            TokenKind::LBracket => {
                let mut elements = Vec::new();
                if self.peek().kind != TokenKind::RBracket {
                    loop {
                        elements.push(self.parse_expr());
                        if self.peek().kind == TokenKind::Comma {
                            self.consume();
                        } else {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RBracket);
                Expr::ArrayLit(elements)
            }
            TokenKind::LParen => {
                let expr = self.parse_expr();
                self.expect(TokenKind::RParen);
                expr
            }
            TokenKind::Not => {
                let expr = self.parse_primary();
                Expr::Unary("!".to_string(), Box::new(expr))
            }
            _ => {
                kex_error("Parse", tok.line, &format!("Unexpected token '{:?}' in expression", tok.kind));
                unreachable!()
            }
        }
    }

    fn expect(&mut self, expected: TokenKind) {
        let tok = self.consume();
        if tok.kind != expected {
            kex_error("Syntax", tok.line, &format!("Expected {:?}, found {:?}", expected, tok.kind));
        }
    }
}

// ============================================================================
// MODULE 4: RUNTIME & INTERPRETER
// ============================================================================

#[derive(Clone)]
pub struct Function {
    pub params: Vec<String>,
    pub body: Vec<Statement>,
}

pub struct KexRuntime {
    env: Vec<HashMap<String, KexValue>>,
    functions: HashMap<String, Function>,
}

impl KexRuntime {
    pub fn new() -> Self {
        KexRuntime {
            env: vec![HashMap::new()],
            functions: HashMap::new(),
        }
    }

    fn get_var(&self, name: &str) -> KexValue {
        for frame in self.env.iter().rev() {
            if let Some(val) = frame.get(name) {
                return val.clone();
            }
        }
        KexValue::Null
    }

    fn set_var(&mut self, name: &str, val: KexValue) {
        for frame in self.env.iter_mut().rev() {
            if frame.contains_key(name) {
                frame.insert(name.to_string(), val);
                return;
            }
        }
        if let Some(frame) = self.env.last_mut() {
            frame.insert(name.to_string(), val);
        }
    }

    fn decl_var(&mut self, name: &str, val: KexValue) {
        if let Some(frame) = self.env.last_mut() {
            frame.insert(name.to_string(), val);
        }
    }

    fn eval_expr(&mut self, expr: &Expr) -> KexValue {
        match expr {
            Expr::Literal(val) => val.clone(),
            Expr::Var(name) => self.get_var(name),
            Expr::ArrayLit(elems) => {
                let evaluated = elems.iter().map(|e| self.eval_expr(e)).collect();
                KexValue::Array(evaluated)
            }
            Expr::Index(arr_expr, idx_expr) => {
                let arr = self.eval_expr(arr_expr);
                let idx = self.eval_expr(idx_expr);
                if let (KexValue::Array(list), KexValue::Number(n)) = (arr, idx) {
                    list.get(n as usize).cloned().unwrap_or(KexValue::Null)
                } else {
                    KexValue::Null
                }
            }
            Expr::KxIn => {
                io::stdout().flush().unwrap();
                let mut buffer = String::new();
                io::stdin().read_line(&mut buffer).unwrap();
                KexValue::String(buffer.trim().to_string())
            }
            Expr::Len(target) => {
                let val = self.eval_expr(target);
                match val {
                    KexValue::String(s) => KexValue::Number(s.len() as f64),
                    KexValue::Array(arr) => KexValue::Number(arr.len() as f64),
                    _ => KexValue::Number(0.0),
                }
            }
            Expr::TypeCast(target, dest_type) => {
                let val = self.eval_expr(target);
                match dest_type.as_str() {
                    "num" | "number" => match val {
                        KexValue::String(s) => KexValue::Number(s.parse().unwrap_or(0.0)),
                        KexValue::Boolean(b) => KexValue::Number(if b { 1.0 } else { 0.0 }),
                        KexValue::Number(n) => KexValue::Number(n),
                        _ => KexValue::Number(0.0),
                    },
                    "str" | "string" => KexValue::String(val.to_string()),
                    "bool" | "boolean" => match val {
                        KexValue::Number(n) => KexValue::Boolean(n != 0.0),
                        KexValue::String(s) => KexValue::Boolean(!s.is_empty()),
                        KexValue::Boolean(b) => KexValue::Boolean(b),
                        _ => KexValue::Boolean(false),
                    },
                    _ => val,
                }
            }
            Expr::Unary(op, inner) => {
                let val = self.eval_expr(inner);
                if op == "!" {
                    if let KexValue::Boolean(b) = val {
                        KexValue::Boolean(!b)
                    } else {
                        KexValue::Boolean(false)
                    }
                } else {
                    KexValue::Null
                }
            }
            Expr::Binary(left, op, right) => {
                let v1 = self.eval_expr(left);
                let v2 = self.eval_expr(right);
                match op.as_str() {
                    "+" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Number(a + b),
                        (KexValue::String(a), KexValue::String(b)) => KexValue::String(format!("{}{}", a, b)),
                        (KexValue::String(a), b) => KexValue::String(format!("{}{}", a, b.to_string())),
                        (a, KexValue::String(b)) => KexValue::String(format!("{}{}", a.to_string(), b)),
                        _ => KexValue::Null,
                    },
                    "-" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Number(a - b),
                        _ => KexValue::Null,
                    },
                    "*" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Number(a * b),
                        _ => KexValue::Null,
                    },
                    "/" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Number(a / b),
                        _ => KexValue::Null,
                    },
                    "%" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Number(a % b),
                        _ => KexValue::Null,
                    },
                    "==" => KexValue::Boolean(v1 == v2),
                    "!=" => KexValue::Boolean(v1 != v2),
                    "<" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Boolean(a < b),
                        _ => KexValue::Boolean(false),
                    },
                    ">" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Boolean(a > b),
                        _ => KexValue::Boolean(false),
                    },
                    "<=" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Boolean(a <= b),
                        _ => KexValue::Boolean(false),
                    },
                    ">=" => match (v1, v2) {
                        (KexValue::Number(a), KexValue::Number(b)) => KexValue::Boolean(a >= b),
                        _ => KexValue::Boolean(false),
                    },
                    "&&" => match (v1, v2) {
                        (KexValue::Boolean(a), KexValue::Boolean(b)) => KexValue::Boolean(a && b),
                        _ => KexValue::Boolean(false),
                    },
                    "||" => match (v1, v2) {
                        (KexValue::Boolean(a), KexValue::Boolean(b)) => KexValue::Boolean(a || b),
                        _ => KexValue::Boolean(false),
                    },
                    _ => KexValue::Null,
                }
            }
            Expr::Call(name, args) => {
                let func = match self.functions.get(name) {
                    Some(f) => f.clone(),
                    None => {
                        eprintln!("{}Runtime Error{}: Call to undefined function '{}'", COLOR_RED, COLOR_RESET, name);
                        exit(1);
                    }
                };
                let evaluated_args: Vec<KexValue> = args.iter().map(|a| self.eval_expr(a)).collect();

                let mut new_frame = HashMap::new();
                for (param, val) in func.params.iter().zip(evaluated_args) {
                    new_frame.insert(param.clone(), val);
                }

                self.env.push(new_frame);
                let mut ret_val = KexValue::Null;
                for stmt in func.body {
                    if let Some(val) = self.execute_statement(stmt) {
                        ret_val = val;
                        break;
                    }
                }
                self.env.pop();
                ret_val
            }
        }
    }

    fn execute_statement(&mut self, stmt: Statement) -> Option<KexValue> {
        match stmt {
            Statement::VarDecl(name, expr) => {
                let val = self.eval_expr(&expr);
                self.decl_var(&name, val);
            }
            Statement::Assign(name, expr) => {
                let val = self.eval_expr(&expr);
                self.set_var(&name, val);
            }
            Statement::ArraySet(name, idx_expr, val_expr) => {
                let idx = self.eval_expr(&idx_expr);
                let val = self.eval_expr(&val_expr);
                if let KexValue::Number(n) = idx {
                    if let Some(KexValue::Array(ref mut arr)) = self.env.last_mut().unwrap().get_mut(&name) {
                        if (n as usize) < arr.len() {
                            arr[n as usize] = val;
                        }
                    }
                }
            }
            Statement::KxOut(expr) => {
                let val = self.eval_expr(&expr);
                println!("{}", val.to_string());
            }
            Statement::SysCall(expr) => {
                let val = self.eval_expr(&expr);
                let cmd = val.to_string();

                let mut process = if cfg!(target_os = "windows") {
                    let mut c = Command::new("cmd");
                    c.arg("/C").arg(&cmd);
                    c
                } else {
                    let mut c = Command::new("sh");
                    c.arg("-c").arg(&cmd);
                    c
                };
                let _ = process.status();
            }
            Statement::If(cond, then_branch, else_branch) => {
                let cond_val = self.eval_expr(&cond);
                if cond_val == KexValue::Boolean(true) {
                    for s in then_branch {
                        if let Some(ret) = self.execute_statement(s) {
                            return Some(ret);
                        }
                    }
                } else {
                    for s in else_branch {
                        if let Some(ret) = self.execute_statement(s) {
                            return Some(ret);
                        }
                    }
                }
            }
            Statement::While(cond, body) => {
                while self.eval_expr(&cond) == KexValue::Boolean(true) {
                    for s in body.clone() {
                        if let Some(ret) = self.execute_statement(s) {
                            return Some(ret);
                        }
                    }
                }
            }
            Statement::FnDecl(name, params, body) => {
                self.functions.insert(name, Function { params, body });
            }
            Statement::Return(expr) => {
                return Some(self.eval_expr(&expr));
            }
            Statement::Expr(expr) => {
                self.eval_expr(&expr);
            }
        }
        None
    }

    pub fn execute(&mut self, statements: Vec<Statement>) {
        for stmt in statements {
            self.execute_statement(stmt);
        }
    }
}

// ============================================================================
// MODULE 5: CLI ENTRYPOINT
// ============================================================================

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("{}KEXDRUN Engine v1.0.0{}", COLOR_CYAN, COLOR_RESET);
        println!("{}Usage:{} kxrun <script.kx>", COLOR_YELLOW, COLOR_RESET);
        return;
    }

    let file_path = &args[1];
    let source = fs::read_to_string(file_path).unwrap_or_else(|_| {
        eprintln!("{}Kex System Error{}: Unable to locate target file '{}'", COLOR_RED, COLOR_RESET, file_path);
        exit(1);
    });

    println!("{}[KEXDRUN]{} Executing core module: {}", COLOR_GREEN, COLOR_RESET, file_path);

    let mut lexer = Lexer::new(&source);
    let tokens = lexer.tokenize();

    let mut parser = Parser::new(tokens);
    let ast = parser.parse();

    let mut runtime = KexRuntime::new();
    runtime.execute(ast);
}
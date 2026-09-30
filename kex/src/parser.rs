use crate::ast::*;
use crate::lexer::{kex_literal_to_value, KexError, LexResult, Token, TokenKind};
use crate::value::KexValue;

// ============================================================================
// MODULE 4: PARSER
// ============================================================================

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Parser { tokens, pos: 0 }
    }

    fn peek(&self) -> &Token {
        self.tokens.get(self.pos).unwrap_or_else(|| {
            self.tokens
                .last()
                .expect("token stream must contain an EOF marker")
        })
    }

    fn peek_kind(&self) -> &TokenKind {
        &self.peek().kind
    }

    fn at(&self, kind: &TokenKind) -> bool {
        self.peek_kind() == kind
    }

    fn advance(&mut self) -> Token {
        let tok = self.peek().clone();
        if self.pos < self.tokens.len() - 1 {
            self.pos += 1;
        }
        tok
    }

    fn eat(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn line(&self) -> usize {
        self.peek().line
    }

    fn err<T>(&self, message: impl Into<String>) -> LexResult<T> {
        let tok = self.peek();
        Err(KexError::new(
            "Parser",
            tok.line,
            tok.col,
            format!("{} (found {})", message.into(), tok.kind.describe()),
        ))
    }

    fn expect(&mut self, kind: TokenKind) -> LexResult<Token> {
        if self.peek_kind() == &kind {
            Ok(self.advance())
        } else {
            self.err(format!("expected {}", kind.describe()))
        }
    }

    fn expect_ident(&mut self) -> LexResult<String> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Identifier(name) => {
                self.advance();
                Ok(name)
            }
            _ => self.err(format!("expected an identifier, found {}", tok.kind.describe())),
        }
    }

    fn eat_semicolon(&mut self) {
        self.eat(&TokenKind::Semicolon);
    }

    pub fn parse(&mut self) -> LexResult<Vec<Statement>> {
        let mut out = Vec::new();
        while !self.at(&TokenKind::Eof) {
            out.push(self.parse_statement()?);
        }
        Ok(out)
    }

    // ----- statements ------------------------------------------------------

    fn parse_statement(&mut self) -> LexResult<Statement> {
        let line = self.line();
        match self.peek_kind().clone() {
            TokenKind::Let | TokenKind::Const => {
                let mutable = self.at(&TokenKind::Let);
                self.advance();
                let name = self.expect_ident()?;
                let init = if self.eat(&TokenKind::Assign) {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.eat_semicolon();
                Ok(Statement::Let {
                    name,
                    init,
                    mutable,
                    line,
                })
            }

            TokenKind::Print | TokenKind::EPrint | TokenKind::Write => {
                let stream = if self.at(&TokenKind::EPrint) {
                    OutStream::Stderr
                } else {
                    OutStream::Stdout
                };
                let newline = !matches!(self.peek_kind(), TokenKind::Write);
                self.advance();
                let mut args = Vec::new();
                if self.eat(&TokenKind::LParen) {
                    args = self.parse_args()?;
                } else {
                    args.push(self.parse_expr()?);
                    while self.eat(&TokenKind::Comma) {
                        args.push(self.parse_expr()?);
                    }
                }
                self.eat_semicolon();
                Ok(Statement::KxOut {
                    args,
                    stream,
                    newline,
                    line,
                })
            }

            TokenKind::Clear => {
                self.advance();
                self.eat_semicolon();
                Ok(Statement::KxOut {
                    args: vec![Expr::Literal(KexValue::String("\u{1b}[2J\u{1b}[H".to_string()))],
                    stream: OutStream::Stdout,
                    newline: false,
                    line,
                })
            }

            TokenKind::SysRun => {
                self.advance();
                let mode = if self.at(&TokenKind::Identifier("capture".to_string())) {
                    self.advance();
                    SysMode::Capture
                } else if self.at(&TokenKind::Identifier("code".to_string())) {
                    self.advance();
                    SysMode::Code
                } else {
                    SysMode::Spawn
                };
                let value = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::SysCall { mode, value, line })
            }

            TokenKind::If => {
                self.advance();
                let cond = self.parse_expr()?;
                let then_branch = self.parse_block()?;
                let mut else_branch = Vec::new();
                self.parse_else_chain(&mut else_branch)?;
                Ok(Statement::If {
                    cond,
                    then_branch,
                    else_branch,
                    line,
                })
            }

            TokenKind::While => {
                self.advance();
                let cond = self.parse_expr()?;
                let body = self.parse_block()?;
                Ok(Statement::While { cond, body, line })
            }

            TokenKind::Do => {
                self.advance();
                let body = self.parse_block()?;
                self.expect(TokenKind::While)?;
                let cond = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::DoWhile { body, cond, line })
            }

            TokenKind::For => {
                self.advance();
                self.parse_for(line)
            }

            TokenKind::Switch => {
                self.advance();
                let subject = self.parse_expr()?;
                self.expect(TokenKind::LBrace)?;
                let mut cases: Vec<SwitchCase> = Vec::new();
                while !self.at(&TokenKind::RBrace) {
                    if self.eat(&TokenKind::Case) {
                        let test = Some(self.parse_expr()?);
                        self.expect(TokenKind::Colon)?;
                        let body = self.parse_case_body()?;
                        cases.push(SwitchCase { test, body });
                    } else if self.eat(&TokenKind::Default) {
                        self.expect(TokenKind::Colon)?;
                        let body = self.parse_case_body()?;
                        cases.push(SwitchCase { test: None, body });
                    } else {
                        return self.err("expected 'case', 'default' or '}' inside switch");
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(Statement::Switch { subject, cases, line })
            }

            TokenKind::Break => {
                self.advance();
                self.eat_semicolon();
                Ok(Statement::Break { line })
            }

            TokenKind::Continue => {
                self.advance();
                self.eat_semicolon();
                Ok(Statement::Continue { line })
            }

            TokenKind::Fn => {
                self.advance();
                let name = self.expect_ident()?;
                let params = self.parse_param_list()?;
                let body = self.parse_block()?;
                Ok(Statement::FnDecl {
                    name,
                    params,
                    body,
                    line,
                })
            }

            TokenKind::Return => {
                self.advance();
                let value = if self.at(&TokenKind::Semicolon) || self.at(&TokenKind::RBrace) {
                    None
                } else {
                    Some(self.parse_expr()?)
                };
                self.eat_semicolon();
                Ok(Statement::Return { value, line })
            }

            TokenKind::Throw => {
                self.advance();
                let value = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::Throw { value, line })
            }

            TokenKind::Try => {
                self.advance();
                let body = self.parse_block()?;
                let mut binding = None;
                let mut handler = Vec::new();
                if self.eat(&TokenKind::Catch) {
                    if let TokenKind::Identifier(name) = self.peek_kind().clone() {
                        self.advance();
                        binding = Some(name);
                    }
                    handler = self.parse_block()?;
                }
                let mut finally = Vec::new();
                if self.eat(&TokenKind::Finally) {
                    finally = self.parse_block()?;
                }
                if binding.is_none() && handler.is_empty() && finally.is_empty() {
                    return self.err("'try' requires a 'catch' or 'finally' block");
                }
                Ok(Statement::Try {
                    body,
                    binding,
                    handler,
                    finally,
                    line,
                })
            }

            TokenKind::Import => {
                self.advance();
                let path = match self.advance().kind {
                    TokenKind::Str(s) => s,
                    other => {
                        return Err(KexError::new(
                            "Parser",
                            line,
                            0,
                            format!(
                                "import expects a quoted module path, found {}",
                                other.describe()
                            ),
                        ))
                    }
                };
                self.eat_semicolon();
                Ok(Statement::Import { path, line })
            }

            TokenKind::Export => {
                self.advance();
                let name = self.expect_ident()?;
                self.expect(TokenKind::Assign)?;
                let value = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::Export { name, value, line })
            }

            TokenKind::Identifier(name) => self.parse_ident_statement(name, line),

            _ => {
                let value = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::ExprStmt { value, line })
            }
        }
    }

    fn parse_else_chain(&mut self, out: &mut Vec<Statement>) -> LexResult<()> {
        if self.eat(&TokenKind::ElseIf) {
            let line = self.line();
            let cond = self.parse_expr()?;
            let then_branch = self.parse_block()?;
            let mut else_branch = Vec::new();
            self.parse_else_chain(&mut else_branch)?;
            out.push(Statement::If {
                cond,
                then_branch,
                else_branch,
                line,
            });
        } else if self.eat(&TokenKind::Else) {
            if self.eat(&TokenKind::If) {
                let line = self.line();
                let cond = self.parse_expr()?;
                let then_branch = self.parse_block()?;
                let mut else_branch = Vec::new();
                self.parse_else_chain(&mut else_branch)?;
                out.push(Statement::If {
                    cond,
                    then_branch,
                    else_branch,
                    line,
                });
            } else {
                out.extend(self.parse_block()?);
            }
        }
        Ok(())
    }

    fn parse_case_body(&mut self) -> LexResult<Vec<Statement>> {
        let mut body = Vec::new();
        loop {
            match self.peek_kind() {
                TokenKind::Case | TokenKind::Default | TokenKind::RBrace | TokenKind::Eof => {
                    break
                }
                _ => body.push(self.parse_statement()?),
            }
        }
        Ok(body)
    }

    fn parse_block(&mut self) -> LexResult<Vec<Statement>> {
        self.expect(TokenKind::LBrace)?;
        let mut body = Vec::new();
        while !self.at(&TokenKind::RBrace) {
            if self.at(&TokenKind::Eof) {
                return self.err("unexpected end of file, expected '}'");
            }
            body.push(self.parse_statement()?);
        }
        self.expect(TokenKind::RBrace)?;
        Ok(body)
    }

    /// True when the `(` at the cursor opens a parameter list rather than a
    /// grouped expression, decided by the token right after its partner `)`.
    fn arrow_ahead(&self) -> bool {
        let mut depth = 0usize;
        let mut index = self.pos;
        while index < self.tokens.len() {
            match self.tokens[index].kind {
                TokenKind::LParen | TokenKind::LBracket | TokenKind::LBrace => depth += 1,
                TokenKind::RParen | TokenKind::RBracket | TokenKind::RBrace => {
                    depth -= 1;
                    if depth == 0 {
                        return matches!(
                            self.tokens.get(index + 1).map(|t| &t.kind),
                            Some(TokenKind::FatArrow)
                        );
                    }
                }
                TokenKind::Eof => break,
                _ => {}
            }
            index += 1;
        }
        false
    }

    /// Shared tail of `x => ...`, `(a, b) => ...` and `() => ...`.
    fn parse_arrow_body(
        &mut self,
        name: &str,
        params: Vec<String>,
    ) -> LexResult<Expr> {
        self.expect(TokenKind::FatArrow)?;
        let line = self.peek().line;
        let body = if self.at(&TokenKind::LBrace) {
            self.parse_block()?
        } else {
            // an expression body returns its value
            let value = self.parse_expr()?;
            vec![Statement::Return { value: Some(value), line }]
        };
        Ok(Expr::AnonFn(name.to_string(), params, body))
    }

    fn parse_param_list(&mut self) -> LexResult<Vec<String>> {        self.expect(TokenKind::LParen)?;
        let mut params = Vec::new();
        while !self.at(&TokenKind::RParen) {
            params.push(self.expect_ident()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(params)
    }

    fn parse_for(&mut self, line: usize) -> LexResult<Statement> {
        // for x in iterable { ... }
        if let TokenKind::Identifier(name) = self.peek_kind().clone() {
            if self.tokens.get(self.pos + 1).map(|t| &t.kind) == Some(&TokenKind::In) {
                self.advance();
                self.advance();
                let iterable = self.parse_expr()?;
                let body = self.parse_block()?;
                return Ok(Statement::ForIn {
                    name,
                    iterable,
                    body,
                    line,
                });
            }
        }

        // for let i = 0; i < n; i = i + 1 { ... }
        let init = match self.peek_kind() {
            TokenKind::Let | TokenKind::Const => Some(Box::new(self.parse_statement()?)),
            TokenKind::Semicolon => {
                self.advance();
                None
            }
            TokenKind::Identifier(name) => {
                let name = name.clone();
                Some(Box::new(self.parse_ident_statement(name, line)?))
            }
            _ => return self.err("malformed 'for' header"),
        };

        let cond = if self.at(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.parse_expr()?)
        };
        self.expect(TokenKind::Semicolon)?;

        let step = if self.at(&TokenKind::LBrace) {
            None
        } else {
            let name = self.expect_ident()?;
            let step_line = self.line();
            let value = match self.peek_kind() {
                TokenKind::PlusPlus => {
                    self.advance();
                    Expr::Literal(KexValue::Number(1.0))
                }
                TokenKind::MinusMinus => {
                    self.advance();
                    Expr::Literal(KexValue::Number(-1.0))
                }
                _ => {
                    self.expect(TokenKind::Assign)?;
                    self.parse_expr()?
                }
            };
            Some(Box::new(Statement::Assign {
                target: AssignTarget::Name(name),
                value,
                line: step_line,
            }))
        };

        let body = self.parse_block()?;
        Ok(Statement::ForClassic {
            init,
            cond,
            step,
            body,
            line,
        })
    }

    fn parse_ident_statement(&mut self, name: String, line: usize) -> LexResult<Statement> {
        // prefix `++x` / `--x`
        if matches!(
            self.tokens.get(self.pos + 1).map(|t| &t.kind),
            Some(TokenKind::PlusPlus) | Some(TokenKind::MinusMinus)
        ) {
            let delta = if self.tokens[self.pos + 1].kind == TokenKind::PlusPlus {
                1.0
            } else {
                -1.0
            };
            self.advance();
            self.advance();
            self.eat_semicolon();
            return Ok(Statement::Incr {
                target: AssignTarget::Name(name),
                delta,
                line,
            });
        }

        let next = self
            .tokens
            .get(self.pos + 1)
            .map(|t| t.kind.clone())
            .unwrap_or(TokenKind::Eof);

        match next {
            TokenKind::Assign => {
                self.advance();
                self.advance();
                let value = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::Assign {
                    target: AssignTarget::Name(name),
                    value,
                    line,
                })
            }
            TokenKind::PlusAssign
            | TokenKind::MinusAssign
            | TokenKind::StarAssign
            | TokenKind::SlashAssign
            | TokenKind::PercentAssign => {
                let op = match next {
                    TokenKind::PlusAssign => BinOp::Add,
                    TokenKind::MinusAssign => BinOp::Sub,
                    TokenKind::StarAssign => BinOp::Mul,
                    TokenKind::SlashAssign => BinOp::Div,
                    _ => BinOp::Rem,
                };
                self.advance();
                self.advance();
                let value = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::CompoundAssign {
                    target: AssignTarget::Name(name),
                    op,
                    value,
                    line,
                })
            }
            TokenKind::PlusPlus | TokenKind::MinusMinus => {
                self.advance();
                self.advance();
                self.eat_semicolon();
                Ok(Statement::Incr {
                    target: AssignTarget::Name(name),
                    delta: if next == TokenKind::PlusPlus { 1.0 } else { -1.0 },
                    line,
                })
            }
            TokenKind::LBracket | TokenKind::Dot => {
                // `name[...]` and `name.key`: parse the whole postfix chain
                // first so calls, nesting and reserved member names all work,
                // then decide whether it is an assignment target.
                let expr = self.parse_postfix()?;
                if matches!(
                    self.peek_kind(),
                    TokenKind::Assign
                        | TokenKind::PlusAssign
                        | TokenKind::MinusAssign
                        | TokenKind::StarAssign
                        | TokenKind::SlashAssign
                        | TokenKind::PercentAssign
                ) {
                    let op_kind = self.advance().kind;
                    let target = match expr.to_assign_target() {
                        Some(target) => target,
                        None => return self.err("cannot assign to this expression"),
                    };
                    if op_kind == TokenKind::Assign {
                        let value = self.parse_expr()?;
                        self.eat_semicolon();
                        return Ok(Statement::Assign {
                            target,
                            value,
                            line,
                        });
                    }
                    let op = match op_kind {
                        TokenKind::PlusAssign => BinOp::Add,
                        TokenKind::MinusAssign => BinOp::Sub,
                        TokenKind::StarAssign => BinOp::Mul,
                        TokenKind::SlashAssign => BinOp::Div,
                        _ => BinOp::Rem,
                    };
                    let value = self.parse_expr()?;
                    self.eat_semicolon();
                    return Ok(Statement::CompoundAssign {
                        target,
                        op,
                        value,
                        line,
                    });
                }
                self.eat_semicolon();
                Ok(Statement::ExprStmt { value: expr, line })
            }
            _ => {
                let value = self.parse_expr()?;
                self.eat_semicolon();
                Ok(Statement::ExprStmt { value, line })
            }
        }
    }

    // ----- expressions -----------------------------------------------------

    pub fn parse_expr(&mut self) -> LexResult<Expr> {
        let cond = self.parse_coalesce()?;
        if self.at(&TokenKind::Question) {
            self.advance();
            let yes = self.parse_expr()?;
            self.expect(TokenKind::Colon)?;
            let no = self.parse_expr()?;
            return Ok(Expr::Ternary(Box::new(cond), Box::new(yes), Box::new(no)));
        }
        Ok(cond)
    }

    fn parse_coalesce(&mut self) -> LexResult<Expr> {
        let left = self.parse_or()?;
        if self.at(&TokenKind::Coalesce) {
            self.advance();
            let right = self.parse_coalesce()?;
            return Ok(Expr::Coalesce(Box::new(left), Box::new(right)));
        }
        Ok(left)
    }

    fn parse_or(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_and()?;
        while self.at(&TokenKind::Or) {
            self.advance();
            let right = self.parse_and()?;
            left = Expr::Binary(BinOp::Or, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_and(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_bitor()?;
        while self.at(&TokenKind::And) {
            self.advance();
            let right = self.parse_bitor()?;
            left = Expr::Binary(BinOp::And, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_bitor(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_bitxor()?;
        while self.at(&TokenKind::Pipe) {
            self.advance();
            let right = self.parse_bitxor()?;
            left = Expr::Binary(BinOp::BitOr, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_bitxor(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_bitand()?;
        while self.at(&TokenKind::Caret) {
            self.advance();
            let right = self.parse_bitand()?;
            left = Expr::Binary(BinOp::BitXor, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_bitand(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_equality()?;
        while self.at(&TokenKind::Amp) {
            self.advance();
            let right = self.parse_equality()?;
            left = Expr::Binary(BinOp::BitAnd, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_equality(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_compare()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Eq => BinOp::Eq,
                TokenKind::Ne => BinOp::Ne,
                _ => break,
            };
            self.advance();
            let right = self.parse_compare()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_compare(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_shift()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Lt => BinOp::Lt,
                TokenKind::Gt => BinOp::Gt,
                TokenKind::Le => BinOp::Le,
                TokenKind::Ge => BinOp::Ge,
                _ => break,
            };
            self.advance();
            let right = self.parse_shift()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_shift(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_range()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Shl => BinOp::Shl,
                TokenKind::Shr => BinOp::Shr,
                _ => break,
            };
            self.advance();
            let right = self.parse_range()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_range(&mut self) -> LexResult<Expr> {
        // prefix range: ..5
        if self.at(&TokenKind::DotDot) {
            self.advance();
            let end = self.parse_add()?;
            return Ok(Expr::Range(
                Box::new(Expr::Literal(KexValue::Number(0.0))),
                Box::new(end),
                false,
                None,
            ));
        }
        let left = self.parse_add()?;
        if self.at(&TokenKind::DotDot) || self.at(&TokenKind::DotDotEq) {
            let inclusive = self.at(&TokenKind::DotDotEq);
            self.advance();
            let step = if self.at(&TokenKind::By) {
                self.advance();
                Some(Box::new(self.parse_add()?))
            } else {
                None
            };
            let end = if self.is_expr_start() {
                self.parse_add()?
            } else {
                Expr::Literal(KexValue::Number(0.0))
            };
            return Ok(Expr::Range(Box::new(left), Box::new(end), inclusive, step));
        }
        Ok(left)
    }

    fn parse_add(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_mul()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Plus => BinOp::Add,
                TokenKind::Minus => BinOp::Sub,
                _ => break,
            };
            self.advance();
            let right = self.parse_mul()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_mul(&mut self) -> LexResult<Expr> {
        let mut left = self.parse_unary()?;
        loop {
            let op = match self.peek_kind() {
                TokenKind::Star => BinOp::Mul,
                TokenKind::Slash => BinOp::Div,
                TokenKind::Percent => BinOp::Rem,
                _ => break,
            };
            self.advance();
            let right = self.parse_unary()?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn parse_unary(&mut self) -> LexResult<Expr> {
        match self.peek_kind() {
            TokenKind::Minus => {
                self.advance();
                let inner = self.parse_unary()?;
                Ok(match inner {
                    Expr::Literal(KexValue::Number(n)) => Expr::Literal(KexValue::Number(-n)),
                    other => Expr::Unary(UnOp::Neg, Box::new(other)),
                })
            }
            TokenKind::Bang => {
                self.advance();
                Ok(Expr::Unary(UnOp::Not, Box::new(self.parse_unary()?)))
            }
            TokenKind::Tilde => {
                self.advance();
                Ok(Expr::Unary(UnOp::BitNot, Box::new(self.parse_unary()?)))
            }
            _ => self.parse_power(),
        }
    }

    fn parse_power(&mut self) -> LexResult<Expr> {
        let base = self.parse_postfix()?;
        if self.at(&TokenKind::Pow) {
            self.advance();
            let exp = self.parse_unary()?;
            return Ok(Expr::Binary(BinOp::Pow, Box::new(base), Box::new(exp)));
        }
        Ok(base)
    }

    fn parse_postfix(&mut self) -> LexResult<Expr> {
        let mut expr = self.parse_primary()?;
        loop {
            match self.peek_kind() {
                TokenKind::LParen => {
                    self.advance();
                    let args = self.parse_args()?;
                    expr = Expr::Call(Box::new(expr), args);
                }
                TokenKind::LBracket => {
                    self.advance();
                    // slice: a[i..j]
                    if self.at(&TokenKind::DotDot) || self.at(&TokenKind::DotDotEq) {
                        let end = self.parse_add()?;
                        self.expect(TokenKind::RBracket)?;
                        expr = Expr::Slice(Box::new(expr), None, Some(Box::new(end)));
                        continue;
                    }
                    let index = self.parse_expr()?;
                    if self.at(&TokenKind::DotDot) || self.at(&TokenKind::DotDotEq) {
                        self.advance();
                        let end = if self.at(&TokenKind::RBracket) {
                            None
                        } else {
                            Some(Box::new(self.parse_add()?))
                        };
                        self.expect(TokenKind::RBracket)?;
                        expr = Expr::Slice(Box::new(expr), Some(Box::new(index)), end);
                    } else {
                        self.expect(TokenKind::RBracket)?;
                        expr = Expr::Index(Box::new(expr), Box::new(index));
                    }
                }
                TokenKind::Dot => {
                    // member names may be reserved words (`obj.fn`, `obj.type`)
                    let name = match self.tokens.get(self.pos + 1).map(|t| &t.kind) {
                        Some(kind) => match kind.word() {
                            Some(name) => name,
                            None => break,
                        },
                        None => break,
                    };
                    self.advance();
                    self.advance();
                    if self.at(&TokenKind::LParen) {
                        self.advance();
                        let args = self.parse_args()?;
                        expr = Expr::Method(Box::new(expr), name, args);
                    } else {
                        expr = Expr::Member(Box::new(expr), name);
                    }
                }
                TokenKind::PlusPlus | TokenKind::MinusMinus => {
                    let delta = if self.at(&TokenKind::PlusPlus) { 1.0 } else { -1.0 };
                    self.advance();
                    let line = self.line();
                    expr = Expr::Call(
                        Box::new(Expr::Var("__kex_incr".to_string())),
                        vec![
                            expr,
                            Expr::Literal(KexValue::Number(delta)),
                            Expr::Literal(KexValue::Number(line as f64)),
                        ],
                    );
                }
                _ => break,
            }
        }
        Ok(expr)
    }

    fn parse_args(&mut self) -> LexResult<Vec<Expr>> {
        let mut args = Vec::new();
        while !self.at(&TokenKind::RParen) {
            args.push(self.parse_expr()?);
            if !self.eat(&TokenKind::Comma) {
                break;
            }
        }
        self.expect(TokenKind::RParen)?;
        Ok(args)
    }

    fn is_expr_start(&self) -> bool {
        match self.peek_kind() {
            TokenKind::Number(_)
            | TokenKind::Str(_)
            | TokenKind::StrTemplate(_)
            | TokenKind::Identifier(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null
            | TokenKind::LParen
            | TokenKind::LBracket
            | TokenKind::LBrace
            | TokenKind::Minus
            | TokenKind::Bang
            | TokenKind::Tilde
            | TokenKind::Len
            | TokenKind::Cast
            | TokenKind::Fn
            | TokenKind::FatArrow => true,
            _ => false,
        }
    }

    fn parse_primary(&mut self) -> LexResult<Expr> {
        let tok = self.peek().clone();
        match tok.kind {
            TokenKind::Number(_)
            | TokenKind::Str(_)
            | TokenKind::True
            | TokenKind::False
            | TokenKind::Null => {
                self.advance();
                Ok(Expr::Literal(kex_literal_to_value(&tok.kind).unwrap()))
            }
            TokenKind::StrTemplate(parts) => {
                self.advance();
                Ok(Expr::Interp(parts))
            }
            TokenKind::Len => {
                self.advance();
                self.expect(TokenKind::LParen)?;
                let inner = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(Expr::Len(Box::new(inner)))
            }
            TokenKind::Cast => {
                self.advance();
                self.expect(TokenKind::LParen)?;
                let inner = self.parse_expr()?;
                self.expect(TokenKind::Comma)?;
                let target = match self.advance().kind {
                    TokenKind::Str(s) => s,
                    other => {
                        return Err(KexError::new(
                            "Parser",
                            tok.line,
                            tok.col,
                            format!(
                                "cast() expects a string type name, found {}",
                                other.describe()
                            ),
                        ))
                    }
                };
                self.expect(TokenKind::RParen)?;
                Ok(Expr::Cast(Box::new(inner), target))
            }
            TokenKind::ReadLine => {
                self.advance();
                Ok(Expr::Var("__kex_readline".to_string()))
            }
            TokenKind::ReadNumber => {
                self.advance();
                Ok(Expr::Var("__kex_readnumber".to_string()))
            }
            TokenKind::Random => {
                self.advance();
                if self.eat(&TokenKind::LParen) {
                    let args = self.parse_args()?;
                    return Ok(Expr::Call(
                        Box::new(Expr::Var("random".to_string())),
                        args,
                    ));
                }
                Ok(Expr::Call(Box::new(Expr::Var("random".to_string())), Vec::new()))
            }
            TokenKind::Now => {
                self.advance();
                if self.eat(&TokenKind::LParen) {
                    let _ = self.parse_args()?;
                }
                Ok(Expr::Call(Box::new(Expr::Var("now".to_string())), Vec::new()))
            }
            TokenKind::Fn => {
                self.advance();
                let name = if matches!(self.peek_kind(), TokenKind::Identifier(_)) {
                    self.expect_ident()?
                } else {
                    "<anonymous>".to_string()
                };
                let params = self.parse_param_list()?;
                let body = self.parse_block()?;
                Ok(Expr::AnonFn(name, params, body))
            }
            TokenKind::Identifier(id) => {
                self.advance();
                if self.at(&TokenKind::FatArrow) {
                    let single = id.clone();
                    return self.parse_arrow_body(&id, vec![single]);
                }
                if self.at(&TokenKind::LParen) {
                    self.advance();
                    let args = self.parse_args()?;
                    return Ok(Expr::Call(Box::new(Expr::Var(id)), args));
                }
                Ok(Expr::Var(id))
            }
            TokenKind::FatArrow => self.parse_arrow_body("<anonymous>", Vec::new()),
            TokenKind::LBracket => {
                self.advance();
                let mut elements = Vec::new();
                while !self.at(&TokenKind::RBracket) {
                    elements.push(self.parse_expr()?);
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBracket)?;
                Ok(Expr::ArrayLit(elements))
            }
            TokenKind::LBrace => {
                self.advance();
                let mut entries = Vec::new();
                while !self.at(&TokenKind::RBrace) {
                    let key = match self.advance().kind {
                        TokenKind::Identifier(name) => name,
                        TokenKind::Str(s) => s,
                        TokenKind::Number(n) => crate::value::format_number(n),
                        other => {
                            return Err(KexError::new(
                                "Parser",
                                tok.line,
                                tok.col,
                                format!("invalid object key {}", other.describe()),
                            ))
                        }
                    };
                    self.expect(TokenKind::Colon)?;
                    let value = self.parse_expr()?;
                    entries.push((key, value));
                    if !self.eat(&TokenKind::Comma) {
                        break;
                    }
                }
                self.expect(TokenKind::RBrace)?;
                Ok(Expr::ObjectLit(entries))
            }
            TokenKind::LParen => {
                if self.arrow_ahead() {
                    self.advance();
                    let mut params = Vec::new();
                    while !self.at(&TokenKind::RParen) {
                        params.push(self.expect_ident()?);
                        if !self.eat(&TokenKind::Comma) {
                            break;
                        }
                    }
                    self.expect(TokenKind::RParen)?;
                    return self.parse_arrow_body("<anonymous>", params);
                }
                self.advance();
                let inner = self.parse_expr()?;
                self.expect(TokenKind::RParen)?;
                Ok(inner)
            }
            _ => Err(KexError::new(
                "Parser",
                tok.line,
                tok.col,
                format!("unexpected {} in expression", tok.kind.describe()),
            )),
        }
    }
}

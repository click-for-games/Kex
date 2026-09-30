use crate::ast::StrPart;
use crate::value::KexValue;

// ============================================================================
// MODULE 2: LEXER & TOKENS
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    // keywords
    Let,
    Const,
    If,
    ElseIf,
    Else,
    While,
    Do,
    For,
    In,
    By,
    Break,
    Continue,
    Fn,
    Return,
    Switch,
    Case,
    Default,
    Try,
    Catch,
    Finally,
    Throw,
    Import,
    Export,
    True,
    False,
    Null,
    ReadLine,
    ReadNumber,
    Print,
    Write,
    EPrint,
    Clear,
    SysRun,
    Len,
    Cast,
    Random,
    Now,

    // literals / identifiers
    Identifier(String),
    Number(f64),
    Str(String),
    StrTemplate(Vec<StrPart>),

    // operators
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Pow,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    PlusPlus,
    MinusMinus,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    Bang,
    Question,
    Coalesce,
    Amp,
    Pipe,
    Caret,
    Tilde,
    Shl,
    Shr,

    // punctuation
    Dot,
    DotDot,
    DotDotEq,
    FatArrow,
    LParen,
    RParen,
    LBrace,
    RBrace,
    LBracket,
    RBracket,
    Comma,
    Semicolon,
    Colon,
    Eof,
}

impl TokenKind {
    /// Identifier spelling for names, including reserved words. Lets member
    /// access work on keys like `fn` or `type`.
    pub fn word(&self) -> Option<String> {
        match self {
            TokenKind::Identifier(name) => Some(name.clone()),
            other => keyword_name(other).map(|k| k.to_string()),
        }
    }

    pub fn describe(&self) -> String {
        match self {
            TokenKind::Identifier(name) => format!("'{}'", name),
            TokenKind::Eof => "end of file".to_string(),
            TokenKind::Str(_) | TokenKind::StrTemplate(_) => "string literal".to_string(),
            other => format!("'{}'", keyword_name(other).unwrap_or("token")),
        }
    }
}

fn keyword_name(kind: &TokenKind) -> Option<&'static str> {
    Some(match kind {
        TokenKind::Let => "let",
        TokenKind::Const => "const",
        TokenKind::If => "if",
        TokenKind::ElseIf => "elif",
        TokenKind::Else => "else",
        TokenKind::While => "while",
        TokenKind::Do => "do",
        TokenKind::For => "for",
        TokenKind::In => "in",
        TokenKind::By => "by",
        TokenKind::Break => "break",
        TokenKind::Continue => "continue",
        TokenKind::Fn => "fn",
        TokenKind::Return => "return",
        TokenKind::Switch => "switch",
        TokenKind::Case => "case",
        TokenKind::Default => "default",
        TokenKind::Try => "try",
        TokenKind::Catch => "catch",
        TokenKind::Finally => "finally",
        TokenKind::Throw => "throw",
        TokenKind::Import => "import",
        TokenKind::Export => "export",
        TokenKind::True => "true",
        TokenKind::False => "false",
        TokenKind::Null => "null",
        _ => return None,
    })
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub line: usize,
    pub col: usize,
}

pub struct Lexer<'a> {
    input: &'a str,
    pos: usize,
    line: usize,
    col: usize,
}

pub type LexResult<T> = Result<T, KexError>;

#[derive(Debug, Clone)]
pub struct KexError {
    pub phase: &'static str,
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl KexError {
    pub fn new(phase: &'static str, line: usize, col: usize, message: impl Into<String>) -> Self {
        KexError {
            phase,
            line,
            col,
            message: message.into(),
        }
    }
}

impl<'a> Lexer<'a> {
    pub fn new(input: &'a str) -> Self {
        Lexer {
            input,
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    fn peek_at(&self, chars: &[char], offset: usize) -> char {
        *chars.get(self.pos + offset).unwrap_or(&'\0')
    }

    fn bump(&mut self, chars: &[char]) -> char {
        let ch = chars[self.pos];
        self.pos += 1;
        if ch == '\n' {
            self.line += 1;
            self.col = 1;
        } else {
            self.col += 1;
        }
        ch
    }

    pub fn tokenize(mut self) -> LexResult<Vec<Token>> {
        let chars: Vec<char> = self.input.chars().collect();
        let mut tokens: Vec<Token> = Vec::new();

        while self.pos < chars.len() {
            let ch = self.peek_at(&chars, 0);

            if ch.is_whitespace() {
                self.bump(&chars);
                continue;
            }

            // comments
            if ch == '/' && self.peek_at(&chars, 1) == '/' {
                while self.pos < chars.len() && self.peek_at(&chars, 0) != '\n' {
                    self.bump(&chars);
                }
                continue;
            }
            if ch == '/' && self.peek_at(&chars, 1) == '*' {
                let (l, c) = (self.line, self.col);
                self.bump(&chars);
                self.bump(&chars);
                loop {
                    if self.pos >= chars.len() {
                        return Err(KexError::new("Lexer", l, c, "unterminated block comment"));
                    }
                    if self.peek_at(&chars, 0) == '*' && self.peek_at(&chars, 1) == '/' {
                        self.bump(&chars);
                        self.bump(&chars);
                        break;
                    }
                    self.bump(&chars);
                }
                continue;
            }

            let line = self.line;
            let col = self.col;

            let simple = match ch {
                '(' => Some(TokenKind::LParen),
                ')' => Some(TokenKind::RParen),
                '{' => Some(TokenKind::LBrace),
                '}' => Some(TokenKind::RBrace),
                '[' => Some(TokenKind::LBracket),
                ']' => Some(TokenKind::RBracket),
                ',' => Some(TokenKind::Comma),
                ';' => Some(TokenKind::Semicolon),
                ':' => Some(TokenKind::Colon),
                '~' => Some(TokenKind::Tilde),
                _ => None,
            };
            if let Some(kind) = simple {
                self.bump(&chars);
                tokens.push(Token { kind, line, col });
                continue;
            }

            match ch {
                '=' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::Eq
                    } else if self.peek_at(&chars, 0) == '>' {
                        self.bump(&chars);
                        TokenKind::FatArrow
                    } else {
                        TokenKind::Assign
                    };
                    tokens.push(Token { kind, line, col });
                }
                '+' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '+' {
                        self.bump(&chars);
                        TokenKind::PlusPlus
                    } else if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::PlusAssign
                    } else {
                        TokenKind::Plus
                    };
                    tokens.push(Token { kind, line, col });
                }
                '-' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '-' {
                        self.bump(&chars);
                        TokenKind::MinusMinus
                    } else if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::MinusAssign
                    } else if self.peek_at(&chars, 0) == '>' {
                        self.bump(&chars);
                        TokenKind::FatArrow
                    } else {
                        TokenKind::Minus
                    };
                    tokens.push(Token { kind, line, col });
                }
                '*' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '*' {
                        self.bump(&chars);
                        TokenKind::Pow
                    } else if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::StarAssign
                    } else {
                        TokenKind::Star
                    };
                    tokens.push(Token { kind, line, col });
                }
                '/' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::SlashAssign
                    } else {
                        TokenKind::Slash
                    };
                    tokens.push(Token { kind, line, col });
                }
                '%' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::PercentAssign
                    } else {
                        TokenKind::Percent
                    };
                    tokens.push(Token { kind, line, col });
                }
                '!' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::Ne
                    } else {
                        TokenKind::Bang
                    };
                    tokens.push(Token { kind, line, col });
                }
                '<' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::Le
                    } else if self.peek_at(&chars, 0) == '<' {
                        self.bump(&chars);
                        TokenKind::Shl
                    } else {
                        TokenKind::Lt
                    };
                    tokens.push(Token { kind, line, col });
                }
                '>' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                        TokenKind::Ge
                    } else if self.peek_at(&chars, 0) == '>' {
                        self.bump(&chars);
                        TokenKind::Shr
                    } else {
                        TokenKind::Gt
                    };
                    tokens.push(Token { kind, line, col });
                }
                '&' => {
                    self.bump(&chars);
                    if self.peek_at(&chars, 0) == '&' {
                        self.bump(&chars);
                    }
                    tokens.push(Token { kind: TokenKind::And, line, col });
                }
                '|' => {
                    self.bump(&chars);
                    if self.peek_at(&chars, 0) == '|' {
                        self.bump(&chars);
                        tokens.push(Token { kind: TokenKind::Or, line, col });
                    } else {
                        tokens.push(Token { kind: TokenKind::Pipe, line, col });
                    }
                }
                '^' => {
                    self.bump(&chars);
                    if self.peek_at(&chars, 0) == '=' {
                        self.bump(&chars);
                    }
                    tokens.push(Token { kind: TokenKind::Caret, line, col });
                }
                '?' => {
                    self.bump(&chars);
                    let kind = if self.peek_at(&chars, 0) == '?' {
                        self.bump(&chars);
                        TokenKind::Coalesce
                    } else {
                        TokenKind::Question
                    };
                    tokens.push(Token { kind, line, col });
                }
                '.' => {
                    self.bump(&chars);
                    let mut kind = TokenKind::Dot;
                    if self.peek_at(&chars, 0) == '.' {
                        self.bump(&chars);
                        if self.peek_at(&chars, 0) == '=' {
                            self.bump(&chars);
                            kind = TokenKind::DotDotEq;
                        } else {
                            kind = TokenKind::DotDot;
                        }
                    }
                    tokens.push(Token { kind, line, col });
                }
                '\'' | '"' => {
                    let parts = self.read_string(&chars, ch, false)?;
                    let kind = if parts.len() == 1 {
                        match &parts[0] {
                            StrPart::Lit(text) => TokenKind::Str(text.clone()),
                            StrPart::Code(..) => TokenKind::StrTemplate(parts),
                        }
                    } else {
                        TokenKind::StrTemplate(parts)
                    };
                    tokens.push(Token { kind, line, col });
                }
                c if c.is_ascii_digit() => {
                    let n = self.read_number(&chars)?;
                    tokens.push(Token {
                        kind: TokenKind::Number(n),
                        line,
                        col,
                    });
                }
                c if is_ident_start(c) => {
                    let start = self.pos;
                    while self.pos < chars.len() && is_ident_continue(self.peek_at(&chars, 0)) {
                        self.bump(&chars);
                    }
                    let ident: String = chars[start..self.pos].iter().collect();
                    let kind = match ident.as_str() {
                        "let" => TokenKind::Let,
                        "const" => TokenKind::Const,
                        "if" => TokenKind::If,
                        "elif" => TokenKind::ElseIf,
                        "else" => TokenKind::Else,
                        "while" => TokenKind::While,
                        "do" => TokenKind::Do,
                        "for" => TokenKind::For,
                        "in" => TokenKind::In,
                        "by" => TokenKind::By,
                        "break" => TokenKind::Break,
                        "continue" => TokenKind::Continue,
                        "fn" => TokenKind::Fn,
                        "return" => TokenKind::Return,
                        "switch" => TokenKind::Switch,
                        "case" => TokenKind::Case,
                        "default" => TokenKind::Default,
                        "try" => TokenKind::Try,
                        "catch" => TokenKind::Catch,
                        "finally" => TokenKind::Finally,
                        "throw" => TokenKind::Throw,
                        "import" => TokenKind::Import,
                        "export" => TokenKind::Export,
                        "true" => TokenKind::True,
                        "false" => TokenKind::False,
                        "null" => TokenKind::Null,
                        "kxin" | "readline" => TokenKind::ReadLine,
                        "kxout" | "print" => TokenKind::Print,
                        "kxerr" | "eprint" => TokenKind::EPrint,
                        "kxwrite" => TokenKind::Write,
                        "kxclear" => TokenKind::Clear,
                        "kxlen" | "len" => TokenKind::Len,
                        "kxcast" | "cast" => TokenKind::Cast,
                        "kxsys" | "syscall" => TokenKind::SysRun,
                        "kxoutln" => TokenKind::Print,
                        "kxin_num" | "readnumber" => TokenKind::ReadNumber,
                        "kxrand" | "random" => TokenKind::Random,
                        "kxnow" | "now" => TokenKind::Now,
                        _ => TokenKind::Identifier(ident),
                    };
                    tokens.push(Token { kind, line, col });
                }
                unknown => {
                    return Err(KexError::new(
                        "Lexer",
                        line,
                        col,
                        format!("unexpected character '{}'", unknown),
                    ));
                }
            }
        }

        tokens.push(Token {
            kind: TokenKind::Eof,
            line: self.line,
            col: self.col,
        });
        Ok(tokens)
    }

    fn read_number(&mut self, chars: &[char]) -> LexResult<f64> {
        let (line, col) = (self.line, self.col);
        let start = self.pos;

        if self.peek_at(chars, 0) == '0' {
            let radix_marker = self.peek_at(chars, 1).to_ascii_lowercase();
            if radix_marker == 'x' || radix_marker == 'b' || radix_marker == 'o' {
                let radix = match radix_marker {
                    'x' => 16,
                    'b' => 2,
                    _ => 8,
                };
                self.bump(chars);
                self.bump(chars);
                let digits_start = self.pos;
                while self.pos < chars.len()
                    && (self.peek_at(chars, 0).is_ascii_alphanumeric() || self.peek_at(chars, 0) == '_')
                {
                    self.bump(chars);
                }
                let raw: String = chars[digits_start..self.pos]
                    .iter()
                    .filter(|c| **c != '_')
                    .collect();
                return i64::from_str_radix(&raw, radix).map(|v| v as f64).map_err(|_| {
                    KexError::new(
                        "Lexer",
                        line,
                        col,
                        format!("invalid base-{} literal '{}'", radix, raw),
                    )
                });
            }
        }

        let mut seen_dot = false;
        let mut seen_exp = false;
        while self.pos < chars.len() {
            let c = self.peek_at(chars, 0);
            if c.is_ascii_digit() || c == '_' {
                self.bump(chars);
            } else if c == '.' && !seen_dot && !seen_exp {
                // don't swallow the dot of a range like 1..5 or a member access
                let next = self.peek_at(chars, 1);
                if next == '.' || !next.is_ascii_digit() {
                    break;
                }
                seen_dot = true;
                self.bump(chars);
            } else if (c == 'e' || c == 'E') && !seen_exp {
                let next = self.peek_at(chars, 1);
                if next.is_ascii_digit() || ((next == '+' || next == '-') && self.peek_at(chars, 2).is_ascii_digit())
                {
                    seen_exp = true;
                    self.bump(chars);
                    if self.peek_at(chars, 0) == '+' || self.peek_at(chars, 0) == '-' {
                        self.bump(chars);
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        let raw: String = chars[start..self.pos]
            .iter()
            .filter(|c| **c != '_')
            .collect();
        raw.parse::<f64>().map_err(|_| {
            KexError::new("Lexer", line, col, format!("invalid number '{}'", raw))
        })
    }

    /// Reads a quoted string. `\\{` escapes a literal brace; `{ ... }` embeds code
    /// when the string is double quoted (or when the raw flag is set).
    fn read_string(
        &mut self,
        chars: &[char],
        quote: char,
        force_template: bool,
    ) -> LexResult<Vec<StrPart>> {
        let (line, col) = (self.line, self.col);
        self.bump(chars); // opening quote
        let mut parts: Vec<StrPart> = Vec::new();
        let mut text = String::new();
        let templated = force_template || quote == '"';

        loop {
            if self.pos >= chars.len() {
                return Err(KexError::new(
                    "Lexer",
                    line,
                    col,
                    "unterminated string literal",
                ));
            }
            let c = self.peek_at(chars, 0);

            if c == quote {
                self.bump(chars);
                break;
            }

            if c == '\\' {
                self.bump(chars);
                let esc = if self.pos < chars.len() {
                    self.bump(chars)
                } else {
                    return Err(KexError::new("Lexer", line, col, "dangling escape"));
                };
                text.push(match esc {
                    'n' => '\n',
                    't' => '\t',
                    'r' => '\r',
                    '0' => '\0',
                    '\\' => '\\',
                    '\'' => '\'',
                    '"' => '"',
                    '{' => '{',
                    '}' => '}',
                    other => {
                        return Err(KexError::new(
                            "Lexer",
                            line,
                            col,
                            format!("unknown escape sequence '\\{}'", other),
                        ))
                    }
                });
                continue;
            }

            if templated && c == '{' {
                // only treat as interpolation when it is not `{{`
                if self.peek_at(chars, 1) == '{' {
                    self.bump(chars);
                    self.bump(chars);
                    text.push('{');
                    continue;
                }
                self.bump(chars); // consume '{'
                let code_start = self.pos;
                let code_line = self.line;
                let mut depth = 1usize;
                while self.pos < chars.len() {
                    let ic = self.peek_at(chars, 0);
                    if ic == '{' {
                        depth += 1;
                    } else if ic == '}' {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    } else if ic == '"' || ic == '\'' {
                        // skip nested strings so their braces are ignored
                        let nested = ic;
                        self.bump(chars);
                        while self.pos < chars.len() && self.peek_at(chars, 0) != nested {
                            if self.peek_at(chars, 0) == '\\' {
                                self.bump(chars);
                            }
                            self.bump(chars);
                        }
                    }
                    self.bump(chars);
                }
                if self.pos >= chars.len() {
                    return Err(KexError::new(
                        "Lexer",
                        code_line,
                        col,
                        "unterminated '{' interpolation",
                    ));
                }
                let code: String = chars[code_start..self.pos].iter().collect();
                self.bump(chars); // consume '}'
                if !text.is_empty() {
                    parts.push(StrPart::Lit(std::mem::take(&mut text)));
                }
                parts.push(StrPart::Code(code, code_line));
                continue;
            }

            if templated && c == '}' && self.peek_at(chars, 1) == '}' {
                self.bump(chars);
                self.bump(chars);
                text.push('}');
                continue;
            }

            if c == '\n' {
                return Err(KexError::new(
                    "Lexer",
                    line,
                    col,
                    "newline inside string literal",
                ));
            }

            text.push(self.bump(chars));
        }

        if !text.is_empty() || parts.is_empty() {
            parts.push(StrPart::Lit(text));
        }
        Ok(parts)
    }
}

pub fn is_ident_start(c: char) -> bool {
    c.is_alphabetic() || c == '_' || c == '$'
}

pub fn is_ident_continue(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '$'
}

pub fn kex_literal_to_value(kind: &TokenKind) -> Option<KexValue> {
    match kind {
        TokenKind::Number(n) => Some(KexValue::Number(*n)),
        TokenKind::Str(s) => Some(KexValue::String(s.clone())),
        TokenKind::True => Some(KexValue::Boolean(true)),
        TokenKind::False => Some(KexValue::Boolean(false)),
        TokenKind::Null => Some(KexValue::Null),
        _ => None,
    }
}

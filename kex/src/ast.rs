// ============================================================================
// MODULE 3: AST
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Pow,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
    BitAnd,
    BitOr,
    BitXor,
    Shl,
    Shr,
}

impl BinOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::Pow => "**",
            BinOp::Eq => "==",
            BinOp::Ne => "!=",
            BinOp::Lt => "<",
            BinOp::Gt => ">",
            BinOp::Le => "<=",
            BinOp::Ge => ">=",
            BinOp::And => "&&",
            BinOp::Or => "||",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnOp {
    Neg,
    Not,
    BitNot,
}

impl UnOp {
    pub fn symbol(&self) -> &'static str {
        match self {
            UnOp::Neg => "-",
            UnOp::Not => "!",
            UnOp::BitNot => "~",
        }
    }
}

/// One piece of an interpolated string literal: raw text or embedded source.
#[derive(Debug, Clone, PartialEq)]
pub enum StrPart {
    Lit(String),
    Code(String, usize),
}

#[derive(Debug, Clone)]
pub enum Expr {
    Literal(KexValueLit),
    Interp(Vec<StrPart>),
    Var(String),
    ArrayLit(Vec<Expr>),
    ObjectLit(Vec<(String, Expr)>),
    Index(Box<Expr>, Box<Expr>),
    Slice(Box<Expr>, Option<Box<Expr>>, Option<Box<Expr>>),
    Member(Box<Expr>, String),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Unary(UnOp, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Coalesce(Box<Expr>, Box<Expr>),
    Range(Box<Expr>, Box<Expr>, bool, Option<Box<Expr>>),
    Call(Box<Expr>, Vec<Expr>),
    Method(Box<Expr>, String, Vec<Expr>),
    AnonFn(String, Vec<String>, Vec<Statement>),
    Cast(Box<Expr>, String),
    Len(Box<Expr>),
}

pub type KexValueLit = crate::value::KexValue;

impl Expr {
    /// Short human readable form used in runtime error messages.
    pub fn describe(&self) -> String {
        match self {
            Expr::Literal(v) => format!("the literal {}", v.to_display()),
            Expr::Interp(_) => "an interpolated string".to_string(),
            Expr::Var(name) => format!("'{}'", name),
            Expr::ArrayLit(_) => "an array literal".to_string(),
            Expr::ObjectLit(_) => "an object literal".to_string(),
            Expr::Index(base, _) => format!("{}[..]", base.describe()),
            Expr::Slice(base, _, _) => format!("{}[..]", base.describe()),
            Expr::Member(base, key) => format!("{}.{}", base.describe(), key),
            Expr::Binary(op, _, _) => format!("a '{}' expression", op.symbol()),
            Expr::Unary(op, _) => format!("a unary '{}' expression", op.symbol()),
            Expr::Ternary(_, _, _) => "a conditional expression".to_string(),
            Expr::Coalesce(_, _) => "a '??' expression".to_string(),
            Expr::Range(_, _, _, _) => "a range".to_string(),
            Expr::Call(callee, _) => format!("{}()", callee.describe()),
            Expr::Method(base, name, _) => format!("{}.{}()", base.describe(), name),
            Expr::AnonFn(name, _, _) => format!("fn {}", name),
            Expr::Cast(_, target) => format!("cast to '{}'", target),
            Expr::Len(_) => "len()".to_string(),
        }
    }

    /// Reinterprets a parsed expression as an assignment target.
    pub fn to_assign_target(self) -> Option<AssignTarget> {
        match self {
            Expr::Var(name) => Some(AssignTarget::Name(name)),
            Expr::Member(base, key) => Some(AssignTarget::Member(*base, key)),
            Expr::Index(base, index) => Some(AssignTarget::Index(*base, *index)),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub enum Statement {
    Let {
        name: String,
        init: Option<Expr>,
        mutable: bool,
        line: usize,
    },
    Assign {
        target: AssignTarget,
        value: Expr,
        line: usize,
    },
    CompoundAssign {
        target: AssignTarget,
        op: BinOp,
        value: Expr,
        line: usize,
    },
    Incr {
        target: AssignTarget,
        delta: f64,
        line: usize,
    },
    KxOut {
        args: Vec<Expr>,
        stream: OutStream,
        newline: bool,
        line: usize,
    },
    SysCall {
        mode: SysMode,
        value: Expr,
        line: usize,
    },
    If {
        cond: Expr,
        then_branch: Vec<Statement>,
        else_branch: Vec<Statement>,
        line: usize,
    },
    While {
        cond: Expr,
        body: Vec<Statement>,
        line: usize,
    },
    DoWhile {
        body: Vec<Statement>,
        cond: Expr,
        line: usize,
    },
    ForClassic {
        init: Option<Box<Statement>>,
        cond: Option<Expr>,
        step: Option<Box<Statement>>,
        body: Vec<Statement>,
        line: usize,
    },
    ForIn {
        name: String,
        iterable: Expr,
        body: Vec<Statement>,
        line: usize,
    },
    Switch {
        subject: Expr,
        cases: Vec<SwitchCase>,
        line: usize,
    },
    Break {
        line: usize,
    },
    Continue {
        line: usize,
    },
    FnDecl {
        name: String,
        params: Vec<String>,
        body: Vec<Statement>,
        line: usize,
    },
    Return {
        value: Option<Expr>,
        line: usize,
    },
    Import {
        path: String,
        line: usize,
    },
    Export {
        name: String,
        value: Expr,
        line: usize,
    },
    Throw {
        value: Expr,
        line: usize,
    },
    Try {
        body: Vec<Statement>,
        binding: Option<String>,
        handler: Vec<Statement>,
        finally: Vec<Statement>,
        line: usize,
    },
    ExprStmt {
        value: Expr,
        line: usize,
    },
}

impl Statement {
    pub fn line(&self) -> usize {
        match self {
            Statement::Let { line, .. }
            | Statement::Assign { line, .. }
            | Statement::CompoundAssign { line, .. }
            | Statement::Incr { line, .. }
            | Statement::KxOut { line, .. }
            | Statement::SysCall { line, .. }
            | Statement::If { line, .. }
            | Statement::While { line, .. }
            | Statement::DoWhile { line, .. }
            | Statement::ForClassic { line, .. }
            | Statement::ForIn { line, .. }
            | Statement::Switch { line, .. }
            | Statement::Break { line }
            | Statement::Continue { line }
            | Statement::FnDecl { line, .. }
            | Statement::Return { line, .. }
            | Statement::Import { line, .. }
            | Statement::Export { line, .. }
            | Statement::Throw { line, .. }
            | Statement::Try { line, .. }
            | Statement::ExprStmt { line, .. } => *line,
        }
    }
}

#[derive(Debug, Clone)]
pub enum AssignTarget {
    Name(String),
    Index(Expr, Expr),
    Member(Expr, String),
}

impl AssignTarget {
    pub fn describe(&self) -> String {
        match self {
            AssignTarget::Name(n) => n.clone(),
            AssignTarget::Index(_, _) => "indexed value".to_string(),
            AssignTarget::Member(_, key) => key.clone(),
        }
    }}

#[derive(Debug, Clone)]
pub struct SwitchCase {
    pub test: Option<Expr>,
    pub body: Vec<Statement>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OutStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SysMode {
    /// Inherit stdio, discard status.
    Spawn,
    /// Capture stdout, return a string.
    Capture,
    /// Return the exit code.
    Code,
}

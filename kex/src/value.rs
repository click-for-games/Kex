use crate::ast::Statement;
use crate::error::Env;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::rc::Rc;

// ============================================================================
// MODULE 1: VALUE & DATA TYPES
// ============================================================================

/// Insertion ordered key/value store, the runtime representation of `{}`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct KexObject {
    pub entries: Vec<(String, KexValue)>,
}

impl KexObject {
    pub fn new() -> Self {
        KexObject {
            entries: Vec::new(),
        }
    }

    pub fn get(&self, key: &str) -> Option<&KexValue> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn set(&mut self, key: &str, value: KexValue) {
        match self.entries.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => self.entries.push((key.to_string(), value)),
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<KexValue> {
        let index = self.entries.iter().position(|(k, _)| k == key)?;
        Some(self.entries.remove(index).1)
    }

    pub fn keys(&self) -> Vec<String> {
        self.entries.iter().map(|(k, _)| k.clone()).collect()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// A range produced by `a..b`, `a..=b` or `range(...)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RangeVal {
    pub start: f64,
    pub end: f64,
    pub step: f64,
    pub inclusive: bool,
}

impl RangeVal {
    pub fn len(&self) -> usize {
        if self.step == 0.0 {
            return 0;
        }
        let span = if self.inclusive {
            (self.end - self.start) / self.step + 1.0
        } else {
            (self.end - self.start) / self.step
        };
        if span <= 0.0 {
            0
        } else {
            span.round() as usize
        }
    }

    pub fn to_array(&self) -> Vec<KexValue> {
        let mut out = Vec::with_capacity(self.len());
        let mut current = self.start;
        for _ in 0..self.len() {
            out.push(KexValue::Number(current));
            current += self.step;
        }
        out
    }
}

/// A user defined function, captured together with its defining scope.
#[derive(Debug)]
pub struct FuncVal {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Statement>,
    pub env: Env,
}

#[derive(Debug, Clone)]
pub enum KexValue {
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
    Array(Vec<KexValue>),
    Object(KexObject),
    Range(RangeVal),
    Func(Rc<RefCell<FuncVal>>),
    Builtin(String),
    Error(String),
}

impl PartialEq for KexValue {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (KexValue::Number(a), KexValue::Number(b)) => a == b,
            (KexValue::String(a), KexValue::String(b)) => a == b,
            (KexValue::Boolean(a), KexValue::Boolean(b)) => a == b,
            (KexValue::Null, KexValue::Null) => true,
            (KexValue::Array(a), KexValue::Array(b)) => a == b,
            (KexValue::Object(a), KexValue::Object(b)) => {
                a.len() == b.len()
                    && a.entries
                        .iter()
                        .all(|(k, v)| b.get(k).map(|o| o == v).unwrap_or(false))
            }
            (KexValue::Range(a), KexValue::Range(b)) => a == b,
            (KexValue::Func(a), KexValue::Func(b)) => Rc::ptr_eq(a, b),
            (KexValue::Builtin(a), KexValue::Builtin(b)) => a == b,
            (KexValue::Error(a), KexValue::Error(b)) => a == b,
            _ => false,
        }
    }
}

impl KexValue {
    pub fn string(s: impl Into<String>) -> KexValue {
        KexValue::String(s.into())
    }

    pub fn type_name(&self) -> &'static str {
        match self {
            KexValue::Number(_) => "num",
            KexValue::String(_) => "str",
            KexValue::Boolean(_) => "bool",
            KexValue::Null => "null",
            KexValue::Array(_) => "array",
            KexValue::Object(_) => "object",
            KexValue::Range(_) => "range",
            KexValue::Func(_) => "fn",
            KexValue::Builtin(_) => "builtin",
            KexValue::Error(_) => "error",
        }
    }

    /// Human readable form, used by `kxout`.
    pub fn to_display(&self) -> String {
        match self {
            KexValue::Number(n) => format_number(*n),
            KexValue::String(s) => s.clone(),
            KexValue::Boolean(b) => b.to_string(),
            KexValue::Null => "null".to_string(),
            KexValue::Array(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.to_display()).collect();
                format!("[{}]", inner.join(", "))
            }
            KexValue::Object(obj) => {
                let inner: Vec<String> = obj
                    .entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.to_display()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            KexValue::Range(r) => {
                let dots = if r.inclusive { "..=" } else { ".." };
                format!(
                    "{}{}{}{}",
                    format_number(r.start),
                    dots,
                    format_number(r.end),
                    if r.step == 1.0 {
                        String::new()
                    } else {
                        format!(" step {}", format_number(r.step))
                    }
                )
            }
            KexValue::Func(f) => format!("<fn {}>", f.borrow().name),
            KexValue::Builtin(name) => format!("<builtin {}>", name),
            KexValue::Error(msg) => msg.clone(),
        }
    }

    /// Quoted / recursive form, used by `inspect` and thrown values.
    pub fn inspect(&self) -> String {
        match self {
            KexValue::String(s) => format!("\"{}\"", escape_string(s)),
            KexValue::Array(items) => {
                let inner: Vec<String> = items.iter().map(|v| v.inspect()).collect();
                format!("[{}]", inner.join(", "))
            }
            KexValue::Object(obj) => {
                let inner: Vec<String> = obj
                    .entries
                    .iter()
                    .map(|(k, v)| format!("{}: {}", k, v.inspect()))
                    .collect();
                format!("{{{}}}", inner.join(", "))
            }
            other => other.to_display(),
        }
    }

    /// Truthiness for `if` / `while` / `&&` / `||`.
    pub fn is_truthy(&self) -> bool {
        match self {
            KexValue::Boolean(b) => *b,
            KexValue::Null => false,
            KexValue::Number(n) => *n != 0.0 && !n.is_nan(),
            KexValue::String(s) => !s.is_empty(),
            KexValue::Array(_) | KexValue::Object(_) | KexValue::Range(_) => true,
            KexValue::Func(_) | KexValue::Builtin(_) => true,
            KexValue::Error(_) => true,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            KexValue::Number(n) => Some(*n),
            KexValue::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            _ => None,
        }
    }

    pub fn as_index(&self) -> Option<usize> {
        match self {
            KexValue::Number(n) if *n >= 0.0 && n.fract() == 0.0 => Some(*n as usize),
            _ => None,
        }
    }

    pub fn trim_text(&self) -> String {
        self.to_display().trim().to_string()
    }
}

/// Keeps `13` instead of `13.0` while still trimming float noise.
pub fn format_number(n: f64) -> String {
    if n.is_nan() {
        return "NaN".to_string();
    }
    if n.is_infinite() {
        return if n > 0.0 { "Infinity" } else { "-Infinity" }.to_string();
    }
    if n == n.trunc() && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        let s = format!("{}", n);
        if s.contains('.') || s.contains('e') {
            s
        } else {
            s
        }
    }
}

pub fn escape_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            '\r' => out.push_str("\\r"),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\'' => out.push_str("\\'"),
            c => out.push(c),
        }
    }
    out
}

pub fn compare(a: &KexValue, b: &KexValue) -> Option<Ordering> {
    match (a, b) {
        (KexValue::Number(x), KexValue::Number(y)) => x.partial_cmp(y),
        (KexValue::String(x), KexValue::String(y)) => Some(x.cmp(y)),
        (KexValue::Boolean(x), KexValue::Boolean(y)) => Some(x.cmp(y)),
        _ => None,
    }
}

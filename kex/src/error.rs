use crate::value::KexValue;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fmt;
use std::rc::Rc;

/// One lexical scope. Frames are shared by reference so that a function can
/// read and write the variables of the scope it was declared in, the same way
/// a closure does in JavaScript.
pub type Frame = Rc<RefCell<HashMap<String, KexValue>>>;

pub type Env = Vec<Frame>;

pub fn new_frame() -> Frame {
    Rc::new(RefCell::new(HashMap::new()))
}

/// Reads a name out of a frame without holding the borrow across a call.
pub fn frame_get(frame: &Frame, name: &str) -> Option<KexValue> {
    frame.borrow().get(name).cloned()
}

impl From<crate::lexer::KexError> for Signal {
    fn from(e: crate::lexer::KexError) -> Self {
        Signal::Error(KexValue::Error(format!(
            "[{} error, line {}, column {}] {}",
            e.phase, e.line, e.col, e.message
        )))
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message())
    }
}

/// Non-local control flow used by the tree walking interpreter.
#[derive(Debug, Clone)]
pub enum Signal {
    Break,
    Continue,
    /// `throw` or a runtime fault. Catchable by `try` / `catch`.
    Error(KexValue),
    /// Process exit, never catchable.
    Exit(i32),
}

impl Signal {
    pub fn message(&self) -> String {
        match self {
            Signal::Break => "break outside of a loop".to_string(),
            Signal::Continue => "continue outside of a loop".to_string(),
            Signal::Error(v) => v.inspect(),
            Signal::Exit(code) => format!("exit({})", code),
        }
    }
}

pub fn throw(value: KexValue) -> Signal {
    Signal::Error(value)
}

pub fn error(msg: impl Into<String>) -> Signal {
    Signal::Error(KexValue::Error(msg.into()))
}

pub fn expected(want: &str, got: &KexValue) -> Signal {
    error(format!(
        "expected {} but received {}",
        want,
        got.type_name()
    ))
}

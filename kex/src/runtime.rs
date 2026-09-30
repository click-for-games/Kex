use crate::ast::*;
use crate::builtins;
use crate::error::{expected, frame_get, new_frame, throw, Env, Signal};
use crate::host::HostBridge;
use crate::lexer::{KexError, LexResult};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::value::*;
use std::cmp::Ordering;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::io::{self, Write};
use std::path::PathBuf;
use std::rc::Rc;

// ============================================================================
// MODULE 5: RUNTIME & INTERPRETER
// ============================================================================

pub struct Runtime {
    pub env: Env,
    pub host: Option<HostBridge>,
    pub script_dir: PathBuf,
    pub call_depth: usize,
    max_depth: usize,
    loaded: Vec<PathBuf>,
    consts: Vec<HashMap<String, ()>>,
}

const MAX_CALL_DEPTH: usize = 2000;

/// What a loop body asked for after running once.
enum Step {
    /// Keep looping; this is also what `continue` produces.
    Next,
    /// `break` was used.
    Stop,
    /// `return` was used, carrying its value.
    Leave(KexValue),
}

impl Runtime {
    pub fn new() -> Self {
        Runtime {
            env: vec![new_frame()],
            host: None,
            script_dir: PathBuf::from("."),
            call_depth: 0,
            max_depth: MAX_CALL_DEPTH,
            loaded: Vec::new(),
            consts: vec![HashMap::new()],
        }
    }

    // ----- environment -----------------------------------------------------

    /// Local declarations shadow the standard library, never the other way round.
    pub fn get_var(&self, name: &str) -> KexValue {
        for frame in self.env.iter().rev() {
            if let Some(value) = frame_get(frame, name) {
                return value;
            }
        }
        if let Some(builtin) = builtins::lookup(name) {
            return KexValue::Builtin(builtin.to_string());
        }
        KexValue::Null
    }

    pub fn set_var(&mut self, name: &str, value: KexValue) {
        for frame in self.env.iter().rev() {
            let mut slot = frame.borrow_mut();
            if slot.contains_key(name) {
                slot.insert(name.to_string(), value);
                return;
            }
        }
        if let Some(frame) = self.env.last() {
            frame.borrow_mut().insert(name.to_string(), value);
        }
    }

    pub fn decl_var(&mut self, name: &str, value: KexValue) {
        if let Some(frame) = self.env.last() {
            frame.borrow_mut().insert(name.to_string(), value);
        }
    }

    fn is_const(&self, name: &str) -> bool {
        self.consts.iter().rev().any(|f| f.contains_key(name))
    }

    fn mark_const(&mut self, name: &str) {
        if let Some(frame) = self.consts.last_mut() {
            frame.insert(name.to_string(), ());
        }
    }

    // ----- program ---------------------------------------------------------

    pub fn run_source(&mut self, source: &str) -> Result<i32, Signal> {
        let ast = compile(source)?;
        self.execute(ast)?;
        Ok(0)
    }

    pub fn check_source(&mut self, source: &str) -> Result<(), KexError> {
        compile(source).map(|_| ())
    }

    fn execute(&mut self, stmts: Vec<Statement>) -> Result<(), Signal> {
        self.exec_block(stmts)?;
        Ok(())
    }

    fn exec_block(&mut self, stmts: Vec<Statement>) -> Result<Option<KexValue>, Signal> {
        for stmt in stmts {
            let line = stmt.line();
            if let Some(value) = self.exec_statement(stmt).map_err(|e| annotate(e, line))? {
                return Ok(Some(value));
            }
        }
        Ok(None)
    }

    fn exec_loop_body(&mut self, body: Vec<Statement>) -> Result<Step, Signal> {
        match self.exec_block(body) {
            Ok(Some(value)) => Ok(Step::Leave(value)),
            Ok(None) => Ok(Step::Next),
            Err(Signal::Break) => Ok(Step::Stop),
            Err(Signal::Continue) => Ok(Step::Next),
            Err(other) => Err(other),
        }
    }

    // ----- statements ------------------------------------------------------

    fn exec_statement(&mut self, stmt: Statement) -> Result<Option<KexValue>, Signal> {
        match stmt {
            Statement::Let {
                name, init, mutable, ..
            } => {
                let value = match init {
                    Some(expr) => self.eval_expr(&expr)?,
                    None => KexValue::Null,
                };
                self.decl_var(&name, value);
                if !mutable {
                    self.mark_const(&name);
                }
                Ok(None)
            }

            Statement::Export { name, value, .. } => {
                let value = self.eval_expr(&value)?;
                self.decl_var(&name, value.clone());
                if let Some(frame) = self.env.last() {
                    frame.borrow_mut().insert(format!("__export_{}", name), value);
                }
                Ok(None)
            }

            Statement::Assign { target, value, .. } => {
                let value = self.eval_expr(&value)?;
                self.store(target, value)?;
                Ok(None)
            }

            Statement::CompoundAssign {
                target, op, value, ..
            } => {
                let rhs = self.eval_expr(&value)?;
                let current = self.eval_target(&target)?;
                let updated = self.binary_values(op, &current, &rhs)?;
                self.store(target, updated)?;
                Ok(None)
            }

            Statement::Incr {
                target, delta, line, ..
            } => {
                let current = self.eval_target(&target)?;
                let updated = match current.as_number() {
                    Some(n) => KexValue::Number(n + delta),
                    None => {
                        return Err(crate::error::error(format!(
                            "line {}: '{}' is not a number",
                            line,
                            target.describe()
                        )))
                    }
                };
                self.store(target, updated)?;
                Ok(None)
            }

            Statement::KxOut {
                args,
                stream,
                newline,
                ..
            } => {
                let mut parts = Vec::new();
                for arg in args {
                    parts.push(self.eval_expr(&arg)?.to_display());
                }
                let rendered = parts.join(" ");
                match stream {
                    OutStream::Stdout => {
                        let mut out = io::stdout();
                        let _ = out.write_all(rendered.as_bytes());
                        if newline {
                            let _ = out.write_all(b"\n");
                        }
                        let _ = out.flush();
                    }
                    OutStream::Stderr => {
                        let mut out = io::stderr();
                        let _ = out.write_all(rendered.as_bytes());
                        if newline {
                            let _ = out.write_all(b"\n");
                        }
                        let _ = out.flush();
                    }
                }
                Ok(None)
            }

            Statement::SysCall { mode, value, .. } => {
                let command = self.eval_expr(&value)?.to_display();
                self.run_system(mode, &command)
            }

            Statement::If {
                cond,
                then_branch,
                else_branch,
                ..
            } => {
                if self.eval_expr(&cond)?.is_truthy() {
                    self.exec_block(then_branch)
                } else {
                    self.exec_block(else_branch)
                }
            }

            Statement::While { cond, body, .. } => {
                let mut guard: u64 = 0;
                while self.eval_expr(&cond)?.is_truthy() {
                    guard += 1;
                    if guard % 1024 == 0 && guard > 10_000_000 {
                        return Err(crate::error::error("while loop exceeded 10,000,000 iterations"));
                    }
                    match self.exec_loop_body(body.clone())? {
                        Step::Leave(v) => return Ok(Some(v)),
                        Step::Stop => break,
                        Step::Next => continue,
                    }
                }
                Ok(None)
            }

            Statement::DoWhile { body, cond, .. } => {
                loop {
                    match self.exec_loop_body(body.clone())? {
                        Step::Leave(v) => return Ok(Some(v)),
                        Step::Stop => break,
                        Step::Next => {}
                    }
                    if !self.eval_expr(&cond)?.is_truthy() {
                        break;
                    }
                }
                Ok(None)
            }

            Statement::ForClassic {
                init,
                cond,
                step,
                body,
                ..
            } => {
                self.env.push(new_frame());
                let outcome = (|rt: &mut Runtime| -> Result<Option<KexValue>, Signal> {
                    if let Some(init) = &init {
                        rt.exec_statement((**init).clone())?;
                    }
                    loop {
                        if let Some(cond) = &cond {
                            if !rt.eval_expr(cond)?.is_truthy() {
                                break;
                            }
                        }
                        match rt.exec_loop_body(body.clone())? {
                            Step::Leave(v) => return Ok(Some(v)),
                            Step::Stop => break,
                            Step::Next => {}
                        }
                        if let Some(step) = &step {
                            rt.exec_statement((**step).clone())?;
                        }
                    }
                    Ok(None)
                })(self);
                self.env.pop();
                outcome
            }

            Statement::ForIn {
                name,
                iterable,
                body,
                ..
            } => {
                let source_value = self.eval_expr(&iterable)?;
                let seq = self.iterate(&source_value)?;
                self.env.push(new_frame());
                let outcome = (|rt: &mut Runtime| -> Result<Option<KexValue>, Signal> {
                    for item in seq {
                        if let Some(frame) = rt.env.last() {
                            frame.borrow_mut().insert(name.clone(), item);
                        }
                        match rt.exec_loop_body(body.clone())? {
                            Step::Leave(v) => return Ok(Some(v)),
                            Step::Stop => break,
                            Step::Next => {}
                        }
                    }
                    Ok(None)
                })(self);
                self.env.pop();
                outcome
            }

            Statement::Switch { subject, cases, .. } => {
                let value = self.eval_expr(&subject)?;
                let mut chosen: Option<usize> = None;
                let mut fallback: Option<usize> = None;
                for (i, case) in cases.iter().enumerate() {
                    match &case.test {
                        Some(test) => {
                            let candidate = self.eval_expr(test)?;
                            if candidate.is_truthy() && self.loose_eq(&value, &candidate) {
                                chosen = Some(i);
                                break;
                            }
                        }
                        None => fallback = Some(i),
                    }
                }
                let start = chosen.or(fallback);
                if let Some(start) = start {
                    for case in &cases[start..] {
                        match self.exec_block(case.body.clone())? {
                            Some(v) => return Ok(Some(v)),
                            None => continue,
                        }
                    }
                }
                Ok(None)
            }

            Statement::Break { .. } => Err(Signal::Break),
            Statement::Continue { .. } => Err(Signal::Continue),

            Statement::FnDecl {
                name, params, body, ..
            } => {
                let handle = Rc::new(RefCell::new(FuncVal {
                    name: name.clone(),
                    params,
                    body,
                    env: self.env.clone(),
                }));
                // allow recursion: the closure can see itself
                if let Some(frame) = handle.borrow().env.last().cloned() {
                    frame.borrow_mut().insert(name.clone(), KexValue::Func(handle.clone()));
                }
                self.decl_var(&name, KexValue::Func(handle));
                Ok(None)
            }

            Statement::Return { value, .. } => {
                let value = match value {
                    Some(expr) => self.eval_expr(&expr)?,
                    None => KexValue::Null,
                };
                Ok(Some(value))
            }

            Statement::Import { path, line } => self.import_module(&path, line),

            Statement::Throw { value, .. } => {
                let value = self.eval_expr(&value)?;
                Err(throw(value))
            }

            Statement::Try {
                body,
                binding,
                handler,
                finally,
                ..
            } => {
                let result = self.exec_block(body);
                let result = match result {
                    Err(Signal::Error(payload)) if !handler.is_empty() => {
                        self.env.push(new_frame());
                        if let Some(var) = &binding {
                            self.decl_var(var, payload);
                        }
                        let handled = self.exec_block(handler);
                        self.env.pop();
                        handled?
                    }
                    other => other?,
                };
                if !finally.is_empty() {
                    if let Some(value) = self.exec_block(finally)? {
                        return Ok(Some(value));
                    }
                }
                Ok(result)
            }

            Statement::ExprStmt { value, .. } => {
                self.eval_expr(&value)?;
                Ok(None)
            }
        }
    }

    fn run_system(&mut self, mode: SysMode, command: &str) -> Result<Option<KexValue>, Signal> {
        use std::process::{Command, Stdio};

        let shell: (&str, &[&str]) = if cfg!(target_os = "windows") {
            ("cmd", &["/C"])
        } else {
            ("sh", &["-c"])
        };

        let mut cmd = Command::new(shell.0);
        cmd.args(shell.1);
        cmd.arg(command);

        match mode {
            SysMode::Spawn => {
                let status = cmd.status().map_err(|e| {
                    crate::error::error(format!("syscall failed: {}", e))
                })?;
                Ok(Some(KexValue::Number(status.code().unwrap_or(0) as f64)))
            }
            SysMode::Code => {
                let status = cmd
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status()
                    .map_err(|e| crate::error::error(format!("syscall failed: {}", e)))?;
                Ok(Some(KexValue::Number(status.code().unwrap_or(0) as f64)))
            }
            SysMode::Capture => {
                let output = cmd
                    .stderr(Stdio::inherit())
                    .output()
                    .map_err(|e| crate::error::error(format!("syscall failed: {}", e)))?;
                let text = String::from_utf8_lossy(&output.stdout).to_string();
                Ok(Some(KexValue::String(text.trim_end().to_string())))
            }
        }
    }

    // ----- assignment ------------------------------------------------------

    fn store(&mut self, target: AssignTarget, value: KexValue) -> Result<(), Signal> {
        match target {
            AssignTarget::Name(name) => {
                if self.is_const(&name) {
                    return Err(crate::error::error(format!(
                        "'{}' is a const and cannot be reassigned",
                        name
                    )));
                }
                self.set_var(&name, value);
                Ok(())
            }
            AssignTarget::Index(base, index) => {
                let index = self.eval_expr(&index)?;
                self.mutate_index(base, index, value)
            }
            AssignTarget::Member(base, key) => self.mutate_member(base, key, value),
        }
    }

    fn mutate_index(&mut self, base: Expr, index: KexValue, value: KexValue) -> Result<(), Signal> {
        let mut container = self.eval_expr(&base)?;
        match &mut container {
            KexValue::Array(items) => {
                let idx = index.as_index().ok_or_else(|| expected("a whole number", &index))?;
                if idx >= items.len() {
                    items.push(value);
                } else {
                    items[idx] = value;
                }
                self.write_back(base, container)
            }
            KexValue::String(text) => {
                let idx = index.as_index().ok_or_else(|| expected("a whole number", &index))?;
                let chars: Vec<char> = text.chars().collect();
                if idx >= chars.len() {
                    return Err(crate::error::error(format!(
                        "string index {} is out of bounds",
                        idx
                    )));
                }
                let rendered = value.to_display();
                let replacement: Vec<char> = rendered.chars().collect();
                let mut next: Vec<char> = Vec::with_capacity(chars.len() + replacement.len());
                next.extend_from_slice(&chars[..idx]);
                next.extend(replacement);
                next.extend_from_slice(&chars[idx + 1..]);
                let updated = KexValue::String(next.into_iter().collect());
                self.write_back(base, updated)
            }
            KexValue::Object(obj) => {
                let key = match &index {
                    KexValue::String(s) => s.clone(),
                    KexValue::Number(n) => format_number(*n),
                    other => return Err(expected("a string key", other)),
                };
                obj.set(&key, value);
                self.write_back(base, container)
            }
            KexValue::Null => {
                self.write_back(base, KexValue::Array(vec![value]))
            }
            other => Err(crate::error::error(format!(
                "cannot index-assign into a {}",
                other.type_name()
            ))),
        }
    }

    /// Reads `receiver.name` without going through the method tables.
    fn member_value(&self, receiver: &KexValue, name: &str) -> Option<KexValue> {
        match receiver {
            KexValue::Object(obj) => obj.get(name).cloned(),
            KexValue::Array(items) => name.parse::<usize>().ok().and_then(|i| items.get(i).cloned()),
            _ => None,
        }
    }

    fn mutate_member(&mut self, base: Expr, key: String, value: KexValue) -> Result<(), Signal> {
        let mut container = self.eval_expr(&base)?;
        match &mut container {
            KexValue::Object(obj) => {
                obj.set(&key, value);
                self.write_back(base, container)
            }
            KexValue::Array(items) => {
                let idx = key.parse::<usize>().map_err(|_| {
                    crate::error::error(format!("array index '{}' is not a number", key))
                })?;
                if idx >= items.len() {
                    items.push(value);
                } else {
                    items[idx] = value;
                }
                self.write_back(base, container)
            }
            KexValue::Null => {
                let mut obj = KexObject::new();
                obj.set(&key, value);
                self.write_back(base, KexValue::Object(obj))
            }
            other => Err(crate::error::error(format!(
                "cannot set '.{}' on a {}",
                key,
                other.type_name()
            ))),
        }
    }

    fn write_back(&mut self, base: Expr, value: KexValue) -> Result<(), Signal> {
        match base {
            Expr::Var(name) => {
                self.set_var(&name, value);
                Ok(())
            }
            Expr::Member(inner, key) => self.mutate_member(*inner, key, value),
            Expr::Index(inner, index) => {
                let index_value = self.eval_expr(&index)?;
                self.mutate_index(*inner, index_value, value)
            }
            other => Err(crate::error::error(format!(
                "unsupported assignment target {}",
                other.describe()
            ))),
        }
    }

    fn eval_target(&mut self, target: &AssignTarget) -> Result<KexValue, Signal> {
        match target {
            AssignTarget::Name(name) => Ok(self.get_var(name)),
            AssignTarget::Index(base, index) => {
                let container = self.eval_expr(base)?;
                let key = self.eval_expr(index)?;
                self.read_index(&container, &key)
            }
            AssignTarget::Member(base, key) => {
                let container = self.eval_expr(base)?;
                match container {
                    KexValue::Object(obj) => Ok(obj.get(key).cloned().unwrap_or(KexValue::Null)),
                    KexValue::Array(items) => Ok(key
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| items.get(i).cloned())
                        .unwrap_or(KexValue::Null)),
                    _ => Ok(KexValue::Null),
                }
            }
        }
    }

    fn read_index(&self, container: &KexValue, key: &KexValue) -> Result<KexValue, Signal> {
        match container {
            KexValue::Array(items) => match key.as_index() {
                Some(i) => Ok(items.get(i).cloned().unwrap_or(KexValue::Null)),
                None => Err(expected("a whole number", key)),
            },
            KexValue::String(text) => {
                let chars: Vec<char> = text.chars().collect();
                let i = key.as_index().ok_or_else(|| expected("a whole number", key))?;
                Ok(chars
                    .get(i)
                    .map(|c| KexValue::String(c.to_string()))
                    .unwrap_or(KexValue::Null))
            }
            KexValue::Object(obj) => {
                let k = match key {
                    KexValue::String(s) => s.clone(),
                    KexValue::Number(n) => format_number(*n),
                    other => return Err(expected("a string key", other)),
                };
                Ok(obj.get(&k).cloned().unwrap_or(KexValue::Null))
            }
            KexValue::Range(r) => {
                let i = key.as_index().ok_or_else(|| expected("a whole number", key))?;
                Ok(r.to_array().get(i).cloned().unwrap_or(KexValue::Null))
            }
            KexValue::Null => Ok(KexValue::Null),
            other => Err(crate::error::error(format!(
                "cannot index a {}",
                other.type_name()
            ))),
        }
    }

    // ----- iteration -------------------------------------------------------

    pub fn iterate(&self, value: &KexValue) -> Result<Vec<KexValue>, Signal> {
        match value {
            KexValue::Array(items) => Ok(items.clone()),
            KexValue::Object(obj) => Ok(obj.keys().into_iter().map(KexValue::String).collect()),
            KexValue::String(text) => {
                Ok(text.chars().map(|c| KexValue::String(c.to_string())).collect())
            }
            KexValue::Range(r) => Ok(r.to_array()),
            KexValue::Null => Ok(Vec::new()),
            KexValue::Number(n) => Ok((0..*n as i64).map(|i| KexValue::Number(i as f64)).collect()),
            other => Err(crate::error::error(format!(
                "cannot iterate over a {}",
                other.type_name()
            ))),
        }
    }

    // ----- expressions -----------------------------------------------------

    pub fn eval_expr(&mut self, expr: &Expr) -> Result<KexValue, Signal> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Interp(parts) => {
                let mut out = String::new();
                for part in parts {
                    match part {
                        StrPart::Lit(text) => out.push_str(text),
                        StrPart::Code(code, line) => {
                            let expr = compile_fragment(code, *line)?;
                            out.push_str(&self.eval_expr(&expr)?.to_display());
                        }
                    }
                }
                Ok(KexValue::String(out))
            }
            Expr::Var(name) => Ok(self.get_var(name)),
            Expr::ArrayLit(items) => {
                let mut out = Vec::with_capacity(items.len());
                for item in items {
                    out.push(self.eval_expr(item)?);
                }
                Ok(KexValue::Array(out))
            }
            Expr::ObjectLit(entries) => {
                let mut obj = KexObject::new();
                for (key, value) in entries {
                    let value = self.eval_expr(value)?;
                    obj.set(key, value);
                }
                Ok(KexValue::Object(obj))
            }
            Expr::Index(base, index) => {
                let container = self.eval_expr(base)?;
                let key = self.eval_expr(index)?;
                self.read_index(&container, &key)
            }
            Expr::Slice(base, start, end) => {
                let container = self.eval_expr(base)?;
                let s = match start {
                    Some(e) => self.eval_expr(e)?.as_index(),
                    None => Some(0),
                };
                match container {
                    KexValue::Array(items) => {
                        let s = s.unwrap_or(0);
                        let e = self.eval_optional_index(end)?;
                        let slice: Vec<KexValue> = items[s.min(items.len())..e]
                            .to_vec();
                        Ok(KexValue::Array(slice))
                    }
                    KexValue::String(text) => {
                        let chars: Vec<char> = text.chars().collect();
                        let s = s.unwrap_or(0);
                        let e = self.eval_optional_index(end)?;
                        let slice: String = chars[s.min(chars.len())..e.min(chars.len())]
                            .iter()
                            .collect();
                        Ok(KexValue::String(slice))
                    }
                    KexValue::Range(r) => {
                        let all = r.to_array();
                        let s = s.unwrap_or(0);
                        let e = self.eval_optional_index(end)?;
                        Ok(KexValue::Array(all[s.min(all.len())..e.min(all.len())].to_vec()))
                    }
                    other => Err(crate::error::error(format!(
                        "cannot slice a {}",
                        other.type_name()
                    ))),
                }
            }
            Expr::Member(base, key) => {
                let container = self.eval_expr(base)?;
                match container {
                    KexValue::Object(obj) => Ok(obj.get(key).cloned().unwrap_or(KexValue::Null)),
                    KexValue::Array(items) => Ok(key
                        .parse::<usize>()
                        .ok()
                        .and_then(|i| items.get(i).cloned())
                        .unwrap_or(KexValue::Null)),
                    KexValue::String(text) => {
                        Ok(text.chars().nth(key.parse::<usize>().unwrap_or(usize::MAX)).map(|c| {
                            KexValue::String(c.to_string())
                        }).unwrap_or(KexValue::Null))
                    }
                    KexValue::Null => Ok(KexValue::Null),
                    other => Err(crate::error::error(format!(
                        "cannot read '.{}' from a {}",
                        key,
                        other.type_name()
                    ))),
                }
            }
            Expr::Binary(op, left, right) => match op {
                BinOp::And => {
                    let l = self.eval_expr(left)?;
                    if !l.is_truthy() {
                        return Ok(KexValue::Boolean(false));
                    }
                    Ok(KexValue::Boolean(self.eval_expr(right)?.is_truthy()))
                }
                BinOp::Or => {
                    let l = self.eval_expr(left)?;
                    if l.is_truthy() {
                        return Ok(KexValue::Boolean(true));
                    }
                    Ok(KexValue::Boolean(self.eval_expr(right)?.is_truthy()))
                }
                _ => {
                    let l = self.eval_expr(left)?;
                    let r = self.eval_expr(right)?;
                    self.binary_values(*op, &l, &r)
                }
            },
            Expr::Unary(op, inner) => {
                let value = self.eval_expr(inner)?;
                match op {
                    UnOp::Neg => match value {
                        KexValue::Number(n) => Ok(KexValue::Number(-n)),
                        KexValue::Boolean(b) => Ok(KexValue::Number(if b { -1.0 } else { 0.0 })),
                        KexValue::String(s) => match s.trim().parse::<f64>() {
                            Ok(n) => Ok(KexValue::Number(-n)),
                            Err(_) => Err(crate::error::error(format!(
                                "cannot negate '{}'",
                                s
                            ))),
                        },
                        other => Err(crate::error::error(format!(
                            "cannot negate a {}",
                            other.type_name()
                        ))),
                    },
                    UnOp::Not => Ok(KexValue::Boolean(!value.is_truthy())),
                    UnOp::BitNot => match value.as_number() {
                        Some(n) => Ok(KexValue::Number(!(n as i64) as f64)),
                        None => Err(expected("a number", &value)),
                    },
                }
            }
            Expr::Ternary(cond, yes, no) => {
                if self.eval_expr(cond)?.is_truthy() {
                    self.eval_expr(yes)
                } else {
                    self.eval_expr(no)
                }
            }
            Expr::Coalesce(left, right) => {
                let l = self.eval_expr(left)?;
                match l {
                    KexValue::Null => self.eval_expr(right),
                    KexValue::Error(_) => self.eval_expr(right),
                    other => Ok(other),
                }
            }
            Expr::Range(start, end, inclusive, step) => {
                let s = self.eval_expr(start)?.as_number().ok_or_else(|| {
                    crate::error::error("range start must be a number")
                })?;
                let e = self.eval_expr(end)?.as_number().ok_or_else(|| {
                    crate::error::error("range end must be a number")
                })?;
                let st = match step {
                    Some(expr) => {
                        let value = self.eval_expr(expr)?;
                        value
                            .as_number()
                            .ok_or_else(|| crate::error::error("range step must be a number"))?
                    }
                    None => 1.0,
                };
                Ok(KexValue::Range(RangeVal {
                    start: s,
                    end: e,
                    step: st,
                    inclusive: *inclusive,
                }))
            }
            Expr::Cast(inner, target) => {
                let value = self.eval_expr(inner)?;
                cast_value(value, target)
            }
            Expr::Len(inner) => {
                let value = self.eval_expr(inner)?;
                Ok(KexValue::Number(length_of(&value) as f64))
            }
            Expr::AnonFn(name, params, body) => {
                let handle = Rc::new(RefCell::new(FuncVal {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                    env: self.env.clone(),
                }));
                if let Some(frame) = handle.borrow().env.last().cloned() {
                    frame.borrow_mut().insert(name.clone(), KexValue::Func(handle.clone()));
                }
                Ok(KexValue::Func(handle))
            }
            Expr::Method(receiver, name, args) => {
                let target = self.eval_expr(receiver)?;
                let mut argv = vec![target.clone()];
                for arg in args {
                    argv.push(self.eval_expr(arg)?);
                }
                // a function stored on the object wins over the built in
                // method of the same name, which is what makes
                // `{ "double": x => x * 2 }.double(4)` work
                if let Some(stored) = self.member_value(&target, name) {
                    if matches!(stored, KexValue::Func(_) | KexValue::Builtin(_)) {
                        return self.call_value(stored, argv.split_off(1));
                    }
                }
                builtins::call_method(self, name, argv)
            }
            Expr::Call(callee, args) => {
                if let Expr::Var(name) = &**callee {
                    if name == "__kex_incr" && args.len() == 3 {
                        let delta = self.eval_expr(&args[1])?;
                        let target = match &args[0] {
                            Expr::Var(v) => AssignTarget::Name(v.clone()),
                            Expr::Index(b, i) => {
                                AssignTarget::Index((**b).clone(), (**i).clone())
                            }
                            Expr::Member(b, k) => {
                                AssignTarget::Member((**b).clone(), k.clone())
                            }
                            other => {
                                return Err(crate::error::error(format!(
                                    "cannot increment {}",
                                    other.describe()
                                )))
                            }
                        };
                        let current = self.eval_target(&target)?;
                        let updated = match current.as_number() {
                            Some(n) => KexValue::Number(n + delta.as_number().unwrap_or(1.0)),
                            None => return Err(expected("a number", &current)),
                        };
                        self.store(target.clone(), updated.clone())?;
                        return Ok(updated);
                    }
                    if name == "__kex_readline" {
                        return Ok(builtins::read_line());
                    }
                    if name == "__kex_readnumber" {
                        let text = builtins::read_line();
                        return Ok(KexValue::Number(
                            text.trim_text().parse::<f64>().unwrap_or(0.0),
                        ));
                    }
                }
                let mut argv = Vec::with_capacity(args.len());
                for arg in args {
                    argv.push(self.eval_expr(arg)?);
                }
                self.invoke(callee, argv)
            }
        }
    }

    fn eval_optional_index(&mut self, expr: &Option<Box<Expr>>) -> Result<usize, Signal> {
        match expr {
            Some(e) => {
                let value = self.eval_expr(e)?;
                Ok(value.as_index().unwrap_or(0))
            }
            None => Ok(usize::MAX),
        }
    }

    pub fn invoke(
        &mut self,
        callee: &Expr,
        args: Vec<KexValue>,
    ) -> Result<KexValue, Signal> {
        let target = self.eval_expr(callee)?;
        self.call_value(target, args)
    }

    pub fn call_value(
        &mut self,
        target: KexValue,
        args: Vec<KexValue>,
    ) -> Result<KexValue, Signal> {
        match target {
            KexValue::Builtin(name) => builtins::dispatch(self, &name, args),
            KexValue::Func(func) => {
                if self.call_depth >= self.max_depth {
                    return Err(crate::error::error(format!(
                        "maximum call depth of {} exceeded",
                        self.max_depth
                    )));
                }
                self.call_depth += 1;
                let (params, body, captured) = {
                    let borrowed = func.borrow();
                    (
                        borrowed.params.clone(),
                        borrowed.body.clone(),
                        borrowed.env.clone(),
                    )
                };
                let depth = self.env.len();
                // captured frames are shared, not copied, so the body sees and
                // can change the variables of its declaring scope
                self.env.extend(captured);
                self.env.push(new_frame());
                if let Some(frame) = self.env.last() {
                    let mut slot = frame.borrow_mut();
                    for (param, value) in params.iter().zip(args.into_iter()) {
                        slot.insert(param.clone(), value);
                    }
                }
                self.consts.push(HashMap::new());
                let result = self.exec_block(body);
                self.consts.pop();
                self.env.truncate(depth);
                self.call_depth -= 1;
                match result? {
                    Some(v) => Ok(v),
                    None => Ok(KexValue::Null),
                }
            }
            other => Err(crate::error::error(format!(
                "a {} is not callable",
                other.type_name()
            ))),
        }
    }

    pub fn loose_eq(&self, a: &KexValue, b: &KexValue) -> bool {
        match (a, b) {
            (KexValue::Number(_), KexValue::String(s)) => s.trim().parse::<f64>().is_ok(),
            (KexValue::String(s), KexValue::Number(_)) => s.trim().parse::<f64>().is_ok(),
            (KexValue::Array(x), KexValue::Array(y)) => x == y,
            _ => a == b,
        }
    }

    pub fn binary_values(
        &self,
        op: BinOp,
        l: &KexValue,
        r: &KexValue,
    ) -> Result<KexValue, Signal> {
        Ok(match op {
            BinOp::Add => match (l, r) {
                (KexValue::Number(a), KexValue::Number(b)) => KexValue::Number(a + b),
                (KexValue::String(a), KexValue::String(b)) => {
                    KexValue::String(format!("{}{}", a, b))
                }
                (KexValue::Array(a), KexValue::Array(b)) => {
                    let mut merged = a.clone();
                    merged.extend(b.clone());
                    KexValue::Array(merged)
                }
                (KexValue::Object(a), KexValue::Object(b)) => {
                    let mut merged = a.clone();
                    for (k, v) in &b.entries {
                        merged.set(k, v.clone());
                    }
                    KexValue::Object(merged)
                }
                _ => KexValue::String(format!("{}{}", l.to_display(), r.to_display())),
            },
            BinOp::Sub
            | BinOp::Mul
            | BinOp::Div
            | BinOp::Rem
            | BinOp::Pow
            | BinOp::BitAnd
            | BinOp::BitOr
            | BinOp::BitXor
            | BinOp::Shl
            | BinOp::Shr => {
                let a = l.as_number().ok_or_else(|| expected("a number", l))?;
                let b = r.as_number().ok_or_else(|| expected("a number", r))?;
                KexValue::Number(match op {
                    BinOp::Sub => a - b,
                    BinOp::Mul => a * b,
                    BinOp::Div => a / b,
                    BinOp::Rem => {
                        if b == 0.0 {
                            return Err(crate::error::error("division by zero"));
                        }
                        a % b
                    }
                    BinOp::Pow => a.powf(b),
                    BinOp::BitAnd => ((a as i64) & (b as i64)) as f64,
                    BinOp::BitOr => ((a as i64) | (b as i64)) as f64,
                    BinOp::BitXor => ((a as i64) ^ (b as i64)) as f64,
                    BinOp::Shl => ((a as i64) << (b as i64)) as f64,
                    BinOp::Shr => ((a as i64) >> (b as i64)) as f64,
                    _ => 0.0,
                })
            }
            BinOp::Eq => KexValue::Boolean(self.loose_eq(l, r)),
            BinOp::Ne => KexValue::Boolean(!self.loose_eq(l, r)),
            BinOp::Lt | BinOp::Gt | BinOp::Le | BinOp::Ge => {
                let ord = compare(l, r).ok_or_else(|| {
                    crate::error::error(format!(
                        "cannot compare {} with {}",
                        l.type_name(),
                        r.type_name()
                    ))
                })?;
                KexValue::Boolean(match op {
                    BinOp::Lt => ord == Ordering::Less,
                    BinOp::Gt => ord == Ordering::Greater,
                    BinOp::Le => ord != Ordering::Greater,
                    BinOp::Ge => ord != Ordering::Less,
                    _ => false,
                })
            }
            BinOp::And => KexValue::Boolean(l.is_truthy() && r.is_truthy()),
            BinOp::Or => KexValue::Boolean(l.is_truthy() || r.is_truthy()),
        })
    }

    // ----- modules ---------------------------------------------------------

    pub fn import_module(&mut self, path: &str, line: usize) -> Result<Option<KexValue>, Signal> {
        let resolved = match self.resolve_module(path) {
            Some(resolved) => resolved,
            None => {
                return Err(crate::error::error(format!(
                    "line {}: cannot resolve module '{}' (searched next to the script and in the pack dir)",
                    line, path
                )))
            }
        };
        if self.loaded.iter().any(|p| p == &resolved) {
            return Ok(None);
        }
        let source = fs::read_to_string(&resolved).map_err(|e| {
            crate::error::error(format!("cannot read module '{}': {}", resolved.display(), e))
        })?;

        let ast = compile(&source)?;
        self.loaded.push(resolved.clone());

        self.env.push(new_frame());
        self.consts.push(HashMap::new());
        let result = self.exec_block(ast);
        self.consts.pop();
        // an import must not swallow the rest of the importing scope, so any
        // value the module produced is deliberately dropped
        result?;
        let published: Vec<(String, KexValue)> = self
            .env
            .pop()
            .map(|frame| frame.borrow().iter().map(|(k, v)| (k.clone(), v.clone())).collect())
            .unwrap_or_default();

        // publish everything the module declared into the importing scope
        for (key, value) in published {
            if let Some(rest) = key.strip_prefix("__export_") {
                self.set_var(rest, value);
            } else if !key.starts_with("__kex") {
                self.decl_var(&key, value);
            }
        }
        Ok(None)
    }

    fn resolve_module(&self, path: &str) -> Option<PathBuf> {
        let normalised = path.replace('\\', "/");
        let stem = normalised.trim_end_matches(".kx");
        let mut candidates: Vec<PathBuf> = Vec::new();

        let has_separator = normalised.contains('/');
        let roots: Vec<PathBuf> = if has_separator {
            vec![self.script_dir.clone(), crate::pack::pack_dir()]
        } else {
            vec![
                self.script_dir.clone(),
                crate::pack::pack_dir(),
                std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            ]
        };

        for root in roots {
            candidates.push(root.join(format!("{}.kx", stem)));
            candidates.push(root.join(&normalised));
            candidates.push(root.join(format!("{}/index.kx", stem)));
            // an installed pack lives in <pack>/<stem>/ and keeps its entry
            // file under the same name
            candidates.push(root.join(format!("{}/{}.kx", stem, stem)));
        }
        if let Ok(exe_dir) = std::env::current_exe() {
            if let Some(parent) = exe_dir.parent() {
                candidates.push(parent.join(format!("{}.kx", stem)));
                candidates.push(parent.join(&normalised));
                candidates.push(parent.join(format!("{}/{}.kx", stem, stem)));
            }
        }
        candidates.into_iter().find(|c| c.is_file())
    }

}

pub fn compile(source: &str) -> LexResult<Vec<Statement>> {
    let tokens = Lexer::new(source).tokenize()?;
    Parser::new(tokens).parse()
}

/// Prefixes a runtime fault with the statement line the first time it surfaces.
fn annotate(signal: Signal, line: usize) -> Signal {
    match signal {
        Signal::Error(KexValue::Error(msg)) if !msg.starts_with("line ") => {
            Signal::Error(KexValue::Error(format!("line {}: {}", line, msg)))
        }
        other => other,
    }
}

fn compile_fragment(source: &str, line: usize) -> LexResult<Expr> {
    let tokens = Lexer::new(source).tokenize()?;
    let mut parser = Parser::new(tokens);
    let stmts = parser.parse()?;
    if stmts.len() == 1 {
        if let Statement::ExprStmt { value, .. } = &stmts[0] {
            return Ok(value.clone());
        }
    }
    let _ = line;
    Err(KexError::new(
        "Parser",
        line,
        0,
        "string interpolation expects a single expression",
    ))
}

pub fn length_of(value: &KexValue) -> usize {
    match value {
        KexValue::String(s) => s.chars().count(),
        KexValue::Array(a) => a.len(),
        KexValue::Object(o) => o.len(),
        KexValue::Range(r) => r.len(),
        _ => 0,
    }
}

pub fn cast_value(value: KexValue, target: &str) -> Result<KexValue, Signal> {
    match target {
        "num" | "number" | "int" | "float" => match &value {
            KexValue::Number(n) => Ok(KexValue::Number(*n)),
            KexValue::String(s) => match s.trim().parse::<f64>() {
                Ok(n) => Ok(KexValue::Number(n)),
                Err(_) if target == "int" => match s.trim().parse::<i64>() {
                    Ok(n) => Ok(KexValue::Number(n as f64)),
                    Err(_) => {
                        Err(crate::error::error(format!("cannot cast '{}' to int", s)))
                    }
                },
                Err(_) => Err(crate::error::error(format!("cannot cast '{}' to num", s))),
            },
            KexValue::Boolean(b) => Ok(KexValue::Number(if *b { 1.0 } else { 0.0 })),
            KexValue::Array(a) => Ok(KexValue::Number(a.len() as f64)),
            KexValue::Null => Ok(KexValue::Number(0.0)),
            other => Err(crate::error::error(format!(
                "cannot cast a {} to num",
                other.type_name()
            ))),
        },
        "str" | "string" => Ok(KexValue::String(value.to_display())),
        "bool" | "boolean" => Ok(KexValue::Boolean(value.is_truthy())),
        "arr" | "array" => Ok(KexValue::Array(match value {
            KexValue::Array(a) => a,
            KexValue::Object(o) => o.entries.into_iter().map(|(_, v)| v).collect(),
            KexValue::String(s) => s.chars().map(|c| KexValue::String(c.to_string())).collect(),
            KexValue::Range(r) => r.to_array(),
            KexValue::Null => Vec::new(),
            other => vec![other],
        })),
        "obj" | "object" => Ok(KexValue::Object(match value {
            KexValue::Object(o) => o,
            other => {
                let mut obj = KexObject::new();
                obj.set("value", other);
                obj
            }
        })),
        "any" => Ok(value),
        other => Err(crate::error::error(format!("unknown cast target '{}'", other))),
    }
}

use crate::error::{error, expected, Signal};
use crate::runtime::length_of;
use crate::value::*;
use std::io::{self, Write};

// ============================================================================
// MODULE 6: BUILTIN STANDARD LIBRARY
// ============================================================================

pub fn read_line() -> KexValue {
    let _ = io::stdout().flush();
    let mut buffer = String::new();
    match io::stdin().read_line(&mut buffer) {
        Ok(0) => KexValue::String(String::new()),
        Ok(_) => KexValue::String(buffer.trim().to_string()),
        Err(_) => KexValue::Null,
    }
}

fn arg(args: &[KexValue], index: usize) -> KexValue {
    args.get(index).cloned().unwrap_or(KexValue::Null)
}

fn need_str(args: &[KexValue], index: usize, who: &str) -> Result<String, Signal> {
    match arg(args, index) {
        KexValue::String(s) => Ok(s),
        other => Err(error(format!(
            "{}() expects a string in slot {} but received a {}",
            who,
            index + 1,
            other.type_name()
        ))),
    }
}

fn need_num(args: &[KexValue], index: usize, who: &str) -> Result<f64, Signal> {
    let value = arg(args, index);
    match &value {
        KexValue::Number(n) => Ok(*n),
        KexValue::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
        KexValue::String(s) => s
            .trim()
            .parse::<f64>()
            .map_err(|_| error(format!("{}() expects a number in slot {}", who, index + 1))),
        other => Err(error(format!(
            "{}() expects a number in slot {} but received a {}",
            who,
            index + 1,
            other.type_name()
        ))),
    }
}

fn need_array(args: &[KexValue], index: usize, who: &str) -> Result<Vec<KexValue>, Signal> {
    match arg(args, index) {
        KexValue::Array(items) => Ok(items),
        other => Err(error(format!(
            "{}() expects an array in slot {} but received a {}",
            who,
            index + 1,
            other.type_name()
        ))),
    }
}

fn need_object(
    args: &[KexValue],
    index: usize,
    who: &str,
) -> Result<KexObject, Signal> {
    match arg(args, index) {
        KexValue::Object(obj) => Ok(obj),
        other => Err(error(format!(
            "{}() expects an object in slot {} but received a {}",
            who,
            index + 1,
            other.type_name()
        ))),
    }
}

fn optional_index(args: &[KexValue], index: usize, fallback: usize) -> usize {
    match args.get(index) {
        Some(KexValue::Number(n)) => {
            if *n < 0.0 {
                fallback
            } else {
                *n as usize
            }
        }
        _ => fallback,
    }
}

// ----- string helpers ------------------------------------------------------

fn to_char_vec(text: &str) -> Vec<char> {
    text.chars().collect()
}

fn from_char_vec(chars: Vec<char>) -> String {
    chars.into_iter().collect()
}

fn string_index(len: usize, raw: &KexValue, who: &str) -> Result<usize, Signal> {
    let n = raw
        .as_number()
        .ok_or_else(|| error(format!("{}() expects a numeric index", who)))?;
    if n < 0.0 || n as usize >= len {
        return Err(error(format!(
            "{}(): index {} is out of bounds",
            who,
            n as i64
        )));
    }
    Ok(n as usize)
}

// ----- registry ------------------------------------------------------------

pub fn lookup(name: &str) -> Option<&'static str> {
    BUILTINS
        .iter()
        .find(|(n, _, _)| *n == name)
        .map(|(n, _, _)| *n)
}

type BuiltinImpl = fn(&mut crate::runtime::Runtime, Vec<KexValue>) -> Result<KexValue, Signal>;

pub static BUILTINS: &[(&str, &str, BuiltinImpl)] = &[
    // ---- output / input
    ("print", "write a value to stdout", |_rt, a| {
        let _ = writeln!(io::stdout(), "{}", arg(&a, 0).to_display());
        Ok(KexValue::Null)
    }),
    ("eprint", "write a value to stderr", |_rt, a| {
        let _ = writeln!(io::stderr(), "{}", arg(&a, 0).to_display());
        Ok(KexValue::Null)
    }),
    ("write", "write a value to stdout without a newline", |_rt, a| {
        let mut out = io::stdout();
        let _ = out.write_all(arg(&a, 0).to_display().as_bytes());
        let _ = out.flush();
        Ok(KexValue::Null)
    }),
    ("input", "read one line from stdin", |_rt, a| {
        if let Some(prompt) = a.first() {
            let mut out = io::stdout();
            let _ = out.write_all(prompt.to_display().as_bytes());
            let _ = out.flush();
        }
        Ok(read_line())
    }),
    ("readline", "read one line from stdin", |_rt, _a| Ok(read_line())),
    ("readnumber", "read one line from stdin and parse it as a number", |_rt, _a| {
        Ok(KexValue::Number(
            read_line().to_display().trim().parse().unwrap_or(0.0),
        ))
    }),
    ("clear", "clear the terminal", |_rt, _a| {
        let mut out = io::stdout();
        let _ = out.write_all(b"\x1b[2J\x1b[H");
        let _ = out.flush();
        Ok(KexValue::Null)
    }),
    ("inspect", "return a quoted, recursive representation", |_rt, a| {
        Ok(KexValue::String(arg(&a, 0).inspect()))
    }),
    ("assert", "throw when the condition is falsy", |_rt, a| {
        if arg(&a, 0).is_truthy() {
            Ok(KexValue::Null)
        } else {
            let msg = a
                .get(1)
                .map(|v| v.to_display())
                .unwrap_or_else(|| "assertion failed".to_string());
            Err(error(msg))
        }
    }),
    ("error", "throw a catchable error", |_rt, a| {
        let value = arg(&a, 0);
        match value {
            KexValue::Null => Err(error("error() requires a message")),
            KexValue::String(s) => Err(error(s)),
            other => Err(Signal::Error(other)),
        }
    }),
    ("exit", "terminate the script with an exit code", |_rt, a| {
        let code = match a.first() {
            Some(v) => v.as_number().unwrap_or(0.0) as i32,
            None => 0,
        };
        Err(Signal::Exit(code))
    }),
    // ---- type helpers
    ("type_of", "return the runtime type name of a value", |_rt, a| {
        Ok(KexValue::string(arg(&a, 0).type_name()))
    }),
    ("is_null", "true when the value is null", |_rt, a| {
        Ok(KexValue::Boolean(matches!(arg(&a, 0), KexValue::Null)))
    }),
    ("to_str", "convert any value to a string", |_rt, a| {
        Ok(KexValue::string(arg(&a, 0).to_display()))
    }),
    ("to_num", "convert any value to a number", |_rt, a| {
        match arg(&a, 0) {
            KexValue::Number(n) => Ok(KexValue::Number(n)),
            KexValue::Boolean(b) => Ok(KexValue::Number(if b { 1.0 } else { 0.0 })),
            KexValue::String(s) => s
                .trim()
                .parse::<f64>()
                .map(KexValue::Number)
                .map_err(|_| error(format!("cannot convert '{}' to a number", s))),
            other => Err(expected("a number", &other)),
        }
    }),
    ("to_int", "truncate a number to an integer", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "to_int")?.trunc()))
    }),
    ("to_bool", "convert any value to a boolean", |_rt, a| {
        Ok(KexValue::Boolean(arg(&a, 0).is_truthy()))
    }),
    ("to_array", "coerce a value into an array", |rt, a| {
        Ok(KexValue::Array(rt.iterate(&arg(&a, 0))?))
    }),
    ("deep_equal", "structural equality check", |_rt, a| {
        Ok(KexValue::Boolean(arg(&a, 0) == arg(&a, 1)))
    }),
    ("clone", "copy a value", |_rt, a| Ok(arg(&a, 0))),
    ("length", "length of a string, array, object or range", |_rt, a| {
        Ok(KexValue::Number(length_of(&arg(&a, 0)) as f64))
    }),
    // ---- strings
    ("upper", "uppercase a string", |_rt, a| {
        Ok(KexValue::string(need_str(&a, 0, "upper")?.to_uppercase()))
    }),
    ("lower", "lowercase a string", |_rt, a| {
        Ok(KexValue::string(need_str(&a, 0, "lower")?.to_lowercase()))
    }),
    ("trim", "strip whitespace from both ends", |_rt, a| {
        Ok(KexValue::string(need_str(&a, 0, "trim")?.trim().to_string()))
    }),
    ("split", "split a string on a separator", |_rt, a| {
        let text = need_str(&a, 0, "split")?;
        let sep = need_str(&a, 1, "split")?;
        let parts: Vec<KexValue> = if sep.is_empty() {
            text.chars().map(|c| KexValue::string(c.to_string())).collect()
        } else {
            text.split(sep.as_str()).map(KexValue::string).collect()
        };
        Ok(KexValue::Array(parts))
    }),
    ("join", "join a list of values into a string", |_rt, a| {
        let items = need_array(&a, 0, "join")?;
        let sep = a
            .get(1)
            .map(|v| v.to_display())
            .unwrap_or_else(String::new);
        let rendered: Vec<String> = items.iter().map(|v| v.to_display()).collect();
        Ok(KexValue::string(rendered.join(&sep)))
    }),
    ("replace", "replace every occurrence of a substring", |_rt, a| {
        let text = need_str(&a, 0, "replace")?;
        let from = need_str(&a, 1, "replace")?;
        let to = need_str(&a, 2, "replace")?;
        Ok(KexValue::string(text.replace(from.as_str(), to.as_str())))
    }),
    ("contains", "substring or membership test", |_rt, a| match arg(&a, 0) {
        KexValue::String(text) => Ok(KexValue::Boolean(
            text.contains(&need_str(&a, 1, "contains")?),
        )),
        KexValue::Array(items) => Ok(KexValue::Boolean(items.contains(&arg(&a, 1)))),
        KexValue::Object(obj) => Ok(KexValue::Boolean(obj.get(&need_str(&a, 1, "contains")?).is_some())),
        _ => Ok(KexValue::Boolean(false)),
    }),
    ("starts_with", "prefix test", |_rt, a| {
        Ok(KexValue::Boolean(
            need_str(&a, 0, "starts_with")?.starts_with(&need_str(&a, 1, "starts_with")?),
        ))
    }),
    ("ends_with", "suffix test", |_rt, a| {
        Ok(KexValue::Boolean(
            need_str(&a, 0, "ends_with")?.ends_with(&need_str(&a, 1, "ends_with")?),
        ))
    }),
    ("repeat", "repeat a string n times", |_rt, a| {
        let n = need_num(&a, 1, "repeat")?;
        if n < 0.0 {
            return Err(error("repeat() needs a non negative count"));
        }
        Ok(KexValue::string(
            need_str(&a, 0, "repeat")?.repeat(n as usize),
        ))
    }),
    ("index_of", "position of a substring or item", |_rt, a| {
        let needle = arg(&a, 1);
        match arg(&a, 0) {
            KexValue::String(text) => {
                let sub = needle.to_display();
                Ok(KexValue::Number(
                    text.find(sub.as_str())
                        .map(|byte| text[..byte].chars().count() as f64)
                        .unwrap_or(-1.0),
                ))
            }
            KexValue::Array(items) => Ok(KexValue::Number(
                items
                    .iter()
                    .position(|v| *v == needle)
                    .map(|i| i as f64)
                    .unwrap_or(-1.0),
            )),
            KexValue::Object(obj) => {
                let key = needle.to_display();
                Ok(KexValue::Number(
                    obj.keys()
                        .iter()
                        .position(|k| *k == key)
                        .map(|i| i as f64)
                        .unwrap_or(-1.0),
                ))
            }
            other => Err(expected("a string or array", &other)),
        }
    }),
    ("slice", "substring or sublist", |_rt, a| {
        let len = length_of(&arg(&a, 0));
        let from = optional_index(&a, 1, 0).min(len);
        let to = match a.get(2) {
            Some(v) => match v.as_index() {
                Some(i) => i.min(len),
                None => len,
            },
            None => len,
        };
        if from > to {
            return Ok(KexValue::Array(Vec::new()));
        }
        match arg(&a, 0) {
            KexValue::String(text) => {
                let chars = to_char_vec(&text);
                Ok(KexValue::string(from_char_vec(chars[from..to].to_vec())))
            }
            KexValue::Array(items) => Ok(KexValue::Array(items[from..to].to_vec())),
            KexValue::Range(r) => {
                let all = r.to_array();
                Ok(KexValue::Array(all[from..to].to_vec()))
            }
            KexValue::Object(obj) => Ok(KexValue::Object(KexObject {
                entries: obj.entries[from..to].to_vec(),
            })),
            other => Err(expected("a string, array or object", &other)),
        }
    }),
    ("char_at", "character at an index", |_rt, a| {
        let chars = to_char_vec(&need_str(&a, 0, "char_at")?);
        let i = string_index(chars.len(), &arg(&a, 1), "char_at")?;
        Ok(chars
            .get(i)
            .map(|c| KexValue::string(c.to_string()))
            .unwrap_or(KexValue::String(String::new())))
    }),
    ("chars", "split a string into an array of characters", |_rt, a| {
        let text = need_str(&a, 0, "chars")?;
        Ok(KexValue::Array(
            text.chars().map(|c| KexValue::string(c.to_string())).collect(),
        ))
    }),
    ("reverse_str", "reverse a string", |_rt, a| {
        let mut chars = to_char_vec(&need_str(&a, 0, "reverse_str")?);
        chars.reverse();
        Ok(KexValue::string(from_char_vec(chars)))
    }),
    ("pad_start", "left pad a string", |_rt, a| {
        let text = need_str(&a, 0, "pad_start")?;
        let width = need_num(&a, 1, "pad_start")? as usize;
        let pad = a
            .get(2)
            .map(|v| v.to_display())
            .unwrap_or_else(|| " ".to_string());
        let pad = if pad.is_empty() { " ".to_string() } else { pad };
        let mut filler = String::new();
        while text.chars().count() + filler.chars().count() < width {
            filler.push_str(&pad);
        }
        let filler: String = filler.chars().take(width.saturating_sub(text.chars().count())).collect();
        Ok(KexValue::string(format!("{}{}", filler, text)))
    }),
    ("pad_end", "right pad a string", |_rt, a| {
        let text = need_str(&a, 0, "pad_end")?;
        let width = need_num(&a, 1, "pad_end")? as usize;
        let pad = a
            .get(2)
            .map(|v| v.to_display())
            .unwrap_or_else(|| " ".to_string());
        let pad = if pad.is_empty() { " ".to_string() } else { pad };
        let mut out = text.clone();
        let mut count = text.chars().count();
        while count < width {
            out.push_str(&pad);
            count += pad.chars().count();
        }
        Ok(KexValue::string(out))
    }),
    ("parse_num", "parse a numeric string", |_rt, a| {
        let text = need_str(&a, 0, "parse_num")?;
        text.trim()
            .parse::<f64>()
            .map(KexValue::Number)
            .map_err(|_| error(format!("'{}' is not a number", text)))
    }),
    ("format", "format values into a template", |_rt, a| {
        let template = need_str(&a, 0, "format")?;
        let mut out = String::new();
        let chars: Vec<char> = template.chars().collect();
        let mut cursor = 0usize;
        let mut next_arg = 1usize;
        while cursor < chars.len() {
            if chars[cursor] == '{' {
                if let Some(offset) = chars[cursor..].iter().position(|c| *c == '}') {
                    let field: String = chars[cursor + 1..cursor + offset].iter().collect();
                    let value = arg(&a, next_arg);
                    next_arg += 1;
                    out.push_str(&match field.as_str() {
                        "" => value.to_display(),
                        "d" => match value.as_number() {
                            Some(n) => format!("{}", n as i64),
                            None => value.to_display(),
                        },
                        "x" => format!("{:x}", value.as_number().unwrap_or(0.0) as i64),
                        "X" => format!("{:X}", value.as_number().unwrap_or(0.0) as i64),
                        "f" => format!("{:.2}", value.as_number().unwrap_or(0.0)),
                        "p" => format!("{:.4}", value.as_number().unwrap_or(0.0)),
                        "q" => value.inspect(),
                        "k" => value.type_name().to_string(),
                        other => format!("{{{}:{}}}", other, value.to_display()),
                    });
                    cursor += offset + 1;
                    continue;
                }
            }
            out.push(chars[cursor]);
            cursor += 1;
        }
        Ok(KexValue::string(out))
    }),
    ("is_empty", "true when a string, array or object has no items", |_rt, a| {
        Ok(KexValue::Boolean(length_of(&arg(&a, 0)) == 0))
    }),
    // ---- arrays
    ("push", "append to an array", |_rt, a| {
        let mut items = need_array(&a, 0, "push")?;
        items.push(arg(&a, 1));
        Ok(KexValue::Array(items))
    }),
    ("pop", "remove and return the last item", |_rt, a| {
        let mut items = need_array(&a, 0, "pop")?;
        Ok(items.pop().unwrap_or(KexValue::Null))
    }),
    ("insert", "insert at an index", |_rt, a| {
        let mut items = need_array(&a, 0, "insert")?;
        let i = need_num(&a, 1, "insert")?.max(0.0) as usize;
        let i = i.min(items.len());
        items.insert(i, arg(&a, 2));
        Ok(KexValue::Array(items))
    }),
    ("remove_at", "remove the item at an index", |_rt, a| {
        let mut items = need_array(&a, 0, "remove_at")?;
        let i = need_num(&a, 1, "remove_at")?.max(0.0) as usize;
        if i >= items.len() {
            return Err(error(format!("remove_at(): index {} is out of bounds", i)));
        }
        Ok(items.remove(i))
    }),
    ("clear_array", "empty an array", |_rt, a| {
        need_array(&a, 0, "clear_array")?;
        Ok(KexValue::Array(Vec::new()))
    }),
    ("reverse", "reverse an array or string", |_rt, a| match arg(&a, 0) {
        KexValue::String(text) => {
            let mut chars = to_char_vec(&text);
            chars.reverse();
            Ok(KexValue::string(from_char_vec(chars)))
        }
        KexValue::Array(mut items) => {
            items.reverse();
            Ok(KexValue::Array(items))
        }
        other => Err(expected("an array or string", &other)),
    }),
    ("sort", "sort an array ascending", |_rt, a| {
        let mut items = need_array(&a, 0, "sort")?;
        items.sort_by(|x, y| compare(x, y).unwrap_or(std::cmp::Ordering::Equal));
        Ok(KexValue::Array(items))
    }),
    ("sum", "add every item of an array", |_rt, a| {
        let items = need_array(&a, 0, "sum")?;
        let mut total = 0.0;
        for item in &items {
            total += item.as_number().ok_or_else(|| {
                error(format!("sum() found a {} inside the array", item.type_name()))
            })?;
        }
        Ok(KexValue::Number(total))
    }),
    ("flatten", "flatten one level of nesting", |_rt, a| {
        let items = need_array(&a, 0, "flatten")?;
        let mut out = Vec::new();
        for item in items {
            match item {
                KexValue::Array(inner) => out.extend(inner),
                other => out.push(other),
            }
        }
        Ok(KexValue::Array(out))
    }),
    ("unique", "remove duplicate items", |_rt, a| {
        let items = need_array(&a, 0, "unique")?;
        let mut out: Vec<KexValue> = Vec::new();
        for item in items {
            if !out.contains(&item) {
                out.push(item);
            }
        }
        Ok(KexValue::Array(out))
    }),
    ("find", "first item satisfying a predicate function", |rt, a| {
        let items = need_array(a.as_slice(), 0, "find")?;
        let predicate = arg(&a, 1);
        for item in items {
            let verdict = rt.call_value(predicate.clone(), vec![item.clone()])?;
            if verdict.is_truthy() {
                return Ok(item);
            }
        }
        Ok(KexValue::Null)
    }),
    ("map", "transform every item with a function", |rt, a| {
        let items = need_array(a.as_slice(), 0, "map")?;
        let func = arg(&a, 1);
        let mut out = Vec::with_capacity(items.len());
        for item in items {
            out.push(rt.call_value(func.clone(), vec![item])?);
        }
        Ok(KexValue::Array(out))
    }),
    ("filter", "keep items satisfying a predicate", |rt, a| {
        let items = need_array(a.as_slice(), 0, "filter")?;
        let func = arg(&a, 1);
        let mut out = Vec::new();
        for item in items {
            if rt.call_value(func.clone(), vec![item.clone()])?.is_truthy() {
                out.push(item);
            }
        }
        Ok(KexValue::Array(out))
    }),
    ("reduce", "fold an array into a single value", |rt, a| {
        let items = need_array(a.as_slice(), 0, "reduce")?;
        let func = arg(&a, 1);
        let mut acc = a.get(2).cloned().unwrap_or(KexValue::Null);
        for item in items {
            acc = rt.call_value(func.clone(), vec![acc, item])?;
        }
        Ok(acc)
    }),
    // ---- objects
    ("keys", "object keys as an array", |_rt, a| {
        let obj = need_object(&a, 0, "keys")?;
        Ok(KexValue::Array(obj.keys().into_iter().map(KexValue::String).collect()))
    }),
    ("values", "object values as an array", |_rt, a| {
        let obj = need_object(&a, 0, "values")?;
        Ok(KexValue::Array(obj.entries.iter().map(|(_, v)| v.clone()).collect()))
    }),
    ("has_key", "membership test for object keys", |_rt, a| {
        let obj = need_object(&a, 0, "has_key")?;
        Ok(KexValue::Boolean(obj.get(&need_str(&a, 1, "has_key")?).is_some()))
    }),
    ("remove_key", "delete a key from an object", |_rt, a| {
        let mut obj = need_object(&a, 0, "remove_key")?;
        let removed = obj.remove(&need_str(&a, 1, "remove_key")?);
        Ok(removed.unwrap_or(KexValue::Null))
    }),
    ("merge", "merge two objects into a new one", |_rt, a| {
        let mut merged = need_object(&a, 0, "merge")?;
        for (k, v) in need_object(&a, 1, "merge")?.entries {
            merged.set(&k, v);
        }
        Ok(KexValue::Object(merged))
    }),
    ("object", "build an object from key/value pairs", |_rt, a| {
        let mut obj = KexObject::new();
        let mut i = 0;
        while i + 1 < a.len() {
            obj.set(&a[i].to_display(), a[i + 1].clone());
            i += 2;
        }
        Ok(KexValue::Object(obj))
    }),
    // ---- math
    ("abs", "absolute value", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "abs")?.abs()))
    }),
    ("floor", "round down", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "floor")?.floor()))
    }),
    ("ceil", "round up", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "ceil")?.ceil()))
    }),
    ("round", "round to the nearest integer", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "round")?.round()))
    }),
    ("sqrt", "square root", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "sqrt")?.sqrt()))
    }),
    ("pow", "raise a number to a power", |_rt, a| {
        Ok(KexValue::Number(
            need_num(&a, 0, "pow")?.powf(need_num(&a, 1, "pow")?),
        ))
    }),
    ("exp", "e raised to a power", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "exp")?.exp()))
    }),
    ("log", "natural logarithm", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "log")?.ln()))
    }),
    ("log2", "base 2 logarithm", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "log2")?.log2()))
    }),
    ("log10", "base 10 logarithm", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "log10")?.log10()))
    }),
    ("sin", "sine", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "sin")?.sin()))),
    ("cos", "cosine", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "cos")?.cos()))),
    ("tan", "tangent", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "tan")?.tan()))),
    ("atan2", "two argument arctangent", |_rt, a| {
        Ok(KexValue::Number(
            need_num(&a, 0, "atan2")?.atan2(need_num(&a, 1, "atan2")?),
        ))
    }),
    ("pi", "the constant pi", |_rt, _a| Ok(KexValue::Number(std::f64::consts::PI))),
    ("e", "the constant e", |_rt, _a| Ok(KexValue::Number(std::f64::consts::E))),
    ("min", "smallest of the arguments", |_rt, a| {
        if a.is_empty() {
            return Ok(KexValue::Null);
        }
        let mut best = f64::INFINITY;
        for i in 0..a.len() {
            let n = need_num(a.as_slice(), i, "min")?;
            if n < best {
                best = n;
            }
        }
        Ok(KexValue::Number(best))
    }),
    ("max", "largest of the arguments", |_rt, a| {
        if a.is_empty() {
            return Ok(KexValue::Null);
        }
        let mut best = f64::NEG_INFINITY;
        for i in 0..a.len() {
            let n = need_num(a.as_slice(), i, "max")?;
            if n > best {
                best = n;
            }
        }
        Ok(KexValue::Number(best))
    }),
    ("clamp", "constrain a value between two bounds", |_rt, a| {
        let v = need_num(&a, 0, "clamp")?;
        let lo = need_num(&a, 1, "clamp")?;
        let hi = need_num(&a, 2, "clamp")?;
        Ok(KexValue::Number(v.max(lo).min(hi)))
    }),
    ("sign", "-1, 0 or 1", |_rt, a| {
        let v = need_num(&a, 0, "sign")?;
        Ok(KexValue::Number(if v > 0.0 {
            1.0
        } else if v < 0.0 {
            -1.0
        } else {
            0.0
        }))
    }),
    ("random", "random float, or an integer inside a range", |_rt, a| {
        if a.is_empty() {
            return Ok(KexValue::Number(pseudo_random()));
        }
        if a.len() == 1 {
            let hi = need_num(&a, 0, "random")? as i64;
            if hi <= 0 {
                return Ok(KexValue::Number(0.0));
            }
            return Ok(KexValue::Number((pseudo_random() * hi as f64) as i64 as f64));
        }
        let lo = need_num(&a, 0, "random")? as i64;
        let hi = need_num(&a, 1, "random")? as i64;
        if hi <= lo {
            return Ok(KexValue::Number(lo as f64));
        }
        Ok(KexValue::Number(
            (lo + (pseudo_random() * ((hi - lo) as f64)) as i64) as f64,
        ))
    }),
    ("range", "build a range value", |_rt, a| {
        let start = need_num(&a, 0, "range")?;
        let end = need_num(&a, 1, "range")?;
        let step = a.get(2).and_then(|v| v.as_number()).unwrap_or(1.0);
        let inclusive = a
            .get(3)
            .map(|v| v.is_truthy())
            .unwrap_or(false);
        if step == 0.0 {
            return Err(error("range() needs a non zero step"));
        }
        Ok(KexValue::Range(RangeVal {
            start,
            end,
            step,
            inclusive,
        }))
    }),
    ("now", "milliseconds since the unix epoch", |_rt, _a| {
        Ok(KexValue::Number(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as f64)
                .unwrap_or(0.0),
        ))
    }),
    // ---- host bridge
    ("host", "call a kexco host function", |rt, a| {
        let name = match a.first() {
            Some(KexValue::String(s)) => s.clone(),
            Some(other) => return Err(expected("a host function name", other)),
            None => return Err(error("host() requires a function name")),
        };
        match &mut rt.host {
            Some(bridge) => bridge.call(&name, &a[1..]).map_err(error),
            None => Err(error(format!(
                "no host bridge is attached; run 'kex pack install kexco' first"
            ))),
        }
    }),
    ("host_ready", "true when a host bridge is available", |rt, _a| {
        Ok(KexValue::Boolean(rt.host.is_some()))
    }),
    ("host_env", "name of the detected host runtime", |rt, _a| {
        Ok(KexValue::string(
            rt.host.as_ref().map(|h| h.runtime_name()).unwrap_or("none"),
        ))
    }),
];

/// Deterministic-enough PRNG so `random()` works without extra dependencies.
fn pseudo_random() -> f64 {
    use std::cell::Cell;
    use std::time::{SystemTime, UNIX_EPOCH};
    thread_local! {
        static SEED: Cell<u64> = Cell::new(0);
    }
    SEED.with(|seed| {
        let mut s = seed.get();
        if s == 0 {
            s = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_nanos() as u64)
                .unwrap_or(0x2545F4914F6CDD1D)
                | 1;
        }
        s ^= s << 13;
        s ^= s >> 7;
        s ^= s << 17;
        seed.set(s);
        (s >> 11) as f64 / (1u64 << 53) as f64
    })
}

pub fn dispatch(
    rt: &mut crate::runtime::Runtime,
    name: &str,
    args: Vec<KexValue>,
) -> Result<KexValue, Signal> {
    match BUILTINS.iter().find(|(n, _, _)| *n == name) {
        Some((_, _, f)) => f(rt, args),
        None => Err(error(format!("'{}' is not a known builtin", name))),
    }
}

/// Method call sugar: `"ab".upper()`, `[1].push(2)`, `{...}.keys()`.
const STRING_METHODS: &[(&str, &str)] = &[
    ("upper", "upper"),
    ("lower", "lower"),
    ("trim", "trim"),
    ("split", "split"),
    ("replace", "replace"),
    ("contains", "contains"),
    ("starts_with", "starts_with"),
    ("startsWith", "starts_with"),
    ("ends_with", "ends_with"),
    ("endsWith", "ends_with"),
    ("repeat", "repeat"),
    ("index_of", "index_of"),
    ("indexOf", "index_of"),
    ("slice", "slice"),
    ("char_at", "char_at"),
    ("charAt", "char_at"),
    ("chars", "chars"),
    ("pad_start", "pad_start"),
    ("padStart", "pad_start"),
    ("pad_end", "pad_end"),
    ("padEnd", "pad_end"),
    ("to_num", "to_num"),
    ("toNum", "to_num"),
    ("to_str", "to_str"),
    ("toStr", "to_str"),
    ("parse_num", "parse_num"),
    ("parseNum", "parse_num"),
    ("reverse", "reverse"),
    ("format", "format"),
    ("len", "length"),
    ("length", "length"),
    ("is_empty", "is_empty"),
    ("empty", "is_empty"),
];

const ARRAY_METHODS: &[(&str, &str)] = &[
    ("push", "push"),
    ("pop", "pop"),
    ("insert", "insert"),
    ("remove", "remove_at"),
    ("index_of", "index_of"),
    ("indexOf", "index_of"),
    ("contains", "contains"),
    ("slice", "slice"),
    ("join", "join"),
    ("len", "length"),
    ("length", "length"),
    ("sort", "sort"),
    ("reverse", "reverse"),
    ("sum", "sum"),
    ("flatten", "flatten"),
    ("unique", "unique"),
    ("map", "map"),
    ("filter", "filter"),
    ("find", "find"),
    ("reduce", "reduce"),
    ("clear", "clear_array"),
    ("clone", "clone"),
];

const OBJECT_METHODS: &[(&str, &str)] = &[
    ("keys", "keys"),
    ("values", "values"),
    ("has_key", "has_key"),
    ("hasKey", "has_key"),
    ("remove_key", "remove_key"),
    ("removeKey", "remove_key"),
    ("merge", "merge"),
    ("len", "length"),
    ("length", "length"),
    ("is_empty", "is_empty"),
    ("empty", "is_empty"),
];

pub fn call_method(
    rt: &mut crate::runtime::Runtime,
    method: &str,
    args: Vec<KexValue>,
) -> Result<KexValue, Signal> {
    let receiver = args.first().cloned().unwrap_or(KexValue::Null);
    let table: &[(&str, &str)] = match &receiver {
        KexValue::String(_) => STRING_METHODS,
        KexValue::Array(_) => ARRAY_METHODS,
        KexValue::Object(_) => OBJECT_METHODS,
        KexValue::Range(_) => ARRAY_METHODS,
        _ => &[],
    };

    // `obj.get(key)` is special cased because it takes an explicit key.
    if let KexValue::Object(obj) = &receiver {
        if method == "get" {
            let key = arg(&args, 1).to_display();
            return Ok(obj.get(&key).cloned().unwrap_or(KexValue::Null));
        }
    }

    if let Some((_, builtin)) = table.iter().find(|(m, _)| *m == method) {
        // `clone` is answered directly and `join` already takes the receiver
        // as slot 0, so both need no re-dispatch tweak.
        if *builtin == "clone" {
            return Ok(receiver);
        }
        return dispatch(rt, builtin, args);
    }

    Err(error(format!(
        "'{}' has no method '{}' (a {} does not provide it)",
        receiver.to_display(),
        method,
        receiver.type_name()
    )))
}

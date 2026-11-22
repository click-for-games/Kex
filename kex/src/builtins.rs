use crate::error::{error, expected, Signal};
use crate::json::{self, Json};
use crate::runtime::length_of;
use crate::value::*;
use std::fs;
use std::io::{self, Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

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

// ----- library helpers -----------------------------------------------------

fn member_at(value: &KexValue, key: &str) -> KexValue {
    match value {
        KexValue::Object(obj) => obj.get(key).cloned().unwrap_or(KexValue::Null),
        KexValue::Array(items) => key
            .parse::<usize>()
            .ok()
            .and_then(|i| items.get(i).cloned())
            .unwrap_or(KexValue::Null),
        _ => KexValue::Null,
    }
}

fn lookup_path(root: &KexValue, path: &str) -> KexValue {
    if path.is_empty() {
        return root.clone();
    }
    let mut current = root.clone();
    for part in path.split('.').filter(|p| !p.is_empty()) {
        current = member_at(&current, part);
    }
    current
}

fn set_path(root: &mut KexValue, parts: &[&str], value: KexValue) {
    if parts.is_empty() {
        *root = value;
        return;
    }
    match root {
        KexValue::Object(obj) => {
            if parts.len() == 1 {
                obj.set(parts[0], value);
            } else {
                let mut child = obj.get(parts[0]).cloned().unwrap_or(KexValue::Null);
                set_path(&mut child, &parts[1..], value);
                obj.set(parts[0], child);
            }
        }
        KexValue::Array(items) => {
            if let Ok(index) = parts[0].parse::<usize>() {
                while items.len() <= index {
                    items.push(KexValue::Null);
                }
                set_path(&mut items[index], &parts[1..], value);
            }
        }
        _ => {}
    }
}

fn path_string(args: &[KexValue], index: usize, who: &str) -> Result<String, Signal> {
    let value = arg(args, index);
    match value {
        KexValue::String(text) => Ok(replace_home(&text)),
        other => Err(error(format!(
            "{}() expects a path string but received a {}",
            who,
            other.type_name()
        ))),
    }
}

fn replace_home(text: &str) -> String {
    if let Some(rest) = text.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME")) {
            return Path::new(&home).join(rest).to_string_lossy().to_string();
        }
    }
    text.to_string()
}

const BASE64_ALPHABET: &[u8; 64] =
    b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn base64_encode(data: &[u8]) -> String {
    let mut out = String::new();
    for chunk in data.chunks(3) {
        let a = chunk[0] as u32;
        let b = chunk.get(1).copied().unwrap_or(0) as u32;
        let c = chunk.get(2).copied().unwrap_or(0) as u32;
        let bits = (a << 16) | (b << 8) | c;
        out.push(BASE64_ALPHABET[((bits >> 18) & 63) as usize] as char);
        out.push(BASE64_ALPHABET[((bits >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            BASE64_ALPHABET[((bits >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            BASE64_ALPHABET[(bits & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}

fn base64_decode(text: &str) -> Result<Vec<u8>, Signal> {
    let clean: Vec<u8> = text
        .bytes()
        .filter(|b| !b.is_ascii_whitespace() && *b != b'=')
        .collect();
    if clean.len() % 4 == 1 {
        return Err(error("base64_decode() received malformed input"));
    }
    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        let mut bits = 0u32;
        for byte in chunk {
            let value = BASE64_ALPHABET.iter().position(|c| *c == *byte).ok_or_else(|| {
                error("base64_decode() received an invalid character")
            })? as u32;
            bits = (bits << 6) | value;
        }
        let used = chunk.len();
        bits <<= (4 - used) * 6;
        out.push((bits >> 16) as u8);
        if used > 2 {
            out.push((bits >> 8) as u8);
        }
        if used > 3 {
            out.push(bits as u8);
        }
    }
    Ok(out)
}

fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    (year + if month <= 2 { 1 } else { 0 }, month, day)
}

struct DateParts {
    year: i64,
    month: i64,
    day: i64,
    hour: i64,
    minute: i64,
    second: i64,
    millis: i64,
    weekday: i64,
    epoch: f64,
}

fn date_parts(epoch_ms: f64) -> DateParts {
    let total = epoch_ms as i64;
    let seconds = total.div_euclid(1000);
    let millis = total.rem_euclid(1000);
    let days = seconds.div_euclid(86400);
    let time = seconds.rem_euclid(86400);
    let (year, month, day) = civil_from_days(days);
    DateParts {
        year,
        month,
        day,
        hour: time / 3600,
        minute: (time % 3600) / 60,
        second: time % 60,
        millis,
        weekday: days.rem_euclid(7),
        epoch: epoch_ms,
    }
}

fn iso_date(parts: &DateParts) -> String {
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        parts.year,
        parts.month,
        parts.day,
        parts.hour,
        parts.minute,
        parts.second,
        parts.millis
    )
}

fn words_of(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|part| !part.is_empty())
        .map(|part| part.to_string())
        .collect()
}

static UUID_COUNTER: AtomicU64 = AtomicU64::new(0);

fn make_uuid() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0x9E3779B97F4A7C15);
    let counter = UUID_COUNTER.fetch_add(1, Ordering::Relaxed);
    let random = (pseudo_random() * u64::MAX as f64) as u64;
    let bytes = nanos
        .to_be_bytes()
        .into_iter()
        .chain((random ^ counter.wrapping_mul(0x9E3779B97F4A7C15)).to_be_bytes())
        .collect::<Vec<u8>>();
    let mut hex: Vec<u8> = bytes.iter().flat_map(|b| [b / 16, b % 16]).collect();
    for (i, nibble) in hex.iter_mut().enumerate() {
        *nibble = match *nibble {
            0..=9 => b'0' + *nibble,
            _ => b'a' + (*nibble - 10),
        };
        let _ = i;
    }
    hex[12] = b'4';
    hex[16] = match hex[16] {
        b'0'..=b'3' => b'8',
        b'4'..=b'7' => b'9',
        b'8'..=b'b' => b'a',
        _ => b'b',
    };
    let text = String::from_utf8(hex).unwrap_or_default();
    format!(
        "{}-{}-{}-{}-{}",
        &text[0..8],
        &text[8..12],
        &text[12..16],
        &text[16..20],
        &text[20..32]
    )
}

fn fnv1a64(text: &str) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in text.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Shared implementation for `format()` and `{value:spec}` interpolation.
pub fn format_spec(value: &KexValue, spec: &str) -> String {
    let spec = spec.trim();
    let spec = spec.strip_prefix(':').unwrap_or(spec);
    let number = value.as_number();
    let mut out = match spec {
        "" => value.to_display(),
        "d" | "i" => match number {
            Some(n) => format!("{}", n as i64),
            None => value.to_display(),
        },
        "x" => format!("{:x}", number.unwrap_or(0.0) as i64),
        "X" => format!("{:X}", number.unwrap_or(0.0) as i64),
        "b" => format!("{:b}", number.unwrap_or(0.0) as i64),
        "o" => format!("{:o}", number.unwrap_or(0.0) as i64),
        "f" => format!("{:.2}", number.unwrap_or(0.0)),
        "e" => format!("{:e}", number.unwrap_or(0.0)),
        "E" => format!("{:E}", number.unwrap_or(0.0)),
        "p" => format!("{:.4}", number.unwrap_or(0.0)),
        "q" => value.inspect(),
        "k" => value.type_name().to_string(),
        "s" => value.to_display(),
        "c" => match number {
            Some(n) => char::from_u32(n as u32).map(|c| c.to_string()).unwrap_or_default(),
            None => value.to_display(),
        },
        other => {
            let precision = other
                .strip_prefix('f')
                .or_else(|| other.strip_prefix('.'))
                .and_then(|rest| rest.trim_end_matches('f').parse::<usize>().ok());
            match precision {
                Some(p) => format!("{:.*}", p, number.unwrap_or(0.0)),
                None => value.to_display(),
            }
        }
    };
    if spec.len() > 1 {
        let (align, width) = match spec.as_bytes().last().copied() {
            Some(b'>') | Some(b'<') | Some(b'^') => {
                let width: String = spec[..spec.len() - 1].chars().filter(|c| c.is_ascii_digit()).collect();
                (spec.chars().last().unwrap(), width.parse::<usize>().ok())
            }
            _ => {
                let digits: String = spec.chars().filter(|c| c.is_ascii_digit()).collect();
                (if spec.starts_with('0') { '0' } else { '>' }, digits.parse::<usize>().ok())
            }
        };
        if let Some(width) = width {
            if out.chars().count() < width {
                let pad = width - out.chars().count();
                out = match align {
                    '<' => format!("{}{}", out, " ".repeat(pad)),
                    '^' => {
                        let left = pad / 2;
                        format!("{}{}{}", " ".repeat(left), out, " ".repeat(pad - left))
                    }
                    '0' => format!("{}{}", "0".repeat(pad), out),
                    _ => format!("{}{}", " ".repeat(pad), out),
                };
            }
        }
    }
    out
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
                    out.push_str(&format_spec(&value, field.trim_start_matches(':')));
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
    // ---- json
    ("json_parse", "parse JSON text into a Kex value", |_rt, a| {
        let text = need_str(&a, 0, "json_parse")?;
        Json::parse(&text)
            .map(|j| json::to_value(&j))
            .map_err(error)
    }),
    ("json_stringify", "turn a Kex value into compact JSON", |_rt, a| {
        Ok(KexValue::string(json::from_value(&arg(&a, 0)).stringify()))
    }),
    ("json_pretty", "turn a Kex value into indented JSON", |_rt, a| {
        fn indent(json: &Json, level: usize, out: &mut String) {
            match json {
                Json::Obj(entries) => {
                    out.push('{');
                    for (i, (key, value)) in entries.iter().enumerate() {
                        if i > 0 { out.push(','); }
                        out.push('\n');
                        out.push_str(&"  ".repeat(level + 1));
                        let mut quoted = String::new();
                        quoted.push('"');
                        for c in key.chars() {
                            match c {
                                '"' => quoted.push_str("\\\""),
                                '\\' => quoted.push_str("\\\\"),
                                _ => quoted.push(c),
                            }
                        }
                        quoted.push('"');
                        out.push_str(&quoted);
                        out.push_str(": ");
                        indent(value, level + 1, out);
                    }
                    out.push('\n');
                    out.push_str(&"  ".repeat(level));
                    out.push('}');
                }
                Json::Arr(items) => {
                    out.push('[');
                    for (i, value) in items.iter().enumerate() {
                        if i > 0 { out.push(','); }
                        out.push('\n');
                        out.push_str(&"  ".repeat(level + 1));
                        indent(value, level + 1, out);
                    }
                    out.push('\n');
                    out.push_str(&"  ".repeat(level));
                    out.push(']');
                }
                other => out.push_str(&other.stringify()),
            }
        }
        let mut out = String::new();
        indent(&json::from_value(&arg(&a, 0)), 0, &mut out);
        Ok(KexValue::string(out))
    }),
    ("json_get", "read a dot separated path from parsed JSON", |_rt, a| {
        let path = if a.len() > 1 { need_str(&a, 1, "json_get")? } else { String::new() };
        Ok(lookup_path(&arg(&a, 0), &path))
    }),
    ("json_set", "set a dot separated path in parsed JSON", |_rt, a| {
        let path = need_str(&a, 1, "json_set")?;
        let mut root = arg(&a, 0);
        let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
        set_path(&mut root, &parts, arg(&a, 2));
        Ok(root)
    }),

    // ---- files and paths
    ("file_read", "read a UTF-8 file", |_rt, a| {
        let path = path_string(&a, 0, "file_read")?;
        fs::read_to_string(&path).map(KexValue::string).map_err(|e| error(format!("file_read({}): {}", path, e)))
    }),
    ("file_write", "write a UTF-8 file, replacing its contents", |_rt, a| {
        let path = path_string(&a, 0, "file_write")?;
        let text = need_str(&a, 1, "file_write")?;
        fs::write(&path, text).map(|_| KexValue::Boolean(true)).map_err(|e| error(format!("file_write({}): {}", path, e)))
    }),
    ("file_append", "append text to a UTF-8 file", |_rt, a| {
        use std::io::Write as _;
        let path = path_string(&a, 0, "file_append")?;
        let text = need_str(&a, 1, "file_append")?;
        let mut file = fs::OpenOptions::new().create(true).append(true).open(&path)
            .map_err(|e| error(format!("file_append({}): {}", path, e)))?;
        file.write_all(text.as_bytes()).map(|_| KexValue::Boolean(true)).map_err(|e| error(format!("file_append({}): {}", path, e)))
    }),
    ("file_exists", "true when a file or folder exists", |_rt, a| {
        Ok(KexValue::Boolean(Path::new(&path_string(&a, 0, "file_exists")?).exists()))
    }),
    ("file_delete", "delete a file or an empty directory", |_rt, a| {
        let path = path_string(&a, 0, "file_delete")?;
        let result = if Path::new(&path).is_dir() { fs::remove_dir(&path) } else { fs::remove_file(&path) };
        result.map(|_| KexValue::Boolean(true)).map_err(|e| error(format!("file_delete({}): {}", path, e)))
    }),
    ("file_copy", "copy a file", |_rt, a| {
        let from = path_string(&a, 0, "file_copy")?;
        let to = path_string(&a, 1, "file_copy")?;
        fs::copy(&from, &to).map(|_| KexValue::Boolean(true)).map_err(|e| error(format!("file_copy: {}", e)))
    }),
    ("file_move", "move or rename a file", |_rt, a| {
        let from = path_string(&a, 0, "file_move")?;
        let to = path_string(&a, 1, "file_move")?;
        fs::rename(&from, &to).map(|_| KexValue::Boolean(true)).map_err(|e| error(format!("file_move: {}", e)))
    }),
    ("file_list", "list the names inside a directory", |_rt, a| {
        let path = path_string(&a, 0, "file_list")?;
        let mut names: Vec<String> = fs::read_dir(&path)
            .map_err(|e| error(format!("file_list({}): {}", path, e)))?
            .filter_map(|entry| entry.ok().map(|e| e.file_name().to_string_lossy().to_string()))
            .collect();
        names.sort();
        Ok(KexValue::Array(names.into_iter().map(KexValue::string).collect()))
    }),
    ("file_mkdir", "create a directory and its parents", |_rt, a| {
        let path = path_string(&a, 0, "file_mkdir")?;
        fs::create_dir_all(&path).map(|_| KexValue::Boolean(true)).map_err(|e| error(format!("file_mkdir({}): {}", path, e)))
    }),
    ("file_meta", "metadata about a file or folder", |_rt, a| {
        let path = path_string(&a, 0, "file_meta")?;
        let meta = fs::metadata(&path).map_err(|e| error(format!("file_meta({}): {}", path, e)))?;
        let mut obj = KexObject::new();
        obj.set("size", KexValue::Number(meta.len() as f64));
        obj.set("is_file", KexValue::Boolean(meta.is_file()));
        obj.set("is_dir", KexValue::Boolean(meta.is_dir()));
        let modified = meta.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as f64).unwrap_or(0.0);
        obj.set("modified", KexValue::Number(modified));
        Ok(KexValue::Object(obj))
    }),
    ("file_name", "final component of a path", |_rt, a| {
        let path = path_string(&a, 0, "file_name")?;
        Ok(KexValue::string(Path::new(&path).file_name().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()))
    }),
    ("file_ext", "extension of a path, without the dot", |_rt, a| {
        let path = path_string(&a, 0, "file_ext")?;
        Ok(KexValue::string(Path::new(&path).extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()))
    }),
    ("file_dir", "parent directory of a path", |_rt, a| {
        let path = path_string(&a, 0, "file_dir")?;
        Ok(KexValue::string(Path::new(&path).parent().map(|s| s.to_string_lossy().to_string()).unwrap_or_default()))
    }),
    ("path_join", "join path components", |_rt, a| {
        let mut out = std::path::PathBuf::new();
        for value in &a {
            out.push(path_string(&[value.clone()], 0, "path_join")?);
        }
        Ok(KexValue::string(out.to_string_lossy().to_string()))
    }),
    ("path_abs", "absolute form of a path", |_rt, a| {
        let path = path_string(&a, 0, "path_abs")?;
        std::fs::canonicalize(&path)
            .or_else(|_| std::env::current_dir().map(|cwd| cwd.join(&path)))
            .map(|p| KexValue::string(p.to_string_lossy().to_string()))
            .map_err(|e| error(format!("path_abs: {}", e)))
    }),
    ("cwd", "current working directory", |_rt, _a| {
        Ok(KexValue::string(std::env::current_dir().map(|p| p.to_string_lossy().to_string()).unwrap_or_default()))
    }),
    ("env_get", "read an environment variable", |_rt, a| {
        let name = need_str(&a, 0, "env_get")?;
        Ok(std::env::var(name).map(KexValue::string).unwrap_or_else(|_| arg(&a, 1)))
    }),
    ("env_set", "set an environment variable", |_rt, a| {
        let name = need_str(&a, 0, "env_set")?;
        let value = need_str(&a, 1, "env_set")?;
        std::env::set_var(name, value);
        Ok(KexValue::Boolean(true))
    }),

    // ---- strings, extended
    ("title", "capitalise every word", |_rt, a| {
        let text = need_str(&a, 0, "title")?;
        let mut out = String::new();
        let mut capital = true;
        for c in text.chars() {
            if c.is_alphanumeric() {
                if capital { out.extend(c.to_uppercase()); } else { out.extend(c.to_lowercase()); }
                capital = false;
            } else {
                out.push(c);
                capital = true;
            }
        }
        Ok(KexValue::string(out))
    }),
    ("slug", "make a URL safe slug", |_rt, a| {
        let text = need_str(&a, 0, "slug")?.to_lowercase();
        let out: String = text.chars().map(|c| if c.is_alphanumeric() { c } else { '-' }).collect();
        Ok(KexValue::string(out.split('-').filter(|s| !s.is_empty()).collect::<Vec<_>>().join("-")))
    }),
    ("camel", "convert text to camelCase", |_rt, a| {
        let words = words_of(&need_str(&a, 0, "camel")?);
        let mut out = String::new();
        for (i, word) in words.iter().enumerate() {
            let mut chars = word.chars();
            if i == 0 {
                out.push_str(&word.to_lowercase());
            } else if let Some(first) = chars.next() {
                out.extend(first.to_uppercase());
                out.push_str(&chars.as_str().to_lowercase());
            }
        }
        Ok(KexValue::string(out))
    }),
    ("snake", "convert text to snake_case", |_rt, a| {
        Ok(KexValue::string(words_of(&need_str(&a, 0, "snake")?).join("_").to_lowercase()))
    }),
    ("kebab", "convert text to kebab-case", |_rt, a| {
        Ok(KexValue::string(words_of(&need_str(&a, 0, "kebab")?).join("-").to_lowercase()))
    }),
    ("trim_start", "trim whitespace from the start", |_rt, a| {
        Ok(KexValue::string(need_str(&a, 0, "trim_start")?.trim_start().to_string()))
    }),
    ("trim_end", "trim whitespace from the end", |_rt, a| {
        Ok(KexValue::string(need_str(&a, 0, "trim_end")?.trim_end().to_string()))
    }),
    ("replace_all", "replace every occurrence of a substring", |_rt, a| {
        let text = need_str(&a, 0, "replace_all")?;
        let from = need_str(&a, 1, "replace_all")?;
        let to = arg(&a, 2).to_display();
        Ok(KexValue::string(text.replace(&from, &to)))
    }),
    ("split_lines", "split text into lines", |_rt, a| {
        let text = need_str(&a, 0, "split_lines")?;
        Ok(KexValue::Array(text.lines().map(|line| KexValue::string(line)).collect()))
    }),
    ("count_of", "count occurrences of a substring", |_rt, a| {
        let text = need_str(&a, 0, "count_of")?;
        let needle = need_str(&a, 1, "count_of")?;
        Ok(KexValue::Number(if needle.is_empty() { 0.0 } else { text.matches(&needle).count() as f64 }))
    }),
    ("is_alpha", "true when text contains only letters", |_rt, a| {
        let text = need_str(&a, 0, "is_alpha")?;
        Ok(KexValue::Boolean(!text.is_empty() && text.chars().all(|c| c.is_alphabetic())))
    }),
    ("is_digit", "true when text contains only digits", |_rt, a| {
        let text = need_str(&a, 0, "is_digit")?;
        Ok(KexValue::Boolean(!text.is_empty() && text.chars().all(|c| c.is_ascii_digit())))
    }),
    ("is_alnum", "true when text contains only letters and digits", |_rt, a| {
        let text = need_str(&a, 0, "is_alnum")?;
        Ok(KexValue::Boolean(!text.is_empty() && text.chars().all(|c| c.is_alphanumeric())))
    }),
    ("is_space", "true when text is empty or all whitespace", |_rt, a| {
        Ok(KexValue::Boolean(need_str(&a, 0, "is_space")?.chars().all(|c| c.is_whitespace())))
    }),
    ("quote", "wrap text in double quotes with escapes", |_rt, a| {
        Ok(KexValue::string(escape_string(&arg(&a, 0).to_display())))
    }),
    ("unquote", "remove matching outer quotes", |_rt, a| {
        let text = need_str(&a, 0, "unquote")?;
        let trimmed = text.trim();
        let inner = if trimmed.len() >= 2
            && ((trimmed.starts_with('"') && trimmed.ends_with('"'))
                || (trimmed.starts_with('\'') && trimmed.ends_with('\'')))
        {
            &trimmed[1..trimmed.len() - 1]
        } else {
            trimmed
        };
        Ok(KexValue::string(inner))
    }),

    // ---- arrays, extended
    ("concat", "join two or more arrays", |_rt, a| {
        let mut out = Vec::new();
        for value in &a {
            match value {
                KexValue::Array(items) => out.extend(items.clone()),
                other => out.push(other.clone()),
            }
        }
        Ok(KexValue::Array(out))
    }),
    ("count", "count matching items, or all items", |rt, a| {
        let items = need_array(&a, 0, "count")?;
        let mut total = 0.0;
        for item in &items {
            let keep = if a.len() > 1 {
                rt.call_value(arg(&a, 1), vec![item.clone()])?.is_truthy()
            } else {
                true
            };
            if keep { total += 1.0; }
        }
        Ok(KexValue::Number(total))
    }),
    ("some", "true when any item matches a predicate", |rt, a| {
        let items = need_array(&a, 0, "some")?;
        for item in &items {
            if rt.call_value(arg(&a, 1), vec![item.clone()])?.is_truthy() {
                return Ok(KexValue::Boolean(true));
            }
        }
        Ok(KexValue::Boolean(false))
    }),
    ("every", "true when all items match a predicate", |rt, a| {
        let items = need_array(&a, 0, "every")?;
        for item in &items {
            if !rt.call_value(arg(&a, 1), vec![item.clone()])?.is_truthy() {
                return Ok(KexValue::Boolean(false));
            }
        }
        Ok(KexValue::Boolean(true))
    }),
    ("sort_by", "sort an array by a key function", |rt, a| {
        let mut items = need_array(&a, 0, "sort_by")?;
        let key_fn = arg(&a, 1);
        let mut keys = Vec::with_capacity(items.len());
        for item in &items {
            keys.push(rt.call_value(key_fn.clone(), vec![item.clone()])?);
        }
        let mut order: Vec<usize> = (0..items.len()).collect();
        order.sort_by(|x, y| compare(&keys[*x], &keys[*y]).unwrap_or(std::cmp::Ordering::Equal));
        items = order.into_iter().map(|i| items[i].clone()).collect();
        Ok(KexValue::Array(items))
    }),
    ("group_by", "group items by a key function", |rt, a| {
        let items = need_array(&a, 0, "group_by")?;
        let key_fn = arg(&a, 1);
        let mut obj = KexObject::new();
        for item in &items {
            let key = rt.call_value(key_fn.clone(), vec![item.clone()])?.to_display();
            let mut group = match obj.get(&key) {
                Some(KexValue::Array(group)) => group.clone(),
                _ => Vec::new(),
            };
            group.push(item.clone());
            obj.set(&key, KexValue::Array(group));
        }
        Ok(KexValue::Object(obj))
    }),
    ("zip", "pair items from two arrays", |_rt, a| {
        let left = need_array(&a, 0, "zip")?;
        let right = need_array(&a, 1, "zip")?;
        let mut out = Vec::new();
        for i in 0..left.len().min(right.len()) {
            out.push(KexValue::Array(vec![left[i].clone(), right[i].clone()]));
        }
        Ok(KexValue::Array(out))
    }),
    ("chunk", "split an array into fixed size chunks", |_rt, a| {
        let items = need_array(&a, 0, "chunk")?;
        let size = need_num(&a, 1, "chunk")? as usize;
        if size == 0 { return Err(error("chunk() needs a positive size")); }
        Ok(KexValue::Array(items.chunks(size).map(|c| KexValue::Array(c.to_vec())).collect()))
    }),
    ("take", "first n items", |_rt, a| {
        let items = need_array(&a, 0, "take")?;
        let n = need_num(&a, 1, "take")?.max(0.0) as usize;
        Ok(KexValue::Array(items.into_iter().take(n).collect()))
    }),
    ("drop", "skip the first n items", |_rt, a| {
        let items = need_array(&a, 0, "drop")?;
        let n = need_num(&a, 1, "drop")?.max(0.0) as usize;
        Ok(KexValue::Array(items.into_iter().skip(n).collect()))
    }),
    ("first", "first item of an array", |_rt, a| Ok(arg(&need_array(&a, 0, "first")?, 0))),
    ("last", "last item of an array", |_rt, a| {
        let items = need_array(&a, 0, "last")?;
        Ok(items.last().cloned().unwrap_or(KexValue::Null))
    }),
    ("avg", "arithmetic mean of an array", |_rt, a| {
        let items = need_array(&a, 0, "avg")?;
        if items.is_empty() { return Ok(KexValue::Number(0.0)); }
        let total: f64 = items.iter().filter_map(|v| v.as_number()).sum();
        Ok(KexValue::Number(total / items.len() as f64))
    }),
    ("min_of", "smallest numeric value in an array", |_rt, a| {
        let items = need_array(&a, 0, "min_of")?;
        Ok(KexValue::Number(items.iter().filter_map(|v| v.as_number()).fold(f64::INFINITY, f64::min)))
    }),
    ("max_of", "largest numeric value in an array", |_rt, a| {
        let items = need_array(&a, 0, "max_of")?;
        Ok(KexValue::Number(items.iter().filter_map(|v| v.as_number()).fold(f64::NEG_INFINITY, f64::max)))
    }),
    ("shuffle", "randomly reorder an array", |_rt, a| {
        let mut items = need_array(&a, 0, "shuffle")?;
        for i in (1..items.len()).rev() {
            let j = (pseudo_random() * (i + 1) as f64) as usize;
            items.swap(i, j);
        }
        Ok(KexValue::Array(items))
    }),

    // ---- objects, extended
    ("entries", "an object as an array of [key, value] pairs", |_rt, a| {
        let obj = need_object(&a, 0, "entries")?;
        Ok(KexValue::Array(obj.entries.into_iter().map(|(k, v)| {
            KexValue::Array(vec![KexValue::string(k), v])
        }).collect()))
    }),
    ("from_entries", "build an object from [key, value] pairs", |_rt, a| {
        let items = need_array(&a, 0, "from_entries")?;
        let mut obj = KexObject::new();
        for item in items {
            if let KexValue::Array(pair) = item {
                if pair.len() >= 2 {
                    obj.set(&pair[0].to_display(), pair[1].clone());
                }
            }
        }
        Ok(KexValue::Object(obj))
    }),
    ("pick", "keep only the named keys", |_rt, a| {
        let obj = need_object(&a, 0, "pick")?;
        let mut out = KexObject::new();
        for key in need_array(&a, 1, "pick")? {
            if let Some(value) = obj.get(&key.to_display()) {
                out.set(&key.to_display(), value.clone());
            }
        }
        Ok(KexValue::Object(out))
    }),
    ("omit", "remove the named keys", |_rt, a| {
        let mut obj = need_object(&a, 0, "omit")?;
        for key in need_array(&a, 1, "omit")? {
            obj.remove(&key.to_display());
        }
        Ok(KexValue::Object(obj))
    }),
    ("invert", "swap the keys and values of an object", |_rt, a| {
        let obj = need_object(&a, 0, "invert")?;
        let mut out = KexObject::new();
        for (key, value) in obj.entries {
            out.set(&value.to_display(), KexValue::string(key));
        }
        Ok(KexValue::Object(out))
    }),
    ("deep_get", "read a dot separated path", |_rt, a| {
        let path = need_str(&a, 1, "deep_get")?;
        Ok(lookup_path(&arg(&a, 0), &path))
    }),
    ("deep_set", "write a dot separated path", |_rt, a| {
        let path = need_str(&a, 1, "deep_set")?;
        let mut root = arg(&a, 0);
        let parts: Vec<&str> = path.split('.').filter(|p| !p.is_empty()).collect();
        set_path(&mut root, &parts, arg(&a, 2));
        Ok(root)
    }),

    // ---- math, extended
    ("atan", "inverse tangent", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "atan")?.atan()))),
    ("sinh", "hyperbolic sine", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "sinh")?.sinh()))),
    ("cosh", "hyperbolic cosine", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "cosh")?.cosh()))),
    ("tanh", "hyperbolic tangent", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "tanh")?.tanh()))),
    ("asinh", "inverse hyperbolic sine", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "asinh")?.asinh()))),
    ("acosh", "inverse hyperbolic cosine", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "acosh")?.acosh()))),
    ("atanh", "inverse hyperbolic tangent", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "atanh")?.atanh()))),
    ("hypot", "length of the hypotenuse", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "hypot")?.hypot(need_num(&a, 1, "hypot")?)))),
    ("cbrt", "cube root", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "cbrt")?.cbrt()))),
    ("trunc", "truncate towards zero", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "trunc")?.trunc()))),
    ("degrees", "radians to degrees", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "degrees")?.to_degrees()))),
    ("radians", "degrees to radians", |_rt, a| Ok(KexValue::Number(need_num(&a, 0, "radians")?.to_radians()))),
    ("log_base", "logarithm with an explicit base", |_rt, a| {
        Ok(KexValue::Number(need_num(&a, 0, "log_base")?.log(need_num(&a, 1, "log_base")?)))
    }),
    ("round_to", "round to a number of decimal places", |_rt, a| {
        let value = need_num(&a, 0, "round_to")?;
        let digits = need_num(&a, 1, "round_to")? as i32;
        let factor = 10f64.powi(digits);
        Ok(KexValue::Number((value * factor).round() / factor))
    }),
    ("gcd", "greatest common divisor", |_rt, a| {
        let mut x = need_num(&a, 0, "gcd")?.abs() as i64;
        let mut y = need_num(&a, 1, "gcd")?.abs() as i64;
        while y != 0 { let r = x % y; x = y; y = r; }
        Ok(KexValue::Number(x as f64))
    }),
    ("lcm", "least common multiple", |_rt, a| {
        let x = need_num(&a, 0, "lcm")?.abs() as i64;
        let y = need_num(&a, 1, "lcm")?.abs() as i64;
        let mut a = x; let mut b = y;
        while b != 0 { let r = a % b; a = b; b = r; }
        Ok(KexValue::Number((x / a.max(1) * y) as f64))
    }),
    ("factorial", "factorial of a non negative integer", |_rt, a| {
        let n = need_num(&a, 0, "factorial")? as u64;
        if n > 20 { return Err(error("factorial() is limited to 20")); }
        let mut out = 1u64;
        for i in 2..=n { out = out.saturating_mul(i); }
        Ok(KexValue::Number(out as f64))
    }),
    ("is_nan", "true when a value is NaN", |_rt, a| {
        Ok(KexValue::Boolean(arg(&a, 0).as_number().map(|n| n.is_nan()).unwrap_or(false)))
    }),
    ("is_infinite", "true when a value is infinite", |_rt, a| {
        Ok(KexValue::Boolean(arg(&a, 0).as_number().map(|n| n.is_infinite()).unwrap_or(false)))
    }),

    // ---- time and dates
    ("sleep", "pause execution for a number of milliseconds", |_rt, a| {
        let ms = need_num(&a, 0, "sleep")?.max(0.0) as u64;
        std::thread::sleep(std::time::Duration::from_millis(ms));
        Ok(KexValue::Boolean(true))
    }),
    ("date_now", "current UTC date and time as an object", |_rt, a| {
        let ms = if a.is_empty() {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
        } else {
            need_num(&a, 0, "date_now")?
        };
        let p = date_parts(ms);
        let mut obj = KexObject::new();
        obj.set("year", KexValue::Number(p.year as f64));
        obj.set("month", KexValue::Number(p.month as f64));
        obj.set("day", KexValue::Number(p.day as f64));
        obj.set("hour", KexValue::Number(p.hour as f64));
        obj.set("minute", KexValue::Number(p.minute as f64));
        obj.set("second", KexValue::Number(p.second as f64));
        obj.set("millis", KexValue::Number(p.millis as f64));
        obj.set("weekday", KexValue::Number(p.weekday as f64));
        obj.set("epoch", KexValue::Number(p.epoch));
        obj.set("iso", KexValue::string(iso_date(&p)));
        Ok(KexValue::Object(obj))
    }),
    ("date_iso", "ISO 8601 UTC timestamp", |_rt, a| {
        let ms = if a.is_empty() {
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as f64).unwrap_or(0.0)
        } else { need_num(&a, 0, "date_iso")? };
        Ok(KexValue::string(iso_date(&date_parts(ms))))
    }),
    ("date_format", "format a timestamp (YYYY MM DD HH mm ss SSS)", |_rt, a| {
        let ms = need_num(&a, 0, "date_format")?;
        let pattern = need_str(&a, 1, "date_format")?;
        let p = date_parts(ms);
        let out = pattern
            .replace("YYYY", &format!("{:04}", p.year))
            .replace("YY", &format!("{:02}", p.year.rem_euclid(100)))
            .replace("MM", &format!("{:02}", p.month))
            .replace("DD", &format!("{:02}", p.day))
            .replace("HH", &format!("{:02}", p.hour))
            .replace("mm", &format!("{:02}", p.minute))
            .replace("ss", &format!("{:02}", p.second))
            .replace("SSS", &format!("{:03}", p.millis));
        Ok(KexValue::string(out))
    }),

    // ---- utilities
    ("base64_encode", "encode text as base64", |_rt, a| {
        let text = need_str(&a, 0, "base64_encode")?;
        Ok(KexValue::string(base64_encode(text.as_bytes())))
    }),
    ("base64_decode", "decode base64 text", |_rt, a| {
        let text = need_str(&a, 0, "base64_decode")?;
        base64_decode(&text).map(|bytes| KexValue::string(String::from_utf8_lossy(&bytes).to_string()))
    }),
    ("uuid", "generate a version 4 UUID", |_rt, _a| Ok(KexValue::string(make_uuid()))),
    ("hash", "stable 64 bit FNV-1a hash as hex", |_rt, a| {
        Ok(KexValue::string(format!("{:016x}", fnv1a64(&arg(&a, 0).to_display()))))
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
    ("title", "title"),
    ("slug", "slug"),
    ("trim_start", "trim_start"),
    ("trimStart", "trim_start"),
    ("trim_end", "trim_end"),
    ("trimEnd", "trim_end"),
    ("replace_all", "replace_all"),
    ("replaceAll", "replace_all"),
    ("split_lines", "split_lines"),
    ("count_of", "count_of"),
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
    ("concat", "concat"),
    ("count", "count"),
    ("some", "some"),
    ("every", "every"),
    ("sort_by", "sort_by"),
    ("sortBy", "sort_by"),
    ("group_by", "group_by"),
    ("groupBy", "group_by"),
    ("zip", "zip"),
    ("chunk", "chunk"),
    ("take", "take"),
    ("drop", "drop"),
    ("first", "first"),
    ("last", "last"),
    ("avg", "avg"),
    ("min_of", "min_of"),
    ("max_of", "max_of"),
    ("shuffle", "shuffle"),
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
    ("entries", "entries"),
    ("pick", "pick"),
    ("omit", "omit"),
    ("invert", "invert"),
    ("deep_get", "deep_get"),
    ("deepGet", "deep_get"),
    ("deep_set", "deep_set"),
    ("deepSet", "deep_set"),
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

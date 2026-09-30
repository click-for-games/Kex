use std::fmt::Write as _;

// ============================================================================
// MODULE: MINIMAL JSON (no external crates, keeps the binary dependency free)
// ============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Obj(entries) => entries.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s.as_str()),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Num(n) => Some(*n),
            Json::Str(s) => s.parse().ok(),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Arr(items) => Some(items.as_slice()),
            _ => None,
        }
    }

    pub fn str_or(&self, key: &str, fallback: &str) -> String {
        self.get(key)
            .and_then(|v| v.as_str())
            .unwrap_or(fallback)
            .to_string()
    }

    pub fn stringify(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::Num(n) => {
                if n.is_finite() {
                    if *n == n.trunc() && n.abs() < 1e15 {
                        let _ = write!(out, "{}", *n as i64);
                    } else {
                        let _ = write!(out, "{}", n);
                    }
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => write_json_string(s, out),
            Json::Arr(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            Json::Obj(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_json_string(k, out);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }

    pub fn parse(input: &str) -> Result<Json, String> {
        let chars: Vec<char> = input.chars().collect();
        let mut p = JsonParser { chars, pos: 0 };
        p.skip_ws();
        let value = p.value()?;
        p.skip_ws();
        Ok(value)
    }
}

fn write_json_string(s: &str, out: &mut String) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct JsonParser {
    chars: Vec<char>,
    pos: usize,
}

impl JsonParser {
    fn peek(&self) -> char {
        *self.chars.get(self.pos).unwrap_or(&'\0')
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), ' ' | '\t' | '\n' | '\r') {
            self.pos += 1;
        }
    }

    fn expect(&mut self, ch: char) -> Result<(), String> {
        if self.peek() == ch {
            self.pos += 1;
            Ok(())
        } else {
            Err(format!(
                "expected '{}' at offset {} but found '{}'",
                ch,
                self.pos,
                self.peek()
            ))
        }
    }

    fn value(&mut self) -> Result<Json, String> {
        self.skip_ws();
        match self.peek() {
            '{' => self.object(),
            '[' => self.array(),
            '"' => Ok(Json::Str(self.string()?)),
            't' => self.literal("true", Json::Bool(true)),
            'f' => self.literal("false", Json::Bool(false)),
            'n' => self.literal("null", Json::Null),
            _ => self.number(),
        }
    }

    fn literal(&mut self, text: &str, value: Json) -> Result<Json, String> {
        for expected in text.chars() {
            if self.peek() != expected {
                return Err(format!("invalid JSON literal at offset {}", self.pos));
            }
            self.pos += 1;
        }
        Ok(value)
    }

    fn number(&mut self) -> Result<Json, String> {
        let start = self.pos;
        if self.peek() == '-' || self.peek() == '+' {
            self.pos += 1;
        }
        while matches!(self.peek(), '0'..='9' | '.' | 'e' | 'E' | '+' | '-') {
            self.pos += 1;
        }
        let raw: String = self.chars[start..self.pos].iter().collect();
        raw.parse::<f64>()
            .map(Json::Num)
            .map_err(|_| format!("invalid JSON number '{}'", raw))
    }

    fn string(&mut self) -> Result<String, String> {
        self.expect('"')?;
        let mut out = String::new();
        loop {
            match self.peek() {
                '\0' => return Err("unterminated JSON string".to_string()),
                '"' => {
                    self.pos += 1;
                    break;
                }
                '\\' => {
                    self.pos += 1;
                    let esc = self.peek();
                    self.pos += 1;
                    out.push(match esc {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        'b' => '\u{8}',
                        'f' => '\u{c}',
                        '/' => '/',
                        '"' => '"',
                        '\\' => '\\',
                        'u' => {
                            let hex: String = self.chars[self.pos..(self.pos + 4).min(self.chars.len())]
                                .iter()
                                .collect();
                            self.pos += 4;
                            char::from_u32(
                                u32::from_str_radix(&hex, 16).map_err(|_| "bad \\u escape".to_string())?,
                            )
                            .unwrap_or('\u{fffd}')
                        }
                        other => return Err(format!("bad escape '\\{}'", other)),
                    });
                }
                c => {
                    out.push(c);
                    self.pos += 1;
                }
            }
        }
        Ok(out)
    }

    fn array(&mut self) -> Result<Json, String> {
        self.expect('[')?;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == ']' {
            self.pos += 1;
            return Ok(Json::Arr(items));
        }
        loop {
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                ',' => self.pos += 1,
                ']' => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(format!("expected ',' or ']' at offset {}", self.pos)),
            }
        }
        Ok(Json::Arr(items))
    }

    fn object(&mut self) -> Result<Json, String> {
        self.expect('{')?;
        let mut entries = Vec::new();
        self.skip_ws();
        if self.peek() == '}' {
            self.pos += 1;
            return Ok(Json::Obj(entries));
        }
        loop {
            self.skip_ws();
            let key = self.string()?;
            self.skip_ws();
            self.expect(':')?;
            let value = self.value()?;
            entries.push((key, value));
            self.skip_ws();
            match self.peek() {
                ',' => self.pos += 1,
                '}' => {
                    self.pos += 1;
                    break;
                }
                _ => return Err(format!("expected ',' or '}}' at offset {}", self.pos)),
            }
        }
        Ok(Json::Obj(entries))
    }
}

use crate::json::Json;
use crate::value::{KexObject, KexValue};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

// ============================================================================
// MODULE 7: HOST BRIDGE (kexco / wasm friendly FFI surface)
// ============================================================================

/// A live connection to an external host process (kexco's JS runtime).
///
/// The wire format is one JSON object per line using the tagged codec that
/// kexco also implements in JavaScript:
///
/// ```json
/// {"id":1,"fn":"dom.query","args":[{"k":"s","v":"#app"}]}
/// -> {"id":1,"ok":true,"value":{"k":"o","v":{"id":{...}}}}
/// -> {"id":1,"ok":false,"error":"selector not found"}
/// ```
pub struct HostBridge {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: u64,
    runtime_name: String,
    script: PathBuf,
}

impl HostBridge {
    pub fn call(&mut self, name: &str, args: &[KexValue]) -> Result<KexValue, String> {
        self.next_id += 1;
        let id = self.next_id;

        let request = Json::Obj(vec![
            ("id".to_string(), Json::Num(id as f64)),
            ("fn".to_string(), Json::Str(name.to_string())),
            (
                "args".to_string(),
                Json::Arr(args.iter().map(encode_value).collect()),
            ),
        ]);

        let mut line = request.stringify();
        line.push('\n');
        self.stdin
            .write_all(line.as_bytes())
            .and_then(|_| self.stdin.flush())
            .map_err(|e| format!("host bridge write failed: {}", e))?;

        let mut response = String::new();
        let read = self
            .stdout
            .read_line(&mut response)
            .map_err(|e| format!("host bridge read failed: {}", e))?;
        if read == 0 {
            return Err("host bridge closed the connection".to_string());
        }

        let parsed = Json::parse(response.trim())
            .map_err(|e| format!("host bridge sent invalid JSON: {}", e))?;

        if parsed.get("ok").map(|v| *v == Json::Bool(false)).unwrap_or(false) {
            let message = parsed
                .get("error")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown host error")
                .to_string();
            return Err(message);
        }
        Ok(decode_value(
            parsed.get("value").unwrap_or(&Json::Null),
        ))
    }

    pub fn runtime_name(&self) -> &str {
        &self.runtime_name
    }

    pub fn script(&self) -> &Path {
        &self.script
    }
}

impl Drop for HostBridge {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Locates the kexco host entry point and boots it.
pub fn attach(search_dirs: &[PathBuf]) -> Option<HostBridge> {
    if std::env::var("KEX_NO_HOST").is_ok() {
        return None;
    }
    for dir in search_dirs {
        let candidate = dir.join("lib").join("kexco.host.js");
        if candidate.is_file() {
            if let Some(bridge) = spawn_host(&candidate) {
                return Some(bridge);
            }
        }
        let flat = dir.join("kexco.host.js");
        if flat.is_file() {
            if let Some(bridge) = spawn_host(&flat) {
                return Some(bridge);
            }
        }
    }
    if let Ok(explicit) = std::env::var("KEXCO_HOST") {
        let path = PathBuf::from(explicit);
        if path.is_file() {
            return spawn_host(&path);
        }
    }
    None
}

fn spawn_host(script: &Path) -> Option<HostBridge> {
    let runtime = if cfg!(target_os = "windows") {
        "node"
    } else {
        "node"
    };

    let mut child = Command::new(runtime)
        .arg(script)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .ok()?;

    let stdin = child.stdin.take()?;
    let stdout = child.stdout.take()?;

    Some(HostBridge {
        child,
        stdin,
        stdout: BufReader::new(stdout),
        next_id: 0,
        runtime_name: "node".to_string(),
        script: script.to_path_buf(),
    })
}

// ----- wire codec ----------------------------------------------------------

pub fn encode_value(value: &KexValue) -> Json {
    match value {
        KexValue::Null => Json::Obj(vec![("k".into(), Json::Str("z".into()))]),
        KexValue::Boolean(b) => Json::Obj(vec![
            ("k".into(), Json::Str("b".into())),
            ("v".into(), Json::Bool(*b)),
        ]),
        KexValue::Number(n) => Json::Obj(vec![
            ("k".into(), Json::Str("n".into())),
            ("v".into(), Json::Num(*n)),
        ]),
        KexValue::String(s) => Json::Obj(vec![
            ("k".into(), Json::Str("s".into())),
            ("v".into(), Json::Str(s.clone())),
        ]),
        KexValue::Array(items) => Json::Obj(vec![
            ("k".into(), Json::Str("a".into())),
            (
                "v".into(),
                Json::Arr(items.iter().map(encode_value).collect()),
            ),
        ]),
        KexValue::Object(obj) => Json::Obj(vec![
            ("k".into(), Json::Str("o".into())),
            (
                "v".into(),
                Json::Obj(obj.entries.iter().map(|(k, v)| (k.clone(), encode_value(v))).collect()),
            ),
        ]),
        KexValue::Range(r) => Json::Obj(vec![
            ("k".into(), Json::Str("r".into())),
            ("v".into(), Json::Arr(vec![Json::Num(r.start), Json::Num(r.end), Json::Num(r.step)])),
        ]),
        KexValue::Error(msg) => Json::Obj(vec![
            ("k".into(), Json::Str("e".into())),
            ("v".into(), Json::Str(msg.clone())),
        ]),
        KexValue::Func(f) => Json::Obj(vec![
            ("k".into(), Json::Str("f".into())),
            ("v".into(), Json::Str(f.borrow().name.clone())),
        ]),
        KexValue::Builtin(name) => Json::Obj(vec![
            ("k".into(), Json::Str("f".into())),
            ("v".into(), Json::Str(name.clone())),
        ]),
    }
}

pub fn decode_value(json: &Json) -> KexValue {
    // plain JSON is accepted as a convenience for host authors
    match json {
        Json::Null => KexValue::Null,
        Json::Bool(b) => KexValue::Boolean(*b),
        Json::Num(n) => KexValue::Number(*n),
        Json::Str(s) => KexValue::String(s.clone()),
        Json::Arr(items) => KexValue::Array(items.iter().map(decode_value).collect()),
        Json::Obj(_) => {
            let tag = json.get("k").and_then(|v| v.as_str()).unwrap_or("");
            let payload = json.get("v");
            match tag {
                "z" => KexValue::Null,
                "b" => KexValue::Boolean(matches!(payload, Some(Json::Bool(true)))),
                "n" => KexValue::Number(payload.and_then(|v| v.as_f64()).unwrap_or(0.0)),
                "s" => KexValue::String(
                    payload
                        .and_then(|v| v.as_str())
                        .unwrap_or_default()
                        .to_string(),
                ),
                "a" => KexValue::Array(match payload {
                    Some(Json::Arr(items)) => items.iter().map(decode_value).collect(),
                    _ => Vec::new(),
                }),
                "o" => {
                    let mut obj = KexObject::new();
                    if let Some(Json::Obj(entries)) = payload {
                        for (key, value) in entries {
                            obj.set(key, decode_value(value));
                        }
                    }
                    KexValue::Object(obj)
                }
                "r" => {
                    let nums: Vec<f64> = match payload {
                        Some(Json::Arr(items)) => items.iter().filter_map(|v| v.as_f64()).collect(),
                        _ => vec![],
                    };
                    KexValue::Range(crate::value::RangeVal {
                        start: nums.first().copied().unwrap_or(0.0),
                        end: nums.get(1).copied().unwrap_or(0.0),
                        step: nums.get(2).copied().unwrap_or(1.0),
                        inclusive: false,
                    })
                }
                "e" => KexValue::Error(
                    payload
                        .and_then(|v| v.as_str())
                        .unwrap_or("host error")
                        .to_string(),
                ),
                "f" => KexValue::Builtin(
                    payload
                        .and_then(|v| v.as_str())
                        .unwrap_or("closure")
                        .to_string(),
                ),
                _ => {
                    // untagged object: decode as a plain Kex object
                    let mut obj = KexObject::new();
                    if let Json::Obj(entries) = json {
                        for (key, value) in entries {
                            obj.set(key, decode_value(value));
                        }
                    }
                    KexValue::Object(obj)
                }
            }
        }
    }
}

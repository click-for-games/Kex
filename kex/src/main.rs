mod ast;
mod builtins;
mod error;
mod host;
mod json;
mod lexer;
mod pack;
mod parser;
mod runtime;
mod value;

use error::Signal;
use runtime::Runtime;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

const COLOR_RED: &str = "\x1b[31;1m";
const COLOR_GREEN: &str = "\x1b[32;1m";
const COLOR_YELLOW: &str = "\x1b[33;1m";
const COLOR_CYAN: &str = "\x1b[36;1m";
const COLOR_DIM: &str = "\x1b[90m";
const COLOR_BOLD: &str = "\x1b[1m";
const COLOR_RESET: &str = "\x1b[0m";

const VERSION: &str = env!("CARGO_PKG_VERSION");

const WEB_MAIN: &str = r#"import "kexco";

let port = env_get("KEX_PORT") == null ? 8080 : parse_num(env_get("KEX_PORT"));
let bind_host = env_get("KEX_HOST") == null ? "127.0.0.1" : env_get("KEX_HOST");

server_static("public", "/");

server_route("GET", "/api/hello", (request) => {
    return json_response({
        "ok": true,
        "message": "Hello from Kex",
        "path": request.path
    });
});

server_route("POST", "/api/echo", (request) => {
    return json_response({
        "ok": true,
        "echo": request.body
    });
});

let bound = server_listen(port, bind_host);
kxout("Kex web server listening on http://" + bound.address);
server_run(50);
"#;

const WEB_INDEX: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>Kex Web Starter</title>
  <link rel="stylesheet" href="/styles.css">
</head>
<body>
  <main>
    <h1>Kex Web Starter</h1>
    <p id="message">Loading...</p>
    <form id="echo-form">
      <input id="echo-input" placeholder="Type something">
      <button type="submit">Send</button>
    </form>
    <pre id="echo-result"></pre>
  </main>
  <script>
    fetch('/api/hello')
      .then((res) => res.json())
      .then((data) => { document.getElementById('message').textContent = data.message; });

    document.getElementById('echo-form').addEventListener('submit', async (event) => {
      event.preventDefault();
      const text = document.getElementById('echo-input').value;
      const res = await fetch('/api/echo', { method: 'POST', body: text });
      const data = await res.json();
      document.getElementById('echo-result').textContent = JSON.stringify(data, null, 2);
    });
  </script>
</body>
</html>
"#;

const WEB_CSS: &str = r#":root {
  color-scheme: dark;
  font-family: system-ui, sans-serif;
}
body {
  margin: 0;
  min-height: 100vh;
  display: grid;
  place-items: center;
  background: #101218;
  color: #f5f7fb;
}
main {
  width: min(560px, 90vw);
  padding: 32px;
  border-radius: 18px;
  background: #1b1f2a;
  box-shadow: 0 20px 60px rgba(0, 0, 0, 0.4);
}
input, button {
  font: inherit;
  padding: 10px 12px;
  border-radius: 8px;
  border: 1px solid #3a4256;
}
input { background: #10131b; color: inherit; }
button { background: #4f8cff; border-color: #4f8cff; cursor: pointer; }
pre { white-space: pre-wrap; color: #9de2b2; }
"#;

fn main() {
    let args: Vec<String> = env::args().collect();
    let code = match args.len() {
        0 | 1 => {
            if args.len() == 1 && is_version_flag(&args[0]) {
                banner();
                0
            } else {
                usage();
                0
            }
        }
        _ => dispatch(&args[1..]),
    };
    exit(code);
}

fn is_version_flag(arg: &str) -> bool {
    matches!(arg, "-v" | "--version" | "version")
}

fn banner() {
    println!("{}KEXDRUN Engine v{}{}", COLOR_CYAN, VERSION, COLOR_RESET);
}

fn usage() {
    println!(
        "{}KEXDRUN Engine v{} {} - the Kex programming language{}",
        COLOR_CYAN, VERSION, COLOR_BOLD, COLOR_RESET
    );
    println!();
    println!("{}USAGE{}", COLOR_BOLD, COLOR_RESET);
    println!("  kex <file.kx>              run a script");
    println!("  kex run <file.kx>          run a script");
    println!("  kex check <file.kx>        parse a script without running it");
    println!("  kex serve <file.kx>        run a web backend with KEX_PORT/KEX_HOST");
    println!("  kex new web <dir>          scaffold a starter website");
    println!("  kex builtins               list the standard library");
    println!("  kex pack <subcommand>      manage kex libraries");
    println!("  kex version                print the version");
    println!();
    println!("{}EXAMPLES{}", COLOR_BOLD, COLOR_RESET);
    println!("  kex main.kx");
    println!("  kex new web my-site");
    println!("  kex serve my-site/main.kx --port 8080");
    println!("  kex pack install kexco");
    println!("  kex pack list");
    println!();
    println!(
        "{}GETTING STARTED{}  kex pack install kexco",
        COLOR_DIM, COLOR_RESET
    );
}

fn dispatch(args: &[String]) -> i32 {
    match args[0].as_str() {
        "run" => match args.get(1) {
            Some(path) => run_script(Path::new(path)),
            None => {
                eprintln!("{}kex run{} expects a file path", COLOR_RED, COLOR_RESET);
                2
            }
        },
        "check" => match args.get(1) {
            Some(path) => check_script(Path::new(path)),
            None => {
                eprintln!("{}kex check{} expects a file path", COLOR_RED, COLOR_RESET);
                2
            }
        },
        "builtins" | "stdlib" => {
            list_builtins();
            0
        }
        "serve" => serve_command(&args[1..]),
        "new" => new_command(&args[1..]),
        "pack" | "kx" | "pkg" => pack_command(&args[1..]),
        "help" | "--help" | "-h" => {
            usage();
            0
        }
        "version" | "-v" | "--version" => {
            banner();
            0
        }
        other if other.starts_with('-') => {
            eprintln!("{}unknown option '{}'{}", COLOR_RED, other, COLOR_RESET);
            usage();
            2
        }
        other => run_script(Path::new(other)),
    }
}

fn check_script(path: &Path) -> i32 {
    let source = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!(
                "{}Kex Error{}: cannot read '{}': {}",
                COLOR_RED, COLOR_RESET, path.display(), e
            );
            return 1;
        }
    };
    let mut rt = Runtime::new();
    match rt.check_source(&source) {
        Ok(()) => {
            println!(
                "{}ok{} {} parses cleanly",
                COLOR_GREEN, COLOR_RESET, path.display()
            );
            0
        }
        Err(e) => {
            eprintln!(
                "{}Kex {} Error{} [Line {}, Column {}]: {}",
                COLOR_RED, e.phase, COLOR_RESET, e.line, e.col, e.message
            );
            1
        }
    }
}

fn serve_command(args: &[String]) -> i32 {
    let Some(file) = args.first() else {
        eprintln!("{}kex serve{} expects a file path", COLOR_RED, COLOR_RESET);
        return 2;
    };

    let mut port = env::var("KEX_PORT").unwrap_or_else(|_| "8080".to_string());
    let mut bind = env::var("KEX_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let mut root: Option<PathBuf> = None;
    let mut index = 1;

    while index < args.len() {
        let arg = &args[index];
        if let Some(value) = arg.strip_prefix("--port=") {
            port = value.to_string();
        } else if let Some(value) = arg.strip_prefix("--host=") {
            bind = value.to_string();
        } else if let Some(value) = arg.strip_prefix("--root=") {
            root = Some(PathBuf::from(value));
        } else {
            match arg.as_str() {
                "--port" | "-p" => {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        eprintln!("{}kex serve{} --port expects a number", COLOR_RED, COLOR_RESET);
                        return 2;
                    };
                    port = value.clone();
                }
                "--host" | "-H" => {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        eprintln!("{}kex serve{} --host expects an address", COLOR_RED, COLOR_RESET);
                        return 2;
                    };
                    bind = value.clone();
                }
                "--root" => {
                    index += 1;
                    let Some(value) = args.get(index) else {
                        eprintln!("{}kex serve{} --root expects a directory", COLOR_RED, COLOR_RESET);
                        return 2;
                    };
                    root = Some(PathBuf::from(value));
                }
                other => {
                    eprintln!("{}kex serve{} unknown option '{}'", COLOR_RED, COLOR_RESET, other);
                    return 2;
                }
            }
        }
        index += 1;
    }

    let path = Path::new(file);
    if !path.is_file() {
        eprintln!(
            "{}Kex System Error{}: unable to locate target file '{}'",
            COLOR_RED,
            COLOR_RESET,
            path.display()
        );
        return 1;
    }

    let base = fs::canonicalize(path)
        .ok()
        .and_then(|full| full.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| {
            path.parent()
                .map(|parent| parent.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        });
    let root = root
        .map(|value| fs::canonicalize(&value).unwrap_or(value))
        .unwrap_or(base);

    env::set_var("KEX_PORT", &port);
    env::set_var("KEX_HOST", &bind);
    env::set_var("KEXCO_ROOT", &root);

    println!(
        "{}serving{} {} on http://{}:{} (static root {})",
        COLOR_CYAN,
        COLOR_RESET,
        path.display(),
        bind,
        port,
        root.display()
    );
    run_script(path)
}

fn new_command(args: &[String]) -> i32 {
    match args.first().map(|value| value.as_str()) {
        Some("web") | None => {
            let directory = args
                .get(1)
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("webapp"));
            scaffold_web(&directory)
        }
        Some(other) => {
            eprintln!("{}kex new{} unknown template '{}'", COLOR_RED, COLOR_RESET, other);
            eprintln!("available templates: web");
            2
        }
    }
}

fn scaffold_web(directory: &Path) -> i32 {
    let public = directory.join("public");
    if let Err(e) = fs::create_dir_all(&public) {
        eprintln!("{}kex new{} could not create '{}': {}", COLOR_RED, COLOR_RESET, public.display(), e);
        return 1;
    }

    let files = [
        (directory.join("main.kx"), WEB_MAIN),
        (public.join("index.html"), WEB_INDEX),
        (public.join("styles.css"), WEB_CSS),
    ];
    for (path, contents) in files {
        if path.exists() {
            println!("{}skipped{} {} (already exists)", COLOR_YELLOW, COLOR_RESET, path.display());
        } else {
            match fs::write(&path, contents) {
                Ok(()) => println!("{}created{} {}", COLOR_GREEN, COLOR_RESET, path.display()),
                Err(e) => {
                    eprintln!("{}kex new{} could not write '{}': {}", COLOR_RED, COLOR_RESET, path.display(), e);
                    return 1;
                }
            }
        }
    }

    println!();
    println!("{}next{} kex serve {}/main.kx --port 8080", COLOR_DIM, COLOR_RESET, directory.display());
    0
}

fn list_builtins() {
    let list = sorted_builtins();
    let width = list.iter().map(|(n, _)| n.len()).max().unwrap_or(0);
    for (name, description) in &list {
        println!(
            "  {}{}{}  {}{}",
            COLOR_GREEN,
            format!("{:width$}", name, width = width),
            COLOR_RESET,
            COLOR_DIM,
            description
        );
    }
    println!();
    println!(
        "{} {} builtins, plus method syntax on strings, arrays and objects{}",
        COLOR_DIM, list.len(), COLOR_RESET
    );
}

fn sorted_builtins() -> Vec<(&'static str, &'static str)> {
    let mut list: Vec<(&'static str, &'static str)> = builtins::BUILTINS
        .iter()
        .map(|(n, d, _)| (*n, *d))
        .collect();
    list.sort_by_key(|(n, _)| *n);
    list
}

fn run_script(path: &Path) -> i32 {
    let mut rt = Runtime::new();
    rt.script_dir = fs::canonicalize(path)
        .ok()
        .and_then(|full| full.parent().map(|parent| parent.to_path_buf()))
        .unwrap_or_else(|| {
            path.parent()
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|| PathBuf::from("."))
        });
    rt.host = host::attach(&host_search_dirs(&rt.script_dir));

    let source = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!(
                "{}Kex System Error{}: unable to locate target file '{}' ({})",
                COLOR_RED,
                COLOR_RESET,
                path.display(),
                e
            );
            return 1;
        }
    };

    match rt.run_source(&source) {
        Ok(code) => code,
        Err(signal) => {
            match signal {
                Signal::Exit(code) => code,
                Signal::Error(value) => {
                    eprintln!(
                        "{}Kex Runtime Error{}: {}",
                        COLOR_RED,
                        COLOR_RESET,
                        value.to_display()
                    );
                    1
                }
                other => {
                    eprintln!(
                        "{}Kex Runtime Error{}: {}",
                        COLOR_RED,
                        COLOR_RESET,
                        other.message()
                    );
                    1
                }
            }
        }
    }
}

fn host_search_dirs(script_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![script_dir.to_path_buf(), script_dir.join("kexco")];
    for ancestor in script_dir.ancestors().skip(1) {
        if ancestor.file_name().is_none() { break; }
        dirs.push(ancestor.to_path_buf());
        dirs.push(ancestor.join("kexco"));
    }
    if let Ok(current) = env::current_dir() {
        dirs.push(current.clone());
        dirs.push(current.join("kexco"));
        for ancestor in current.ancestors().skip(1) {
            if ancestor.file_name().is_none() { break; }
            dirs.push(ancestor.to_path_buf());
            dirs.push(ancestor.join("kexco"));
        }
    }
    if let Some(name) = pack::find("kexco") {
        dirs.push(name.installed_dir());
    }
    dirs.push(pack::pack_dir().join("kexco"));
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("kexco"));
            dirs.push(parent.to_path_buf());
        }
    }
    dirs.push(PathBuf::from("."));
    dirs
}

fn pack_command(args: &[String]) -> i32 {
    match args.first().map(|s| s.as_str()) {
        Some("install") | Some("i") | Some("add") => {
            let Some(name) = args.get(1) else {
                eprintln!(
                    "{}kex pack install{} expects a package name, folder or url",
                    COLOR_RED, COLOR_RESET
                );
                return 2;
            };
            println!(
                "{}kex pack{} resolving '{}' ...",
                COLOR_CYAN, COLOR_RESET, name
            );
            match pack::install(name) {
                Ok(dir) => {
                    println!(
                        "{}installed{} {} -> {}",
                        COLOR_GREEN, COLOR_RESET, name, dir.display()
                    );
                    let entry = dir.join(
                        pack::find(name)
                            .map(|p| p.entry)
                            .unwrap_or_else(|| "mod.kx".to_string()),
                    );
                    if entry.is_file() {
                        println!(
                            "{}use it with{}   import \"{}\";",
                            COLOR_DIM, COLOR_RESET, name
                        );
                    }
                    0
                }
                Err(e) => {
                    eprintln!("{}kex pack error{}: {}", COLOR_RED, COLOR_RESET, e);
                    1
                }
            }
        }
        Some("list") | Some("ls") => {
            let list = pack::installed();
            if list.is_empty() {
                println!(
                    "{}no packages installed in {}{}",
                    COLOR_DIM,
                    pack::pack_dir().display(),
                    COLOR_RESET
                );
                println!(
                    "{}try{} kex pack install kexco",
                    COLOR_DIM, COLOR_RESET
                );
                return 0;
            }
            println!(
                "{}installed packages{} ({})",
                COLOR_BOLD,
                COLOR_RESET,
                pack::pack_dir().display()
            );
            for (name, version, description) in list {
                println!(
                    "  {}{:<12}{} {:<8} {}{}{}",
                    COLOR_GREEN, name, COLOR_RESET, version, COLOR_DIM, description, COLOR_RESET
                );
            }
            0
        }
        Some("info") | Some("show") => {
            let Some(name) = args.get(1) else {
                eprintln!("{}kex pack info{} expects a package name", COLOR_RED, COLOR_RESET);
                return 2;
            };
            match pack::find(name) {
                Some(pkg) => {
                    println!("{}{}{}", COLOR_BOLD, pkg.name, COLOR_RESET);
                    println!("  version      {}", pkg.version);
                    println!("  description  {}", pkg.description);
                    println!("  source       {}", pkg.source);
                    println!("  entry        {}", pkg.entry);
                    if !pkg.host.is_empty() {
                        println!("  host         {}", pkg.host);
                    }
                    println!("  registry     {}", pkg.origin);
                    println!("  install dir  {}", pkg.installed_dir().display());
                    println!(
                        "  installed    {}",
                        if pkg.is_installed() { "yes" } else { "no" }
                    );
                    if let Some(v) = pkg.installed_version() {
                        println!("  local version {}", v);
                    }
                    0
                }
                None => {
                    eprintln!("{}'{}' is not in any registry{}", COLOR_RED, name, COLOR_RESET);
                    1
                }
            }
        }
        Some("remove") | Some("uninstall") | Some("rm") => {
            let Some(name) = args.get(1) else {
                eprintln!("{}kex pack remove{} expects a package name", COLOR_RED, COLOR_RESET);
                return 2;
            };
            match pack::remove(name) {
                Ok(()) => {
                    println!("{}removed{} {}", COLOR_GREEN, COLOR_RESET, name);
                    0
                }
                Err(e) => {
                    eprintln!("{}kex pack error{}: {}", COLOR_RED, COLOR_RESET, e);
                    1
                }
            }
        }
        Some("update") => {
            let Some(name) = args.get(1) else {
                println!("{}registered packages{}", COLOR_BOLD, COLOR_RESET);
                for pkg in pack::registry() {
                    println!(
                        "  {}{:<12}{} {:<8} {}{}{}",
                        COLOR_GREEN, pkg.name, COLOR_RESET, pkg.version, COLOR_DIM, pkg.description, COLOR_RESET
                    );
                }
                return 0;
            };
            println!("{}updating{} {} ...", COLOR_CYAN, COLOR_RESET, name);
            match pack::install(name) {
                Ok(_) => {
                    println!("{}up to date{}", COLOR_GREEN, COLOR_RESET);
                    0
                }
                Err(e) => {
                    eprintln!("{}kex pack error{}: {}", COLOR_RED, COLOR_RESET, e);
                    1
                }
            }
        }
        Some("search") => {
            let needle = args.get(1).map(|s| s.to_ascii_lowercase()).unwrap_or_default();
            let matches: Vec<_> = pack::registry()
                .into_iter()
                .filter(|p| {
                    needle.is_empty()
                        || p.name.to_ascii_lowercase().contains(&needle)
                        || p.description.to_ascii_lowercase().contains(&needle)
                })
                .collect();
            if matches.is_empty() {
                println!("{}nothing matches '{}'{}", COLOR_DIM, needle, COLOR_RESET);
                return 0;
            }
            for pkg in matches {
                println!(
                    "  {}{:<12}{} {:<8} {}{}{}",
                    COLOR_GREEN, pkg.name, COLOR_RESET, pkg.version, COLOR_DIM, pkg.description, COLOR_RESET
                );
            }
            0
        }
        Some("path") => {
            println!("{}", pack::pack_dir().display());
            0
        }
        Some("home") => {
            println!("{}", pack::kex_home().display());
            0
        }
        Some("host") => {
            let mut rt = Runtime::new();
            rt.host = host::attach(&host_search_dirs(Path::new(".")));
            match &rt.host {
                Some(bridge) => {
                    println!("{}host attached{} via {}", COLOR_GREEN, COLOR_RESET, bridge.script().display());
                    0
                }
                None => {
                    println!(
                        "{}no host attached{} (kex pack install kexco)",
                        COLOR_YELLOW, COLOR_RESET
                    );
                    1
                }
            }
        }
        Some("help") | None => {
            println!("{}", pack::help());
            0
        }
        Some(other) => {
            eprintln!(
                "{}unknown pack subcommand '{}'{}",
                COLOR_RED, other, COLOR_RESET
            );
            println!("{}", pack::help());
            2
        }
    }
}

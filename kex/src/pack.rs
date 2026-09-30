use crate::json::Json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

// ============================================================================
// MODULE 8: kex pack  (terminal package manager for kex libraries)
// ============================================================================

/// Registry baked into the binary so `kex pack install kexco` always resolves.
const EMBEDDED_REGISTRY: &str = include_str!("../registry.json");

pub struct Package {
    pub name: String,
    pub version: String,
    pub description: String,
    pub source: String,
    pub entry: String,
    pub host: String,
    pub origin: String,
}

impl Package {
    fn from_json(json: &Json) -> Option<Package> {
        let name = json.get("name")?.as_str()?.to_string();
        Some(Package {
            name,
            version: json.str_or("version", "0.0.0"),
            description: json.str_or("description", ""),
            source: json.str_or("source", ""),
            entry: json.str_or("entry", "mod.kx"),
            host: json.str_or("host", ""),
            origin: String::new(),
        })
    }

    pub fn installed_dir(&self) -> PathBuf {
        pack_dir().join(&self.name)
    }

    pub fn is_installed(&self) -> bool {
        self.installed_dir().join(&self.entry).is_file()
    }

    pub fn installed_version(&self) -> Option<String> {
        let manifest = self.installed_dir().join("kexpack.json");
        let text = fs::read_to_string(manifest).ok()?;
        let json = Json::parse(&text).ok()?;
        Some(json.str_or("version", "unknown"))
    }
}

/// Root of the per user package tree: `%LOCALAPPDATA%\kex` or `~/.kex`.
pub fn pack_dir() -> PathBuf {
    if let Ok(custom) = std::env::var("KEX_HOME") {
        return PathBuf::from(custom).join("pack");
    }
    if cfg!(target_os = "windows") {
        if let Ok(local) = std::env::var("LOCALAPPDATA") {
            return PathBuf::from(local).join("kex").join("pack");
        }
    }
    if let Ok(home) = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE")) {
        return PathBuf::from(home).join(".kex").join("pack");
    }
    PathBuf::from(".kex").join("pack")
}

pub fn kex_home() -> PathBuf {
    pack_dir()
        .parent()
        .map(|p| p.to_path_buf())
        .unwrap_or_else(|| PathBuf::from(".kex"))
}

/// Search roots for an on disk registry: exe dir, pack dir, cwd.
fn registry_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
            if let Some(grand) = parent.parent() {
                dirs.push(grand.to_path_buf());
            }
        }
    }
    dirs.push(pack_dir());
    dirs.push(kex_home());
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd);
    }
    dirs
}

pub fn registry() -> Vec<Package> {
    let mut packages: Vec<Package> = Vec::new();
    let mut seen: Vec<String> = Vec::new();

    let push = |text: &str, origin: &str, packages: &mut Vec<Package>, seen: &mut Vec<String>| {
        if let Ok(json) = Json::parse(text) {
            if let Some(list) = json.get("packages").and_then(|v| v.as_array()) {
                for entry in list {
                    if let Some(mut pkg) = Package::from_json(entry) {
                        if seen.contains(&pkg.name) {
                            continue;
                        }
                        pkg.origin = origin.to_string();
                        seen.push(pkg.name.clone());
                        packages.push(pkg);
                    }
                }
            }
        }
    };

    // on disk registries take precedence over the embedded copy
    for dir in registry_dirs() {
        let path = dir.join("registry.json");
        if let Ok(text) = fs::read_to_string(&path) {
            push(
                &text,
                &dir.display().to_string(),
                &mut packages,
                &mut seen,
            );
        }
    }
    push(EMBEDDED_REGISTRY, "embedded", &mut packages, &mut seen);
    packages
}

pub fn find(name: &str) -> Option<Package> {
    registry()
        .into_iter()
        .find(|p| p.name.eq_ignore_ascii_case(name))
}

// ----- source resolution ---------------------------------------------------

enum Source {
    Directory(PathBuf),
    Archive(PathBuf),
    Remote { url: String },
}

/// Resolves a registry `source` field or a user supplied spec.
fn resolve_source(spec: &str) -> Result<Source, String> {
    if spec.is_empty() {
        return Err("the package has no source".to_string());
    }
    if spec.starts_with("http://") || spec.starts_with("https://") {
        return Ok(Source::Remote {
            url: spec.to_string(),
        });
    }
    if let Some(rest) = spec.strip_prefix("file://") {
        let path = PathBuf::from(rest);
        return Ok(classify(path));
    }

    // relative sources are looked up next to every registry.json
    let as_path = PathBuf::from(spec);
    if as_path.is_absolute() {
        return Ok(classify(as_path));
    }
    for dir in registry_dirs() {
        let candidate = dir.join(&as_path);
        if candidate.exists() {
            return Ok(classify(candidate));
        }
    }
    // a bare name is not a path, so it can only be a fully qualified url
    if !spec.contains("://") {
        return Err(format!(
            "cannot find the source for '{}': looked next to every registry.json. Pass a path (kex pack install ./kexco) or an archive url instead",
            spec
        ));
    }
    Ok(Source::Remote {
        url: spec.to_string(),
    })
}

fn classify(path: PathBuf) -> Source {
    if path.is_dir() {
        Source::Directory(path)
    } else {
        Source::Archive(path)
    }
}

// ----- install -------------------------------------------------------------

/// Builds a package description for a path or URL that is not in a registry.
/// Entry points are guessed the way a person would: `<name>.kx`, `index.kx`,
/// `main.kx`, `src/<name>.kx`, then any single top level `.kx` file.
fn ad_hoc(spec: &str) -> Result<Package, String> {
    let looks_remote = spec.starts_with("http://")
        || spec.starts_with("https://")
        || spec.starts_with("file://");
    let local = if let Some(rest) = spec.strip_prefix("file://") {
        Some(PathBuf::from(rest))
    } else if looks_remote {
        None
    } else {
        Some(PathBuf::from(spec))
    };

    let name = if looks_remote {
        let tail = spec
            .split(['?', '#'])
            .next()
            .unwrap_or(spec)
            .trim_end_matches('/')
            .rsplit('/')
            .next()
            .unwrap_or("package")
            .to_string();
        let stem = tail
            .rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or(tail);
        sanitize_name(&stem)
    } else {
        let path = match &local {
            Some(path) => path.clone(),
            None => return Err(format!("cannot work out a local path from '{}'", spec)),
        };
        let base = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "package".to_string());
        let stem = if path.is_dir() {
            base
        } else {
            base.rsplit_once('.')
                .map(|(s, _)| s.to_string())
                .unwrap_or(base)
        };
        sanitize_name(&stem)
    };

    if name.is_empty() {
        return Err(format!("cannot work out a package name from '{}'", spec));
    }

    let entry = match &local {
        Some(path) if path.is_dir() => guess_entry(path, &name)
            .ok_or_else(|| format!("no .kx entry file found in '{}'", path.display()))?,
        Some(path) => path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| format!("{}.kx", name)),
        None => format!("{}.kx", name),
    };

    let host = local
        .as_ref()
        .filter(|p| p.is_dir())
        .and_then(|p| guess_host(p))
        .unwrap_or_default();

    Ok(Package {
        name,
        version: "0.0.0".to_string(),
        description: format!("installed from '{}'", spec),
        source: spec.to_string(),
        entry,
        host,
        origin: spec.to_string(),
    })
}

fn sanitize_name(raw: &str) -> String {
    raw.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

fn guess_entry(dir: &Path, name: &str) -> Option<String> {
    for candidate in [
        format!("{}.kx", name),
        "index.kx".to_string(),
        "main.kx".to_string(),
        "mod.kx".to_string(),
        format!("src/{}.kx", name),
        "lib/main.kx".to_string(),
    ] {
        if dir.join(&candidate).is_file() {
            return Some(candidate);
        }
    }
    let mut loose: Vec<String> = Vec::new();
    if let Ok(iter) = fs::read_dir(dir) {
        for entry in iter.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "kx").unwrap_or(false) && path.is_file() {
                if let Some(name) = path.file_name() {
                    loose.push(name.to_string_lossy().to_string());
                }
            }
        }
    }
    loose.sort();
    loose.into_iter().next()
}

fn guess_host(dir: &Path) -> Option<String> {
    for candidate in [
        "lib/kexco.host.js",
        "kexco.host.js",
        "lib/host.js",
        "host.js",
    ] {
        if dir.join(candidate).is_file() {
            return Some(candidate.to_string());
        }
    }
    None
}

pub fn install(name: &str) -> Result<PathBuf, String> {
    let pkg = match find(name) {
        Some(found) => found,
        None => match ad_hoc(name) {
            Ok(found) => found,
            Err(_) => {
                return Err(format!(
                    "package '{}' is not in any registry and is not an existing path or URL (searched the bundled registry, {} and the exe folder)",
                    name,
                    pack_dir().display()
                ))
            }
        },
    };

    let source = resolve_source(&pkg.source)?;
    let staging = pack_dir().join(".staging").join(&pkg.name);
    if staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }
    fs::create_dir_all(&staging)
        .map_err(|e| format!("cannot create '{}': {}", staging.display(), e))?;

    match source {
        Source::Directory(dir) => {
            copy_tree(&dir, &staging)?;
        }
        Source::Archive(path) => {
            extract(&path, &staging)?;
        }
        Source::Remote { url } => {
            let archive = download(&url)?;
            extract(&archive, &staging)?;
            let _ = fs::remove_file(&archive);
        }
    }

    // archives frequently wrap everything in a single top level folder
    let root = normalise_root(&staging);
    if !root.join(&pkg.entry).is_file() {
        if find_entry(&root, &pkg.entry).is_some() {
            let promoted = staging.join("__promote");
            let _ = fs::remove_dir_all(&promoted);
            copy_tree(&root, &promoted)?;
            let _ = fs::remove_dir_all(&staging);
            fs::rename(&promoted, &staging).map_err(|e| e.to_string())?;
        } else {
            let listing = describe_dir(&staging);
            return Err(format!(
                "'{}' installed but '{}' is missing. contents:\n  {}",
                pkg.name,
                pkg.entry,
                listing
            ));
        }
    }

    let manifest = Json::Obj(vec![
        ("name".into(), Json::Str(pkg.name.clone())),
        ("version".into(), Json::Str(pkg.version.clone())),
        ("entry".into(), Json::Str(pkg.entry.clone())),
        ("host".into(), Json::Str(pkg.host.clone())),
        (
            "description".into(),
            Json::Str(pkg.description.clone()),
        ),
    ]);
    fs::write(
        staging.join("kexpack.json"),
        manifest.stringify(),
    )
    .map_err(|e| format!("cannot write the manifest: {}", e))?;

    let target = pkg.installed_dir();
    let _ = fs::remove_dir_all(&target);
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    fs::rename(&staging, &target).map_err(|e| {
        format!(
            "cannot move '{}' into place: {}",
            staging.display(),
            e
        )
    })?;

    Ok(target)
}

fn find_entry(root: &Path, entry: &str) -> Option<PathBuf> {
    let mut depth = 0;
    let mut queue = vec![root.to_path_buf()];
    while let Some(dir) = queue.pop() {
        depth += 1;
        if depth > 64 {
            break;
        }
        let entries: Vec<PathBuf> = match fs::read_dir(&dir) {
            Ok(iter) => iter.flatten().map(|e| e.path()).collect(),
            Err(_) => continue,
        };
        for path in &entries {
            if path.is_file() && path.file_name().map(|n| n == entry).unwrap_or(false) {
                return Some(path.clone());
            }
        }
        for path in entries {
            if path.is_dir() {
                queue.push(path);
            }
        }
    }
    None
}

fn normalise_root(staging: &Path) -> PathBuf {
    let entries: Vec<PathBuf> = fs::read_dir(staging)
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let dirs: Vec<&PathBuf> = entries.iter().filter(|p| p.is_dir()).collect();
    if entries.len() == 1 && dirs.len() == 1 {
        dirs[0].clone()
    } else {
        staging.to_path_buf()
    }
}

pub fn remove(name: &str) -> Result<(), String> {
    let dir = pack_dir().join(name);
    if !dir.is_dir() {
        return Err(format!("'{}' is not installed", name));
    }
    fs::remove_dir_all(&dir).map_err(|e| format!("cannot remove '{}': {}", dir.display(), e))
}

pub fn installed() -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(pack_dir()) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            let manifest = path.join("kexpack.json");
            let (version, description) = match fs::read_to_string(&manifest)
                .ok()
                .and_then(|t| Json::parse(&t).ok())
            {
                Some(json) => (
                    json.str_or("version", "0.0.0"),
                    json.str_or("description", ""),
                ),
                None => ("-".to_string(), String::new()),
            };
            out.push((name, version, description));
        }
    }
    out.sort();
    out
}

// ----- filesystem / archive helpers ----------------------------------------

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    fs::create_dir_all(to).map_err(|e| format!("cannot create '{}': {}", to.display(), e))?;
    let entries = fs::read_dir(from)
        .map_err(|e| format!("cannot read '{}': {}", from.display(), e))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let target = to.join(entry.file_name());
        let kind = entry
            .file_type()
            .map_err(|e| e.to_string())?;
        if kind.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else if kind.is_file() {
            fs::copy(entry.path(), &target)
                .map_err(|e| format!("cannot copy '{}': {}", entry.path().display(), e))?;
        }
    }
    Ok(())
}

fn describe_dir(dir: &Path) -> String {
    let mut names = Vec::new();
    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            names.push(entry.file_name().to_string_lossy().to_string());
        }
    }
    names.sort();
    names.join("\n  ")
}

fn extract(archive: &Path, into: &Path) -> Result<(), String> {
    if !archive.is_file() {
        return Err(format!("'{}' is not a file", archive.display()));
    }
    let mut attempts: Vec<(&str, Vec<String>)> = vec![
        ("tar", vec!["-xf".into(), archive.display().to_string(), "-C".into(), into.display().to_string()]),
    ];
    if cfg!(target_os = "windows") {
        attempts.push((
            "powershell",
            vec![
                "-NoProfile".into(),
                "-Command".into(),
                format!(
                    "Expand-Archive -LiteralPath '{}' -DestinationPath '{}' -Force",
                    archive.display(),
                    into.display()
                ),
            ],
        ));
    } else {
        attempts.push((
            "unzip",
            vec![
                "-o".into(),
                archive.display().to_string(),
                "-d".into(),
                into.display().to_string(),
            ],
        ));
    }

    let mut last = String::new();
    for (program, args) in attempts {
        let output = Command::new(program).args(&args).output();
        match output {
            Ok(out) if out.status.success() => return Ok(()),
            Ok(out) => {
                last = format!(
                    "{} failed: {}",
                    program,
                    String::from_utf8_lossy(&out.stderr).trim().to_string()
                )
            }
            Err(e) => last = format!("{} could not run: {}", program, e),
        }
    }
    Err(format!(
        "could not unpack '{}' ({}). Install tar or unzip, or point kex pack at an unpacked folder.",
        archive.display(),
        last
    ))
}

fn download(url: &str) -> Result<PathBuf, String> {
    let name = url.rsplit('/').next().unwrap_or("download.bin");
    let temp = std::env::temp_dir().join(format!("kexpack-{}", name));
    let _ = fs::remove_file(&temp);

    let curl = Command::new("curl")
        .args(["-fsSL", "-o"])
        .arg(&temp)
        .arg(url)
        .output();
    if let Ok(out) = curl {
        if out.status.success() && temp.is_file() {
            return Ok(temp);
        }
    }

    #[cfg(target_os = "windows")]
    {
        let ps = Command::new("powershell")
            .args([
                "-NoProfile",
                "-Command",
                &format!(
                    "[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12; Invoke-WebRequest -Uri '{}' -OutFile '{}' -UseBasicParsing",
                    url,
                    temp.display()
                ),
            ])
            .output();
        if let Ok(out) = ps {
            if out.status.success() && temp.is_file() {
                return Ok(temp);
            }
        }
    }

    let wget = Command::new("wget")
        .arg("-q")
        .arg("-O")
        .arg(&temp)
        .arg(url)
        .output();
    if let Ok(out) = wget {
        if out.status.success() && temp.is_file() {
            return Ok(temp);
        }
    }

    Err(format!(
        "could not download '{}'. Install curl, wget or PowerShell, or use a local folder instead.",
        url
    ))
}

// ----- help ----------------------------------------------------------------

pub fn help() -> String {
    "kex pack - terminal package manager for kex libraries\n\n\
     USAGE\n  \
     kex pack install <name|path|url>   install a package\n  \
     kex pack list                     list installed packages\n  \
     kex pack info <name>              show registry and install details\n  \
     kex pack remove <name>            uninstall a package\n  \
     kex pack search <text>            search the registry\n  \
     kex pack update <name>            reinstall the latest registry version\n  \
     kex pack path                     print the pack directory\n\n\
     ENV\n  \
     KEX_HOME          overrides the kex home directory\n  \
     KEXCO_HOST        explicit path to a kexco host entry point\n  \
     KEX_NO_HOST=1     disables the host bridge"
        .to_string()
}

# Kex Programming Language (`.kx`)

Kex is a lightweight, interpreted programming language written in Rust. It ships as a
single `kex.exe` binary, a Windows installer, and an official VS Code extension with
syntax highlighting out of the box.

- **Engine** — `kex/`, the Rust CLI (`KEXDRUN`). Tokenizer, parser, tree-walking
  interpreter and 87 builtins. One external crate dependency, no runtime requirements.
- **kexco** — `kexco/`, the standard library pack. A web platform (DOM, events, timers,
  storage, fetch) for Kex, bridged to a Node.js host. Includes a **headless adapter**, so
  the exact same `.kx` code runs in a terminal with no browser.
- **VS Code extension** — `vscode-extension/`, grammar and language configuration.
- **Installer** — `installer.iss`, an Inno Setup 6 script that packages `kex.exe` and the
  `.vsix`, adds the install directory to the system `PATH`, and installs the extension.

---

## Repository layout

```
kex/                    Rust engine (Cargo project)
  src/                    ast, lexer, parser, runtime, builtins, host, pack, json, error
  main.kx                 sample program used as a smoke test
  registry.json           package registry compiled into the binary
kexco/                  kexco standard library pack
  kexco.kx                 the Kex-facing API
  lib/kexco.js             implementation + headless DOM
  lib/kexco.host.js        the Node.js host process
tests/                  test scripts written in Kex
vscode-extension/       VS Code extension sources + built .vsix
Output/                 installer output (KexSetup.exe)
installer.iss          Inno Setup script
```

---

## Quick start

### Windows installer

Download `KexSetup.exe` from the releases page, or compile it yourself (see
[Building](#building)). Run it, leave **Add Kex to System PATH** checked, then open a
new terminal.

### Build from source

```bash
git clone https://github.com/click-for-games/Kex.git
cd Kex
cargo build --release --manifest-path kex/Cargo.toml
kex/target/release/kex.exe kex/main.kx
```

The interpreter itself has no runtime dependencies. Node.js is only needed if you want to
use the `kexco` library.

---

## Building

### 1. The engine

```bash
cargo build --release --manifest-path kex/Cargo.toml
# -> kex/target/release/kex.exe
```

### 2. The VS Code extension

```bash
cd vscode-extension
npx @vscode/vsce package --out kex-lang-1.0.0.vsix
```

`installer.iss` expects the archive at `vscode-extension/kex-lang-1.0.0.vsix`, so keep that
exact name and location.

### 3. The installer

Requires [Inno Setup 6](https://jrsoftware.org/isinfo.php). Run the script from the
**repository root**, because its `[Files]` entries are relative to the working directory:

```bash
"C:\Program Files (x86)\Inno Setup 6\ISCC.exe" installer.iss
# -> Output/KexSetup.exe
```

The installer installs to `{autopf}\KexEngine`, creates a `Kex Language` start menu group,
and then:

1. appends `{app}` to the **system** `PATH` (so `kex` works in any terminal), and
2. runs `code --install-extension "{app}\kex-lang-1.0.0.vsix" --force` if the VS Code CLI
   is present.

Both steps are skipped silently if the tools are unavailable.

---

## CLI

```
kex <file.kx>              run a script
kex run <file.kx>          run a script
kex check <file.kx>        parse a script without running it
kex builtins               list the standard library
kex pack <subcommand>      manage kex libraries
kex version                print the version
```

`kex pack` (aliases `kex kx`, `kex pkg`):

| Subcommand | Description |
| --- | --- |
| `install <name\|path\|url>` | Install a package, an archive, or a local folder. Aliases: `i`, `add` |
| `list` | List installed packages. Alias: `ls` |
| `info <name>` | Show package metadata. Alias: `show` |
| `remove <name>` | Uninstall a package. Aliases: `uninstall`, `rm` |
| `update [name]` | Reinstall a package, or list the registry when no name is given |
| `search [text]` | Search the registry |
| `path` | Print the pack directory |
| `home` | Print the Kex home directory |
| `host` | Report whether a host bridge is attached |

**Exit codes:** `0` success, `1` lexer/parser/runtime/pack error, `2` CLI usage error.

---

## Language tour

### Comments and declarations

```kex
// line comment
/* block
   comment */

let mutable = 1;      // let is reassignable
const FIXED = 2;      // const is immutable
```

Note the inversion relative to JavaScript: **`let` is mutable, `const` is immutable.**
Semicolons are optional everywhere.

### Strings

Double quotes interpolate, single quotes are literal:

```kex
let n = 10;
kxout("sum is {n + 2}");    // -> sum is 12
kxout('no {interpolation}'); // -> no {interpolation}
```

Escapes: `\n \t \r \0 \\ \' \" \{ \}`. Strings are single-line; a raw newline inside a
literal is an error.

### Control flow

```kex
if (n % 2 == 0) {
    kxout("even");
} elif (n % 3 == 0) {   // `else if` also works
    kxout("three");
} else {
    kxout("odd");
}

let total = 0;
for i in range(1, 5) { total = total + i; }   // for-in
for v in range(0, 6, 2) { kxout(v); }        // with a step

let k = 0;
while (k < 3) { kxout(k); k++; }

do { kxout("once"); } while (false);

switch (n) {
    case 10: kxout("ten");
    default: kxout("other");
}
```

### Functions

```kex
fn add(x, y) { return x + y; }

let square = (v) => v * v;
let addAll = (a, b) => { return a + b; };
```

Arrow functions are anonymous function expressions and capture their enclosing scope.

### Arrays and objects

```kex
let nums = [5, 3, 9, 1];
nums[0] = 42;                 // index assignment mutates in place
kxout(slice(nums, 1, 3));     // [3, 9]
kxout(map(nums, (v) => v + 1));

let cfg = { host: "local", port: 8080 };
kxout(cfg.host, keys(cfg));   // local [host, port]
```

### Operators

`+ - * / % **` · `== != < > <= >=` · `&& || !` · `& | ^` · `<< >>` · `??` (coalesce) ·
`?:` (ternary) · `..` `..=` `.. by` (ranges) · `++ --` · `= += -= *= /= %=`

Numbers are `f64` and support `0x` / `0b` / `0o` radix literals, `1_000_000` digit
separators and `1e-3` exponents.

### System calls

```kex
kxsys "echo hello";   // or: syscall "echo hello"
```

Runs through `cmd /C` on Windows and `sh -c` elsewhere.

### Modules

```kex
import "kexco";

kxout(kexco_version);
```

A module runs once and its top-level declarations are copied into the importing scope;
`export` marks a name explicitly.

---

## Standard library

`kex builtins` prints all 87. Highlights:

| Group | Functions |
| --- | --- |
| Output | `print` / `kxout`, `eprint`, `write`, `clear` |
| Input | `input` / `kxin`, `readline`, `readnumber` |
| Types | `type_of`, `to_str`, `to_num`, `to_int`, `to_bool`, `to_array`, `cast`, `is_null`, `deep_equal` |
| Strings | `upper`, `lower`, `trim`, `split`, `join`, `replace`, `contains`, `starts_with`, `ends_with`, `repeat`, `index_of`, `slice`, `char_at`, `chars`, `pad_start`, `pad_end`, `format`, `parse_num` |
| Arrays | `push`, `pop`, `insert`, `remove_at`, `sort`, `reverse`, `slice`, `sum`, `flatten`, `unique`, `map`, `filter`, `find`, `reduce` |
| Objects | `keys`, `values`, `has_key`, `remove_key`, `merge`, `object` |
| Math | `abs`, `floor`, `ceil`, `round`, `sqrt`, `pow`, `exp`, `log`, `log2`, `log10`, `sin`, `cos`, `tan`, `atan2`, `min`, `max`, `clamp`, `sign`, `random`, `range`, `now`, `pi`, `e` |
| Errors | `error`, `assert`, `exit` |
| Host | `host`, `host_ready`, `host_env` |

### Method syntax

Builtins are also reachable as methods on strings, arrays and objects. Many have a
`camelCase` alias, which is the *only* place `camelCase` is accepted:

```kex
let s = "  AbC  ";
kxout(s.trim(), s.upper(), s.contains("b"), s.repeat(2));
kxout(s.startsWith("A"), s.indexOf("b"), s.charAt(0), s.padStart(3, "0"));

let a = [3, 1, 2];
kxout(a.sort(), a.slice(0, 2), a.join("-"), a.sum(), a.indexOf(1));

let o = { a: 1, b: 2 };
kxout(o.keys(), o.values(), o.hasKey("a"), o.is_empty());
```

Use the `length(x)` builtin for lengths — `len` is a keyword, so `a.len()` and
`a.length()` do not parse.

### Array mutators return new arrays

**Array values are plain values, not references.** The mutating builtins (`push`, `pop`,
`insert`, `sort`, `reverse`, `remove_at`, `clear`, `flatten`, `unique`) operate on a copy
and return the new array, so the result must be assigned back:

```kex
let a = [];
a = push(a, "x");     // correct
a.push("y");          // silently discarded
```

Index assignment (`nums[0] = 42`) *does* mutate in place. See
[Known limitations](#known-limitations).

---

## kexco — the standard library pack

`kexco` gives Kex a web platform. It requires **Node.js on `PATH`**, which the engine
spawns as a child process and talks to over stdio.

```bash
kex pack install kexco
```

```kex
import "kexco";

fn render(items) {
    let list = query("#list");
    for item in items {
        append(list, element("li", item.label, "entry"));
    }
}

route("GET", "/api/items", 200, '{"ok":true}');
kxout("requests -> ", routes().length);
```

It provides 76 functions over ~70 host calls: **DOM** (create/query/append/attributes/
classes/styles/data/HTML), **events** (`on`, `off`, `fire`, `pump`), **timers**
(`timer_start`, `tick`, `pending_timers`), **local and session storage**, **fetch**
against a routable fake backend, JSON, base64 and URL helpers.

### Events are pumped, never called back

Handlers are registered by integer id and stored on the Kex side. The host records what
happened into a queue; `pump()` drains it and dispatches. Nothing is ever invoked across
the bridge, which keeps the protocol safe to move to a sandboxed host.

Timers are deterministic: the clock only moves when you ask, via `tick(ms)`.

### The headless adapter

`kexco/lib/kexco.js` ships a complete in-memory web platform — a real element tree with
working `querySelector`, two independent storage objects, an event queue and a route
table. It is selected automatically when no browser `document` is present, so tests and
scripts run identically in a terminal:

```
kexco 1.0.0 host ready (headless adapter, win32)
```

### Host bridge protocol

One JSON object per line, over stdin/stdout (stderr is free for logging). Values use a
tagged codec (`z` null, `b` bool, `n` num, `s` str, `a` array, `o` object, `r` range,
`e` error, `f` function).

```jsonc
// request
{"id":1,"fn":"dom.query","args":[{"k":"s","v":"#app"}]}
// response
{"id":1,"ok":true,"value":{"k":"o","v":{}}}
// failure
{"id":1,"ok":false,"error":"selector not found"}
```

The engine looks for `kexco.host.js` in the script directory, the installed pack, and next
to the executable.

---

## Packages

The pack directory resolves in this order:

1. `$KEX_HOME/pack`
2. `%LOCALAPPDATA%\kex\pack` (Windows)
3. `~/.kex/pack`

`kex/registry.json` is compiled into the binary, so `kex pack install kexco` works with no
setup. On-disk `registry.json` files take precedence, and a package can also be installed
straight from a folder or a `.zip`/`.tar.gz` URL:

```bash
kex pack install ./kexco
```

### Environment variables

| Variable | Purpose |
| --- | --- |
| `KEX_HOME` | Overrides the Kex home / pack directory |
| `KEXCO_HOST` | Absolute path to the host bridge script |
| `KEX_NO_HOST` | Set to disable the host bridge entirely |
| `KEXCO_ADAPTER` | `browser`, `headless` or `auto` (default) |
| `KEXCO_ROUTES` | JSON seed for the headless route table |
| `KEXCO_STORAGE` | JSON seed for headless `localStorage` |
| `KEXCO_DOC` | JSON seed for the headless document tree |

---

## Tests

There is no test harness; the tests are Kex scripts that assert and exit non-zero on
failure.

```bash
kex pack install kexco
kex tests/kexco_dom.kx     # 28/28 passing
kex tests/kexco_state.kx   # see Known limitations
```

`kex/main.kx` doubles as a smoke test of the language.

---

## Known limitations

These are current, reproducible engine bugs. They are documented rather than hidden so
you do not lose time to them.

### Array mutators do not mutate

`KexValue::Array` is a `Vec<KexValue>` (`kex/src/value.rs`), so array values have no
identity. `push`, `pop`, `sort`, `reverse`, `insert`, `remove_at`, `clear`, `flatten` and
`unique` clone and return a new array, and the method form (`a.push(x)`) discards the
result entirely. Reassign instead. This is why `tests/kexco_state.kx` currently reports
29/42 — its event and timer handlers push into an array from inside a closure, and that
write is lost.

### The classic `for` loop is unusable

```kex
for let j = 0; j < 3; j = j + 1; { }   // parse error
for let j = 0; j < 3; j++ { }          // parses, but never terminates
```

**Avoid it — the second form is an infinite loop.** Use `for ... in range(...)`, which
works correctly and supports a `by` step.

### `break` does not work inside `switch`

`Statement::Break` unconditionally raises `Signal::Break` (`kex/src/runtime.rs`), which
the `switch` handler does not catch, so it escapes as *"break outside of a loop"*. Switch
cases also fall through by design — `switch` runs every case from the match onward.

### `catch` cannot bind the error

`try { } catch (e) { }` is a parse error: the parser looks for an identifier directly
after `catch` and does not accept parentheses. Use a bare `catch { }`. `finally` works.

### `syscall` capture mode is statement-only

`kxsys capture "cmd";` parses as a statement, but `let out = kxsys capture "cmd";` is a
parse error, and the captured output is discarded. Only the default spawn mode is usable
in an expression position.

### `format()` specifiers are unreachable

Only `{}` parses. `{n:x}`, `{n:0x}` and friends fail in the lexer or parser, so the
`{d} {x} {X} {f} {p} {q} {k}` specifiers implemented in the builtin cannot be reached
from source. Use string interpolation instead.

### Parser error messages are garbled

Some parse failures print `expected 'token' (found 'token')` — the offending token is not
interpolated, so the message and its column number are the only useful signal.

### Other

- `a.len()` and `a.length()` do not parse, because `len` is a keyword token. Use the
  `length(x)` builtin.
- `to_str` on an array produces the inspect form `[1, 2, 3]`, not `1,2,3`.
- `bumpalo` is declared in `kex/Cargo.toml` but never imported.
- `docs/PROTOCOL.md` is referenced by `kexco/lib/kexco.js` and `kexco.host.js` but does
  not exist; the protocol is described above instead.
- `installer.iss` uses `IsTaskSelected`, which Inno Setup 6.7 renamed to
  `WizardIsTaskSelected`. It still compiles, with a deprecation hint.

---

## VS Code extension

Source lives in `vscode-extension/`. Manual install:

```bash
code --install-extension vscode-extension/kex-lang-1.0.0.vsix --force
```

It contributes a `kex` language (`source.kx`) with `//` and `/* */` comment
configuration, aliases `Kex` / `kx`, and the `.kx` extension.

The bundled `.vscode/tasks.json` adds a **KEXDRUN: Execute File** build task bound to
Ctrl+Shift+B, which runs the active file through the engine.

---

## Contributing

```bash
git checkout -b feature/my-change
cargo build --release --manifest-path kex/Cargo.toml
kex kex/main.kx
git commit -m "Add some AmazingFeature"
git push origin feature/my-change
```

Keep the three artifacts in sync when you touch the language or the extension:
`kex.exe`, `kex-lang-1.0.0.vsix` and `KexSetup.exe`.

---

## License

MIT — see [LICENSE](LICENSE). Copyright (c) 2026 Ayham Abrahamsson (click-for-games).

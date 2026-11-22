# kexco host bridge protocol

The Kex engine and the Node host exchange one JSON object per line.

- **stdout is reserved** for protocol frames in the host process. Diagnostics go to stderr.
- The engine assigns a numeric `id` to each request. The host echoes it in the response.
- Responses may arrive out of order when a host function is asynchronous.
- `KEX_NO_HOST` disables the bridge; `KEXCO_HOST` overrides the host script path.

## Framing

Request:

```json
{"id":1,"fn":"dom.query","args":[{"k":"s","v":"#app"}]}
```

Success:

```json
{"id":1,"ok":true,"value":{"k":"o","v":{"id":{"k":"s","v":"app"}}}}
```

Failure:

```json
{"id":1,"ok":false,"error":"selector not found"}
```

`host.ping` and `host.shutdown` are handled by the host entry point itself.

## Tagged value codec

| Tag | Type | Payload |
| --- | --- | --- |
| `z` | null | none |
| `b` | boolean | JSON boolean |
| `n` | number | JSON number |
| `s` | string | JSON string |
| `a` | array | array of tagged values |
| `o` | object | object of tagged values |
| `r` | range | `[start, end, step]` |
| `e` | error | message string |
| `f` | function | name string |

Untagged JSON objects are accepted as a convenience for host authors and decoded as plain
Kex objects.

## Function namespaces

| Prefix | Examples |
| --- | --- |
| `host.` | `host.info`, `host.version`, `host.capabilities` |
| `dom.` | `dom.create`, `dom.query`, `dom.append`, `dom.setHtml` |
| `events.` | `events.on`, `events.fire`, `events.drain` |
| `storage.` / `session.` | `storage.set`, `session.get`, `storage.all` |
| `timers.` | `timers.set`, `timers.tick`, `timers.pending` |
| `net.` | `net.route`, `net.get`, `net.post` |
| `json.` / `base64.` / `url.` / `env.` | utility calls |
| `server.` | localhost HTTP backend |

Call `host.capabilities()` to discover optional features. The Node host reports
`server` and `server.static`.

## Server request flow

1. Node receives an HTTP request and builds a request object.
2. The engine calls `server.wait(ms)` if it needs to wait for work.
3. The engine calls `server.drain()` to take queued requests.
4. Each request carries a handler id registered earlier by `server.route(...)`.
5. `server_pump()` in `kexco.kx` invokes the matching Kex function.
6. The handler returns `{ status, body, headers }`, a plain body, or `null`.
7. The engine calls `server.respond(id, status, body, headers)`; Node resolves the HTTP
   response.

`server.simulate(method, path, body, headers)` injects the same request shape without
opening a socket, which keeps backend tests deterministic.

## Request object

```js
{
  id: 7,
  method: "GET",
  path: "/api/users/42",
  url: "/api/users/42?full=1",
  query: { full: "1" },
  params: { id: "42" },
  headers: { /* node request headers */ },
  body: "",
  handler: 3
}
```

The handler value is an integer id into the Kex-side `kexco_handlers` table. No JavaScript
function reference is ever sent to Kex.

## Static files

`server.static(dir, prefix)` resolves `dir` under `KEXCO_ROOT`. Paths that escape that root
are rejected. Directory requests fall back to `index.html`. Common HTML, CSS, JavaScript,
JSON, image, font and text extensions receive an appropriate `Content-Type`.

## Async behavior

Host functions may return a promise. `kexco.host.js` waits for it and writes exactly one
response frame when it settles. Rejections become `ok: false` responses with the error
message.

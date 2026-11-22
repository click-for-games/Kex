'use strict';
/**
 * kexco.server.js - a small real HTTP backend for the Kex web platform.
 *
 * The Node host owns the TCP listener. Incoming requests are queued; Kex code
 * drains the queue with server_pump(), runs a Kex handler and answers with
 * server_respond(). This keeps every callback on the Kex side and makes the
 * protocol deterministic and sandboxed.
 *
 * Licensed under MIT.
 */

const http = require('http');
const fs = require('fs');
const path = require('path');

const MIME = {
  '.html': 'text/html; charset=utf-8',
  '.htm': 'text/html; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.json': 'application/json; charset=utf-8',
  '.svg': 'image/svg+xml',
  '.png': 'image/png',
  '.jpg': 'image/jpeg',
  '.jpeg': 'image/jpeg',
  '.gif': 'image/gif',
  '.ico': 'image/x-icon',
  '.webp': 'image/webp',
  '.txt': 'text/plain; charset=utf-8',
  '.map': 'application/json; charset=utf-8',
  '.woff': 'font/woff',
  '.woff2': 'font/woff2',
};

function createServerApi(options = {}) {
  const rootDir = options.root || process.env.KEXCO_ROOT || process.cwd();
  const routes = new Map();
  const pending = new Map();
  const queue = [];
  const log = [];
  let server = null;
  let nextId = 1;
  let staticDir = path.resolve(rootDir, 'public');
  let staticPrefix = '/';
  let waiters = [];
  let lastResponse = null;

  function normalise(method) {
    return String(method || 'GET').toUpperCase();
  }

  function key(method, routePath) {
    return `${normalise(method)} ${routePath}`;
  }

  function match(method, pathname) {
    const exact = routes.get(key(method, pathname));
    if (exact) return { handler: exact, params: {} };
    const wanted = pathname.split('/').filter(Boolean);
    for (const [routeKey, handler] of routes.entries()) {
      const [routeMethod, routePath] = routeKey.split(' ');
      if (routeMethod !== normalise(method)) continue;
      const parts = routePath.split('/').filter(Boolean);
      if (parts.length !== wanted.length) continue;
      const params = {};
      let ok = true;
      for (let i = 0; i < parts.length; i += 1) {
        if (parts[i].startsWith(':')) params[parts[i].slice(1)] = decodeURIComponent(wanted[i]);
        else if (parts[i] !== wanted[i]) { ok = false; break; }
      }
      if (ok) return { handler, params };
    }
    return null;
  }

  function notify() {
    const waiting = waiters;
    waiters = [];
    waiting.forEach((wake) => wake());
  }

  function writeResponse(res, status, body, headers) {
    const outgoing = {};
    Object.keys(headers || {}).forEach((name) => {
      outgoing[String(name)] = headers[name];
    });
    let payload = body === undefined || body === null ? '' : body;
    if (payload && typeof payload === 'object') {
      payload = JSON.stringify(payload);
      if (!outgoing['Content-Type'] && !outgoing['content-type']) {
        outgoing['Content-Type'] = 'application/json; charset=utf-8';
      }
    } else if (typeof payload === 'string' && !outgoing['Content-Type'] && !outgoing['content-type']) {
      outgoing['Content-Type'] = 'text/html; charset=utf-8';
    }
    if (res && res.writeHead) {
      res.writeHead(status, outgoing);
      res.end(payload);
    }
    lastResponse = { status, body: payload, headers: outgoing };
  }

  function serveStatic(entry, res) {
    const prefix = staticPrefix === '/' ? '' : staticPrefix;
    if (prefix && !entry.path.startsWith(prefix)) return false;
    let relative = decodeURIComponent(entry.path.slice(prefix.length));
    if (!relative || relative === '/') relative = 'index.html';
    const target = path.resolve(staticDir, `.${relative.startsWith('/') ? '' : '/'}${relative}`);
    if (target !== staticDir && !target.startsWith(staticDir + path.sep)) return false;
    if (entry.path.endsWith('/') && !path.extname(target)) {
      const index = path.join(target, 'index.html');
      if (fs.existsSync(index)) return sendFile(index, res);
    }
    if (!fs.existsSync(target) || !fs.statSync(target).isFile()) return false;
    return sendFile(target, res);
  }

  function sendFile(file, res) {
    try {
      const body = fs.readFileSync(file);
      const type = MIME[path.extname(file).toLowerCase()] || 'application/octet-stream';
      res.writeHead(200, { 'Content-Type': type, 'Cache-Control': 'no-cache' });
      res.end(body);
      lastResponse = { status: 200, body: body.toString('utf8'), headers: { 'Content-Type': type } };
      return true;
    } catch (err) {
      return false;
    }
  }

  function handle(req, res) {
    const chunks = [];
    req.on('data', (chunk) => chunks.push(chunk));
    req.on('end', () => {
      let url;
      try {
        url = new URL(req.url, 'http://kexco.local');
      } catch (err) {
        writeResponse(res, 400, 'bad request');
        return;
      }
      const query = {};
      url.searchParams.forEach((value, name) => { query[name] = value; });
      const matched = match(req.method, url.pathname);
      const entry = {
        id: nextId++,
        method: normalise(req.method),
        path: url.pathname,
        url: req.url,
        query,
        params: matched ? matched.params : {},
        headers: req.headers,
        body: Buffer.concat(chunks).toString('utf8'),
        handler: matched ? matched.handler : null,
      };
      log.push({ id: entry.id, method: entry.method, path: entry.path, status: null });
      if (log.length > 100) log.shift();
      if (!matched) {
        if (serveStatic(entry, res)) {
          log[log.length - 1].status = 200;
        } else {
          writeResponse(res, 404, JSON.stringify({ error: 'not found', path: entry.path }), {
            'Content-Type': 'application/json; charset=utf-8',
          });
          log[log.length - 1].status = 404;
        }
        return;
      }
      pending.set(entry.id, res);
      queue.push(entry);
      notify();
    });
  }

  return {
    name: 'kexco-server',
    listen(port, host) {
      if (server) throw new Error('server is already listening; call server_close() first');
      const chosen = port === undefined || port === null ? 8080 : Number(port);
      const bind = host || '127.0.0.1';
      server = http.createServer(handle);
      server.on('error', (err) => {
        process.stderr.write(`kexco server error: ${err.message}\n`);
      });
      server.listen(chosen, bind);
      return { port: chosen, host: bind, address: `${bind}:${chosen}` };
    },
    close() {
      if (!server) return false;
      server.close();
      server = null;
      pending.forEach((res) => writeResponse(res, 503, 'server closed'));
      pending.clear();
      queue.length = 0;
      notify();
      return true;
    },
    running: () => Boolean(server && server.listening),
    address() {
      if (!server) return null;
      const info = server.address();
      return info && typeof info === 'object' ? { port: info.port, host: info.address } : null;
    },
    route(method, routePath, handler) {
      if (!routePath || !String(routePath).startsWith('/')) {
        throw new Error('server routes must start with /');
      }
      routes.set(key(method, routePath), handler);
      return String(routePath);
    },
    unroute(method, routePath) {
      return routes.delete(key(method, routePath));
    },
    routes: () => [...routes.keys()],
    static(dir, prefix) {
      staticDir = path.resolve(rootDir, String(dir || 'public'));
      staticPrefix = String(prefix || '/');
      return staticDir;
    },
    staticInfo: () => ({ dir: staticDir, prefix: staticPrefix, root: rootDir }),
    drain: () => queue.splice(0, queue.length),
    pendingCount: () => queue.length,
    respond(id, status, body, headers) {
      const res = pending.get(Number(id));
      const code = Number(status) || 200;
      writeResponse(res, code, body, headers || {});
      pending.delete(Number(id));
      const record = log.find((item) => item.id === Number(id));
      if (record) record.status = code;
      return true;
    },
    wait(ms) {
      if (queue.length) return Promise.resolve(queue.length);
      const delay = Math.max(0, Number(ms) || 0);
      return new Promise((resolve) => {
        let timer = null;
        const wake = () => {
          if (timer) clearTimeout(timer);
          waiters = waiters.filter((item) => item !== wake);
          resolve(queue.length);
        };
        timer = setTimeout(wake, delay);
        waiters.push(wake);
      });
    },
    simulate(method, routePath, body, headers) {
      const matched = match(method, routePath || '/');
      const entry = {
        id: nextId++,
        method: normalise(method),
        path: routePath || '/',
        url: routePath || '/',
        query: {},
        params: matched ? matched.params : {},
        headers: headers || {},
        body: body === undefined ? '' : String(body),
        handler: matched ? matched.handler : null,
      };
      pending.set(entry.id, { simulated: true });
      log.push({ id: entry.id, method: entry.method, path: entry.path, status: null });
      if (log.length > 100) log.shift();
      queue.push(entry);
      notify();
      return entry.id;
    },
    lastResponse: () => lastResponse,
    requests: () => log.slice(),
  };
}

module.exports = { createServerApi, MIME };

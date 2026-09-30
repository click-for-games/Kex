#!/usr/bin/env node
/**
 * kexco host entry point.
 *
 * The Kex engine spawns this file with `node` and talks to it over stdin and
 * stdout using the newline delimited JSON protocol described in
 * `kexco.js`. stdout is reserved for the protocol, so anything diagnostic is
 * written to stderr.
 *
 * Environment:
 *   KEXCO_ADAPTER   "browser" | "headless" (default: auto detect)
 *   KEXCO_DOC       path to a JSON file seeding the headless document tree
 *   KEXCO_ROUTES    path to a JSON file of { "GET /url": { status, body } }
 *   KEXCO_STORAGE   path to a JSON file used as the initial localStorage
 *
 * Licensed under MIT.
 */

'use strict';

const fs = require('fs');
const path = require('path');
const readline = require('readline');
const kexco = require(path.join(__dirname, 'kexco.js'));

function loadJson(file, fallback) {
  if (!file) return fallback;
  try {
    return JSON.parse(fs.readFileSync(file, 'utf8'));
  } catch (err) {
    process.stderr.write(`kexco: could not read ${file}: ${err.message}\n`);
    return fallback;
  }
}

// ---------------------------------------------------------------------------
// adapter selection
// ---------------------------------------------------------------------------

function buildAdapter() {
  const wanted = (process.env.KEXCO_ADAPTER || 'auto').toLowerCase();

  const routes = loadJson(process.env.KEXCO_ROUTES, {}) || {};
  const storageSeed = loadJson(process.env.KEXCO_STORAGE, {}) || {};
  const docSeed = loadJson(process.env.KEXCO_DOC, null);

  const seedChildren = [];
  if (docSeed && Array.isArray(docSeed.children)) {
    docSeed.children.forEach((spec) => {
      const node = kexco.HeadlessElement ? new kexco.HeadlessElement(spec.tag || 'div') : null;
      if (!node) return;
      if (spec.id) node.setAttribute('id', spec.id);
      if (spec.class) node.setAttribute('class', spec.class);
      if (spec.text) node.textContent = String(spec.text);
      if (spec.tag === '#text') node.textContent = String(spec.text || '');
      Object.keys(spec.attributes || {}).forEach((k) => node.setAttribute(k, spec.attributes[k]));
      Object.keys(spec.style || {}).forEach((k) => {
        node.style[k] = String(spec.style[k]);
      });
      (spec.children || []).forEach((child) => {
        const sub = new kexco.HeadlessElement(child.tag || 'div');
        if (child.id) sub.setAttribute('id', child.id);
        if (child.class) sub.setAttribute('class', child.class);
        if (child.text) sub.textContent = String(child.text);
        node.appendChild(sub);
      });
      seedChildren.push(node);
    });
  }

  const document = kexco.seedDocument(seedChildren);
  const storage = new kexco.HeadlessStorage();
  Object.keys(storageSeed).forEach((k) => storage.setItem(k, storageSeed[k]));

  const headless = kexco.createHeadlessAdapter({
    document,
    localStorage: storage,
  });

  Object.keys(routes).forEach((key) => {
    const [method, url] = key.split(' ');
    const stub = routes[key];
    headless.route(method, url, stub.status === undefined ? 200 : stub.status, stub.body, stub.headers);
  });

  if (wanted === 'browser') {
    const browser = kexco.createBrowserAdapter();
    if (browser) return browser;
    process.stderr.write('kexco: no browser available, falling back to the headless adapter\n');
  }
  return headless;
}

const adapter = buildAdapter();
const library = kexco.buildLibrary(adapter);

process.stderr.write(
  `kexco ${kexco.VERSION} host ready (${adapter.name} adapter, ${require('os').platform()})\n`
);

// ---------------------------------------------------------------------------
// protocol loop
// ---------------------------------------------------------------------------

const rl = readline.createInterface({ input: process.stdin, terminal: false });
const out = process.stdout;

// `doFetch` is async, so responses can complete out of order; the id field
// lets the engine match them up.
function write(payload) {
  out.write(JSON.stringify(payload) + '\n');
}

rl.on('line', (line) => {
  const text = line.trim();
  if (!text) return;

  let request;
  try {
    request = JSON.parse(text);
  } catch (err) {
    write({ id: null, ok: false, error: `malformed request: ${err.message}` });
    return;
  }

  const id = request.id === undefined ? null : request.id;
  const fn = request.fn;
  const args = Array.isArray(request.args) ? request.args : [];

  if (fn === 'host.ping') {
    write({ id, ok: true, value: { k: 's', v: 'pong' } });
    return;
  }

  if (fn === 'host.shutdown') {
    write({ id, ok: true, value: { k: 'z' } });
    setTimeout(() => process.exit(0), 0);
    return;
  }

  let result;
  try {
    result = library.dispatch(fn, args);
  } catch (err) {
    write({ id, ok: false, error: err.message || String(err) });
    return;
  }

  if (result && typeof result.then === 'function') {
    result.then(
      (value) => write({ id, ok: true, value: kexco.encode(value) }),
      (err) => write({ id, ok: false, error: err.message || String(err) })
    );
    return;
  }

  write({ id, ok: true, value: kexco.encode(result) });
});

rl.on('close', () => process.exit(0));

process.on('uncaughtException', (err) => {
  process.stderr.write(`kexco: uncaught ${err.stack || err.message}\n`);
});

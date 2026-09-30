/**
 * kexco - the web library for the Kex programming language.
 *
 * kexco gives Kex scripts a first class bridge to the web platform: DOM,
 * fetch, storage, timers, events, URLs and JSON. It ships two adapters behind
 * one API:
 *
 *   - a browser adapter  (document / window / localStorage / fetch)
 *   - a headless adapter  (an in-memory DOM + a fetch shim) so the exact same
 *     .kx code can be exercised from a terminal test run.
 *
 * The Kex side talks to this file over a newline delimited JSON protocol, one
 * request object per line on stdin, one response object per line on stdout.
 * See `docs/PROTOCOL.md` for the framing and `kexco.kx` for the Kex facade.
 *
 * Wire protocol (shared with src/host.rs in the Kex engine):
 *
 *   request   {"id":1,"fn":"dom.query","args":[{"k":"s","v":"#app"}]}
 *   response  {"id":1,"ok":true,"value":{"k":"o","v":{...}}}
 *   failure   {"id":1,"ok":false,"error":"selector not found"}
 *
 * Licensed under MIT.
 */

'use strict';

const VERSION = '1.0.0';

// ===========================================================================
// 1. VALUE CODEC - identical to the Rust implementation in kex/src/host.rs
// ===========================================================================

const K = {
  NULL: 'z',
  BOOL: 'b',
  NUM: 'n',
  STR: 's',
  ARRAY: 'a',
  OBJECT: 'o',
  RANGE: 'r',
  ERROR: 'e',
  FUNC: 'f',
};

/** Encode a KexValue into the tagged wire form. */
function encode(value) {
  if (value === null || value === undefined) return { k: K.NULL };
  switch (typeof value) {
    case 'boolean':
      return { k: K.BOOL, v: value };
    case 'number':
      return { k: K.NUM, v: Number.isFinite(value) ? value : 0 };
    case 'string':
      return { k: K.STR, v: value };
    case 'bigint':
      return { k: K.NUM, v: Number(value) };
    default:
      break;
  }
  if (Array.isArray(value)) {
    return { k: K.ARRAY, v: value.map(encode) };
  }
  if (value instanceof Error) {
    return { k: K.ERROR, v: value.message };
  }
  if (typeof value === 'object') {
    const out = {};
    for (const key of Object.keys(value)) out[key] = encode(value[key]);
    return { k: K.OBJECT, v: out };
  }
  return { k: K.STR, v: String(value) };
}

/** Decode the tagged wire form back into a plain JavaScript value. */
function decode(json) {
  if (json === null || json === undefined) return null;
  if (typeof json !== 'object') return json;
  if (Array.isArray(json)) return json.map(decode);
  if (json.k === undefined) {
    const out = {};
    for (const key of Object.keys(json)) out[key] = decode(json[key]);
    return out;
  }
  const payload = json.v;
  switch (json.k) {
    case K.NULL:
      return null;
    case K.BOOL:
      return payload === true;
    case K.NUM:
      return typeof payload === 'number' ? payload : 0;
    case K.STR:
      return typeof payload === 'string' ? payload : '';
    case K.ARRAY:
      return Array.isArray(payload) ? payload.map(decode) : [];
    case K.OBJECT: {
      const out = {};
      if (payload && typeof payload === 'object') {
        for (const key of Object.keys(payload)) out[key] = decode(payload[key]);
      }
      return out;
    }
    case K.RANGE: {
      const n = Array.isArray(payload) ? payload : [0, 0, 1];
      return { start: n[0] | 0, end: n[1] | 0, step: n[2] === undefined ? 1 : n[2] };
    }
    case K.ERROR:
      throw new Error(typeof payload === 'string' ? payload : 'kexco: host error');
    case K.FUNC:
      return payload;
    default:
      return payload;
  }
}

// ===========================================================================
// 2. HEADLESS ADAPTER - a tiny in-memory DOM used by the terminal testers
// ===========================================================================

function isTextNode(node) {
  return node.tagName === '#TEXT';
}

class HeadlessElement {
  constructor(tag) {
    this.tagName = String(tag).toUpperCase();
    this.children = [];
    this.attributes = {};
    this.style = {};
    this.listeners = {};
    this.rawHtml = null;
    this.parentNode = null;
    this.data = '';
    this.id = '';
    this.className = '';
    this.dataset = {};
  }

  /** Derived from the child list, exactly like the real DOM. */
  get textContent() {
    if (isTextNode(this)) return this.data;
    return this.children.map((c) => c.textContent).join('');
  }

  set textContent(text) {
    this.rawHtml = null;
    const value = text === undefined || text === null ? '' : String(text);
    if (isTextNode(this)) {
      this.data = value;
      return;
    }
    this.children = [];
    if (value !== '') this.children.push(textNode(value));
  }

  get classList() {
    const self = this;
    return {
      add(...names) {
        const set = new Set(self.className.split(/\s+/).filter(Boolean));
        names.forEach((n) => set.add(n));
        self.className = [...set].join(' ');
      },
      remove(...names) {
        const set = new Set(self.className.split(/\s+/).filter(Boolean));
        names.forEach((n) => set.delete(n));
        self.className = [...set].join(' ');
      },
      contains(name) {
        return self.className.split(/\s+/).includes(name);
      },
      toggle(name) {
        if (this.contains(name)) this.remove(name);
        else this.add(name);
      },
    };
  }

  setAttribute(name, value) {
    if (name === 'id') this.id = String(value);
    if (name === 'class') this.className = String(value);
    this.attributes[name] = String(value);
  }

  getAttribute(name) {
    if (name in this.attributes) return this.attributes[name];
    if (name === 'id') return this.id || null;
    if (name === 'class') return this.className || null;
    return null;
  }

  removeAttribute(name) {
    delete this.attributes[name];
    if (name === 'id') this.id = '';
    if (name === 'class') this.className = '';
  }

  hasAttribute(name) {
    return this.getAttribute(name) !== null;
  }

  appendChild(child) {
    if (child.parentNode) child.parentNode.removeChild(child);
    this.children.push(child);
    if (child) child.parentNode = this;
    return child;
  }

  insertBefore(child, before) {
    if (child.parentNode) child.parentNode.removeChild(child);
    const i = before ? this.children.indexOf(before) : -1;
    if (i < 0) this.children.push(child);
    else this.children.splice(i, 0, child);
    if (child) child.parentNode = this;
    return child;
  }

  append(...nodes) {
    nodes.forEach((n) => this.appendChild(n));
  }

  removeChild(child) {
    const i = this.children.indexOf(child);
    if (i >= 0) {
      this.children.splice(i, 1);
      if (child) child.parentNode = null;
    }
    return child;
  }

  remove() {
    const parent = this.parentNode;
    if (parent) parent.removeChild(this);
  }

  addEventListener(type, handler) {
    (this.listeners[type] = this.listeners[type] || []).push(handler);
  }

  removeEventListener(type, handler) {
    const list = this.listeners[type] || [];
    const i = list.indexOf(handler);
    if (i >= 0) list.splice(i, 1);
  }

  /** Synchronously dispatch an event, mirroring the browser contract. */
  dispatchEvent(event) {
    const payload = {
      type: event.type,
      target: this,
      currentTarget: this,
      key: event.key,
      detail: event.detail,
      preventDefault() {},
      stopPropagation() {},
    };
    (this.listeners[event.type] || []).forEach((handler) => {
      if (typeof handler === 'function') handler(payload);
    });
    return true;
  }

  querySelector(selector) {
    return findAll(this, selector)[0] || null;
  }

  querySelectorAll(selector) {
    return findAll(this, selector);
  }

  get innerHTML() {
    if (this.rawHtml !== null) return this.rawHtml;
    return this.children.map((c) => serialize(c)).join('');
  }

  set innerHTML(html) {
    this.children = [];
    const source = html === undefined || html === null ? '' : String(html);
    parseHtml(this, source);
    this.rawHtml = source;
  }

  get firstChild() {
    return this.children[0] || null;
  }

  get innerText() {
    return this.textContent;
  }

  set innerText(text) {
    this.textContent = String(text);
  }

  focus() {
    this.focused = true;
  }

  get outerHTML() {
    return serialize(this);
  }

  matches(selector) {
    return matches(this, selector);
  }
}

function textNode(value) {
  const node = new HeadlessElement('#text');
  node.data = String(value);
  return node;
}

const VOID_TAGS = new Set([
  'area', 'base', 'br', 'col', 'embed', 'hr', 'img', 'input',
  'link', 'meta', 'param', 'source', 'track', 'wbr',
]);

const ENTITIES = {
  amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ',
};

function decodeEntities(text) {
  return text.replace(/&(#x?[0-9a-fA-F]+|[a-zA-Z]+);/g, (whole, body) => {
    if (body[0] === '#') {
      const code =
        body[1] === 'x' || body[1] === 'X'
          ? parseInt(body.slice(2), 16)
          : parseInt(body.slice(1), 10);
      return Number.isFinite(code) ? String.fromCodePoint(code) : whole;
    }
    const named = ENTITIES[body.toLowerCase()];
    return named === undefined ? whole : named;
  });
}

/**
 * A deliberately small HTML parser: it understands tags, attributes, void
 * elements and text, which is everything a Kex program realistically needs.
 * Anything more exotic stays in `rawHtml` so getHtml() still round trips.
 */
function parseHtml(root, html) {
  const stack = [root];
  const push = (node) => {
    const parent = stack[stack.length - 1];
    if (isTextNode(node)) {
      if (node.textContent) parent.appendChild(node);
      return;
    }
    parent.appendChild(node);
    if (!VOID_TAGS.has(node.tagName.toLowerCase())) stack.push(node);
  };

  const tag = /<\/?([a-zA-Z][a-zA-Z0-9-]*)((?:\s+[^<>]*?)?)\/?>/g;
  let cursor = 0;
  let match = tag.exec(html);

  while (match) {
    if (match.index > cursor) {
      const text = decodeEntities(html.slice(cursor, match.index));
      if (text.trim()) push(textNode(text));
    }
    const [whole, name, attrText] = match;
    if (whole[1] === '/') {
      if (stack.length > 1) stack.pop();
    } else {
      const node = new HeadlessElement(name);
      const attr = /([a-zA-Z_:][-a-zA-Z0-9_:.]*)(?:\s*=\s*("([^"]*)"|'([^']*)'|([^\s"'>]+)))?/g;
      let hit = attr.exec(attrText || '');
      while (hit) {
        const key = hit[1];
        const value = hit[3] !== undefined ? hit[3]
          : hit[4] !== undefined ? hit[4]
          : hit[5] !== undefined ? hit[5]
          : '';
        node.setAttribute(key, decodeEntities(value));
        hit = attr.exec(attrText || '');
      }
      push(node);
    }
    cursor = match.index + whole.length;
    match = tag.exec(html);
  }

  if (cursor < html.length) {
    const text = decodeEntities(html.slice(cursor));
    if (text.trim()) push(textNode(text));
  }
}

function serialize(node) {
  if (!node) return '';
  if (isTextNode(node)) return node.textContent;
  const attrs = Object.keys(node.attributes)
    .map((k) => ` ${k}="${node.attributes[k]}"`)
    .join('');
  const id = node.id ? ` id="${node.id}"` : '';
  const cls = node.className ? ` class="${node.className}"` : '';
  return `<${node.tagName.toLowerCase()}${id}${cls}${attrs}>${
    node.textContent || node.innerHTML
  }</${node.tagName.toLowerCase()}>`;
}

function matches(node, selector) {
  if (isTextNode(node)) return false;
  const sel = String(selector).trim();
  if (sel === '*') return true;
  if (sel.startsWith('#')) return node.id === sel.slice(1);
  if (sel.startsWith('.')) return node.classList.contains(sel.slice(1));
  const attr = sel.match(/^([a-zA-Z][\w-]*)?\[([\w-]+)(?:=["']?([^"'\]]*)["']?)?\]$/);
  if (attr) {
    const [, tag, name, value] = attr;
    if (tag && node.tagName !== tag.toUpperCase()) return false;
    if (value === undefined) return node.hasAttribute(name);
    return node.getAttribute(name) === value;
  }
  const parts = sel.split(/(?=[.#])/);
  for (const part of parts) {
    if (!part) continue;
    if (part.startsWith('#')) {
      if (node.id !== part.slice(1)) return false;
    } else if (part.startsWith('.')) {
      if (!node.classList.contains(part.slice(1))) return false;
    } else if (node.tagName !== part.toUpperCase()) {
      return false;
    }
  }
  return true;
}

function findAll(root, selector) {
  const out = [];
  const walk = (node) => {
    for (const child of node.children) {
      if (matches(child, selector)) out.push(child);
      if (child.children && child.children.length) walk(child);
    }
  };
  walk(root);
  return out;
}

class HeadlessStorage {
  constructor() {
    this.map = new Map();
  }

  getItem(key) {
    return this.map.has(String(key)) ? this.map.get(String(key)) : null;
  }

  setItem(key, value) {
    this.map.set(String(key), String(value));
  }

  removeItem(key) {
    this.map.delete(String(key));
  }

  clear() {
    this.map.clear();
  }

  key(index) {
    return [...this.map.keys()][index] ?? null;
  }

  get length() {
    return this.map.size;
  }
}

/** Builds a document tree for the headless adapter. */
function seedDocument(bodyChildren) {
  const html = new HeadlessElement('html');
  const head = new HeadlessElement('head');
  head.appendChild(textNode('kexco headless'));
  const body = new HeadlessElement('body');
  (bodyChildren || []).forEach((child) => body.appendChild(child));
  html.appendChild(head);
  html.appendChild(body);
  return { documentElement: html, head, body };
}

function createHeadlessAdapter(options) {
  const config = options || {};
  const { document, window, localStorage, sessionStorage, fetch } = config;
  const storage = localStorage || new HeadlessStorage();
  const session = sessionStorage || new HeadlessStorage();
  const routeTable = new Map();

  function requireElement(value, who) {
    if (value instanceof HeadlessElement) return value;
    if (value && value.__kexcoNode) {
      const node = registry.get(value.id);
      if (node) return node;
    }
    throw new Error(`${who}: expected a DOM element handle, received ${typeof value}`);
  }

  function resolveElement(handle) {
    if (handle === null || handle === undefined) return null;
    if (handle instanceof HeadlessElement) return handle;
    if (handle && handle.__kexcoNode) return registry.get(handle.id) || null;
    return null;
  }

  // Elements cross the wire as {__kexcoNode: true, id} handles so identity
  // survives the JSON round trip, exactly like the browser adapter.
  const registry = new Map();
  let handleCounter = 0;
  function wrap(node) {
    if (!node) return null;
    const id = ++handleCounter;
    registry.set(id, node);
    return { __kexcoNode: true, id };
  }

  // Events and timers never call back into Kex directly. The host records
  // them and the Kex runtime drains the queue, which keeps the whole bridge
  // free of host-to-guest callbacks (and therefore wasm safe).
  const eventQueue = [];
  const timerQueue = [];

  const api = {
    name: 'headless',
    version: VERSION,
    userAgent: 'kexco-headless/' + VERSION,
    language: 'en',
    readyState: 'complete',

    document: {
      title: 'kexco headless document',
      readyState: 'complete',
      body: document ? document.body : null,
      documentElement: document ? document.documentElement : null,
      head: document ? document.head : null,
    },

    window: window || {},

    storage: {
      get length() {
        return storage.length;
      },
      key: (i) => storage.key(i),
      getItem: (k) => storage.getItem(k),
      setItem: (k, v) => {
        storage.setItem(k, v);
        return null;
      },
      removeItem: (k) => {
        storage.removeItem(k);
        return null;
      },
      clear: () => {
        storage.clear();
        return null;
      },
    },

    session: {
      get length() {
        return session.length;
      },
      key: (i) => session.key(i),
      getItem: (k) => session.getItem(k),
      setItem: (k, v) => {
        session.setItem(k, v);
        return null;
      },
      removeItem: (k) => {
        session.removeItem(k);
        return null;
      },
      clear: () => {
        session.clear();
        return null;
      },
    },

    // ---- DOM construction
    createElement(tag) {
      return wrap(new HeadlessElement(tag));
    },
    createTextNode(text) {
      return wrap(textNode(text));
    },
    createDocumentFragment() {
      return wrap(new HeadlessElement('#fragment'));
    },
    querySelector(selector) {
      const root = document ? document.documentElement : null;
      if (!root) return null;
      return wrap(root.querySelector(selector));
    },
    querySelectorAll(selector) {
      const root = document ? document.documentElement : null;
      if (!root) return [];
      return root.querySelectorAll(selector).map(wrap);
    },
    getElementById(id) {
      return api.querySelector(`#${id}`);
    },
    body() {
      return wrap(document ? document.body : null);
    },
    documentElement() {
      return wrap(document ? document.documentElement : null);
    },
    getElementsByTagName(tag) {
      return api.querySelectorAll(tag);
    },
    getElementsByClassName(name) {
      return api.querySelectorAll(`.${name}`);
    },

    // ---- element operations
    append(parent, child) {
      const p = requireElement(parent, 'dom.append');
      const c = resolveElement(child);
      if (!c) throw new Error('dom.append: child is not an element');
      p.appendChild(c);
      return wrap(p);
    },
    prepend(parent, child) {
      const p = requireElement(parent, 'dom.prepend');
      const c = resolveElement(child);
      if (!c) throw new Error('dom.prepend: child is not an element');
      p.insertBefore(c, p.children[0] || null);
      return wrap(p);
    },
    remove(node) {
      const c = resolveElement(node);
      if (c) c.remove();
      return null;
    },
    setText(node, text) {
      const c = requireElement(node, 'dom.setText');
      c.textContent = String(text);
      return wrap(c);
    },
    getText(node) {
      const c = requireElement(node, 'dom.getText');
      return c.textContent;
    },
    setHtml(node, html) {
      const c = requireElement(node, 'dom.setHtml');
      c.innerHTML = String(html);
      return wrap(c);
    },
    getHtml(node) {
      const c = requireElement(node, 'dom.getHtml');
      return c.innerHTML;
    },
    setAttr(node, name, value) {
      const c = requireElement(node, 'dom.setAttr');
      c.setAttribute(String(name), String(value));
      return wrap(c);
    },
    getAttr(node, name) {
      const c = requireElement(node, 'dom.getAttr');
      return c.getAttribute(String(name));
    },
    removeAttr(node, name) {
      const c = requireElement(node, 'dom.removeAttr');
      c.removeAttribute(String(name));
      return wrap(c);
    },
    hasAttr(node, name) {
      const c = requireElement(node, 'dom.hasAttr');
      return c.hasAttribute(String(name));
    },
    addClass(node, name) {
      const c = requireElement(node, 'dom.addClass');
      c.classList.add(String(name));
      return wrap(c);
    },
    removeClass(node, name) {
      const c = requireElement(node, 'dom.removeClass');
      c.classList.remove(String(name));
      return wrap(c);
    },
    hasClass(node, name) {
      const c = requireElement(node, 'dom.hasClass');
      return c.classList.contains(String(name));
    },
    setStyle(node, property, value) {
      const c = requireElement(node, 'dom.setStyle');
      c.style[String(property)] = String(value);
      return wrap(c);
    },
    getStyle(node, property) {
      const c = requireElement(node, 'dom.getStyle');
      const key = String(property);
      return c.style[key] !== undefined ? c.style[key] : '';
    },
    setData(node, key, value) {
      const c = requireElement(node, 'dom.setData');
      c.dataset[String(key)] = String(value);
      return wrap(c);
    },
    getData(node, key) {
      const c = requireElement(node, 'dom.getData');
      const v = c.dataset[String(key)];
      return v === undefined ? null : v;
    },
    getTag(node) {
      return requireElement(node, 'dom.getTag').tagName;
    },
    getId(node) {
      return requireElement(node, 'dom.getId').id;
    },
    childCount(node) {
      return requireElement(node, 'dom.childCount').children.length;
    },

    // ---- events
    on(node, type, handlerId) {
      const c = requireElement(node, 'dom.on');
      c.__kexcoHandlers = c.__kexcoHandlers || {};
      c.__kexcoHandlers[String(handlerId)] = String(type);
      return wrap(c);
    },
    fire(node, type, detail) {
      const c = requireElement(node, 'dom.fire');
      (c.listeners[type] || []).forEach((h) =>
        h({ type, target: c, detail, preventDefault() {}, stopPropagation() {} })
      );
      Object.keys(c.__kexcoHandlers || {}).forEach((id) => {
        if (c.__kexcoHandlers[id] !== String(type)) return;
        eventQueue.push({
          node: wrap(c),
          type: String(type),
          handler: Number(id),
          detail: detail === undefined ? null : detail,
        });
      });
      c.__kexcoFired = c.__kexcoFired || [];
      c.__kexcoFired.push({ type, detail: detail === undefined ? null : detail });
      return c.__kexcoFired.length;
    },
    fired(node) {
      const c = resolveElement(node);
      if (!c || !c.__kexcoFired) return [];
      return c.__kexcoFired.slice();
    },
    drain() {
      const out = eventQueue.slice();
      eventQueue.length = 0;
      return out;
    },

    // ---- timers (deterministic: the clock only moves when kex asks)
    setTimeout(delay) {
      const id = timerQueue.length + 1;
      timerQueue.push({ id, due: Number(delay) || 0, fired: false });
      return id;
    },
    clearTimeout(id) {
      const i = timerQueue.findIndex((t) => t.id === Number(id));
      if (i >= 0) timerQueue.splice(i, 1);
      return null;
    },
    tick(ms) {
      const budget = Number(ms) || 0;
      const fired = [];
      for (let i = timerQueue.length - 1; i >= 0; i -= 1) {
        if (timerQueue[i].due <= budget && !timerQueue[i].fired) {
          timerQueue[i].fired = true;
          fired.push(timerQueue[i].id);
        }
      }
      return fired;
    },
    pendingTimers() {
      return timerQueue
        .filter((t) => !t.fired)
        .map((t) => ({ id: t.id, due: t.due }));
    },

    // ---- network
    route(method, path, status, body, headers) {
      routeTable.set(`${String(method).toUpperCase()} ${path}`, {
        status: Number(status) || 200,
        body: body === undefined ? '' : body,
        headers: headers || {},
      });
      return path;
    },
    async fetch(method, url, options) {
      const opts = options || {};
      const key = `${String(method).toUpperCase()} ${url}`;
      const stub = routeTable.get(key);
      if (stub) {
        return {
          ok: stub.status >= 200 && stub.status < 300,
          status: stub.status,
          headers: Object.assign({}, stub.headers),
          text: async () => String(stub.body),
          json: async () => JSON.parse(String(stub.body)),
        };
      }
      if (fetch) {
        const res = await fetch(url, Object.assign({ method }, opts));
        return {
          ok: res.ok,
          status: res.status,
          headers: Object.assign({}, res.headers || {}),
          text: async () => res.text(),
          json: async () => res.json(),
        };
      }
      throw new Error(
        `fetch: no route registered for '${key}'. Call net.route(method, url, status, body) first.`
      );
    },
    routes() {
      return [...routeTable.keys()];
    },

    // ---- misc
    jsonParse(text) {
      return JSON.parse(String(text));
    },
    jsonStringify(value) {
      return JSON.stringify(value === undefined ? null : value);
    },
    btoa(text) {
      return Buffer.from(String(text), 'utf8').toString('base64');
    },
    atob(text) {
      return Buffer.from(String(text), 'base64').toString('utf8');
    },
    now() {
      return Date.now();
    },
    uuid() {
      const hex = () => Math.floor(Math.random() * 0x10000).toString(16).padStart(4, '0');
      return `${hex()}${hex()}-${hex()}-4${hex().slice(1)}-a${hex().slice(1)}-${hex()}${hex()}${hex()}`;
    },
    location() {
      return {
        href: 'https://kexco.local/',
        host: 'kexco.local',
        hostname: 'kexco.local',
        protocol: 'https:',
        pathname: '/',
        search: '',
        hash: '',
      };
    },
  };

  return api;
}

// ===========================================================================
// 3. BROWSER ADAPTER - the real web platform
// ===========================================================================

function createBrowserAdapter() {
  if (typeof document === 'undefined') return null;

  const requireElement = (value, who) => {
    if (value && typeof value === 'object' && value.__kexcoNode) return value.__kexcoNode;
    if (value && typeof value.nodeType === 'number') return value;
    throw new Error(`${who}: expected a DOM element handle`);
  };

  // DOM nodes cross the boundary as {__kexcoNode: true, id: <n>} handles so
  // that the payload stays JSON serialisable.
  const registry = new Map();
  let handleCounter = 0;
  const wrap = (node) => {
    if (!node) return null;
    const id = ++handleCounter;
    registry.set(id, node);
    return { __kexcoNode: true, id };
  };
  const unwrap = (handle) => {
    if (handle && handle.__kexcoNode) return registry.get(handle.id) || null;
    if (handle && handle.nodeType) return handle;
    return null;
  };

  const listeners = new Map();
  const timers = new Map();
  const eventQueue = [];
  let timerCounter = 0;

  return {
    name: 'browser',
    version: VERSION,
    userAgent: typeof navigator !== 'undefined' ? navigator.userAgent : 'kexco',
    language: typeof navigator !== 'undefined' ? navigator.language : 'en',
    readyState: document.readyState,

    document: {
      title: document.title,
      readyState: document.readyState,
      body: wrap(document.body),
      documentElement: wrap(document.documentElement),
      head: wrap(document.head),
    },

    storage: {
      get length() {
        return localStorage.length;
      },
      key: (i) => localStorage.key(i),
      getItem: (k) => localStorage.getItem(k),
      setItem: (k, v) => {
        localStorage.setItem(k, String(v));
        return null;
      },
      removeItem: (k) => {
        localStorage.removeItem(k);
        return null;
      },
      clear: () => {
        localStorage.clear();
        return null;
      },
    },

    session: {
      get length() {
        return sessionStorage.length;
      },
      key: (i) => sessionStorage.key(i),
      getItem: (k) => sessionStorage.getItem(k),
      setItem: (k, v) => {
        sessionStorage.setItem(k, String(v));
        return null;
      },
      removeItem: (k) => {
        sessionStorage.removeItem(k);
        return null;
      },
      clear: () => {
        sessionStorage.clear();
        return null;
      },
    },

    createElement: (tag) => wrap(document.createElement(tag)),
    createTextNode: (text) => wrap(document.createTextNode(String(text))),
    querySelector: (sel) => wrap(document.querySelector(sel)),
    querySelectorAll: (sel) => [...document.querySelectorAll(sel)].map(wrap),
    getElementById: (id) => wrap(document.getElementById(id)),
    getElementsByTagName: (tag) => [...document.getElementsByTagName(tag)].map(wrap),
    getElementsByClassName: (name) => [...document.getElementsByClassName(name)].map(wrap),
    body: () => wrap(document.body),
    documentElement: () => wrap(document.documentElement),

    append: (parent, child) => {
      const p = requireElement(parent, 'dom.append');
      const c = unwrap(child);
      if (!c) throw new Error('dom.append: child handle is stale');
      p.appendChild(c);
      return wrap(p);
    },
    prepend: (parent, child) => {
      const p = requireElement(parent, 'dom.prepend');
      p.prepend(unwrap(child));
      return wrap(p);
    },
    remove: (node) => {
      const c = unwrap(node);
      if (c && c.parentNode) c.parentNode.removeChild(c);
      return null;
    },
    setText: (node, text) => {
      const c = requireElement(node, 'dom.setText');
      c.textContent = String(text);
      return wrap(c);
    },
    getText: (node) => requireElement(node, 'dom.getText').textContent,
    setHtml: (node, html) => {
      const c = requireElement(node, 'dom.setHtml');
      c.innerHTML = String(html);
      return wrap(c);
    },
    getHtml: (node) => requireElement(node, 'dom.getHtml').innerHTML,
    setAttr: (node, name, value) => {
      const c = requireElement(node, 'dom.setAttr');
      c.setAttribute(String(name), String(value));
      return wrap(c);
    },
    getAttr: (node, name) => requireElement(node, 'dom.getAttr').getAttribute(String(name)),
    removeAttr: (node, name) => {
      const c = requireElement(node, 'dom.removeAttr');
      c.removeAttribute(String(name));
      return wrap(c);
    },
    hasAttr: (node, name) => requireElement(node, 'dom.hasAttr').hasAttribute(String(name)),
    addClass: (node, name) => {
      const c = requireElement(node, 'dom.addClass');
      c.classList.add(String(name));
      return wrap(c);
    },
    removeClass: (node, name) => {
      const c = requireElement(node, 'dom.removeClass');
      c.classList.remove(String(name));
      return wrap(c);
    },
    hasClass: (node, name) => requireElement(node, 'dom.hasClass').classList.contains(String(name)),
    setStyle: (node, property, value) => {
      const c = requireElement(node, 'dom.setStyle');
      c.style.setProperty(String(property), String(value));
      return wrap(c);
    },
    getStyle: (node, property) => {
      const c = requireElement(node, 'dom.getStyle');
      return window.getComputedStyle(c).getPropertyValue(String(property)) || c.style[String(property)] || '';
    },
    setData: (node, key, value) => {
      const c = requireElement(node, 'dom.setData');
      c.dataset[String(key)] = String(value);
      return wrap(c);
    },
    getData: (node, key) => {
      const c = requireElement(node, 'dom.getData');
      const v = c.dataset[String(key)];
      return v === undefined ? null : v;
    },
    getTag: (node) => requireElement(node, 'dom.getTag').tagName,
    getId: (node) => requireElement(node, 'dom.getId').id,
    childCount: (node) => requireElement(node, 'dom.childCount').childNodes.length,

    on(node, type, handlerId) {
      const c = requireElement(node, 'dom.on');
      const handler = (event) => {
        eventQueue.push({
          type: String(type),
          handler: Number(handlerId),
          detail: event.detail === undefined ? null : event.detail,
        });
      };
      listeners.set(String(handlerId), handler);
      c.addEventListener(String(type), handler);
      return wrap(c);
    },
    off(node, handlerId) {
      listeners.delete(String(handlerId));
      return null;
    },
    fire(node, type, detail) {
      const c = requireElement(node, 'dom.fire');
      const ev = new CustomEvent(String(type), { detail });
      return c.dispatchEvent(ev);
    },
    drain() {
      const out = eventQueue.slice();
      eventQueue.length = 0;
      return out;
    },
    fired: () => [],

    setTimeout(delay) {
      const id = ++timerCounter;
      const due = Number(delay) || 0;
      // the browser fires for real, the queue records it for `tick` to drain
      timers.set(String(id), setTimeout(() => {
        timers.delete(String(id));
        eventQueue.push({ type: 'timer', handler: id, detail: due });
      }, due));
      return id;
    },
    clearTimeout(id) {
      const key = String(id);
      if (timers.has(key)) {
        clearTimeout(timers.get(key));
        timers.delete(key);
      }
      return null;
    },
    tick: () => 0,
    pendingTimers: () => [...timers.keys()].map((id) => ({ id: Number(id), due: 0 })),

    route: (method, path, status, body) => {
      routes.set(`${String(method).toUpperCase()} ${path}`, { status, body });
      return path;
    },
    async fetch(method, url, options) {
      const key = `${String(method).toUpperCase()} ${url}`;
      if (routes.has(key)) {
        const stub = routes.get(key);
        return {
          ok: stub.status >= 200 && stub.status < 300,
          status: stub.status,
          headers: {},
          text: async () => String(stub.body),
          json: async () => JSON.parse(String(stub.body)),
        };
      }
      const res = await fetch(url, Object.assign({ method }, options || {}));
      return {
        ok: res.ok,
        status: res.status,
        headers: {},
        text: async () => res.text(),
        json: async () => res.json(),
      };
    },
    routes: () => [...routes.keys()],

    jsonParse: (text) => JSON.parse(String(text)),
    jsonStringify: (value) => JSON.stringify(value === undefined ? null : value),
    btoa: (text) => globalThis.btoa(unescape(encodeURIComponent(String(text)))),
    atob: (text) => decodeURIComponent(escape(globalThis.atob(String(text)))),
    now: () => Date.now(),
    uuid: () =>
      typeof crypto !== 'undefined' && crypto.randomUUID
        ? crypto.randomUUID()
        : 'kexco-' + Math.random().toString(16).slice(2),
    location: () => ({
      href: location.href,
      host: location.host,
      hostname: location.hostname,
      protocol: location.protocol,
      pathname: location.pathname,
      search: location.search,
      hash: location.hash,
    }),
  };
}

const routes = new Map();

// ===========================================================================
// 4. HOST DISPATCH - maps wire function names onto the adapter
// ===========================================================================

function buildDispatch(adapter) {
  return function dispatch(name, rawArgs) {
    const args = rawArgs.map(decode);
    const a = args;

    const need = (n) => {
      if (a.length < n) throw new Error(`${name}: expected ${n} argument(s), received ${a.length}`);
    };

    // ---- meta
    switch (name) {
      case 'host.info':
        return { adapter: adapter.name, version: adapter.version, engine: 'kexco' };
      case 'host.version':
        return adapter.version;
      case 'host.capabilities':
        return [
          'dom', 'events', 'storage', 'storage.session', 'timers', 'net',
          'json', 'base64', 'url', 'env', 'text',
        ];

      // ---- dom
      case 'dom.create':
        need(1);
        return adapter.createElement(a[0]);
      case 'dom.createText':
        need(1);
        return adapter.createTextNode(a[0]);
      case 'dom.query':
        need(1);
        return adapter.querySelector(a[0]);
      case 'dom.queryAll':
        need(1);
        return adapter.querySelectorAll(a[0]);
      case 'dom.byId':
        need(1);
        return adapter.getElementById(a[0]);
      case 'dom.byTag':
        need(1);
        return adapter.getElementsByTagName(a[0]);
      case 'dom.byClass':
        need(1);
        return adapter.getElementsByClassName(a[0]);
      case 'dom.append':
        need(2);
        return adapter.append(a[0], a[1]);
      case 'dom.prepend':
        need(2);
        return adapter.prepend(a[0], a[1]);
      case 'dom.remove':
        need(1);
        return adapter.remove(a[0]);
      case 'dom.setText':
        need(2);
        return adapter.setText(a[0], a[1]);
      case 'dom.getText':
        need(1);
        return adapter.getText(a[0]);
      case 'dom.setHtml':
        need(2);
        return adapter.setHtml(a[0], a[1]);
      case 'dom.getHtml':
        need(1);
        return adapter.getHtml(a[0]);
      case 'dom.setAttr':
        need(3);
        return adapter.setAttr(a[0], a[1], a[2]);
      case 'dom.getAttr':
        need(2);
        return adapter.getAttr(a[0], a[1]);
      case 'dom.removeAttr':
        need(2);
        return adapter.removeAttr(a[0], a[1]);
      case 'dom.hasAttr':
        need(2);
        return adapter.hasAttr(a[0], a[1]);
      case 'dom.addClass':
        need(2);
        return adapter.addClass(a[0], a[1]);
      case 'dom.removeClass':
        need(2);
        return adapter.removeClass(a[0], a[1]);
      case 'dom.hasClass':
        need(2);
        return adapter.hasClass(a[0], a[1]);
      case 'dom.setStyle':
        need(3);
        return adapter.setStyle(a[0], a[1], a[2]);
      case 'dom.getStyle':
        need(2);
        return adapter.getStyle(a[0], a[1]);
      case 'dom.setData':
        need(3);
        return adapter.setData(a[0], a[1], a[2]);
      case 'dom.getData':
        need(2);
        return adapter.getData(a[0], a[1]);
      case 'dom.getTag':
        need(1);
        return adapter.getTag(a[0]);
      case 'dom.getId':
        need(1);
        return adapter.getId(a[0]);
      case 'dom.children':
        need(1);
        return adapter.childCount(a[0]);
      case 'dom.title':
        return adapter.document.title;
      case 'dom.setTitle':
        need(1);
        adapter.document.title = String(a[0]);
        return adapter.document.title;
      case 'dom.readyState':
        return adapter.readyState;
      case 'dom.body':
        return adapter.body();
      case 'dom.document':
        return adapter.documentElement();

      // ---- events
      case 'events.on':
        need(3);
        return adapter.on(a[0], a[1], a[2]);
      case 'events.off':
        need(2);
        return adapter.off ? adapter.off(a[0], a[1]) : null;
      case 'events.fire':
        need(2);
        return adapter.fire(a[0], a[1], a[2]);
      case 'events.fired':
        need(1);
        return adapter.fired(a[0]);
      case 'events.drain':
        return adapter.drain();

      // ---- storage
      case 'storage.set':
        need(2);
        return adapter.storage.setItem(a[0], a[1]);
      case 'storage.get':
        need(1);
        return adapter.storage.getItem(a[0]);
      case 'storage.remove':
        need(1);
        return adapter.storage.removeItem(a[0]);
      case 'storage.clear':
        return adapter.storage.clear();
      case 'storage.length':
        return adapter.storage.length;
      case 'storage.key':
        need(1);
        return adapter.storage.key(a[0]);
      case 'storage.all': {
        const out = {};
        for (let i = 0; i < adapter.storage.length; i += 1) {
          const k = adapter.storage.key(i);
          out[k] = adapter.storage.getItem(k);
        }
        return out;
      }
      case 'session.set':
        need(2);
        return adapter.session.setItem(a[0], a[1]);
      case 'session.get':
        need(1);
        return adapter.session.getItem(a[0]);
      case 'session.remove':
        need(1);
        return adapter.session.removeItem(a[0]);
      case 'session.clear':
        return adapter.session.clear();
      case 'session.length':
        return adapter.session.length;
      case 'session.key':
        need(1);
        return adapter.session.key(a[0]);
      case 'session.all': {
        const out = {};
        for (let i = 0; i < adapter.session.length; i += 1) {
          const k = adapter.session.key(i);
          out[k] = adapter.session.getItem(k);
        }
        return out;
      }

      // ---- timers
      case 'timers.set': {
        need(1);
        return adapter.setTimeout(a[0]);
      }
      case 'timers.clear':
        need(1);
        return adapter.clearTimeout(a[0]);
      case 'timers.tick':
        need(1);
        return adapter.tick(Number(a[0]) || 0);
      case 'timers.pending':
        return adapter.pendingTimers();

      // ---- net
      case 'net.route':
        need(3);
        return adapter.route(a[0], a[1], a[2], a[3] === undefined ? '' : a[3], a[4]);
      case 'net.routes':
        return adapter.routes();
      case 'net.get':
        need(1);
        return doFetch('GET', a[0]);
      case 'net.post':
        need(2);
        return doFetch('POST', a[0], a[1]);
      case 'net.put':
        need(2);
        return doFetch('PUT', a[0], a[1]);
      case 'net.delete':
        need(1);
        return doFetch('DELETE', a[0]);

      // ---- utilities
      case 'json.parse':
        need(1);
        return adapter.jsonParse(a[0]);
      case 'json.stringify':
        return adapter.jsonStringify(a[0]);
      case 'base64.encode':
        need(1);
        return adapter.btoa(a[0]);
      case 'base64.decode':
        need(1);
        return adapter.atob(a[0]);
      case 'env.now':
        return adapter.now();
      case 'env.uuid':
        return adapter.uuid();
      case 'env.userAgent':
        return adapter.userAgent;
      case 'env.language':
        return adapter.language;
      case 'url.location':
        return adapter.location();
      case 'url.parse': {
        need(1);
        const raw = String(a[0]);
        const scheme = raw.includes('://') ? raw.split('://')[0] : null;
        const rest = scheme ? raw.slice(scheme.length + 3) : raw;
        const slash = rest.indexOf('/');
        const hostPart = slash < 0 ? rest : rest.slice(0, slash);
        const pathPart = slash < 0 ? '/' : rest.slice(slash);
        return {
          scheme,
          host: hostPart,
          hostname: hostPart.split(':')[0],
          port: hostPart.includes(':') ? Number(hostPart.split(':')[1]) : null,
          pathname: pathPart.split('?')[0].split('#')[0],
          search: pathPart.includes('?') ? pathPart.split('?')[1].split('#')[0] : '',
          hash: pathPart.includes('#') ? pathPart.split('#')[1] : '',
        };
      }

      default:
        throw new Error(
          `unknown kexco host function '${name}'. Call host.info() for the adapter and host.capabilities() for the API surface.`
        );
    }
  };

  async function doFetch(method, url, body) {
    const res = await adapter.fetch(method, url, body === undefined ? {} : { body });
    const text = await res.text();
    let parsed = null;
    try {
      parsed = JSON.parse(text);
    } catch (err) {
      parsed = null;
    }
    return {
      ok: res.ok,
      status: res.status,
      text,
      json: parsed,
    };
  }
}

// ===========================================================================
// 5. PUBLIC ENTRY POINTS
// ===========================================================================

function buildLibrary(adapter) {
  const dispatch = buildDispatch(adapter);
  return {
    VERSION,
    adapter: adapter.name,
    encode,
    decode,
    dispatch: (name, args) => dispatch(name, args || []),
    raw: adapter,
    createHeadlessAdapter,
    createBrowserAdapter,
  };
}

/** Picks the best available adapter for the current process. */
function defaultAdapter() {
  const browser = createBrowserAdapter();
  if (browser) return browser;
  return createHeadlessAdapter({ document: seedDocument([]) });
}

const kexco = {
  VERSION,
  encode,
  decode,
  HeadlessElement,
  HeadlessStorage,
  seedDocument,
  createHeadlessAdapter,
  createBrowserAdapter,
  defaultAdapter,
  buildLibrary,
};

if (typeof module !== 'undefined' && module.exports) {
  module.exports = kexco;
  module.exports.default = kexco;
}
if (typeof globalThis !== 'undefined') {
  globalThis.Kexco = kexco;
}

if (typeof window !== 'undefined' && typeof document !== 'undefined') {
  // browser bundle: expose the library and the dispatch entry point
  window.kexco = kexco;
  window.KEXCO_DISPATCH = buildDispatch(defaultAdapter());
}

(function () {
  // xmas:test, shaped like bun:test. Collection (describe/test/hooks) builds a tree while the file
  // loads; __xmas_test_start then runs it and leaves a JSON summary in __xmas_test_result.
  const HAS = Object.prototype.hasOwnProperty;
  const now = () => (typeof performance !== 'undefined' && performance.now ? performance.now() : Date.now());

  class Raw { constructor(s) { this.s = s; } }
  class Asym {
    constructor(label, match) { this.label = label; this.match = match; }
    asymmetricMatch(other) { return this.match(other); }
    toString() { return this.label; }
  }
  const isAsym = (v) => v instanceof Asym;
  class ExpectError extends Error {
    constructor(message) { super(message); this.name = 'error'; }
  }

  // ---------- printing ----------
  function fmt(v, depth = 0, seen = []) {
    switch (typeof v) {
      case 'string': return JSON.stringify(v);
      case 'bigint': return v + 'n';
      case 'symbol': return v.toString();
      case 'function': return '[Function ' + (v.name || '(anonymous)') + ']';
      case 'undefined': return 'undefined';
      case 'number': return Object.is(v, -0) ? '-0' : String(v);
      case 'boolean': return String(v);
    }
    if (v === null) return 'null';
    if (v instanceof Raw) return v.s;
    if (isAsym(v)) return v.toString();
    if (seen.includes(v)) return '[Circular]';
    if (v instanceof Date) return isNaN(v) ? 'Invalid Date' : v.toISOString();
    if (v instanceof RegExp) return String(v);
    if (v instanceof Error) return String(v);
    if (depth > 4) return Array.isArray(v) ? '[Array]' : '[Object]';
    seen = seen.concat([v]);
    let parts, open, close, pad = ' ';
    if (Array.isArray(v)) { parts = v.map((x) => fmt(x, depth + 1, seen)); open = '['; close = ']'; pad = ''; }
    else if (v instanceof Map) { parts = [...v].map(([k, x]) => fmt(k, depth + 1, seen) + ' => ' + fmt(x, depth + 1, seen)); open = 'Map {'; close = '}'; }
    else if (v instanceof Set) { parts = [...v].map((x) => fmt(x, depth + 1, seen)); open = 'Set {'; close = '}'; }
    else if (ArrayBuffer.isView(v)) { parts = Array.from(v, String); open = v.constructor.name + ' ['; close = ']'; pad = ''; }
    else {
      const ctor = v.constructor && v.constructor.name;
      parts = Object.keys(v).map((k) => (/^[A-Za-z_$][\w$]*$/.test(k) ? k : JSON.stringify(k)) + ': ' + fmt(v[k], depth + 1, seen));
      open = (ctor && ctor !== 'Object' ? ctor + ' ' : '') + '{'; close = '}';
    }
    if (parts.length === 0) return open + close;
    const one = open + pad + parts.join(', ') + pad + close;
    if (one.length <= 72 && !one.includes('\n')) return one;
    return open + '\n' + parts.map((p) => '  ' + p.replace(/\n/g, '\n  ')).join(',\n') + ',\n' + close;
  }

  // ---------- equality ----------
  const keysOf = (o, strict) => {
    let k = Reflect.ownKeys(o).filter((key) => Object.prototype.propertyIsEnumerable.call(o, key));
    if (!strict) k = k.filter((key) => o[key] !== undefined);
    return k;
  };
  function equals(a, b, strict, seen = []) {
    if (isAsym(a)) return a.asymmetricMatch(b);
    if (isAsym(b)) return b.asymmetricMatch(a);
    if (Object.is(a, b)) return true;
    if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return false;
    if (Object.prototype.toString.call(a) !== Object.prototype.toString.call(b)) return false;
    if (a instanceof Date) return +a === +b || (isNaN(a) && isNaN(b));
    if (a instanceof RegExp) return String(a) === String(b);
    if (a instanceof Error) return a.name === b.name && a.message === b.message;
    for (const [x, y] of seen) if (x === a && y === b) return true;
    seen = seen.concat([[a, b]]);
    if (strict && Object.getPrototypeOf(a) !== Object.getPrototypeOf(b)) return false;
    if (Array.isArray(a)) {
      if (a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) {
        if (strict && (i in a) !== (i in b)) return false;
        if (!equals(a[i], b[i], strict, seen)) return false;
      }
      return true;
    }
    if (ArrayBuffer.isView(a)) {
      if (a.length !== b.length) return false;
      for (let i = 0; i < a.length; i++) if (!Object.is(a[i], b[i])) return false;
      return true;
    }
    if (a instanceof ArrayBuffer) return equals(new Uint8Array(a), new Uint8Array(b), strict, seen);
    if (a instanceof Map) {
      if (a.size !== b.size) return false;
      for (const [k, v] of a) {
        if (b.has(k)) { if (!equals(v, b.get(k), strict, seen)) return false; }
        else if (![...b].some(([k2, v2]) => equals(k, k2, strict, seen) && equals(v, v2, strict, seen))) return false;
      }
      return true;
    }
    if (a instanceof Set) {
      if (a.size !== b.size) return false;
      for (const v of a) if (!b.has(v) && ![...b].some((w) => equals(v, w, strict, seen))) return false;
      return true;
    }
    const ka = keysOf(a, strict), kb = keysOf(b, strict);
    if (ka.length !== kb.length) return false;
    for (const k of ka) {
      if (!HAS.call(b, k)) return false;
      if (!equals(a[k], b[k], strict, seen)) return false;
    }
    return true;
  }
  function subset(obj, sub) {
    if (isAsym(sub)) return sub.asymmetricMatch(obj);
    if (Array.isArray(sub)) return Array.isArray(obj) && obj.length === sub.length && sub.every((x, i) => subset(obj[i], x));
    if (sub && typeof sub === 'object' && !(sub instanceof Date) && !(sub instanceof RegExp) && !(sub instanceof Map) && !(sub instanceof Set)) {
      if (obj === null || typeof obj !== 'object') return false;
      return Reflect.ownKeys(sub).every((k) => subset(obj[k], sub[k]) && (sub[k] !== undefined || k in obj));
    }
    return equals(obj, sub, false);
  }
  function getPath(obj, path) {
    const parts = Array.isArray(path) ? path : String(path).replace(/\[(\d+)\]/g, '.$1').split('.').filter((p) => p !== '');
    let cur = obj;
    for (const p of parts) {
      if (cur === null || cur === undefined || !(Object(cur) && p in Object(cur))) return { found: false };
      cur = cur[p];
    }
    return { found: true, value: cur };
  }

  // ---------- mocks ----------
  const allMocks = new Set();
  let callOrder = 0;
  function mockFn(impl) {
    let implementation = impl;
    const once = [];
    const state = { calls: [], results: [], contexts: [], instances: [], invocationCallOrder: [] };
    Object.defineProperty(state, 'lastCall', { get: () => state.calls[state.calls.length - 1], enumerable: true });
    let restorer = null;
    let label = 'jest.fn()';
    const fn = function (...args) {
      state.calls.push(args); state.contexts.push(this); state.instances.push(this); state.invocationCallOrder.push(++callOrder);
      try {
        const use = once.length ? once.shift() : implementation;
        const value = use ? use.apply(this, args) : undefined;
        state.results.push({ type: 'return', value });
        return value;
      } catch (error) {
        state.results.push({ type: 'throw', value: error });
        throw error;
      }
    };
    fn.mock = state;
    fn._isMockFunction = true;
    fn.mockImplementation = (f) => { implementation = f; return fn; };
    fn.mockImplementationOnce = (f) => { once.push(f); return fn; };
    fn.mockReturnValue = (v) => { implementation = () => v; return fn; };
    fn.mockReturnValueOnce = (v) => { once.push(() => v); return fn; };
    fn.mockResolvedValue = (v) => { implementation = () => Promise.resolve(v); return fn; };
    fn.mockResolvedValueOnce = (v) => { once.push(() => Promise.resolve(v)); return fn; };
    fn.mockRejectedValue = (v) => { implementation = () => Promise.reject(v); return fn; };
    fn.mockRejectedValueOnce = (v) => { once.push(() => Promise.reject(v)); return fn; };
    fn.mockReturnThis = () => { implementation = function () { return this; }; return fn; };
    fn.getMockImplementation = () => implementation;
    fn.mockName = (n) => { label = n; return fn; };
    fn.getMockName = () => label;
    fn.mockClear = () => { for (const k of Object.keys(state)) if (Array.isArray(state[k])) state[k].length = 0; return fn; };
    fn.mockReset = () => { fn.mockClear(); implementation = undefined; once.length = 0; return fn; };
    fn.mockRestore = () => { fn.mockReset(); if (restorer) { restorer(); restorer = null; } return fn; };
    fn._setRestorer = (r) => { restorer = r; };
    allMocks.add(fn);
    return fn;
  }
  const isMock = (f) => typeof f === 'function' && f._isMockFunction === true;
  function spyOn(obj, name) {
    const original = obj[name];
    if (typeof original !== 'function') throw new Error("Cannot spy on '" + String(name) + "': it is not a function");
    const own = HAS.call(obj, name);
    const spy = mockFn(function (...args) { return original.apply(this, args); });
    spy._setRestorer(() => { if (own) obj[name] = original; else delete obj[name]; });
    obj[name] = spy;
    return spy;
  }
  const mock = Object.assign((impl) => mockFn(impl), {
    clearAllMocks: () => allMocks.forEach((m) => m.mockClear()),
    resetAllMocks: () => allMocks.forEach((m) => m.mockReset()),
    restore: () => allMocks.forEach((m) => m.mockRestore()),
    restoreAllMocks: () => allMocks.forEach((m) => m.mockRestore()),
  });
  const jest = { fn: mockFn, spyOn, isMockFunction: isMock, clearAllMocks: mock.clearAllMocks, resetAllMocks: mock.resetAllMocks, restoreAllMocks: mock.restoreAllMocks };

  // ---------- expect ----------
  const counters = { expects: 0 };
  let cur = { assertions: 0, expected: null, hasAssertions: false };
  const R = (pass, expected, received, extra) => Object.assign({ pass, expected, received }, extra);
  const num = (n) => typeof n === 'number' || typeof n === 'bigint';
  const typeName = (v) => (v === null ? 'null' : typeof v);
  const callsOf = (m, name) => {
    if (!isMock(m)) throw new ExpectError('expect(received).' + name + '(): received value must be a mock or spy function');
    return m.mock.calls;
  };
  const matchers = {
    toBe: (a, e) => R(Object.is(a, e), e, a),
    toEqual: (a, e) => R(equals(a, e, false), e, a),
    toStrictEqual: (a, e) => R(equals(a, e, true), e, a),
    toBeTruthy: (a) => R(!!a, null, a, { only: true }),
    toBeFalsy: (a) => R(!a, null, a, { only: true }),
    toBeTrue: (a) => R(a === true, true, a),
    toBeFalse: (a) => R(a === false, false, a),
    toBeNull: (a) => R(a === null, null, a),
    toBeUndefined: (a) => R(a === undefined, undefined, a),
    toBeDefined: (a) => R(a !== undefined, undefined, a, { expectedText: 'defined' }),
    toBeNaN: (a) => R(typeof a === 'number' && isNaN(a), NaN, a),
    toBeNil: (a) => R(a === null || a === undefined, null, a, { expectedText: 'null or undefined' }),
    toBeGreaterThan: (a, e) => R(a > e, e, a, { expectedText: '> ' + fmt(e) }),
    toBeGreaterThanOrEqual: (a, e) => R(a >= e, e, a, { expectedText: '>= ' + fmt(e) }),
    toBeLessThan: (a, e) => R(a < e, e, a, { expectedText: '< ' + fmt(e) }),
    toBeLessThanOrEqual: (a, e) => R(a <= e, e, a, { expectedText: '<= ' + fmt(e) }),
    toBeCloseTo: (a, e, digits = 2) => R(Math.abs(a - e) < Math.pow(10, -digits) / 2, e, a, { expectedText: fmt(e) + ' (within ' + digits + ' digits)' }),
    toBeInteger: (a) => R(Number.isInteger(a), null, a, { only: true }),
    toBeFinite: (a) => R(Number.isFinite(a), null, a, { only: true }),
    toBePositive: (a) => R(num(a) && a > 0, null, a, { only: true }),
    toBeNegative: (a) => R(num(a) && a < 0, null, a, { only: true }),
    toBeNumber: (a) => R(typeof a === 'number', 'number', a),
    toBeString: (a) => R(typeof a === 'string', 'string', a),
    toBeBoolean: (a) => R(typeof a === 'boolean', 'boolean', a),
    toBeFunction: (a) => R(typeof a === 'function', 'function', a),
    toBeObject: (a) => R(a !== null && typeof a === 'object', 'object', a),
    toBeArray: (a) => R(Array.isArray(a), 'array', a),
    toBeSymbol: (a) => R(typeof a === 'symbol', 'symbol', a),
    toBeDate: (a) => R(a instanceof Date, 'Date', a),
    toBeEmpty: (a) => R(a === '' || (a && typeof a === 'object' && (a.length === 0 || a.size === 0 || (!('length' in a) && !('size' in a) && Object.keys(a).length === 0))), null, a, { only: true }),
    toBeInstanceOf: (a, C) => R(a instanceof C, C, a),
    toBeTypeOf: (a, t) => R(typeName(a) === t, t, a),
    toBeOneOf: (a, list) => R([...list].some((x) => equals(a, x, false)), [...list], a),
    toSatisfy: (a, f) => R(!!f(a), null, a, { expectedText: 'a value satisfying the predicate' }),
    toContain: (a, item) => {
      if (typeof a === 'string') return R(a.includes(item), item, a);
      return R(a != null && Array.from(a).some((x) => Object.is(x, item) || x === item), item, a);
    },
    toContainEqual: (a, item) => R(a != null && Array.from(a).some((x) => equals(x, item, false)), item, a),
    toHaveLength: (a, n) => R(a != null && a.length === n, n, a != null ? a.length : a),
    toHaveProperty: (a, path, ...rest) => {
      const found = a === null || a === undefined ? { found: false } : getPath(a, path);
      const pass = found.found && (rest.length === 0 || equals(found.value, rest[0], false));
      return R(pass, rest.length ? rest[0] : path, found.found ? found.value : a, { expectedText: rest.length ? fmt(path) + ' = ' + fmt(rest[0]) : fmt(path) });
    },
    toMatch: (a, e) => R(typeof a === 'string' && (e instanceof RegExp ? e.test(a) : a.includes(e)), e, a),
    toMatchObject: (a, e) => R(a !== null && typeof a === 'object' && subset(a, e), e, a),
    toStartWith: (a, e) => R(typeof a === 'string' && a.startsWith(e), e, a),
    toEndWith: (a, e) => R(typeof a === 'string' && a.endsWith(e), e, a),
    toInclude: (a, e) => R(typeof a === 'string' && a.includes(e), e, a),
    toThrow: (fn, exp) => {
      if (typeof fn !== 'function') throw new ExpectError('expect(received).toThrow(): received value must be a function');
      let thrown = false, err;
      try { fn(); } catch (e) { thrown = true; err = e; }
      const msg = err && err.message !== undefined ? String(err.message) : String(err);
      let pass;
      if (!thrown) pass = false;
      else if (exp === undefined) pass = true;
      else if (typeof exp === 'string') pass = msg.includes(exp);
      else if (exp instanceof RegExp) pass = exp.test(msg);
      else if (typeof exp === 'function') pass = err instanceof exp;
      else if (exp instanceof Error) pass = msg === exp.message;
      else if (isAsym(exp)) pass = exp.asymmetricMatch(err);
      else pass = false;
      return R(pass, exp === undefined ? new Raw('a thrown error') : exp, thrown ? err : new Raw('function did not throw'));
    },
    toHaveBeenCalled: (m) => R(callsOf(m, 'toHaveBeenCalled').length > 0, null, new Raw('Number of calls: ' + m.mock.calls.length), { only: true }),
    toHaveBeenCalledTimes: (m, n) => R(callsOf(m, 'toHaveBeenCalledTimes').length === n, n, m.mock.calls.length),
    toHaveBeenCalledWith: (m, ...args) => R(callsOf(m, 'toHaveBeenCalledWith').some((c) => equals(c, args, false)), args, m.mock.calls),
    toHaveBeenLastCalledWith: (m, ...args) => {
      const c = callsOf(m, 'toHaveBeenLastCalledWith');
      return R(c.length > 0 && equals(c[c.length - 1], args, false), args, c[c.length - 1]);
    },
    toHaveBeenNthCalledWith: (m, n, ...args) => {
      const c = callsOf(m, 'toHaveBeenNthCalledWith');
      return R(c.length >= n && equals(c[n - 1], args, false), args, c[n - 1]);
    },
    toHaveReturned: (m) => R(callsOf(m, 'toHaveReturned') && m.mock.results.some((r) => r.type === 'return'), null, new Raw('Number of returns: ' + m.mock.results.filter((r) => r.type === 'return').length), { only: true }),
    toHaveReturnedTimes: (m, n) => R(callsOf(m, 'toHaveReturnedTimes') && m.mock.results.filter((r) => r.type === 'return').length === n, n, m.mock.results.filter((r) => r.type === 'return').length),
    toHaveReturnedWith: (m, v) => R(callsOf(m, 'toHaveReturnedWith') && m.mock.results.some((r) => r.type === 'return' && equals(r.value, v, false)), v, m.mock.results.map((r) => r.value)),
  };
  const aliases = {
    toThrowError: 'toThrow', toBeCalled: 'toHaveBeenCalled', toBeCalledTimes: 'toHaveBeenCalledTimes', toBeCalledWith: 'toHaveBeenCalledWith',
    lastCalledWith: 'toHaveBeenLastCalledWith', nthCalledWith: 'toHaveBeenNthCalledWith', toReturn: 'toHaveReturned', toReturnWith: 'toHaveReturnedWith',
  };
  for (const [alias, real] of Object.entries(aliases)) matchers[alias] = matchers[real];

  function compose(name, inv, r, args) {
    if (r.message) return r.message();
    const head = 'expect(received).' + (inv ? 'not.' : '') + name + '(' + (args.length ? 'expected' : '') + ')\n\n';
    const received = 'Received: ' + fmt(r.received);
    if (r.only) return head + (inv ? 'Expected: not ' + (r.expectedText || 'matching') + '\n' : '') + received;
    const expected = r.expectedText !== undefined ? r.expectedText : fmt(r.expected);
    return head + 'Expected: ' + (inv ? 'not ' : '') + expected + '\n' + received;
  }

  class Expect {
    constructor(actual, state) { this.a = actual; this.s = state; }
    get not() { return new Expect(this.a, { inv: !this.s.inv, mode: this.s.mode }); }
    get resolves() { return new Expect(this.a, { inv: this.s.inv, mode: 'resolves' }); }
    get rejects() { return new Expect(this.a, { inv: this.s.inv, mode: 'rejects' }); }
    _run(name, args) {
      const st = this.s;
      const check = (value) => {
        counters.expects++; cur.assertions++;
        const r = matchers[name](value, ...args);
        if (!!r.pass === st.inv) throw new ExpectError(compose(name, st.inv, r, args));
      };
      if (st.mode === 'resolves') {
        const p = typeof this.a === 'function' ? this.a() : this.a;
        return Promise.resolve(p).then(check, (e) => { throw new ExpectError('Expected promise that resolves, but it rejected with ' + fmt(e)); });
      }
      if (st.mode === 'rejects') {
        const p = typeof this.a === 'function' ? this.a() : this.a;
        return Promise.resolve(p).then((v) => { throw new ExpectError('Expected promise that rejects, but it resolved to ' + fmt(v)); }, (e) => check(name === 'toThrow' || name === 'toThrowError' ? () => { throw e; } : e));
      }
      return check(this.a);
    }
  }
  const attach = (name) => { Expect.prototype[name] = function (...args) { return this._run(name, args); }; };
  Object.keys(matchers).forEach(attach);

  const expect = (actual) => new Expect(actual, { inv: false, mode: null });
  expect.extend = (defs) => {
    for (const [name, fn] of Object.entries(defs)) {
      matchers[name] = (a, ...args) => {
        const r = fn.call({ equals: (x, y) => equals(x, y, false), utils: { printReceived: fmt, printExpected: fmt } }, a, ...args);
        return { pass: !!r.pass, message: r.message };
      };
      attach(name);
    }
  };
  expect.assertions = (n) => { cur.expected = n; };
  expect.hasAssertions = () => { cur.hasAssertions = true; };
  expect.fail = (message) => { throw new ExpectError(message === undefined ? 'fail() was called' : message); };
  const typeCheck = (C, v) => {
    if (C === String) return typeof v === 'string' || v instanceof String;
    if (C === Number) return typeof v === 'number' || v instanceof Number;
    if (C === Boolean) return typeof v === 'boolean' || v instanceof Boolean;
    if (C === BigInt) return typeof v === 'bigint';
    if (C === Symbol) return typeof v === 'symbol';
    if (C === Function) return typeof v === 'function';
    if (C === Object) return v !== null && (typeof v === 'object' || typeof v === 'function');
    return v instanceof C;
  };
  const asymmetric = (inv) => ({
    any: (C) => new Asym('Any<' + (C && C.name) + '>', (v) => typeCheck(C, v) !== inv),
    anything: () => new Asym('Anything', (v) => (v !== null && v !== undefined) !== inv),
    objectContaining: (o) => new Asym('ObjectContaining ' + fmt(o), (v) => (v !== null && typeof v === 'object' && Reflect.ownKeys(o).every((k) => k in v && equals(v[k], o[k], false))) !== inv),
    arrayContaining: (a) => new Asym('ArrayContaining ' + fmt(a), (v) => (Array.isArray(v) && a.every((x) => v.some((y) => equals(x, y, false)))) !== inv),
    stringContaining: (s) => new Asym('StringContaining ' + fmt(s), (v) => (typeof v === 'string' && v.includes(s)) !== inv),
    stringMatching: (r) => new Asym('StringMatching ' + String(r), (v) => (typeof v === 'string' && (r instanceof RegExp ? r.test(v) : v.includes(r))) !== inv),
  });
  Object.assign(expect, asymmetric(false));
  expect.not = asymmetric(true);

  // ---------- collection ----------
  let defaultTimeout = 5000;
  class Suite {
    constructor(name, parent, mode) {
      this.name = name; this.parent = parent; this.mode = mode; this.children = [];
      this.hooks = { beforeAll: [], beforeEach: [], afterEach: [], afterAll: [] };
      this.started = false; this.hookError = null;
    }
  }
  const root = new Suite('', null, 'run');
  let current = root;
  let hasOnly = false;

  function addTest(base, name, fn, options) {
    const timeout = typeof options === 'number' ? options : options && options.timeout;
    const t = { type: 'test', name: String(name), fn, mode: base.mode, failing: !!base.failing, timeout, parent: current };
    if (base.mode === 'only') hasOnly = true;
    if (typeof fn !== 'function' && base.mode !== 'todo') t.mode = 'todo';
    current.children.push(t);
  }
  function addSuite(base, name, fn) {
    const s = new Suite(String(name), current, base.mode);
    if (base.mode === 'only') hasOnly = true;
    current.children.push(s);
    const saved = current;
    current = s;
    try { if (typeof fn === 'function') fn(); } finally { current = saved; }
  }
  function title(pattern, row, index) {
    let args = Array.isArray(row) ? row.slice() : [row];
    let out = String(pattern).replace(/%([sdifjopP#%])/g, (m, c) => {
      if (c === '%') return '%';
      if (c === '#') return String(index);
      if (args.length === 0) return m;
      const v = args.shift();
      if (c === 's') return typeof v === 'string' ? v : fmt(v);
      if (c === 'd' || c === 'i') return String(c === 'i' ? parseInt(v) : Number(v));
      if (c === 'f') return String(parseFloat(v));
      return c === 'j' ? JSON.stringify(v) : fmt(v);
    });
    if (row && typeof row === 'object' && !Array.isArray(row)) out = out.replace(/\$([A-Za-z_][\w.]*)/g, (m, p) => { const f = getPath(row, p); return f.found ? (typeof f.value === 'string' ? f.value : fmt(f.value)) : m; });
    return out;
  }
  function chain(add, base, kind) {
    const f = (name, fn, options) => add(base, name, fn, options);
    const sub = (patch) => chain(add, Object.assign({}, base, patch), kind);
    Object.defineProperties(f, {
      skip: { get: () => sub({ mode: 'skip' }) },
      todo: { get: () => sub({ mode: 'todo' }) },
      only: { get: () => sub({ mode: 'only' }) },
      failing: { get: () => sub({ failing: true }) },
      concurrent: { get: () => f },
      serial: { get: () => f },
    });
    f.if = (cond) => (cond ? f : sub({ mode: 'skip' }));
    f.skipIf = (cond) => (cond ? sub({ mode: 'skip' }) : f);
    f.todoIf = (cond) => (cond ? sub({ mode: 'todo' }) : f);
    f.each = (table) => (name, fn, options) => {
      Array.from(table).forEach((row, i) => {
        const args = Array.isArray(row) ? row : [row];
        const wrapped = fn.length > args.length ? Object.assign((done) => fn(...args, done), {}) : () => fn(...args);
        add(base, title(name, row, i), wrapped, options);
      });
    };
    return f;
  }
  const test = chain(addTest, { mode: 'run' }, 'test');
  const describe = chain(addSuite, { mode: 'run' }, 'suite');
  const hook = (kind) => (fn, options) => { current.hooks[kind].push({ fn, timeout: typeof options === 'number' ? options : options && options.timeout }); };
  const beforeAll = hook('beforeAll'), beforeEach = hook('beforeEach'), afterEach = hook('afterEach'), afterAll = hook('afterAll');

  // ---------- running ----------
  let opts = { timeout: 5000, bail: false, pattern: null };
  const totals = { pass: 0, fail: 0, skip: 0, todo: 0 };
  let stopped = false;

  function invoke(fn, ms) {
    return new Promise((resolve, reject) => {
      let done = false;
      const timer = setTimeout(() => { if (!done) { done = true; reject(new Error('Test timed out after ' + ms + 'ms')); } }, ms);
      const finish = (error, failed) => { if (done) return; done = true; clearTimeout(timer); failed ? reject(error) : resolve(); };
      try {
        if (fn.length >= 1) {
          fn((error) => finish(error, !!error));
        } else {
          const r = fn();
          if (r && typeof r.then === 'function') r.then(() => finish(), (e) => finish(e === undefined ? new Error('Promise rejected with undefined') : e, true));
          else finish();
        }
      } catch (e) { finish(e, true); }
    });
  }
  const runHook = (h) => invoke(h.fn, h.timeout || opts.timeout);
  const chainOf = (s) => { const out = []; for (let x = s; x; x = x.parent) out.unshift(x); return out; };
  const selected = (t, chain) => !hasOnly || t.mode === 'only' || chain.some((s) => s.mode === 'only');
  const nameOf = (t, chain) => chain.slice(1).map((s) => s.name).concat(t.name).join(' > ');
  const message = (e) => (e && e.message !== undefined ? String(e.message) : String(e));

  const out = (text) => process.stdout.write(text + '\n');
  function report(kind, name, ms, error) {
    totals[kind]++;
    const time = ms === undefined ? '' : ' [' + (ms < 10 ? ms.toFixed(2) : ms.toFixed(0)) + 'ms]';
    out('(' + kind + ') ' + name + time);
    if (error !== undefined) out(message(error).split('\n').map((l) => '    ' + l).join('\n'));
  }
  async function ensureStarted(s) {
    if (s.started) return;
    s.started = true;
    try { for (const h of s.hooks.beforeAll) await runHook(h); } catch (e) { s.hookError = e; }
  }
  async function runTest(t, chain, forced) {
    const name = nameOf(t, chain);
    if (opts.pattern && !new RegExp(opts.pattern).test(name)) return;
    if (!selected(t, chain)) return;
    if (forced === 'skip' || t.mode === 'skip') return report('skip', name);
    if (forced === 'todo' || t.mode === 'todo') return report('todo', name);
    const start = now();
    let error, failed = false;
    cur = { assertions: 0, expected: null, hasAssertions: false };
    try {
      for (const s of chain) await ensureStarted(s);
      for (const s of chain) if (s.hookError) throw s.hookError;
      for (const s of chain) for (const h of s.hooks.beforeEach) await runHook(h);
      await invoke(t.fn, t.timeout || opts.timeout);
      if (cur.expected !== null && cur.assertions !== cur.expected) throw new ExpectError('expect.assertions(' + cur.expected + '): ' + cur.assertions + ' assertion(s) were run');
      if (cur.hasAssertions && cur.assertions === 0) throw new ExpectError('expect.hasAssertions(): no assertions were run');
    } catch (e) { error = e; failed = true; }
    for (const s of chain.slice().reverse()) {
      for (const h of s.hooks.afterEach) { try { await runHook(h); } catch (e) { if (!failed) { error = e; failed = true; } } }
    }
    const ms = now() - start;
    if (t.failing) {
      if (failed) return report('pass', name, ms);
      failed = true; error = new Error('Expected this test to fail, but it passed');
    }
    if (failed) { report('fail', name, ms, error); if (opts.bail) stopped = true; }
    else report('pass', name, ms);
  }
  async function walk(suite, forced) {
    const chain = chainOf(suite);
    const mode = forced || (suite.mode === 'skip' ? 'skip' : suite.mode === 'todo' ? 'todo' : null);
    for (const child of suite.children) {
      if (stopped) break;
      if (child.type === 'test') await runTest(child, chainOf(suite), mode);
      else await walk(child, mode);
    }
    if (suite.started) {
      for (const h of suite.hooks.afterAll) {
        try { await runHook(h); } catch (e) { report('fail', chain.slice(1).map((s) => s.name).concat('afterAll').join(' > '), undefined, e); }
      }
    }
  }
  globalThis.__xmas_test_start = function (json) {
    opts = Object.assign(opts, JSON.parse(json));
    walk(root, null).then(
      () => { globalThis.__xmas_test_result = JSON.stringify(Object.assign({ expects: counters.expects }, totals)); },
      (e) => { globalThis.__xmas_test_result = JSON.stringify({ error: String((e && e.stack) || e) }); },
    );
  };

  const skipped = (f) => f.skip;
  const api = {
    describe, test, it: test, expect, beforeAll, beforeEach, afterAll, afterEach, mock, spyOn, jest, vi: jest,
    xtest: skipped(test), xit: skipped(test), xdescribe: skipped(describe),
    setDefaultTimeout: (ms) => { opts.timeout = ms; },
  };
  globalThis.__xmas_test_module = api;
  globalThis.__xmas_test_globals = () => { for (const k of Object.keys(api)) globalThis[k] = api[k]; };
})();

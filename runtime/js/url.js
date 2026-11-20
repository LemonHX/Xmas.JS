(function () {
  const parse = (input, base) => JSON.parse(__url_parse(String(input), base === undefined || base === null ? null : String(base)));
  const assign = (href, component, value) => JSON.parse(__url_set(href, component, String(value)));

  class URLSearchParams {
    constructor(init) {
      this._list = [];
      this._owner = null;
      if (init === undefined || init === null) return;
      if (typeof init === 'string') {
        const text = init.startsWith('?') ? init.slice(1) : init;
        if (!text) return;
        for (const piece of text.split('&')) {
          if (!piece) continue;
          const at = piece.indexOf('=');
          const key = at < 0 ? piece : piece.slice(0, at);
          const value = at < 0 ? '' : piece.slice(at + 1);
          this._list.push([decode(key), decode(value)]);
        }
        return;
      }
      if (Array.isArray(init)) {
        for (const pair of init) this._list.push([String(pair[0]), String(pair[1])]);
        return;
      }
      if (typeof init[Symbol.iterator] === 'function') {
        for (const pair of init) this._list.push([String(pair[0]), String(pair[1])]);
        return;
      }
      for (const key of Object.keys(init)) this._list.push([String(key), String(init[key])]);
    }
    get size() { return this._list.length; }
    append(key, value) { this._list.push([String(key), String(value)]); this._changed(); }
    delete(key) { key = String(key); this._list = this._list.filter((pair) => pair[0] !== key); this._changed(); }
    get(key) { const found = this._list.find((pair) => pair[0] === String(key)); return found ? found[1] : null; }
    getAll(key) { return this._list.filter((pair) => pair[0] === String(key)).map((pair) => pair[1]); }
    has(key) { return this._list.some((pair) => pair[0] === String(key)); }
    set(key, value) {
      key = String(key);
      const at = this._list.findIndex((pair) => pair[0] === key);
      if (at < 0) this._list.push([key, String(value)]);
      else { this._list[at] = [key, String(value)]; this._list = this._list.filter((pair, index) => index === at || pair[0] !== key); }
      this._changed();
    }
    sort() {
      this._list = this._list.map((pair, index) => [pair, index])
        .sort((left, right) => left[0][0] < right[0][0] ? -1 : left[0][0] > right[0][0] ? 1 : left[1] - right[1])
        .map((entry) => entry[0]);
      this._changed();
    }
    forEach(callback, thisArg) { for (const [key, value] of this._list) callback.call(thisArg, value, key, this); }
    *keys() { for (const [key] of this._list) yield key; }
    *values() { for (const [, value] of this._list) yield value; }
    *entries() { for (const pair of this._list) yield [pair[0], pair[1]]; }
    [Symbol.iterator]() { return this.entries(); }
    toString() { return this._list.map(([key, value]) => encode(key) + '=' + encode(value)).join('&'); }
    _changed() { if (this._owner) this._owner._search = this.toString(); }
  }

  const encode = (text) => encodeURIComponent(text).replace(/%20/g, '+').replace(/[!'()~]/g, (c) => '%' + c.charCodeAt(0).toString(16).toUpperCase());
  const decode = (text) => {
    const plus = text.replace(/\+/g, ' ');
    try { return decodeURIComponent(plus); } catch { return plus; }
  };

  class URL {
    constructor(input, base) {
      const text = input instanceof URL ? input.href : String(input);
      this._parts = parse(text, base);
      this._params = null;
    }
    static canParse(input, base) {
      try { parse(String(input), base); return true; } catch { return false; }
    }
    static parse(input, base) {
      try { return new URL(input, base); } catch { return null; }
    }
    get href() { return this._parts.href; }
    set href(value) { this._parts = parse(String(value)); this._params = null; }
    get protocol() { return this._parts.protocol; }
    set protocol(value) { this._set('protocol', value); }
    get username() { return this._parts.username; }
    set username(value) { this._set('username', value); }
    get password() { return this._parts.password; }
    set password(value) { this._set('password', value); }
    get host() { return this._parts.host; }
    set host(value) { this._set('host', value); }
    get hostname() { return this._parts.hostname; }
    set hostname(value) { this._set('hostname', value); }
    get port() { return this._parts.port; }
    set port(value) { this._set('port', value); }
    get pathname() { return this._parts.pathname; }
    set pathname(value) { this._set('pathname', value); }
    get search() { return this._params ? (this._params.toString() ? '?' + this._params.toString() : '') : this._parts.search; }
    set search(value) {
      this._set('search', value);
      this._params = null;
    }
    get hash() { return this._parts.hash; }
    set hash(value) { this._set('hash', value); }
    get origin() { return this._parts.origin; }
    get searchParams() {
      if (!this._params) {
        this._params = new URLSearchParams(this._parts.search ? this._parts.search.slice(1) : '');
        this._params._owner = this;
      }
      return this._params;
    }
    toString() { return this.href; }
    toJSON() { return this.href; }
    _set(component, value) {
      this._parts = assign(this._parts.href, component, value);
    }
  }

  globalThis.URL = URL;
  globalThis.URLSearchParams = URLSearchParams;
  globalThis.__url_module = { URL, URLSearchParams, default: { URL, URLSearchParams } };
})();

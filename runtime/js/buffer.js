(function () {
  const ALPHABET = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/';
  const INDEX = new Map();
  for (let i = 0; i < ALPHABET.length; i++) INDEX.set(ALPHABET[i], i);

  const toBase64 = (bytes) => {
    let out = '';
    for (let i = 0; i < bytes.length; i += 3) {
      const a = bytes[i];
      const b = i + 1 < bytes.length ? bytes[i + 1] : 0;
      const c = i + 2 < bytes.length ? bytes[i + 2] : 0;
      out += ALPHABET[a >> 2];
      out += ALPHABET[((a & 3) << 4) | (b >> 4)];
      out += i + 1 < bytes.length ? ALPHABET[((b & 15) << 2) | (c >> 6)] : '=';
      out += i + 2 < bytes.length ? ALPHABET[c & 63] : '=';
    }
    return out;
  };

  const fromBase64 = (text) => {
    const clean = text.replace(/[^A-Za-z0-9+/]/g, '');
    const out = new Uint8Array(Math.floor((clean.length * 3) / 4));
    let at = 0;
    for (let i = 0; i < clean.length; i += 4) {
      const a = INDEX.get(clean[i]) || 0;
      const b = INDEX.get(clean[i + 1]) || 0;
      const c = INDEX.get(clean[i + 2]) || 0;
      const d = INDEX.get(clean[i + 3]) || 0;
      out[at++] = (a << 2) | (b >> 4);
      if (i + 2 < clean.length) out[at++] = ((b & 15) << 4) | (c >> 2);
      if (i + 3 < clean.length) out[at++] = ((c & 3) << 6) | d;
    }
    return out.slice(0, at);
  };

  const encoding = (name) => {
    const text = String(name || 'utf8').toLowerCase();
    if (text === 'utf-8' || text === 'utf8') return 'utf8';
    if (text === 'utf16le' || text === 'utf-16le' || text === 'ucs2' || text === 'ucs-2') return 'utf16le';
    if (text === 'latin1' || text === 'binary') return 'latin1';
    if (text === 'base64' || text === 'base64url') return 'base64';
    if (text === 'hex') return 'hex';
    if (text === 'ascii') return 'ascii';
    throw new TypeError('Unknown encoding:' + name);
  };

  class Buffer extends Uint8Array {
    // QuickJS's subarray doesn't honor species, returns a bare Uint8Array; swap the prototype back,
    // The memory is still shared (same as Node's subarray), with no extra copy.
    subarray(start, end) {
      const view = super.subarray(start, end);
      if (view.constructor !== Buffer) Object.setPrototypeOf(view, Buffer.prototype);
      return view;
    }
    slice(start, end) {
      const copy = super.slice(start, end);
      if (copy.constructor !== Buffer) Object.setPrototypeOf(copy, Buffer.prototype);
      return copy;
    }
    static from(value, encodingOrOffset, length) {
      if (typeof value === 'string') {
        return Buffer.fromString(value, typeof encodingOrOffset === 'string' ? encodingOrOffset : 'utf8');
      }
      if (value instanceof ArrayBuffer) {
        const offset = typeof encodingOrOffset === 'number' ? encodingOrOffset : 0;
        const size = typeof length === 'number' ? length : value.byteLength - offset;
        const out = new Buffer(size);
        out.set(new Uint8Array(value, offset, size));
        return out;
      }
      if (ArrayBuffer.isView(value)) {
        const out = new Buffer(value.length);
        out.set(value);
        return out;
      }
      if (value && typeof value.length === 'number') {
        const out = new Buffer(value.length);
        out.set(value);
        return out;
      }
      throw new TypeError('Buffer.from does not recognize this argument');
    }
    static fromString(text, name) {
      const bytes = fromBase64(__bytes_encode(text, encoding(name)));
      const out = new Buffer(bytes.length);
      out.set(bytes);
      return out;
    }
    static alloc(size, fill, name) {
      const out = new Buffer(size);
      if (fill !== undefined) out.fill(fill, name);
      return out;
    }
    static allocUnsafe(size) { return new Buffer(size); }
    static isBuffer(value) { return value instanceof Buffer; }
    // Node's Buffer.compare(a, b): compares byte by byte, returns -1 / 0 / 1
    static compare(a, b) {
      const left = Buffer.from(a);
      const right = Buffer.from(b);
      const shared = Math.min(left.length, right.length);
      for (let i = 0; i < shared; i += 1) {
        if (left[i] !== right[i]) return left[i] < right[i] ? -1 : 1;
      }
      if (left.length === right.length) return 0;
      return left.length < right.length ? -1 : 1;
    }
    static byteLength(value, name) {
      if (typeof value === 'string') return Buffer.fromString(value, name).length;
      if (ArrayBuffer.isView(value)) return value.byteLength;
      if (value instanceof ArrayBuffer) return value.byteLength;
      return 0;
    }
    static concat(list, totalLength) {
      const total = typeof totalLength === 'number' ? totalLength : list.reduce((sum, item) => sum + item.length, 0);
      const out = new Buffer(total);
      let at = 0;
      for (const item of list) {
        if (at >= total) break;
        const take = Math.min(item.length, total - at);
        out.set(item.subarray(0, take), at);
        at += take;
      }
      return out;
    }
    fill(value, name) {
      if (typeof value === 'string') {
        const bytes = Buffer.fromString(value, name);
        for (let i = 0; i < this.length; i++) this[i] = bytes[i % bytes.length];
        return this;
      }
      return super.fill(value);
    }
    toString(name, start, end) {
      const offset = typeof name === 'object' && name !== null ? name : null;
      const codec = encoding(offset ? offset.encoding : name);
      const from = offset && offset.offset !== undefined ? offset.offset : (start || 0);
      const to = offset && offset.length !== undefined ? from + offset.length : (end === undefined ? this.length : end);
      return __bytes_decode(toBase64(this.subarray(from, to)), codec);
    }
    toJSON() {
      return { type: 'Buffer', data: Array.from(this) };
    }
    equals(other) {
      if (!other || other.length !== this.length) return false;
      for (let i = 0; i < this.length; i++) if (this[i] !== other[i]) return false;
      return true;
    }
    compare(other) {
      const size = Math.min(this.length, other.length);
      for (let i = 0; i < size; i++) {
        if (this[i] !== other[i]) return this[i] < other[i] ? -1 : 1;
      }
      return this.length === other.length ? 0 : this.length < other.length ? -1 : 1;
    }
  }

  class Blob {
    constructor(parts, options) {
      const list = parts === undefined ? [] : parts;
      this._parts = [];
      for (const part of list) {
        if (typeof part === 'string') this._parts.push(Buffer.from(part));
        else if (part instanceof Buffer) this._parts.push(part);
        else if (part instanceof Blob) this._parts.push(part._joined());
        else this._parts.push(Buffer.from(part));
      }
      const settings = options || {};
      this.type = String(settings.type || '').toLowerCase();
    }
    _joined() { return Buffer.concat(this._parts, this.size); }
    get size() { return this._parts.reduce((sum, part) => sum + part.length, 0); }
    text() { return Promise.resolve(this._joined().toString('utf8')); }
    arrayBuffer() {
      const bytes = this._joined();
      return Promise.resolve(bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.length));
    }
    bytes() { return Promise.resolve(this._joined()); }
    slice(start, end, type) {
      const bytes = this._joined();
      const from = start === undefined ? 0 : start < 0 ? Math.max(bytes.length + start, 0) : Math.min(start, bytes.length);
      const to = end === undefined ? bytes.length : end < 0 ? Math.max(bytes.length + end, 0) : Math.min(end, bytes.length);
      return new Blob([bytes.subarray(from, Math.max(from, to))], { type: type === undefined ? this.type : type });
    }
  }

  const atob = (text) => {
    const bytes = fromBase64(String(text));
    let out = '';
    for (const byte of bytes) out += String.fromCharCode(byte);
    return out;
  };

  const btoa = (text) => {
    const source = String(text);
    const bytes = new Uint8Array(source.length);
    for (let i = 0; i < source.length; i++) {
      const code = source.charCodeAt(i);
      if (code > 255) throw new Error('btoa only accepts latin1 characters');
      bytes[i] = code;
    }
    return toBase64(bytes);
  };

  globalThis.Buffer = Buffer;
  globalThis.Blob = Blob;
  globalThis.atob = atob;
  globalThis.btoa = btoa;

  globalThis.__buffer_module = { Buffer, Blob, atob, btoa, default: { Buffer, Blob, atob, btoa } };
})();

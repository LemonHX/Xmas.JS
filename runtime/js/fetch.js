// fetch: Headers, Request, Response, FormData, File and fetch itself.
//
// The transport is native (see modules/fetch.rs and host/http.rs). This file is the API
// surface: header names fold to lower case and iterate in order, bodies are byte streams,
// and every promise is settled here so a request never touches a promise from Rust.
//
// Request bodies are read into memory before they are sent. A body that is itself a
// ReadableStream is refused for now; everything else (string, bytes, Blob, FormData,
// URLSearchParams) works.

(function () {
  'use strict';

  const SEP = '\u0000';
  const ABORTED = 'The operation was aborted';

  const encoder = (text) => new Uint8Array(__text_to_bytes(text, 'utf8'));
  const decoder = (bytes) => __bytes_to_text(bytes, 'utf8');

  const typeError = (message) => new TypeError(message);
  const abortError = () => new DOMException(ABORTED, 'AbortError');

  // ---------------------------------------------------------------- headers

  const TOKEN_SYMBOLS = "!#$%&'*+-.^_|~";

  function isTokenCharacter(code) {
    if (code >= 0x61 && code <= 0x7a) return true;
    if (code >= 0x30 && code <= 0x39) return true;
    if (code >= 0x41 && code <= 0x5a) return true;
    return TOKEN_SYMBOLS.indexOf(String.fromCharCode(code)) >= 0;
  }

  function checkName(name) {
    const text = String(name);
    if (text.length === 0) throw typeError('header name is empty');
    for (let i = 0; i < text.length; i += 1) {
      if (!isTokenCharacter(text.charCodeAt(i))) {
        throw typeError('invalid header name: ' + text);
      }
    }
    return text.toLowerCase();
  }

  function checkValue(value) {
    let text = String(value);
    text = text.replace(/^[\t\n\r ]+|[\t\n\r ]+$/g, '');
    for (let i = 0; i < text.length; i += 1) {
      const code = text.charCodeAt(i);
      if (code === 0 || code === 10 || code === 13) {
        throw typeError('invalid header value: ' + text);
      }
    }
    return text;
  }

  const HEADERS_INNER = new WeakMap();

  class Headers {
    constructor(init) {
      HEADERS_INNER.set(this, new Map());
      if (init === undefined || init === null) return;
      if (init instanceof Headers) {
        for (const [name, value] of init.entries()) this.append(name, value);
        return;
      }
      if (Array.isArray(init)) {
        for (const pair of init) {
          if (!Array.isArray(pair) || pair.length !== 2) {
            throw typeError('each header pair must be a name and a value');
          }
          this.append(pair[0], pair[1]);
        }
        return;
      }
      if (typeof init === 'object') {
        for (const name of Object.keys(init)) this.append(name, init[name]);
        return;
      }
      throw typeError('headers must be an object, an array or a Headers');
    }

    append(name, value) {
      const key = checkName(name);
      const text = checkValue(value);
      const map = HEADERS_INNER.get(this);
      const entry = map.get(key);
      if (entry === undefined) map.set(key, { name: String(name), values: [text] });
      else entry.values.push(text);
      this._sorted = null;
    }

    set(name, value) {
      const key = checkName(name);
      HEADERS_INNER.get(this).set(key, { name: String(name), values: [checkValue(value)] });
      this._sorted = null;
    }

    get(name) {
      const entry = HEADERS_INNER.get(this).get(checkName(name));
      if (entry === undefined) return null;
      return entry.values.join(', ');
    }

    has(name) {
      return HEADERS_INNER.get(this).has(checkName(name));
    }

    delete(name) {
      HEADERS_INNER.get(this).delete(checkName(name));
      this._sorted = null;
    }

    getSetCookie() {
      const entry = HEADERS_INNER.get(this).get('set-cookie');
      return entry === undefined ? [] : entry.values.slice();
    }

    _list() {
      if (this._sorted) return this._sorted;
      const map = HEADERS_INNER.get(this);
      const names = Array.from(map.keys()).sort();
      const out = [];
      for (const key of names) {
        const entry = map.get(key);
        if (key === 'set-cookie') {
          for (const value of entry.values) out.push([key, value]);
        } else {
          out.push([key, entry.values.join(', ')]);
        }
      }
      this._sorted = out;
      return out;
    }

    keys() {
      return this._list().map((pair) => pair[0])[Symbol.iterator]();
    }

    values() {
      return this._list().map((pair) => pair[1])[Symbol.iterator]();
    }

    entries() {
      return this._list()[Symbol.iterator]();
    }

    forEach(callback, thisArg) {
      for (const [name, value] of this._list()) callback.call(thisArg, value, name, this);
    }

    [Symbol.iterator]() {
      return this.entries();
    }

    get [Symbol.toStringTag]() {
      return 'Headers';
    }
  }

  function headersWire(headers) {
    let text = '';
    for (const [name, value] of headers._list()) text += name + SEP + value + SEP;
    return text;
  }

  function headersFromWire(pairs) {
    const headers = new Headers();
    for (let i = 0; i + 1 < pairs.length; i += 2) headers.append(pairs[i], pairs[i + 1]);
    return headers;
  }

  // ---------------------------------------------------------------- bytes

  // A body that is already a stream is never turned into bytes: a server writes it out as it is
  // produced, and the response getter hands the same stream back.
  function isStreamBody(value) {
    return value !== null && typeof value === 'object' && typeof value.getReader === 'function';
  }

  function asBytes(value) {
    if (value === null || value === undefined) return null;
    if (isStreamBody(value)) {
      throw typeError('a stream body has to be handed over as a stream');
    }
    if (typeof value === 'string') return encoder(value);
    if (value instanceof ArrayBuffer) return new Uint8Array(value);
    if (ArrayBuffer.isView(value)) {
      return new Uint8Array(value.buffer, value.byteOffset, value.byteLength).slice();
    }
    if (value instanceof Blob) {
      if (typeof value._joined === 'function') return value._joined();
      throw typeError('this Blob has to be read asynchronously');
    }
    if (typeof URLSearchParams !== 'undefined' && value instanceof URLSearchParams) {
      return encoder(value.toString());
    }
    if (value instanceof FormData) {
      const encoded = value._multipart();
      return encoded;
    }
    if (value instanceof ReadableStream) {
      throw typeError('a ReadableStream as a request body is not supported yet');
    }
    return encoder(String(value));
  }

  function concatBytes(chunks) {
    if (chunks.length === 1) return chunks[0];
    let total = 0;
    for (const chunk of chunks) total += chunk.length;
    const out = new Uint8Array(total);
    let offset = 0;
    for (const chunk of chunks) {
      out.set(chunk, offset);
      offset += chunk.length;
    }
    return out;
  }

  // ---------------------------------------------------------------- body mixin

  // One place that consumes a body stream: text, json, bytes, blob all go through here.
  function consume(owner) {
    if (owner._bodyUsed) {
      return Promise.reject(typeError('body has already been consumed'));
    }
    owner._bodyUsed = true;
    const chunks = [];
    // Only a response that came off the wire has a native body to read chunk by chunk. A request
    // built in JS carries its bytes in _bytes, and its body getter makes a stream out of them.
    if (typeof owner._native === 'number') {
      const id = owner._native;
      owner._native = null;
      return new Promise((resolve, reject) => {
        const step = () => {
          __fetch_read(id).then((chunk) => {
            if (chunk === 'eof') {
              __fetch_free(id);
              resolve(concatBytes(chunks));
              return;
            }
            chunks.push(new Uint8Array(chunk));
            step();
          }, (error) => {
            __fetch_free(id);
            reject(error);
          });
        };
        step();
      });
    }
    const stream = owner.body;
    if (stream === null) return Promise.resolve(new Uint8Array(0));
    const reader = stream.getReader();
    return new Promise((resolve, reject) => {
      const step = () => {
        reader.read().then((result) => {
          if (result.done) {
            resolve(concatBytes(chunks));
            return;
          }
          const value = result.value;
          chunks.push(value instanceof Uint8Array ? value : new Uint8Array(value));
          step();
        }, reject);
      };
      step();
    });
  }

  function bodyMethods(target) {
    target.prototype.arrayBuffer = function arrayBuffer() {
      return consume(this).then((bytes) => bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength));
    };
    target.prototype.bytes = function bytes() {
      return consume(this);
    };
    target.prototype.text = function text() {
      return consume(this).then(decoder);
    };
    target.prototype.json = function json() {
      return this.text().then((text) => JSON.parse(text));
    };
    target.prototype.blob = function blob() {
      return consume(this).then((bytes) => new Blob([bytes], { type: this.headers.get('content-type') || '' }));
    };
    target.prototype.formData = function formData() {
      return this.text().then((text) => {
        const type = this.headers.get('content-type') || '';
        if (type.indexOf('application/x-www-form-urlencoded') === 0) {
          return FormData._fromUrlEncoded(text);
        }
        throw typeError('formData() only understands urlencoded bodies for now, got ' + type);
      });
    };
  }

  // ---------------------------------------------------------------- FormData

  const FORM_INNER = new WeakMap();

  class FormData {
    constructor() {
      FORM_INNER.set(this, []);
    }

    append(name, value, filename) {
      const field = String(name);
      if (value instanceof Blob) {
        FORM_INNER.get(this).push({
          name: field,
          value,
          filename: filename === undefined ? (value.name === undefined ? 'blob' : value.name) : String(filename),
        });
        return;
      }
      FORM_INNER.get(this).push({ name: field, value: String(value), filename: null });
    }

    set(name, value, filename) {
      const field = String(name);
      const list = FORM_INNER.get(this);
      const index = list.findIndex((entry) => entry.name === field);
      if (index >= 0) list.splice(index, list.length - index);
      this.append(name, value, filename);
    }

    get(name) {
      const field = String(name);
      for (const entry of FORM_INNER.get(this)) {
        if (entry.name === field) return entry.value;
      }
      return null;
    }

    getAll(name) {
      const field = String(name);
      return FORM_INNER.get(this)
        .filter((entry) => entry.name === field)
        .map((entry) => entry.value);
    }

    has(name) {
      const field = String(name);
      return FORM_INNER.get(this).some((entry) => entry.name === field);
    }

    delete(name) {
      const field = String(name);
      const list = FORM_INNER.get(this);
      for (let i = list.length - 1; i >= 0; i -= 1) {
        if (list[i].name === field) list.splice(i, 1);
      }
    }

    keys() {
      return FORM_INNER.get(this).map((entry) => entry.name)[Symbol.iterator]();
    }

    values() {
      return FORM_INNER.get(this).map((entry) => entry.value)[Symbol.iterator]();
    }

    entries() {
      return FORM_INNER.get(this).map((entry) => [entry.name, entry.value])[Symbol.iterator]();
    }

    forEach(callback, thisArg) {
      for (const entry of FORM_INNER.get(this)) callback.call(thisArg, entry.value, entry.name, this);
    }

    [Symbol.iterator]() {
      return this.entries();
    }

    get [Symbol.toStringTag]() {
      return 'FormData';
    }

    // The multipart body, with the boundary the content-type header needs.
    _multipart() {
      let boundary = '----XmasFormBoundary';
      const alphabet = 'abcdefghijklmnopqrstuvwxyz0123456789';
      for (let i = 0; i < 16; i += 1) {
        boundary += alphabet[Math.floor(Math.random() * alphabet.length)];
      }
      const parts = [];
      for (const entry of FORM_INNER.get(this)) {
        let head = '--' + boundary + '\r\nContent-Disposition: form-data; name="' + entry.name + '"';
        let payload;
        if (entry.filename !== null) {
          head += '; filename="' + entry.filename + '"';
          if (entry.value.type) head += '\r\nContent-Type: ' + entry.value.type;
          payload = entry.value._joined();
        } else {
          payload = encoder(entry.value);
        }
        parts.push(encoder(head + '\r\n\r\n'));
        parts.push(payload);
        parts.push(encoder('\r\n'));
      }
      parts.push(encoder('--' + boundary + '--\r\n'));
      const bytes = concatBytes(parts);
      bytes.boundary = boundary;
      return bytes;
    }

    static _fromUrlEncoded(text) {
      const form = new FormData();
      const body = String(text);
      if (body.length === 0) return form;
      for (const pair of body.split('&')) {
        const index = pair.indexOf('=');
        const name = index < 0 ? pair : pair.slice(0, index);
        const value = index < 0 ? '' : pair.slice(index + 1);
        form.append(decodeURIComponent(name.replace(/\+/g, ' ')), decodeURIComponent(value.replace(/\+/g, ' ')));
      }
      return form;
    }
  }

  // ---------------------------------------------------------------- Blob with a name

  class File extends Blob {
    constructor(parts, name, options) {
      super(parts, options);
      const settings = options || {};
      this.name = String(name);
      this.lastModified = settings.lastModified === undefined ? Date.now() : Number(settings.lastModified);
    }

    get [Symbol.toStringTag]() {
      return 'File';
    }
  }

  // ---------------------------------------------------------------- Request

  const REQUEST_INNER = new WeakMap();

  class Request {
    constructor(input, init) {
      const options = init || {};
      let url;
      let method = options.method;
      let headers;
      let body = null;
      let signal = options.signal === undefined ? null : options.signal;
      let redirect = options.redirect;

      if (input instanceof Request) {
        url = input.url;
        if (method === undefined) method = input.method;
        if (redirect === undefined) redirect = input.redirect;
        headers = new Headers(input.headers);
        if (options.headers !== undefined) {
          for (const [name, value] of new Headers(options.headers).entries()) headers.set(name, value);
        }
        if (options.body === undefined) {
          const inputBytes = input._bytes;
          body = inputBytes === null ? null : inputBytes.slice();
        }
        if (signal === null) signal = input.signal;
      } else {
        url = String(input);
        headers = new Headers(options.headers);
      }

      if (method === undefined) method = body === null && input instanceof Request ? input.method : 'GET';
      method = String(method).toUpperCase();
      if (!/^[A-Z]+$/.test(method)) throw typeError('invalid method: ' + method);
      if ((method === 'GET' || method === 'HEAD') && options.body !== undefined && options.body !== null) {
        throw typeError('a ' + method + ' request cannot have a body');
      }

      if (options.body !== undefined && options.body !== null) {
        body = asBytes(options.body);
        if (options.body instanceof FormData) {
          headers.set('content-type', 'multipart/form-data; boundary=' + body.boundary);
        }
        if (typeof options.body === 'string' || (typeof URLSearchParams !== 'undefined' && options.body instanceof URLSearchParams)) {
          if (!headers.has('content-type')) headers.set('content-type', 'text/plain;charset=UTF-8');
        }
      }
      if (options.body !== undefined && options.body !== null && options.body instanceof Blob) {
        if (options.body.type && !headers.has('content-type')) headers.set('content-type', options.body.type);
      }

      REQUEST_INNER.set(this, { url, method, headers, signal, redirect: redirect || 'follow' });
      this._bytes = body;
      this._bodyUsed = false;
      this._stream = null;
    }

    get url() { return REQUEST_INNER.get(this).url; }
    get method() { return REQUEST_INNER.get(this).method; }
    get headers() { return REQUEST_INNER.get(this).headers; }
    get signal() { return REQUEST_INNER.get(this).signal; }
    get redirect() { return REQUEST_INNER.get(this).redirect; }
    get bodyUsed() { return this._bodyUsed; }

    get body() {
      if (this._bytes === null) return null;
      if (this._stream === null) {
        const bytes = this._bytes;
        this._stream = new ReadableStream({
          type: 'byte',
          start(controller) {
            if (bytes.length > 0) controller.enqueue(bytes.slice());
            controller.close();
          },
        });
      }
      return this._stream;
    }

    clone() {
      const copy = Object.create(Request.prototype);
      const inner = REQUEST_INNER.get(this);
      REQUEST_INNER.set(copy, {
        url: inner.url,
        method: inner.method,
        headers: new Headers(inner.headers),
        signal: inner.signal,
        redirect: inner.redirect,
      });
      copy._bytes = this._bytes === null ? null : this._bytes.slice();
      copy._bodyUsed = false;
      copy._stream = null;
      return copy;
    }

    get [Symbol.toStringTag]() {
      return 'Request';
    }
  }

  bodyMethods(Request);

  // ---------------------------------------------------------------- Response

  const RESPONSE_INNER = new WeakMap();

  function makeStream(id) {
    return new ReadableStream({
      type: 'byte',
      pull(controller) {
        return __fetch_read(id).then((chunk) => {
          if (chunk === 'eof') {
            __fetch_free(id);
            controller.close();
            return;
          }
          const bytes = new Uint8Array(chunk);
          if (bytes.length > 0) controller.enqueue(bytes);
        }, (error) => {
          __fetch_free(id);
          controller.error(error);
        });
      },
      cancel() {
        __fetch_free(id);
      },
    });
  }

  class Response {
    constructor(body, init) {
      const options = init || {};
      const status = options.status === undefined ? 200 : Number(options.status);
      if (!(status >= 200 && status <= 599) && status !== 101) {
        throw new RangeError('a response status must be between 200 and 599');
      }
      if (body !== undefined && body !== null && (status === 204 || status === 205 || status === 304)) {
        throw typeError('a ' + status + ' response cannot have a body');
      }
      RESPONSE_INNER.set(this, {
        status,
        statusText: options.statusText === undefined ? '' : String(options.statusText),
        headers: new Headers(options.headers),
        url: '',
        redirected: false,
      });
      this._native = null;
      // A body that is a stream is kept as one: the server writes it out as it is produced, and a
      // response that came off the wire reads that way too.
      this._source = isStreamBody(body) ? body : null;
      this._bytes = this._source === null ? asBytes(body) : null;
      this._bodyUsed = false;
      this._stream = null;
    }

    static _fromNative(id, description) {
      const fields = description.split(SEP);
      const response = Object.create(Response.prototype);
      const headers = new Headers();
      for (let i = 4; i + 1 < fields.length; i += 2) headers.append(fields[i], fields[i + 1]);
      RESPONSE_INNER.set(response, {
        status: Number(fields[0]),
        statusText: fields[1],
        headers,
        url: fields[2],
        redirected: fields[3] === '1',
      });
      // A response that cannot have a body has none, and its native side is released now.
      const bodyless = fields[0] === '204' || fields[0] === '205' || fields[0] === '304';
      response._native = bodyless ? null : id;
      if (bodyless) __fetch_free(id);
      response._source = null;
      response._bytes = null;
      response._bodyUsed = false;
      response._stream = null;
      return response;
    }

    get status() { return RESPONSE_INNER.get(this).status; }
    get statusText() { return RESPONSE_INNER.get(this).statusText; }
    get headers() { return RESPONSE_INNER.get(this).headers; }
    get url() { return RESPONSE_INNER.get(this).url; }
    get redirected() { return RESPONSE_INNER.get(this).redirected; }
    get ok() {
      const status = RESPONSE_INNER.get(this).status;
      return status >= 200 && status < 300;
    }
    get type() { return 'default'; }
    get bodyUsed() { return this._bodyUsed; }

    get body() {
      if (this._source !== null) return this._source;
      if (this._native !== null) {
        if (this._stream === null) this._stream = makeStream(this._native);
        return this._stream;
      }
      if (this._bytes === null) return null;
      if (this._stream === null) {
        const bytes = this._bytes;
        this._stream = new ReadableStream({
          type: 'byte',
          start(controller) {
            if (bytes.length > 0) controller.enqueue(bytes.slice());
            controller.close();
          },
        });
      }
      return this._stream;
    }

    clone() {
      if (this._bodyUsed) throw typeError('body has already been consumed');
      const copy = Object.create(Response.prototype);
      RESPONSE_INNER.set(copy, {
        status: this.status,
        statusText: this.statusText,
        headers: new Headers(this.headers),
        url: this.url,
        redirected: this.redirected,
      });
      copy._native = null;
      copy._source = null;
      copy._bytes = null;
      copy._bodyUsed = false;
      copy._stream = null;
      if (this._native !== null) {
        // Both copies read one stream: tee splits what comes off the socket.
        const branches = this.body.tee();
        this._native = null;
        this._stream = branches[0];
        copy._stream = branches[1];
        return copy;
      }
      copy._bytes = this._bytes === null ? null : this._bytes.slice();
      return copy;
    }

    static json(data, init) {
      const text = JSON.stringify(data);
      const response = new Response(text, init);
      if (!response.headers.has('content-type')) {
        response.headers.set('content-type', 'application/json');
      }
      return response;
    }

    static redirect(url, status) {
      const code = status === undefined ? 302 : Number(status);
      if ([301, 302, 303, 307, 308].indexOf(code) < 0) {
        throw new RangeError('invalid redirect status: ' + code);
      }
      return new Response(null, { status: code, headers: { location: String(url) } });
    }

    static error() {
      const response = Object.create(Response.prototype);
      RESPONSE_INNER.set(response, {
        status: 0,
        statusText: '',
        headers: new Headers(),
        url: '',
        redirected: false,
      });
      response._native = null;
      response._source = null;
      response._bytes = null;
      response._bodyUsed = false;
      response._stream = null;
      return response;
    }

    get [Symbol.toStringTag]() {
      return 'Response';
    }
  }

  bodyMethods(Response);

  // ---------------------------------------------------------------- fetch

  let nextId = 1;

  function fetch(input, init) {
    let request;
    try {
      request = new Request(input, init);
    } catch (error) {
      return Promise.reject(error);
    }
    const signal = request.signal;
    if (signal !== null && signal.aborted) return Promise.reject(abortError());

    const id = nextId;
    nextId += 1;
    const onAbort = () => __fetch_abort(id);
    if (signal !== null && typeof signal.addEventListener === 'function') {
      signal.addEventListener('abort', onAbort);
    }

    const body = request._bytes;
    return __fetch(id, request.method, request.url, headersWire(request.headers), request.redirect, body)
      .then((description) => {
        if (signal !== null && typeof signal.removeEventListener === 'function') {
          signal.removeEventListener('abort', onAbort);
        }
        return Response._fromNative(id, description);
      }, (error) => {
        if (signal !== null && typeof signal.removeEventListener === 'function') {
          signal.removeEventListener('abort', onAbort);
        }
        if (error && error.message === ABORTED) throw abortError();
        throw error;
      });
  }

  globalThis.fetch = fetch;
  globalThis.Headers = Headers;
  globalThis.Request = Request;
  globalThis.Response = Response;
  globalThis.FormData = FormData;
  globalThis.File = File;

  globalThis.__fetch_module = {
    fetch,
    Headers,
    Request,
    Response,
    FormData,
    File,
    Blob: globalThis.Blob,
  };
})();

(function () {
  const bytesOf = (data, encoding) => typeof data === 'string'
    ? Buffer.from(data, encoding || 'utf8')
    : Buffer.from(data);
  // The digest returns an ArrayBuffer; convert to a string using encoding if requested, otherwise it is a Buffer
  const out = (buffer, encoding) => {
    const buf = Buffer.from(buffer);
    if (!encoding) return buf;
    return buf.toString(String(encoding).toLowerCase());
  };

  class Hash {
    constructor(algorithm) {
      this.algorithm = String(algorithm).toLowerCase();
      this.__id = __hash_new(this.algorithm);
      if (!this.__id) throw new TypeError(`Digest method not supported: ${algorithm}`);
      this.__closed = false;
    }
    update(data, encoding) {
      if (this.__closed) throw new Error('Hash has already been digested');
      __hash_update(this.__id, bytesOf(data, encoding));
      return this;
    }
    digest(encoding) {
      if (this.__closed) throw new Error('Hash has already been digested');
      this.__closed = true;
      return out(__hash_digest(this.__id), encoding);
    }
    copy() {
      const clone = Object.create(Hash.prototype);
      clone.algorithm = this.algorithm;
      clone.__id = __hash_copy(this.__id);
      clone.__closed = false;
      return clone;
    }
  }

  class Hmac {
    constructor(algorithm, key) {
      this.algorithm = String(algorithm).toLowerCase();
      this.__id = __hmac_new(this.algorithm, bytesOf(key));
      if (!this.__id) throw new TypeError(`Hmac method not supported: ${algorithm}`);
      this.__closed = false;
    }
    update(data, encoding) {
      if (this.__closed) throw new Error('Hmac has already been digested');
      __hmac_update(this.__id, bytesOf(data, encoding));
      return this;
    }
    digest(encoding) {
      if (this.__closed) throw new Error('Hmac has already been digested');
      this.__closed = true;
      return out(__hmac_digest(this.__id), encoding);
    }
  }

  const randomBytes = (size) => Buffer.from(__random_bytes(Number(size)));

  const randomFillSync = (view, offset, size) => {
    const start = offset === undefined ? 0 : Number(offset);
    const length = size === undefined ? view.length - start : Number(size);
    const filled = randomBytes(length);
    view.set(filled, start);
    return view;
  };

  const randomUUID = () => __random_uuid();

  const randomInt = (min, max) => {
    let low = Number(min);
    let high = max === undefined ? low : Number(max);
    if (max === undefined) { low = 0; }
    if (!Number.isInteger(low) || !Number.isInteger(high) || high <= low) {
      throw new RangeError('randomInt: invalid range');
    }
    return __random_int(low, high);
  };

  const getRandomValues = (view) => {
    const filled = randomBytes(view.BYTES_PER_ELEMENT * view.length);
    new Uint8Array(view.buffer, view.byteOffset, view.byteLength).set(filled);
    return view;
  };

  const timingSafeEqual = (a, b) => {
    const left = Buffer.from(a);
    const right = Buffer.from(b);
    if (left.length !== right.length) {
      throw new RangeError('timingSafeEqual: the two inputs have different lengths');
    }
    let diff = 0;
    for (let i = 0; i < left.length; i += 1) diff |= left[i] ^ right[i];
    return diff === 0;
  };

  const hash = (algorithm, data, encoding) => out(__hash_once(String(algorithm).toLowerCase(), bytesOf(data)), encoding);

  const crypto = {
    createHash: (algorithm) => new Hash(algorithm),
    createHmac: (algorithm, key) => new Hmac(algorithm, key),
    hash,
    randomBytes,
    randomFillSync,
    randomUUID,
    randomInt,
    getRandomValues,
    timingSafeEqual,
    webcrypto: { getRandomValues, randomUUID },
  };

  globalThis.crypto = crypto;
  globalThis.Crypto = { prototype: crypto };
  globalThis.__crypto_module = crypto;
})();

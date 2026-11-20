(function () {
  // Node's StringDecoder: the byte work is native (src/modules/string_decoder.rs,
  // which carries Node's chunk-boundary algorithm), this is the shape scripts see.

  const NUL = String.fromCharCode(0);

  class StringDecoder {
    constructor(encoding) {
      const created = __sd_create(encoding === undefined || encoding === null ? 'utf8' : String(encoding));
      const split = created.indexOf(NUL);
      this.encoding = created.slice(0, split);
      this._id = Number(created.slice(split + 1));
    }

    write(buffer) {
      return __sd_write(this._id, buffer);
    }

    end(buffer) {
      return buffer === undefined ? __sd_end(this._id) : __sd_end(this._id, buffer);
    }

    [Symbol.dispose]() {}
  }

  globalThis.__string_decoder = { StringDecoder, default: { StringDecoder } };
})();

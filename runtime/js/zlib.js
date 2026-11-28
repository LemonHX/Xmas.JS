(function () {
  // Node's zlib. The compression itself is native (src/modules/zlib.rs, which
  // carries LLRT's one-shot functions); what is here is the option and callback
  // shape, because that is what scripts touch.

  // Node exposes zlib.constants with the values from zlib.h and brotli's header.
  const constants = {
    Z_NO_COMPRESSION: 0,
    Z_BEST_SPEED: 1,
    Z_BEST_COMPRESSION: 9,
    Z_DEFAULT_COMPRESSION: -1,
    Z_NO_FLUSH: 0,
    Z_PARTIAL_FLUSH: 1,
    Z_SYNC_FLUSH: 2,
    Z_FULL_FLUSH: 3,
    Z_FINISH: 4,
    Z_BLOCK: 5,
    Z_TREES: 6,
    Z_OK: 0,
    Z_STREAM_END: 1,
    Z_NEED_DICT: 2,
    Z_ERRNO: -1,
    Z_STREAM_ERROR: -2,
    Z_DATA_ERROR: -3,
    Z_MEM_ERROR: -4,
    Z_BUF_ERROR: -5,
    Z_VERSION_ERROR: -6,
    Z_NO_COMPRESSION_LEVEL: 0,
    Z_DEFAULT_STRATEGY: 0,
    Z_FILTERED: 1,
    Z_HUFFMAN_ONLY: 2,
    Z_RLE: 3,
    Z_FIXED: 4,
    Z_MIN_WINDOWBITS: 8,
    Z_MAX_WINDOWBITS: 15,
    Z_DEFAULT_WINDOWBITS: 15,
    Z_MIN_CHUNK: 64,
    Z_MAX_CHUNK: Infinity,
    Z_DEFAULT_CHUNK: 16384,
    Z_MIN_MEMLEVEL: 1,
    Z_MAX_MEMLEVEL: 9,
    Z_DEFAULT_MEMLEVEL: 8,
    Z_MIN_LEVEL: -1,
    Z_MAX_LEVEL: 9,
    Z_DEFAULT_LEVEL: -1,
    BROTLI_OPERATION_PROCESS: 0,
    BROTLI_OPERATION_FLUSH: 1,
    BROTLI_OPERATION_FINISH: 2,
    BROTLI_OPERATION_EMIT_METADATA: 3,
    BROTLI_PARAM_MODE: 0,
    BROTLI_PARAM_QUALITY: 1,
    BROTLI_PARAM_SIZE_HINT: 5,
    BROTLI_MIN_QUALITY: 0,
    BROTLI_MAX_QUALITY: 11,
    BROTLI_DEFAULT_QUALITY: 11,
    BROTLI_MIN_WINDOW_BITS: 10,
    BROTLI_MAX_WINDOW_BITS: 24,
    BROTLI_DEFAULT_WINDOW: 22,
  };

  const levelOf = (options) => (options && options.level !== undefined ? Number(options.level) : -1);
  const limitOf = (options) =>
    options && options.maxOutputLength !== undefined ? Number(options.maxOutputLength) : 0;

  // A range complaint keeps its class; anything else is a bad payload.
  const convert = (thrown) => {
    const text = String((thrown && thrown.message) || thrown);
    if (text.startsWith('RangeError: ')) {
      const error = new RangeError(text.slice('RangeError: '.length));
      error.code = 'ERR_BUFFER_TOO_LARGE';
      return error;
    }
    const error = new Error(text);
    error.errno = constants.Z_DATA_ERROR;
    error.code = 'Z_DATA_ERROR';
    return error;
  };

  const sync = (kind) => (data, options) => {
    try {
      // Native gives bytes as an ArrayBuffer; Node returns a Buffer.
      return Buffer.from(__zlib_run(kind, data, levelOf(options), limitOf(options)));
    } catch (thrown) {
      throw convert(thrown);
    }
  };

  const whenDone = (kind) => (data, options, callback) => {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      throw new TypeError('The "callback" argument must be of type function');
    }
    __zlib_start(kind, data, levelOf(options), limitOf(options)).then(
      (output) => callback(null, Buffer.from(output)),
      (thrown) => callback(convert(thrown)),
    );
  };

  const pairs = [
    'deflate',
    'deflateRaw',
    'gzip',
    'inflate',
    'inflateRaw',
    'gunzip',
    'brotliCompress',
    'brotliDecompress',
  ];

  // zstd takes no level here: this runtime writes stored blocks, so there is nothing a level
  // could ask for. The argument is accepted and ignored so Bun's signatures keep working.
  const zstdSync = (compress) => (data, options) => {
    try {
      return Buffer.from(__zstd_run(compress, data, limitOf(options)));
    } catch (thrown) {
      throw convert(thrown);
    }
  };

  const zstd = (compress) => (data, options, callback) => {
    if (typeof options === 'function') {
      callback = options;
      options = undefined;
    }
    if (typeof callback !== 'function') {
      throw new TypeError('The "callback" argument must be of type function');
    }
    try {
      const output = __zstd_run(compress, data, limitOf(options));
      // The callback is called off the stack, as Node does, so a throw inside it is not
      // swallowed by this try.
      Promise.resolve().then(() => callback(null, Buffer.from(output)));
    } catch (thrown) {
      Promise.resolve().then(() => callback(convert(thrown)));
    }
  };

  const module = { constants, crc32: (data, value) => __crc32(data, value === undefined ? 0 : Number(value)) };
  for (const kind of pairs) {
    module[kind] = whenDone(kind);
    module[kind + 'Sync'] = sync(kind);
  }
  for (const [name, compress] of [
    ['zstdCompress', true],
    ['zstdDecompress', false],
  ]) {
    module[name] = zstd(compress);
    module[name + 'Sync'] = zstdSync(compress);
  }
  module.default = module;

  globalThis.__zlib_module = module;
})();

// TextEncoder and TextDecoder: the two globals every handler writes SSE with and every runtime is
// expected to have. The conversion is the buffer module's, so a string crosses the engine once.

(function () {
  'use strict';

  // The labels the converter underneath knows. Anything else is refused rather than guessed at:
  // silently decoding as utf-8 is how a mojibake bug survives a test suite.
  const LABELS = {
    'utf-8': 'utf8',
    utf8: 'utf8',
    'unicode-1-1-utf-8': 'utf8',
    'utf-16le': 'utf16le',
    'utf-16': 'utf16le',
    ucs2: 'utf16le',
    'ucs-2': 'utf16le',
    latin1: 'latin1',
    binary: 'latin1',
    ascii: 'ascii',
    'iso-8859-1': 'latin1',
  };

  const CANONICAL = {
    utf8: 'utf-8',
    utf16le: 'utf-16le',
    latin1: 'windows-1252',
    ascii: 'windows-1252',
  };

  function viewOf(input) {
    if (input instanceof Uint8Array) return input;
    if (ArrayBuffer.isView(input)) {
      return new Uint8Array(input.buffer, input.byteOffset, input.byteLength);
    }
    if (input instanceof ArrayBuffer) return new Uint8Array(input);
    if (typeof SharedArrayBuffer !== 'undefined' && input instanceof SharedArrayBuffer) {
      return new Uint8Array(input);
    }
    throw new TypeError('this value has no bytes to decode');
  }

  class TextEncoder {
    constructor() {
      this.encoding = 'utf-8';
    }

    encode(text) {
      if (text === undefined) return new Uint8Array(0);
      return new Uint8Array(__text_to_bytes(String(text), 'utf8'));
    }

    encodeInto(source, destination) {
      const bytes = this.encode(source);
      const target = viewOf(destination);
      const written = Math.min(bytes.length, target.length);
      target.set(bytes.subarray(0, written));
      return { read: written === bytes.length ? String(source).length : written, written };
    }
  }

  class TextDecoder {
    constructor(label, options) {
      const name = label === undefined ? 'utf-8' : String(label).toLowerCase();
      const encoding = LABELS[name];
      if (encoding === undefined) throw new RangeError('unsupported encoding: ' + name);
      this.encoding = CANONICAL[encoding];
      this.fatal = Boolean(options && options.fatal);
      this.ignoreBOM = Boolean(options && options.ignoreBOM);
      this._encoding = encoding;
    }

    decode(input) {
      if (input === undefined) return '';
      const bytes = viewOf(input);
      if (bytes.length === 0) return '';
      return __bytes_to_text(bytes, this._encoding);
    }
  }

  globalThis.TextEncoder = TextEncoder;
  globalThis.TextDecoder = TextDecoder;
})();

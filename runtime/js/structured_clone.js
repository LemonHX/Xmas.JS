(function () {
  // structuredClone, after the HTML structured clone algorithm: cycles and shared references are kept,
  // Map, Set, Date, RegExp, Error, ArrayBuffer and typed arrays are copied, and what cannot be cloned
  // (functions, symbols, class instances' methods) throws a DataCloneError.
  const fail = (what) => {
    const message = what + ' could not be cloned.';
    const error = typeof DOMException === 'function' ? new DOMException(message, 'DataCloneError') : new Error(message);
    if (!(error instanceof Error) || error.name !== 'DataCloneError') error.name = 'DataCloneError';
    return error;
  };
  const ERRORS = { Error, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError };

  function clone(value, seen, transfer) {
    const type = typeof value;
    if (type === 'function') throw fail(String(value).slice(0, 40));
    if (type === 'symbol') throw fail('Symbol()');
    if (value === null || type !== 'object') return value;
    if (seen.has(value)) return seen.get(value);

    const tag = Object.prototype.toString.call(value);
    let out;
    switch (tag) {
      case '[object Boolean]': return seen.set(value, new Boolean(value.valueOf())).get(value);
      case '[object Number]': return seen.set(value, new Number(value.valueOf())).get(value);
      case '[object String]': return seen.set(value, new String(value.valueOf())).get(value);
      case '[object BigInt]': return seen.set(value, Object(value.valueOf())).get(value);
      case '[object Date]': return seen.set(value, new Date(value.getTime())).get(value);
      case '[object RegExp]': {
        out = new RegExp(value.source, value.flags);
        seen.set(value, out);
        return out;
      }
      case '[object ArrayBuffer]': {
        out = value.slice(0);
        seen.set(value, out);
        return out;
      }
      case '[object Map]': {
        out = new Map();
        seen.set(value, out);
        for (const [k, v] of value) out.set(clone(k, seen, transfer), clone(v, seen, transfer));
        return out;
      }
      case '[object Set]': {
        out = new Set();
        seen.set(value, out);
        for (const v of value) out.add(clone(v, seen, transfer));
        return out;
      }
      case '[object Array]': {
        out = new Array(value.length);
        seen.set(value, out);
        for (const key of Object.keys(value)) out[key] = clone(value[key], seen, transfer);
        return out;
      }
      default:
    }
    if (ArrayBuffer.isView(value)) {
      const buffer = clone(value.buffer, seen, transfer);
      const Ctor = value instanceof DataView ? DataView : (globalThis[tag.slice(8, -1)] || Uint8Array);
      out = value instanceof DataView
        ? new DataView(buffer, value.byteOffset, value.byteLength)
        : new Ctor(buffer, value.byteOffset, value.length);
      seen.set(value, out);
      return out;
    }
    if (value instanceof Error) {
      const Ctor = ERRORS[value.name] || Error;
      out = Object.create(Ctor.prototype);
      seen.set(value, out);
      Object.defineProperty(out, 'message', { value: String(value.message), writable: true, configurable: true });
      if (value.stack !== undefined) Object.defineProperty(out, 'stack', { value: String(value.stack), writable: true, configurable: true });
      if (value.cause !== undefined) Object.defineProperty(out, 'cause', { value: clone(value.cause, seen, transfer), writable: true, configurable: true });
      return out;
    }
    if (tag === '[object Promise]' || tag === '[object WeakMap]' || tag === '[object WeakSet]' || tag === '[object WeakRef]') {
      throw fail(tag.slice(8, -1));
    }
    // A plain object, or an instance of a class: own enumerable string keys are copied, the prototype is not.
    out = {};
    seen.set(value, out);
    for (const key of Object.keys(value)) out[key] = clone(value[key], seen, transfer);
    return out;
  }

  globalThis.structuredClone = function structuredClone(value, options) {
    if (arguments.length === 0) throw new TypeError("The \"value\" argument must be specified");
    return clone(value, new Map(), options && options.transfer);
  };
})();

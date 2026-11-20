(function () {
  // inspect and the ANSI table are native (src/modules/util.rs). Everything
  // here is the part Node writes in JavaScript too: the % spec, the wrappers,
  // the type predicates. format follows LLRT's build_formatted_string in what
  // each spec does, written as a scan instead of a byte loop because the scan
  // is where the string already is.

  const customInspect = Symbol.for('nodejs.util.inspect.custom');

  const inspect = (value, options) => __util_inspect(value, !!(options && options.colors));
  inspect.custom = customInspect;
  inspect.defaultOptions = { depth: 4, colors: false, showHidden: false };

  const inspectFor = (options) => (value) => __util_inspect(value, !!(options && options.colors));

  const json = (value) => {
    try {
      const text = JSON.stringify(value);
      return text === undefined ? 'undefined' : text;
    } catch {
      return '[Circular]';
    }
  };

  const formatWith = (options, first, rest) => {
    const show = inspectFor(options);
    if (typeof first !== 'string') {
      const all = first === undefined && rest.length === 0 ? [] : [first].concat(rest);
      return all.map(show).join(' ');
    }
    if (rest.length === 0) return first;
    let index = 0;
    const text = first.replace(/%([sdifjoOc%])/g, (whole, spec) => {
      if (spec === '%') return '%';
      if (index >= rest.length) return whole;
      const value = rest[index];
      index += 1;
      switch (spec) {
        case 's':
          return typeof value === 'bigint' ? value + 'n' : String(value);
        case 'd':
          if (typeof value === 'bigint') return value + 'n';
          if (value instanceof Number) value = value.valueOf();
          return typeof value === 'symbol' ? 'NaN' : String(Number(value));
        case 'i':
          return String(parseInt(value, 10));
        case 'f':
          return String(parseFloat(value));
        case 'j':
          return json(value);
        case 'c':
          return '';
        default:
          return show(value);
      }
    });
    // Arguments the string did not use are appended, as Node does.
    if (index >= rest.length) return text;
    return text + ' ' + rest.slice(index).map(show).join(' ');
  };

  const format = (...args) => formatWith(null, args[0], args.slice(1));
  const formatWithOptions = (options, ...args) => formatWith(options, args[0], args.slice(1));

  const styleText = (format, text, _options) => {
    const body = String(text);
    const formats = Array.isArray(format) ? format : [format];
    const opens = [];
    const closes = [];
    for (const one of formats) {
      const pair = __util_ansi(String(one)).split('\u0000');
      opens.push(pair[0]);
      closes.unshift(pair[1]);
    }
    return opens.join('') + body + closes.join('');
  };

  const inherits = (constructor, superConstructor) => {
    if (constructor === undefined || constructor === null) {
      throw new TypeError('The "constructor" argument must be of type function');
    }
    if (superConstructor === undefined || superConstructor === null) {
      throw new TypeError('The "superConstructor" argument must be of type function');
    }
    Object.setPrototypeOf(constructor.prototype, superConstructor.prototype);
    Object.defineProperty(constructor, 'super_', {
      value: superConstructor,
      writable: true,
      configurable: true,
    });
    Object.defineProperty(constructor.prototype, 'constructor', {
      value: constructor,
      writable: true,
      configurable: true,
    });
  };

  const customPromisify = Symbol.for('nodejs.util.promisify.custom');

  const promisify = (original) => {
    if (typeof original !== 'function') {
      throw new TypeError('The "original" argument must be of type function');
    }
    if (original[customPromisify]) return original[customPromisify];
    function fn(...args) {
      const self = this;
      return new Promise((resolve, reject) => {
        original.call(self, ...args, (error, value) => (error ? reject(error) : resolve(value)));
      });
    }
    Object.setPrototypeOf(fn, Object.getPrototypeOf(original));
    Object.defineProperty(fn, 'name', { value: original.name, configurable: true });
    return fn;
  };
  promisify.custom = customPromisify;

  const callbackify = (original) => {
    if (typeof original !== 'function') {
      throw new TypeError('The "original" argument must be of type function');
    }
    return function (...args) {
      const callback = args.pop();
      if (typeof callback !== 'function') {
        throw new TypeError('The last argument must be of type function');
      }
      original.apply(this, args).then(
        (value) => queueMicrotask(() => callback(null, value)),
        (error) => queueMicrotask(() => callback(error)),
      );
    };
  };

  const deprecate = (fn, message, code) => {
    let warned = false;
    return function (...args) {
      if (!warned) {
        warned = true;
        console.error('DeprecationWarning: ' + String(message) + (code ? ' [' + code + ']' : ''));
      }
      return fn.apply(this, args);
    };
  };

  const debuglog = () => {
    const log = () => {};
    log.enabled = false;
    return log;
  };

  const stripVTControlCharacters = (text) => String(text).replace(/\u001b\[[0-9;?]*[ -\/]*[@-~]/g, '');

  const toUSVString = (value) => String(value);

  const isObject = (value) => value !== null && typeof value === 'object';
  const tag = (value) => Object.prototype.toString.call(value);

  const types = {
    isPromise: (value) => tag(value) === '[object Promise]',
    isDate: (value) => tag(value) === '[object Date]',
    isRegExp: (value) => tag(value) === '[object RegExp]',
    isMap: (value) => tag(value) === '[object Map]',
    isSet: (value) => tag(value) === '[object Set]',
    isWeakMap: (value) => tag(value) === '[object WeakMap]',
    isWeakSet: (value) => tag(value) === '[object WeakSet]',
    isArrayBuffer: (value) => tag(value) === '[object ArrayBuffer]',
    isSharedArrayBuffer: (value) => tag(value) === '[object SharedArrayBuffer]',
    isTypedArray: (value) => ArrayBuffer.isView(value) && !(value instanceof DataView),
    isUint8Array: (value) => tag(value) === '[object Uint8Array]',
    isUint8ClampedArray: (value) => tag(value) === '[object Uint8ClampedArray]',
    isAsyncFunction: (value) => tag(value) === '[object AsyncFunction]',
    isGeneratorFunction: (value) => tag(value) === '[object GeneratorFunction]',
    isNativeError: (value) => value instanceof Error,
    isProxy: () => false,
    isBoxedPrimitive: (value) =>
      value instanceof Number || value instanceof String || value instanceof Boolean || typeof value === 'bigint' || typeof value === 'symbol',
    isAnyArrayBuffer: (value) => {
      const name = tag(value);
      return name === '[object ArrayBuffer]' || name === '[object SharedArrayBuffer]';
    },
  };

  // Structural equality, the way Node's isDeepStrictEqual reads it: same
  // prototype, same own enumerable keys, NaN equal to NaN, -0 apart from +0,
  // cycles paired off rather than assumed equal.
  const deepEqual = (left, right, seen) => {
    if (Object.is(left, right)) return true;
    if (!isObject(left) || !isObject(right)) return false;
    if (Object.getPrototypeOf(left) !== Object.getPrototypeOf(right)) return false;

    for (const pair of seen) {
      if (pair[0] === left && pair[1] === right) return true;
    }
    seen.push([left, right]);

    const leftTag = tag(left);
    if (leftTag !== tag(right)) return false;

    if (leftTag === '[object Date]') return Object.is(left.getTime(), right.getTime());
    if (leftTag === '[object RegExp]') return String(left) === String(right);
    if (leftTag === '[object Number]' || leftTag === '[object String]' || leftTag === '[object Boolean]') {
      return Object.is(left.valueOf(), right.valueOf());
    }
    if (leftTag === '[object Error]' || leftTag === '[object NativeError]') {
      return left.name === right.name && left.message === right.message;
    }
    if (leftTag === '[object ArrayBuffer]' || leftTag === '[object SharedArrayBuffer]') {
      const a = new Uint8Array(left);
      const b = new Uint8Array(right);
      if (a.length !== b.length) return false;
      for (let i = 0; i < a.length; i += 1) if (a[i] !== b[i]) return false;
      return true;
    }
    if (ArrayBuffer.isView(left)) {
      if (!ArrayBuffer.isView(right) || left.length !== right.length) return false;
      for (let i = 0; i < left.length; i += 1) if (!Object.is(left[i], right[i])) return false;
      return true;
    }
    if (leftTag === '[object Map]') {
      if (left.size !== right.size) return false;
      for (const [key, value] of left) {
        if (!right.has(key) || !deepEqual(value, right.get(key), seen)) return false;
      }
      return true;
    }
    if (leftTag === '[object Set]') {
      if (left.size !== right.size) return false;
      for (const value of left) {
        if (!right.has(value)) return false;
      }
      return true;
    }

    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    if (leftKeys.length !== rightKeys.length) return false;
    for (const name of leftKeys) {
      if (!Object.prototype.propertyIsEnumerable.call(right, name)) return false;
      if (!deepEqual(left[name], right[name], seen)) return false;
    }
    return true;
  };

  const isDeepStrictEqual = (left, right) => deepEqual(left, right, []);

  // Node's parseArgs: the shape is what scripts read, so strict mode and the
  // type checks are kept and the rest is left out.
  const parseArgs = (config) => {
    const settings = config || {};
    const options = settings.options || {};
    const strict = settings.strict !== false;
    const allowPositionals = !!settings.allowPositionals || strict ? !!settings.allowPositionals : true;
    const args = settings.args || process.argv.slice(2);
    const values = {};
    const positionals = [];
    const unknown = [];
    for (let index = 0; index < args.length; index += 1) {
      const arg = args[index];
      if (arg === '--') {
        positionals.push(...args.slice(index + 1));
        break;
      }
      if (arg.startsWith('--')) {
        const body = arg.slice(2);
        const equals = body.indexOf('=');
        const name = equals >= 0 ? body.slice(0, equals) : body;
        const definition = options[name];
        if (!definition) {
          if (strict) throw new TypeError('Unknown option ' + arg);
          unknown.push(name);
          continue;
        }
        const type = definition.type || (definition.multiple ? 'string' : 'boolean');
        let value = true;
        if (equals >= 0) {
          value = body.slice(equals + 1);
        } else if (type !== 'boolean' && index + 1 < args.length) {
          index += 1;
          value = args[index];
        }
        if (type === 'boolean' && equals < 0) {
          value = true;
        }
        if (definition.multiple) {
          if (!values[name]) values[name] = [];
          values[name].push(value);
        } else {
          if (values[name] !== undefined && strict) throw new TypeError('Option ' + arg + ' cannot be given twice');
          values[name] = value;
        }
        continue;
      }
      if (arg.length > 1 && arg[0] === '-') {
        const body = arg.slice(1);
        const definition = options[body];
        if (!definition) {
          if (strict) throw new TypeError('Unknown option ' + arg);
          unknown.push(body);
          continue;
        }
        let value = true;
        if ((definition.type || 'boolean') !== 'boolean' && index + 1 < args.length) {
          index += 1;
          value = args[index];
        }
        values[body] = value;
        continue;
      }
      positionals.push(arg);
    }
    if (!allowPositionals && positionals.length > 0) {
      throw new TypeError('Unexpected argument ' + positionals[0]);
    }
    return { values, positionals };
  };

  const utils = {
    format,
    formatWithOptions,
    inspect,
    inherits,
    promisify,
    callbackify,
    deprecate,
    debuglog,
    debug: debuglog,
    types,
    styleText,
    stripVTControlCharacters,
    isDeepStrictEqual,
    isArray: Array.isArray,
    toUSVString,
    parseArgs,
    getCallSites: () => [],
  };

  globalThis.__util_module = utils;
  globalThis.util = utils;
})();

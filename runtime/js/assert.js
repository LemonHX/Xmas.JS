(function () {
  // LLRT's assert module (llrt_assert/src/lib.rs, 78 lines) covers assert.ok and
  // nothing else, so copying it would leave strictEqual and friends missing.
  // Its ok semantics are kept: only the falsy values fail, a string message
  // replaces the default one, an Error message is thrown as it is. Everything
  // below that is the rest of Node's surface.

  class AssertionError extends Error {
    constructor(options) {
      const settings = options || {};
      super(settings.message === undefined ? 'Failed' : String(settings.message));
      this.name = 'AssertionError';
      this.code = 'ERR_ASSERTION';
      this.actual = settings.actual;
      this.expected = settings.expected;
      this.operator = settings.operator;
      this.generatedMessage = settings.generatedMessage === true;
      if (settings.stackStartFn) Error.captureStackTrace?.(this, settings.stackStartFn);
    }
  }

  const show = (value) => util.inspect(value, { colors: false });

  const describe = (actual, expected, operator, message) => {
    if (message !== undefined && message !== null) return String(message);
    return show(actual) + ' ' + operator + ' ' + show(expected);
  };

  const fail = (actual, expected, message, operator, stackStart) => {
    throw new AssertionError({
      message: describe(actual, expected, operator, message),
      actual,
      expected,
      operator,
      generatedMessage: message === undefined || message === null,
      stackStartFn: stackStart,
    });
  };

  const ok = (value, message) => {
    if (value) return;
    throw new AssertionError({
      message: message === undefined || message === null ? 'The expression evaluated to a falsy value' : String(message),
      actual: value,
      expected: true,
      operator: '==',
      generatedMessage: message === undefined || message === null,
      stackStartFn: ok,
    });
  };

  const equal = (actual, expected, message) => {
    if (actual == expected) return;
    fail(actual, expected, message, '==', equal);
  };

  const notEqual = (actual, expected, message) => {
    if (actual != expected) return;
    fail(actual, expected, message, '!=', notEqual);
  };

  const strictEqual = (actual, expected, message) => {
    if (Object.is(actual, expected)) return;
    fail(actual, expected, message, 'strictEqual', strictEqual);
  };

  const notStrictEqual = (actual, expected, message) => {
    if (!Object.is(actual, expected)) return;
    fail(actual, expected, message, 'notStrictEqual', notStrictEqual);
  };

  // Loose deep equality: same structure, prototypes ignored, primitives
  // compared the way == compares them.
  const looseDeep = (left, right, seen) => {
    if (left == right) return true;
    if (Object.is(left, right)) return true;
    if (left === null || right === null) return false;
    if (typeof left !== 'object' || typeof right !== 'object') return false;
    for (const pair of seen) {
      if (pair[0] === left && pair[1] === right) return true;
    }
    seen.push([left, right]);
    if (ArrayBuffer.isView(left) || ArrayBuffer.isView(right)) {
      if (!ArrayBuffer.isView(left) || !ArrayBuffer.isView(right)) return false;
      if (left.length !== right.length) return false;
      for (let index = 0; index < left.length; index += 1) {
        if (left[index] != right[index]) return false;
      }
      return true;
    }
    const leftKeys = Object.keys(left);
    const rightKeys = Object.keys(right);
    if (leftKeys.length !== rightKeys.length) return false;
    for (const name of leftKeys) {
      if (!Object.prototype.propertyIsEnumerable.call(right, name)) return false;
      if (!looseDeep(left[name], right[name], seen)) return false;
    }
    return true;
  };

  const deepEqual = (actual, expected, message) => {
    if (looseDeep(actual, expected, [])) return;
    fail(actual, expected, message, 'deepEqual', deepEqual);
  };

  const notDeepEqual = (actual, expected, message) => {
    if (!looseDeep(actual, expected, [])) return;
    fail(actual, expected, message, 'notDeepEqual', notDeepEqual);
  };

  const deepStrictEqual = (actual, expected, message) => {
    if (util.isDeepStrictEqual(actual, expected)) return;
    fail(actual, expected, message, 'deepStrictEqual', deepStrictEqual);
  };

  const notDeepStrictEqual = (actual, expected, message) => {
    if (!util.isDeepStrictEqual(actual, expected)) return;
    fail(actual, expected, message, 'notDeepStrictEqual', notDeepStrictEqual);
  };

  const messageOf = (error) => (error && error.message !== undefined ? String(error.message) : String(error));

  const checkThrown = (thrown, expected, name) => {
    if (thrown === undefined) {
      throw new AssertionError({
        message: 'Missing expected exception' + (expected ? ': ' + (expected.name || show(expected)) : ''),
        operator: name,
        stackStartFn: checkThrown,
      });
    }
    if (expected === undefined || expected === null) return;
    if (expected instanceof RegExp) {
      if (expected.test(messageOf(thrown))) return;
      throw new AssertionError({ message: 'The error message ' + show(messageOf(thrown)) + ' does not match ' + show(String(expected)), operator: name });
    }
    if (typeof expected === 'function') {
      if (thrown instanceof expected) return;
      if (expected.prototype !== undefined && expected.prototype instanceof Error && thrown instanceof expected) return;
      // Node also accepts a plain validation function
      if (expected(thrown) !== false && !(expected.prototype && expected.prototype instanceof Error)) return;
      throw new AssertionError({ message: 'The error is not an instance of ' + (expected.name || 'the given function'), operator: name });
    }
    if (typeof expected === 'object') {
      for (const key of Object.keys(expected)) {
        if (thrown[key] !== expected[key]) {
          throw new AssertionError({
            message: 'The error property ' + key + ' is ' + show(thrown[key]) + ', expected ' + show(expected[key]),
            operator: name,
          });
        }
      }
      return;
    }
    throw new AssertionError({ message: 'The "expected" argument must be a function, RegExp or object', operator: name });
  };

  const throws = (fn, expected, message) => {
    if (typeof fn !== 'function') {
      throw new TypeError('The "fn" argument must be of type function');
    }
    let thrown;
    try {
      fn();
    } catch (error) {
      thrown = error;
    }
    if (thrown !== undefined && message !== undefined && message !== null && expected === undefined) {
      // assert.throws(fn, message) is not a Node shape; message is second only
      // when expected is missing, which Node rejects.
      throw new TypeError('The "error" argument must be a function, RegExp or object');
    }
    checkThrown(thrown, expected, 'throws');
  };

  const doesNotThrow = (fn, expected, message) => {
    if (typeof fn !== 'function') {
      throw new TypeError('The "fn" argument must be of type function');
    }
    try {
      fn();
    } catch (error) {
      throw new AssertionError({
        message: message === undefined || message === null ? 'Got unwanted exception: ' + messageOf(error) : String(message),
        actual: error,
        operator: 'doesNotThrow',
        stackStartFn: doesNotThrow,
      });
    }
  };

  // Node takes either a promise or a function returning one.
  const pending = (fn) => (typeof fn === 'function' ? fn() : fn);

  const rejects = async (fn, expected, message) => {
    if (typeof fn !== 'function' && !util.types.isPromise(fn)) {
      throw new TypeError('The "fn" argument must be of type function or a promise');
    }
    let thrown;
    let rejected = false;
    try {
      await pending(fn);
    } catch (error) {
      rejected = true;
      thrown = error;
    }
    if (!rejected) {
      throw new AssertionError({
        message: message === undefined || message === null ? 'Missing expected rejection' : String(message),
        operator: 'rejects',
        stackStartFn: rejects,
      });
    }
    checkThrown(thrown, expected, 'rejects');
  };

  const doesNotReject = async (fn, expected, message) => {
    if (typeof fn !== 'function' && !util.types.isPromise(fn)) {
      throw new TypeError('The "fn" argument must be of type function or a promise');
    }
    try {
      await pending(fn);
    } catch (error) {
      throw new AssertionError({
        message: message === undefined || message === null ? 'Got unwanted rejection: ' + messageOf(error) : String(message),
        actual: error,
        operator: 'doesNotReject',
        stackStartFn: doesNotReject,
      });
    }
  };

  const match = (value, pattern, message) => {
    const text = String(value);
    if (pattern instanceof RegExp ? pattern.test(text) : text.includes(String(pattern))) return;
    fail(value, pattern, message, 'match', match);
  };

  const doesNotMatch = (value, pattern, message) => {
    const text = String(value);
    if (!(pattern instanceof RegExp ? pattern.test(text) : text.includes(String(pattern)))) return;
    fail(value, pattern, message, 'doesNotMatch', doesNotMatch);
  };

  const ifError = (value) => {
    if (!value) return;
    if (value instanceof Error) throw value;
    throw new AssertionError({
      message: 'ifError got unwanted exception: ' + (typeof value === 'object' ? show(value) : String(value)),
      actual: value,
      expected: null,
      operator: 'ifError',
      stackStartFn: ifError,
    });
  };

  const callable = ok;
  Object.defineProperties(callable, {
    ok: { value: ok },
    fail: { value: (message) => fail(undefined, undefined, message, 'fail', fail) },
    equal: { value: equal },
    notEqual: { value: notEqual },
    strictEqual: { value: strictEqual },
    notStrictEqual: { value: notStrictEqual },
    deepEqual: { value: deepEqual },
    notDeepEqual: { value: notDeepEqual },
    deepStrictEqual: { value: deepStrictEqual },
    notDeepStrictEqual: { value: notDeepStrictEqual },
    throws: { value: throws },
    doesNotThrow: { value: doesNotThrow },
    rejects: { value: rejects },
    doesNotReject: { value: doesNotReject },
    match: { value: match },
    doesNotMatch: { value: doesNotMatch },
    ifError: { value: ifError },
    AssertionError: { value: AssertionError },
  });
  callable.strict = callable;
  callable.assert = callable;

  globalThis.__assert_module = callable;
  globalThis.assert = callable;
})();

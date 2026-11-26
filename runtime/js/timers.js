(function () {
  globalThis.__timer_invoke = (callback, args) => callback(...args);

  globalThis.setTimeout = (callback, delay = 0, ...args) =>
    __timer_set(callback, Number(delay) || 0, args, false);
  globalThis.setInterval = (callback, delay = 0, ...args) =>
    __timer_set(callback, Number(delay) || 0, args, true);
  globalThis.setImmediate = (callback, ...args) => __timer_set(callback, -1, args, false);
  globalThis.clearTimeout = (handle) => __timer_clear(handle);
  globalThis.clearInterval = (handle) => __timer_clear(handle);
  globalThis.queueMicrotask = (callback) => Promise.resolve().then(callback);

  globalThis.__timers_module = {
    setTimeout: globalThis.setTimeout,
    setInterval: globalThis.setInterval,
    setImmediate: globalThis.setImmediate,
    clearTimeout: globalThis.clearTimeout,
    clearInterval: globalThis.clearInterval,
    queueMicrotask: globalThis.queueMicrotask,
  };
  const abortError = () => Object.assign(new Error('The operation was aborted'), { name: 'AbortError', code: 'ABORT_ERR' });
  const delayed = (start, value, options) => new Promise((resolve, reject) => {
    const signal = options && options.signal;
    if (signal && signal.aborted) return reject(abortError());
    const onAbort = () => { globalThis.clearTimeout(handle); reject(abortError()); };
    const handle = start(() => { if (signal) signal.removeEventListener('abort', onAbort); resolve(value); });
    if (signal) signal.addEventListener('abort', onAbort, { once: true });
  });
  const promisesSetTimeout = (delay = 0, value, options) => delayed((fn) => globalThis.setTimeout(fn, delay), value, options);
  const promisesSetImmediate = (value, options) => delayed((fn) => globalThis.setImmediate(fn), value, options);
  async function* promisesSetInterval(delay = 0, value, options) {
    const signal = options && options.signal;
    while (true) {
      if (signal && signal.aborted) throw abortError();
      await promisesSetTimeout(delay, undefined, options);
      yield value;
    }
  }
  globalThis.__timers_promises = {
    setTimeout: promisesSetTimeout,
    setImmediate: promisesSetImmediate,
    setInterval: promisesSetInterval,
    scheduler: {
      wait: (delay, options) => promisesSetTimeout(delay, undefined, options),
      yield: () => promisesSetImmediate(),
    },
  };
})();

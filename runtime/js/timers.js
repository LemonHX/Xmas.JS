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
  globalThis.__timers_promises = {
    setTimeout: (delay = 0, value) =>
      new Promise((resolve) => globalThis.setTimeout(() => resolve(value), delay)),
    setImmediate: (value) =>
      new Promise((resolve) => globalThis.setImmediate(() => resolve(value))),
  };
})();

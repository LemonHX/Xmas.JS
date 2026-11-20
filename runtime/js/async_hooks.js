(function () {
  // Hooks are accepted and never called: the engine exposes no promise hook, so
  // there is nothing to observe. See src/modules/async_hooks.rs.
  const createHook = (callbacks) => {
    const hooks = callbacks || {};
    let enabled = false;
    return {
      enable() {
        enabled = true;
      },
      disable() {
        enabled = false;
      },
      get enabled() {
        return enabled;
      },
      // Kept so code that pokes at them does not break.
      init: hooks.init,
      before: hooks.before,
      after: hooks.after,
      destroy: hooks.destroy,
      promiseResolve: hooks.promiseResolve,
    };
  };

  const executionAsyncId = () => 0;
  const triggerAsyncId = () => 0;
  const currentId = () => 0;

  // A shell with the right shape. Capturing the async context is exactly what
  // the missing promise hook would provide, so runInAsyncScope cannot restore a
  // store it never saw.
  class AsyncResource {
    constructor(type, options) {
      this.type = String(type);
      this._options = options || {};
    }
    runInAsyncScope(fn, thisArg, ...args) {
      return fn.apply(thisArg, args);
    }
    emitDestroy() {
      return this;
    }
    asyncId() {
      return 0;
    }
    triggerAsyncId() {
      return 0;
    }
    bind(fn, thisArg) {
      return (...args) => fn.apply(thisArg, args);
    }
    static bind(fn, type, thisArg) {
      return fn.bind(thisArg);
    }
  }

  // One store slot per realm. run/enterWith/exit move it; bind captures it.
  class AsyncLocalStorage {
    constructor() {
      Object.defineProperty(this, '_stack', { value: [], enumerable: false });
      this._disabled = false;
    }
    // Depth is enough to restore: run/enterWith are LIFO, so the stack only
    // ever grows at the end. No copy per call.
    run(store, callback, ...args) {
      const depth = this._stack.length;
      this._stack.push(store);
      try {
        return callback(...args);
      } finally {
        this._stack.length = depth;
      }
    }
    enterWith(store) {
      this._stack.push(store);
    }
    exit(callback, ...args) {
      const saved = this._stack.splice(0, this._stack.length);
      try {
        return callback(...args);
      } finally {
        for (const value of saved) this._stack.push(value);
      }
    }
    getStore() {
      return this._disabled || this._stack.length === 0
        ? undefined
        : this._stack[this._stack.length - 1];
    }
    disable() {
      this._disabled = true;
      this._stack.length = 0;
      return this;
    }
    bind(fn) {
      const store = this.getStore();
      return (...args) => this.run(store, () => fn(...args));
    }
    snapshot() {
      const store = this.getStore();
      return () => this.run(store, () => {});
    }
  }

  globalThis.__async_hooks_module = {
    createHook,
    executionAsyncId,
    triggerAsyncId,
    currentId,
    AsyncLocalStorage,
    AsyncResource,
  };
})();

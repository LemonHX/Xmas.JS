(function () {
  if (!globalThis.EventTarget) throw new Error('abort must be installed after events');

  const abortError = (message) => {
    const error = new Error(message);
    error.name = 'AbortError';
    return error;
  };

  class AbortSignal extends EventTarget {
    constructor() {
      super();
      this._aborted = false;
      this._reason = undefined;
      this.onabort = null;
    }
    get aborted() { return this._aborted; }
    get reason() { return this._reason; }
    throwIfAborted() { if (this._aborted) throw this._reason; }
    static abort(reason) {
      const signal = new AbortSignal();
      signal._abort(reason === undefined ? abortError('signal is aborted') : reason);
      return signal;
    }
    static timeout(milliseconds) {
      const signal = new AbortSignal();
      const delay = Number(milliseconds) || 0;
      if (delay > 0) {
        setTimeout(() => signal._abort(abortError('signal timed out')), delay);
      } else {
        signal._abort(abortError('signal timed out'));
      }
      return signal;
    }
    static any(signals) {
      const signal = new AbortSignal();
      for (const candidate of signals) {
        if (candidate.aborted) { signal._abort(candidate.reason); break; }
        candidate.addEventListener('abort', () => signal._abort(candidate.reason));
      }
      return signal;
    }
    _abort(reason) {
      if (this._aborted) return;
      this._aborted = true;
      this._reason = reason;
      this.dispatchEvent(new Event('abort'));
      if (typeof this.onabort === 'function') this.onabort.call(this, new Event('abort'));
    }
  }

  class AbortController {
    constructor() {
      this.signal = new AbortSignal();
    }
    abort(reason) {
      this.signal._abort(reason === undefined ? abortError('signal is aborted') : reason);
    }
  }

  globalThis.AbortSignal = AbortSignal;
  globalThis.AbortController = AbortController;

  globalThis.__abort_module = { AbortController, AbortSignal };
})();

(function () {
  class EventEmitter {
    constructor() {
      Object.defineProperty(this, '_events', { value: new Map(), enumerable: false });
    }
    addListener(name, listener) {
      const list = this._events.get(name);
      if (list) list.push(listener); else this._events.set(name, [listener]);
      return this;
    }
    on(name, listener) { return this.addListener(name, listener); }
    prependListener(name, listener) {
      const list = this._events.get(name);
      if (list) list.unshift(listener); else this._events.set(name, [listener]);
      return this;
    }
    once(name, listener) {
      // A once wrapper removes itself while the list is being walked, so emits
      // for this name have to run over a snapshot; every other name does not.
      if (!this._snapshot) Object.defineProperty(this, '_snapshot', { value: new Set(), enumerable: false });
      this._snapshot.add(name);
      const wrapper = (...args) => {
        this.removeListener(name, wrapper);
        listener.apply(this, args);
      };
      wrapper.listener = listener;
      return this.addListener(name, wrapper);
    }
    removeListener(name, listener) {
      const list = this._events.get(name);
      if (!list) return this;
      const at = list.findIndex((item) => item === listener || item.listener === listener);
      if (at >= 0) list.splice(at, 1);
      return this;
    }
    off(name, listener) { return this.removeListener(name, listener); }
    removeAllListeners(name) {
      if (name === undefined) this._events.clear(); else this._events.delete(name);
      return this;
    }
    emit(name, ...args) {
      const list = this._events.get(name);
      if (!list || list.length === 0) {
        if (name === 'error') {
          const error = args[0];
          throw error instanceof Error ? error : new Error('Unhandled error');
        }
        return false;
      }
      const snapshot = this._snapshot && this._snapshot.has(name) ? list.slice() : list;
      for (let index = 0; index < snapshot.length; index += 1) snapshot[index].apply(this, args);
      return true;
    }
    listeners(name) {
      return (this._events.get(name) || []).map((item) => item.listener || item);
    }
    listenerCount(name) { return (this._events.get(name) || []).length; }
    eventNames() { return [...this._events.keys()]; }
  }

  const listeners = Symbol('listeners');

  class Event {
    constructor(type, options) {
      const settings = options || {};
      this.type = String(type);
      this.bubbles = !!settings.bubbles;
      this.cancelable = !!settings.cancelable;
      this.composed = !!settings.composed;
      this.target = null;
      this.currentTarget = null;
      this.defaultPrevented = false;
      this.eventPhase = 0;
      this.isTrusted = false;
      this.timeStamp = __now_us() / 1000;
      this._stop = false;
      this._stopImmediate = false;
      this._dispatching = false;
    }
    preventDefault() { if (this.cancelable) this.defaultPrevented = true; }
    stopPropagation() { this._stop = true; }
    stopImmediatePropagation() { this._stop = true; this._stopImmediate = true; }
    composedPath() { return this.currentTarget ? [this.currentTarget] : []; }
  }

  class CustomEvent extends Event {
    constructor(type, options) {
      super(type, options);
      this.detail = options && 'detail' in options ? options.detail : null;
    }
  }

  class EventTarget {
    constructor() {
      Object.defineProperty(this, listeners, { value: new Map(), enumerable: false });
    }
    addEventListener(type, callback, options) {
      if (typeof callback !== 'function') return;
      const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
      const once = !!(options && typeof options === 'object' && options.once);
      const key = String(type);
      const list = this[listeners].get(key) || [];
      if (list.some((item) => item.callback === callback && item.capture === capture)) return;
      list.push({ callback, capture, once });
      this[listeners].set(key, list);
    }
    removeEventListener(type, callback, options) {
      const capture = typeof options === 'boolean' ? options : !!(options && options.capture);
      const list = this[listeners].get(String(type));
      if (!list) return;
      const at = list.findIndex((item) => item.callback === callback && item.capture === capture);
      if (at >= 0) list.splice(at, 1);
    }
    dispatchEvent(event) {
      if (!(event instanceof Event)) throw new TypeError('dispatchEvent requires Event');
      event.target = this;
      event.currentTarget = this;
      event.eventPhase = 2;
      event._dispatching = true;
      const list = (this[listeners].get(event.type) || []).slice();
      for (const item of list) {
        if (event._stopImmediate) break;
        if (item.once) this.removeEventListener(event.type, item.callback, item.capture);
        try {
          item.callback.call(this, event);
        } catch (error) {
          // Errors thrown by listeners must not propagate through dispatchEvent to the caller (browsers/Node don't do this either)
          if (globalThis.console) console.error('Listener threw:', error && error.message ? error.message : error);
        }
      }
      event._dispatching = false;
      event.currentTarget = null;
      event.eventPhase = 0;
      return !event.defaultPrevented;
    }
  }

  globalThis.EventEmitter = EventEmitter;
  globalThis.Event = Event;
  globalThis.CustomEvent = CustomEvent;
  globalThis.EventTarget = EventTarget;

  // Static helpers of node:events.
  const once = (emitter, name, options) => new Promise((resolve, reject) => {
    const signal = options && options.signal;
    if (signal && signal.aborted) return reject(Object.assign(new Error('The operation was aborted'), { name: 'AbortError', code: 'ABORT_ERR' }));
    const isTarget = typeof emitter.addEventListener === 'function' && typeof emitter.on !== 'function';
    const cleanup = () => {
      if (isTarget) { emitter.removeEventListener(name, onEvent); } else { emitter.removeListener(name, onEvent); if (name !== 'error') emitter.removeListener('error', onError); }
      if (signal) signal.removeEventListener('abort', onAbort);
    };
    const onEvent = (...args) => { cleanup(); resolve(args); };
    const onError = (error) => { cleanup(); reject(error); };
    const onAbort = () => { cleanup(); reject(Object.assign(new Error('The operation was aborted'), { name: 'AbortError', code: 'ABORT_ERR' })); };
    if (isTarget) emitter.addEventListener(name, onEvent, { once: true });
    else { emitter.on(name, onEvent); if (name !== 'error') emitter.on('error', onError); }
    if (signal) signal.addEventListener('abort', onAbort, { once: true });
  });
  const on = (emitter, name) => {
    const queue = [];
    const waiting = [];
    let failure = null;
    let finished = false;
    const handler = (...args) => { const w = waiting.shift(); if (w) w.resolve({ value: args, done: false }); else queue.push(args); };
    const errorHandler = (error) => { failure = error; const w = waiting.shift(); if (w) w.reject(error); };
    emitter.on(name, handler);
    if (name !== 'error') emitter.on('error', errorHandler);
    const stop = () => { finished = true; emitter.removeListener(name, handler); emitter.removeListener('error', errorHandler); for (const w of waiting.splice(0)) w.resolve({ value: undefined, done: true }); };
    return {
      next() {
        if (queue.length) return Promise.resolve({ value: queue.shift(), done: false });
        if (failure) { const e = failure; failure = null; return Promise.reject(e); }
        if (finished) return Promise.resolve({ value: undefined, done: true });
        return new Promise((resolve, reject) => waiting.push({ resolve, reject }));
      },
      return() { stop(); return Promise.resolve({ value: undefined, done: true }); },
      throw(error) { stop(); return Promise.reject(error); },
      [Symbol.asyncIterator]() { return this; },
    };
  };
  const listenerCount = (emitter, name) => emitter.listenerCount(name);
  const getEventListeners = (emitter, name) => (typeof emitter.listeners === 'function' ? emitter.listeners(name) : []);
  const setMaxListeners = (count, ...targets) => { if (targets.length === 0) EventEmitter.defaultMaxListeners = count; for (const t of targets) if (typeof t.setMaxListeners === 'function') t.setMaxListeners(count); };
  const addAbortListener = (signal, listener) => {
    if (signal.aborted) { queueMicrotask(() => listener({ target: signal, type: 'abort' })); return { [Symbol.dispose]() {} }; }
    signal.addEventListener('abort', listener, { once: true });
    return { [Symbol.dispose]() { signal.removeEventListener('abort', listener); } };
  };
  const errorMonitor = Symbol('events.errorMonitor');
  if (EventEmitter.defaultMaxListeners === undefined) EventEmitter.defaultMaxListeners = 10;
  Object.assign(EventEmitter, { once, on, listenerCount, getEventListeners, setMaxListeners, errorMonitor, addAbortListener, EventEmitter });

  globalThis.__events_module = {
    default: EventEmitter,
    once, on, listenerCount, getEventListeners, setMaxListeners, errorMonitor, addAbortListener,
    EventEmitter,
    Event,
    CustomEvent,
    EventTarget,
  };
})();

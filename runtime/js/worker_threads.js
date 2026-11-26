(function () {
  // node:worker_threads: this runtime has one thread per realm, so the main thread is all there is.
  class Worker {
    constructor() { throw new Error('worker_threads.Worker is not supported by this runtime'); }
  }
  class MessagePort {
    constructor() { this._other = null; this._handlers = []; }
    postMessage(value) { const other = this._other; if (other) queueMicrotask(() => other._handlers.forEach((fn) => fn(value))); }
    on(name, fn) { if (name === 'message') this._handlers.push(fn); return this; }
    once(name, fn) { return this.on(name, fn); }
    off(name, fn) { this._handlers = this._handlers.filter((h) => h !== fn); return this; }
    close() { this._handlers = []; }
    unref() { return this; }
    ref() { return this; }
  }
  class MessageChannel {
    constructor() { this.port1 = new MessagePort(); this.port2 = new MessagePort(); this.port1._other = this.port2; this.port2._other = this.port1; }
  }
  globalThis.__worker_threads_module = {
    isMainThread: true, parentPort: null, workerData: null, threadId: 0,
    Worker, MessageChannel, MessagePort,
    receiveMessageOnPort: () => undefined,
  };
})();

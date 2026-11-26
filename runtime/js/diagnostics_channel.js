(function () {
  // node:diagnostics_channel. Channels keep their subscribers and publish to them, which is all
  // that libraries (lru-cache and others) use it for; tracing channels run the function and
  // publish the start/end/error events around it.
  const channels = new Map();
  class Channel {
    constructor(name) { this.name = name; this._subs = []; }
    get hasSubscribers() { return this._subs.length > 0; }
    subscribe(fn) { this._subs.push(fn); }
    unsubscribe(fn) {
      const i = this._subs.indexOf(fn);
      if (i < 0) return false;
      this._subs.splice(i, 1);
      return true;
    }
    publish(message) { for (const fn of this._subs.slice()) fn(message, this.name); }
    bindStore() {}
    unbindStore() { return false; }
    runStores(data, fn, thisArg, ...args) { return fn.apply(thisArg, args); }
  }
  const channel = (name) => {
    if (typeof name !== 'string' && typeof name !== 'symbol') throw new TypeError('The "channel" argument must be of type string or symbol');
    let c = channels.get(name);
    if (!c) { c = new Channel(name); channels.set(name, c); }
    return c;
  };
  const hasSubscribers = (name) => channels.has(name) && channels.get(name).hasSubscribers;
  const subscribe = (name, fn) => channel(name).subscribe(fn);
  const unsubscribe = (name, fn) => channel(name).unsubscribe(fn);
  const EVENTS = ['start', 'end', 'asyncStart', 'asyncEnd', 'error'];
  class TracingChannel {
    constructor(name) {
      if (typeof name === 'string') for (const e of EVENTS) this[e] = channel('tracing:' + name + ':' + e);
      else for (const e of EVENTS) this[e] = name[e];
    }
    get hasSubscribers() { return EVENTS.some((e) => this[e].hasSubscribers); }
    subscribe(handlers) { for (const e of EVENTS) if (handlers[e]) this[e].subscribe(handlers[e]); }
    unsubscribe(handlers) { let ok = true; for (const e of EVENTS) if (handlers[e]) ok = this[e].unsubscribe(handlers[e]) && ok; return ok; }
    traceSync(fn, context = {}, thisArg, ...args) {
      this.start.publish(context);
      try { const result = fn.apply(thisArg, args); context.result = result; return result; }
      catch (error) { context.error = error; this.error.publish(context); throw error; }
      finally { this.end.publish(context); }
    }
    tracePromise(fn, context = {}, thisArg, ...args) {
      this.start.publish(context);
      let p;
      try { p = Promise.resolve(fn.apply(thisArg, args)); }
      catch (error) { context.error = error; this.error.publish(context); this.end.publish(context); throw error; }
      this.end.publish(context);
      return p.then((result) => { context.result = result; this.asyncStart.publish(context); this.asyncEnd.publish(context); return result; },
        (error) => { context.error = error; this.error.publish(context); this.asyncStart.publish(context); this.asyncEnd.publish(context); throw error; });
    }
    traceCallback(fn, position = -1, context = {}, thisArg, ...args) {
      return this.traceSync(fn, context, thisArg, ...args);
    }
  }
  const tracingChannel = (name) => new TracingChannel(name);
  globalThis.__diagnostics_channel_module = { channel, hasSubscribers, subscribe, unsubscribe, tracingChannel, Channel, TracingChannel };
})();

(function () {
  const EE = globalThis.EventEmitter;
  const defer = (fn) => setTimeout(fn, 0);

  // Normalize what the caller handed us. Byte streams keep strings as strings so
  // the native side can encode them straight into its buffer (no Buffer is built
  // only to be copied); anything with a non-UTF-8 encoding becomes bytes first.
  const asChunk = (owner, chunk, encoding) => {
    if (chunk === undefined || chunk === null) return chunk;
    if (owner.objectMode) return chunk;
    if (typeof chunk === 'string') {
      if (encoding && encoding !== 'utf8' && encoding !== 'utf-8') return Buffer.from(chunk, encoding);
      return chunk;
    }
    if (chunk instanceof ArrayBuffer || chunk instanceof Uint8Array) return chunk;
    return Buffer.from(String(chunk), encoding || 'utf8');
  };

  // Chunks come back as ArrayBuffers; Buffer.from over one is a view, not a copy.
  const wrap = (value) => (value instanceof ArrayBuffer ? Buffer.from(value) : value);
  const asBytes = (value) => (typeof value === 'string' ? Buffer.from(value) : wrap(value));

  // Byte length of one chunk, as the queue counts it: bytes for a byte stream,
  // one per value for an object stream.
  const sizeOf = (owner, chunk) => {
    if (chunk === undefined || chunk === null) return 0;
    if (owner.objectMode) return 1;
    if (typeof chunk === 'string') return chunk.length;
    return chunk.byteLength !== undefined ? chunk.byteLength : (chunk.length || 0);
  };

  // The native queue is authoritative, but every length query used to be a host
  // call, and the flowing path asks once per chunk. The counter is mirrored here
  // from the values the native calls return, so the hot path stays in JS.
  const lengthOf = (stream) => stream._buffered;

  class Stream extends EE {
    pipe(destination, options) {
      return destination;
    }
  }

  class Readable extends Stream {
    constructor(options) {
      super();
      const settings = options || {};
      this.objectMode = !!settings.objectMode || !!settings.readableObjectMode;
      this.highWaterMark = settings.highWaterMark || 16384;
      this._readFn = typeof settings.read === 'function' ? settings.read : null;
      this._stream = __stream_open(this.objectMode);
      this._buffered = 0;
      this._ended = false;
      this._endEmitted = false;
      this._destroyed = false;
      this._flowing = false;
      this._reading = false;
      this._flushing = false;
      this._error = null;
      this._waiters = [];
      this.readable = true;
      this.readableEnded = false;
      this.readableFlowing = null;
    }

    // Do not override on: EventEmitter's on forwards to this.addListener, so
    // overriding both makes them call each other forever.
    addListener(name, listener) {
      super.addListener(name, listener);
      if (name === 'data') this.resume();
      return this;
    }

    push(chunk) {
      if (this._destroyed) return false;
      if (chunk === null) {
        this._ended = true;
        __stream_end(this._stream);
        this._flush();
        return false;
      }
      const data = asChunk(this, chunk);
      // Flowing with an empty queue and nobody waiting: hand the chunk straight
      // to the data listeners instead of queueing it in the native buffer.
      if (this._flowing && this._waiters.length === 0 && lengthOf(this) === 0) {
        this.emit('data', asBytes(data));
        return true;
      }
      const length = __stream_push(this._stream, data);
      if (length < 0) return false;
      this._buffered = length;
      if (this.listenerCount('readable')) this.emit('readable');
      this._flush();
      return length < this.highWaterMark;
    }

    unshift(chunk) {
      if (chunk === null || this._destroyed) return;
      const length = __stream_unshift(this._stream, asChunk(this, chunk));
      if (length >= 0) this._buffered = length;
      this._flush();
    }

    readableLength() {
      return this._destroyed ? 0 : this._buffered;
    }

    read(size) {
      if (this._destroyed) return null;
      const wanted = this.objectMode || size === undefined || size === null ? -1 : size;
      let chunk = this._buffered > 0 ? __stream_read(this._stream, wanted) : null;
      if (chunk === null) {
        this._pull();
        chunk = this._buffered > 0 ? __stream_read(this._stream, wanted) : null;
      }
      if (chunk === null) return null;
      const value = wrap(chunk);
      this._buffered -= sizeOf(this, value);
      this._flush();
      return value;
    }

    resume() {
      this._flowing = true;
      this.readableFlowing = true;
      this._flush();
      return this;
    }

    pause() {
      this._flowing = false;
      this.readableFlowing = false;
      return this;
    }

    isPaused() {
      return !this._flowing;
    }

    pipe(destination, options) {
      const settings = options || {};
      const onData = (chunk) => {
        if (destination.write(chunk) === false) this.pause();
      };
      const onDrain = () => this.resume();
      const onEnd = () => {
        if (settings.end !== false) destination.end();
      };
      const onError = (error) => {
        cleanup();
        if (destination.destroy) destination.destroy(error);
        else destination.emit('error', error);
      };
      const cleanup = () => {
        this.removeListener('data', onData);
        this.removeListener('end', onEnd);
        this.removeListener('error', onError);
        destination.removeListener('drain', onDrain);
      };
      this.on('data', onData);
      this.once('end', onEnd);
      this.once('error', onError);
      if (destination.on) destination.on('drain', onDrain);
      return destination;
    }

    destroy(error) {
      if (this._destroyed) return this;
      this._destroyed = true;
      this.readable = false;
      this._ended = true;
      if (error) {
        this._error = error;
        const waiters = this._waiters.splice(0);
        for (const waiter of waiters) waiter.reject(error);
        this.emit('error', error);
      }
      this.emit('close');
      __stream_close(this._stream);
      return this;
    }

    _pull() {
      if (!this._readFn || this._reading || this._ended || this._destroyed) return;
      this._reading = true;
      try {
        this._readFn(this.highWaterMark);
      } catch (error) {
        this._reading = false;
        this.destroy(error);
        return;
      }
      this._reading = false;
    }

    _flush() {
      if (this._flushing || this._destroyed) return;
      this._flushing = true;
      for (;;) {
        let moved = false;
        if (this._error) break;
        while (this._flowing && this._waiters.length === 0 && this._buffered > 0) {
          const chunk = wrap(__stream_read(this._stream, -1));
          this._buffered -= sizeOf(this, chunk);
          moved = true;
          this.emit('data', chunk);
          if (this._destroyed || this._error) break;
        }
        while (this._waiters.length && (this._buffered > 0 || this._ended)) {
          const waiter = this._waiters.shift();
          moved = true;
          if (this._buffered > 0) {
            const chunk = wrap(__stream_read(this._stream, -1));
            this._buffered -= sizeOf(this, chunk);
            waiter.resolve(chunk);
          } else {
            waiter.resolve(null);
          }
        }
        if (this._ended && this._buffered <= 0 && !this._endEmitted) {
          this._endEmitted = true;
          this.readable = false;
          this.readableEnded = true;
          moved = true;
          this.emit('end');
          this.emit('close');
        }
        if (!this._ended && this._buffered <= 0 && !this._reading) {
          const before = this._buffered;
          this._pull();
          if (this._buffered !== before) moved = true;
        }
        if (!moved) break;
      }
      this._flushing = false;
    }

    _nextChunk() {
      if (this._error) return Promise.reject(this._error);
      if (this._buffered > 0) {
        const chunk = wrap(__stream_read(this._stream, -1));
        this._buffered -= sizeOf(this, chunk);
        return Promise.resolve(chunk);
      }
      if (this._ended || this._destroyed) return Promise.resolve(null);
      return new Promise((resolve, reject) => {
        this._waiters.push({ resolve, reject });
        this._flush();
      });
    }

    [Symbol.asyncIterator]() {
      const stream = this;
      return {
        next() {
          return stream._nextChunk().then((chunk) => (
            chunk === null ? { value: undefined, done: true } : { value: chunk, done: false }
          ));
        },
        return() {
          stream.destroy();
          return Promise.resolve({ value: undefined, done: true });
        },
        [Symbol.asyncIterator]() { return this; },
      };
    }

    toArray() {
      return (async () => {
        const parts = [];
        for await (const chunk of this) parts.push(chunk);
        return parts;
      })();
    }

    static from(iterable, options) {
      const stream = new Readable(Object.assign({ objectMode: true }, options || {}));
      (async () => {
        try {
          for await (const item of iterable) {
            if (stream._destroyed) return;
            stream.push(item);
          }
          stream.push(null);
        } catch (error) {
          stream.destroy(error);
        }
      })();
      return stream;
    }
  }

  const initWritable = (target, options) => {
    const settings = options || {};
    target.objectMode = target.objectMode || !!settings.objectMode || !!settings.writableObjectMode;
    target.highWaterMark = settings.highWaterMark || target.highWaterMark || 16384;
    target._writeFn = typeof settings.write === 'function' ? settings.write : null;
    target._finalFn = typeof settings.final === 'function' ? settings.final : null;
    target._callbacks = [];
    target._pending = 0;
    target._needDrain = false;
    target._writing = false;
    target._draining = false;
    target._ending = false;
    target._finishing = false;
    target.writable = true;
    target.writableEnded = false;
    target.writableFinished = false;
  };

  const toWriteChunk = (data) => (typeof data === 'string' ? Buffer.from(data) : wrap(data));

  // Shared tail of both write paths: settle one write, then look at what is left.
  const settledWrite = (target, callback, error) => {
    target._writing = false;
    if (error) {
      target.writable = false;
      if (callback) callback(error);
      target.emit('error', error);
      return;
    }
    try {
      if (callback) callback(null);
    } catch (caught) {
      target.emit('error', caught);
    }
    // Node only fires drain after a write that came back false once.
    if (target._needDrain && target._pending < target.highWaterMark) {
      target._needDrain = false;
      target.emit('drain');
    }
    target._drainWrites();
    target._finishIfDone();
  };

  const writableMethods = {
    write(chunk, encoding, callback) {
      if (typeof encoding === 'function') {
        callback = encoding;
        encoding = undefined;
      }
      if (this.writableEnded) {
        const error = new Error('write after end');
        if (callback) defer(() => callback(error));
        else this.emit('error', error);
        return false;
      }
      const data = asChunk(this, chunk, encoding);
      const size = sizeOf(this, data);
      this._pending += size;
      // An idle writer takes the chunk straight to _write: queueing it in the
      // native buffer only to shift it back out costs two host calls per write.
      if (!this._writing && this._callbacks.length === 0) {
        this._writeNow(data, size, callback || null);
      } else {
        __stream_write(this._stream, data);
        this._callbacks.push(callback || null);
        this._drainWrites();
      }
      const writable = this._pending < this.highWaterMark;
      if (!writable) this._needDrain = true;
      return writable;
    },

    end(chunk, encoding, callback) {
      if (typeof chunk === 'function') {
        callback = chunk;
        chunk = undefined;
      } else if (typeof encoding === 'function') {
        callback = encoding;
        encoding = undefined;
      }
      if (chunk !== undefined && chunk !== null) this.write(chunk, encoding);
      this.writableEnded = true;
      this._ending = true;
      if (callback) this.once('finish', callback);
      this._finishIfDone();
      return this;
    },

    _writeNow(data, size, callback) {
      this._writing = true;
      let settled = false;
      const done = (error) => {
        if (settled) return;
        settled = true;
        this._pending -= size;
        settledWrite(this, callback, error);
      };
      if (!this._writeFn) {
        done(null);
        return;
      }
      try {
        this._writeFn(toWriteChunk(data), 'buffer', done);
      } catch (error) {
        done(error);
      }
    },

    _drainWrites() {
      if (this._draining) return;
      this._draining = true;
      while (!this._writing && this._callbacks.length > 0) {
        const raw = __stream_shift_write(this._stream);
        if (raw === null) break;
        const callback = this._callbacks.shift();
        const data = this.objectMode ? raw : wrap(raw);
        const size = sizeOf(this, data);
        this._writing = true;
        let settled = false;
        const done = (error) => {
          if (settled) return;
          settled = true;
          this._pending -= size;
          settledWrite(this, callback, error);
        };
        if (!this._writeFn) {
          done(null);
          continue;
        }
        try {
          this._writeFn(data, 'buffer', done);
        } catch (error) {
          done(error);
          return;
        }
      }
      this._draining = false;
      this._finishIfDone();
    },

    _finishIfDone() {
      if (!this._ending || this._finishing || this._writing) return;
      if (this._callbacks.length > 0 || this._pending > 0) return;
      this._finishing = true;
      const finish = () => {
        this.writable = false;
        this.writableFinished = true;
        this.emit('finish');
        this.emit('close');
      };
      if (!this._finalFn) {
        finish();
        return;
      }
      try {
        this._finalFn((error) => {
          if (error) this.emit('error', error);
          else finish();
        });
      } catch (error) {
        this.emit('error', error);
      }
    },
  };

  class Writable extends Stream {
    constructor(options) {
      super();
      this._stream = __stream_open(false);
      this._destroyed = false;
      initWritable(this, options);
    }
  }
  Object.assign(Writable.prototype, writableMethods);

  class Duplex extends Readable {
    constructor(options) {
      super(options);
      initWritable(this, options);
    }
  }
  Object.assign(Duplex.prototype, writableMethods);

  class Transform extends Duplex {
    constructor(options) {
      super(options);
      const settings = options || {};
      this._transformFn = typeof settings.transform === 'function'
        ? settings.transform
        : (chunk, encoding, done) => done(null, chunk);
      this._flushTransformFn = typeof settings.flush === 'function' ? settings.flush : null;
      this._startTransform();
      // Sealing the write side has to seal the read side too, otherwise downstream
      // never sees 'end' and pipeline waits forever.
      if (!settings.final) {
        this._finalFn = (done) => {
          const close = () => {
            this.push(null);
            done();
          };
          if (!this._flushTransformFn) {
            close();
            return;
          }
          try {
            this._flushTransformFn((error, output) => {
              if (error) return done(error);
              if (output !== undefined && output !== null) this.push(output);
              close();
            });
          } catch (error) {
            done(error);
          }
        };
      }
    }
  }
  Object.assign(Transform.prototype, writableMethods, {
    _startTransform() {
      this._writeFn = (data, encoding, done) => {
        try {
          this._transformFn(data, encoding, (error, output) => {
            if (error) return done(error);
            if (output !== undefined && output !== null) this.push(output);
            done(null);
          });
        } catch (error) {
          done(error);
        }
      };
    },
  });

  class PassThrough extends Transform {}

  const finished = (stream, callback) => {
    const promise = new Promise((resolve, reject) => {
      let done = false;
      const settle = (error) => {
        if (done) return;
        done = true;
        if (error) reject(error);
        else resolve();
      };
      stream.once('error', settle);
      stream.once('end', () => settle());
      stream.once('finish', () => settle());
      if (stream.readableEnded || stream.writableFinished) settle();
    });
    if (callback) {
      promise.then(() => callback(null), (error) => callback(error));
      return undefined;
    }
    return promise;
  };

  const pipeline = (...args) => {
    const callback = typeof args[args.length - 1] === 'function' ? args.pop() : null;
    const streams = args;
    const promise = new Promise((resolve, reject) => {
      if (streams.length === 0) {
        resolve();
        return;
      }
      let settled = false;
      const fail = (error) => {
        if (settled) return;
        settled = true;
        for (const stream of streams) {
          if (stream && typeof stream.destroy === 'function') {
            try { stream.destroy(error); } catch (ignored) { /* already tearing down */ }
          }
        }
        reject(error);
      };
      for (const stream of streams) {
        if (stream && typeof stream.once === 'function') stream.once('error', fail);
      }
      for (let index = 0; index < streams.length - 1; index += 1) {
        streams[index].pipe(streams[index + 1]);
      }
      const last = streams[streams.length - 1];
      const isReadableOnly = last instanceof Readable && !(last instanceof Writable);
      last.once(isReadableOnly ? 'end' : 'finish', () => {
        if (settled) return;
        settled = true;
        resolve();
      });
    });
    if (callback) {
      promise.then(() => callback(null), (error) => callback(error));
      return undefined;
    }
    return promise;
  };

  const api = {
    Stream, Readable, Writable, Duplex, Transform, PassThrough,
    getDefaultHighWaterMark: (objectMode) => (objectMode ? 16 : 65536), setDefaultHighWaterMark() {},
    pipeline, finished, promises: { pipeline, finished },
  };
  api.default = api;
  globalThis.stream = api;
  globalThis.__stream_module = api;
  globalThis.__stream_promises = { pipeline, finished };
})();

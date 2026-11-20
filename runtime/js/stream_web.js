(function () {
  'use strict';

  // The WHATWG streams API. The queue lives in Rust (see stream_web.rs); what is
  // here is the state machine and the promise plumbing, which is where the spec
  // spends its time anyway.
  //
  // Two things are deliberately simpler than the specification, both invisible
  // to ordinary code: a BYOB read returns a view over the caller's own buffer
  // instead of detaching that buffer, and byobRequest is always null, so a source
  // that wants to fill a view has to use autoAllocateChunkSize or enqueue itself.

  const typeError = (message) => new TypeError(message);
  const rangeError = (message) => new RangeError(message);
  const resolved = (value) => Promise.resolve(value);
  const rejected = (reason) => Promise.reject(reason);
  const isView = (value) => ArrayBuffer.isView(value);

  class CountQueuingStrategy {
    constructor(options) {
      const settings = options || {};
      this.highWaterMark = settings.highWaterMark === undefined ? 1 : settings.highWaterMark;
      this.size = () => 1;
    }
  }

  class ByteLengthQueuingStrategy {
    constructor(options) {
      const settings = options || {};
      this.highWaterMark = settings.highWaterMark === undefined ? 0 : settings.highWaterMark;
      this.size = (chunk) => chunk.byteLength;
    }
  }

  // ---------------------------------------------------------------------
  // ReadableStream
  // ---------------------------------------------------------------------

  class ReadableStream {
    constructor(source, strategy) {
      if (source === undefined || source === null) source = {};
      const settings = strategy || {};
      const type = source.type;
      if (type !== undefined && type !== 'byte') {
        throw rangeError('The type of a ReadableStream source has to be byte');
      }
      const bytes = type === 'byte';
      this._source = source;
      this._byte = bytes;
      this._queue = __sw_open(bytes);
      this._queueOpen = true;
      this._state = 'readable';
      this._closeRequested = false;
      this._storedError = null;
      this._reader = null;
      this._size = 0;
      this._sizes = [];
      this._highWaterMark = settings.highWaterMark === undefined ? (bytes ? 0 : 1) : settings.highWaterMark;
      this._strategySize = typeof settings.size === 'function' ? settings.size : null;
      this._pulling = false;
      this._requests = [];
      this._onDrain = null;
      this._autoAllocate = bytes && source.autoAllocateChunkSize ? source.autoAllocateChunkSize : 0;
      let settleClosed;
      let failClosed;
      this._closed = new Promise((resolve, reject) => {
        settleClosed = resolve;
        failClosed = reject;
      });
      // Nobody may be waiting on it; a rejection here must not be reported as
      // unhandled, which the engine treats as an error at the end of the turn.
      this._closed.catch(() => {});
      this._settleClosed = settleClosed;
      this._failClosed = failClosed;
      this._controller = bytes
        ? new ReadableByteStreamController(this)
        : new ReadableStreamDefaultController(this);
      let started;
      const start = source.start;
      if (typeof start === 'function') {
        try {
          started = start.call(source, this._controller);
        } catch (error) {
          this._error(error);
        }
      }
      this._started = Promise.resolve(started);
      this._started.catch((error) => {
        this._error(error);
      });
    }

    get locked() {
      return this._reader !== null;
    }

    cancel(reason) {
      if (this._reader !== null) {
        return rejected(typeError('Cannot cancel a stream that is locked to a reader'));
      }
      return this._cancel(reason);
    }

    _cancel(reason) {
      if (this._state === 'errored') return rejected(this._storedError);
      if (this._state === 'closed') return resolved(undefined);
      this._release(typeError('The stream was cancelled'));
      const cancel = this._source.cancel;
      if (typeof cancel !== 'function') return resolved(undefined);
      try {
        return Promise.resolve(cancel.call(this._source, reason)).then(() => undefined);
      } catch (error) {
        return rejected(error);
      }
    }

    // Hand a chunk to the queue, then to whoever is waiting. Called by both
    // controllers.
    _enqueue(chunk, size) {
      if (this._closeRequested) throw typeError('The stream is closing');
      if (this._state !== 'readable') throw typeError('The stream is not readable');
      if (this._byte) {
        if (!isView(chunk)) throw typeError('A byte stream only takes ArrayBufferView chunks');
        if (chunk.byteLength === 0) throw typeError('A byte stream does not take empty chunks');
        size = chunk.byteLength;
      }
      __sw_put(this._queue, chunk);
      this._size += size;
      if (!this._byte) this._sizes.push(size);
      this._drain();
    }

    _dequeue(max) {
      const taken = __sw_take(this._queue, max);
      if (this._byte) {
        const view = new Uint8Array(taken);
        this._size -= view.length;
        if (this._onDrain !== null) this._onDrain(view.length);
        return view;
      }
      const size = this._sizes.length > 0 ? this._sizes.shift() : 1;
      this._size -= size;
      if (this._onDrain !== null) this._onDrain(size);
      return taken;
    }

    // Serve pending reads from the queue, oldest first.
    _drain() {
      while (this._requests.length > 0 && __sw_count(this._queue) > 0) {
        const request = this._requests.shift();
        request.resolve({ value: this._dequeue(request.max || 0), done: false });
      }
      if (this._requests.length > 0 && this._closeRequested) this._finish();
      else this._maybePull();
    }

    _maybePull() {
      if (this._pulling) return;
      if (this._state !== 'readable' || this._closeRequested) return;
      const pull = this._source.pull;
      if (typeof pull !== 'function') return;
      const room = this._highWaterMark - this._size;
      if (this._requests.length === 0 && room <= 0) return;
      this._pulling = true;
      this._started
        .then(() => pull.call(this._source, this._controller))
        .then(
          () => {
            this._pulling = false;
            this._maybePull();
          },
          (error) => {
            this._pulling = false;
            this._error(error);
          }
        );
    }

    _close() {
      if (this._closeRequested) throw typeError('The stream is already closing');
      if (this._state !== 'readable') throw typeError('The stream is not readable');
      this._closeRequested = true;
      if (__sw_count(this._queue) === 0) this._finish();
      else this._drain();
    }

    // Closed with chunks still queued: the reads that are already waiting get
    // them, then the stream reports done.
    _finish() {
      if (this._state !== 'readable') return;
      this._state = 'closed';
      this._releaseQueued();
      const requests = this._requests;
      this._requests = [];
      for (const request of requests) {
        request.resolve({ value: undefined, done: true });
      }
      this._settleClosed();
    }

    _error(reason) {
      if (this._state === 'errored') return;
      this._state = 'errored';
      this._storedError = reason;
      this._releaseQueued();
      const requests = this._requests;
      this._requests = [];
      for (const request of requests) request.reject(reason);
      this._failClosed(reason);
    }

    // Whatever is left in the queue is dropped, and the native queue with it.
    _releaseQueued() {
      this._size = 0;
      this._sizes = [];
      if (!this._queueOpen) return;
      this._queueOpen = false;
      __sw_reset(this._queue);
      __sw_drop(this._queue);
    }

    // Cancelled or released: readers waiting on a chunk are told why.
    _release(reason) {
      if (this._state === 'readable') {
        this._state = 'closed';
        this._releaseQueued();
        const requests = this._requests;
        this._requests = [];
        for (const request of requests) request.reject(reason);
        this._settleClosed();
      }
    }

    getReader(options) {
      const mode = options && options.mode;
      if (mode !== undefined && mode !== 'byob') {
        throw rangeError('The only reader mode is byob');
      }
      if (this._reader !== null) throw typeError('The stream is already locked to a reader');
      return mode === 'byob'
        ? new ReadableStreamBYOBReader(this)
        : new ReadableStreamDefaultReader(this);
    }

    values(options) {
      const reader = this.getReader();
      const preventCancel = !!(options && options.preventCancel);
      const stream = this;
      return {
        next() {
          return reader.read();
        },
        return(value) {
          const done = resolved({ value, done: true });
          try {
            reader.releaseLock();
          } catch (error) {
            return rejected(error);
          }
          if (!preventCancel) return Promise.resolve(stream.cancel()).then(() => done);
          return done;
        },
        throw(value) {
          return rejected(value);
        },
        [Symbol.asyncIterator]() {
          return this;
        },
      };
    }

    [Symbol.asyncIterator](options) {
      return this.values(options);
    }

    tee() {
      return this._tee();
    }

    pipeTo(destination, options) {
      return this._pipeTo(destination, options);
    }

    pipeThrough(transform, options) {
      return this._pipeTo(transform.writable, options).catch(() => {}), transform.readable;
    }
  }

  class ReadableStreamDefaultController {
    constructor(stream) {
      this._stream = stream;
    }

    get desiredSize() {
      const stream = this._stream;
      if (stream._state === 'errored') return null;
      if (stream._state === 'closed') return 0;
      return stream._highWaterMark - stream._size;
    }

    enqueue(chunk) {
      const stream = this._stream;
      let size = 1;
      if (stream._strategySize !== null) {
        try {
          size = stream._strategySize(chunk);
        } catch (error) {
          stream._error(error);
          throw error;
        }
      }
      stream._enqueue(chunk, size);
    }

    close() {
      this._stream._close();
    }

    error(reason) {
      this._stream._error(reason);
    }
  }

  class ReadableByteStreamController {
    constructor(stream) {
      this._stream = stream;
    }

    get byobRequest() {
      return null;
    }

    get desiredSize() {
      const stream = this._stream;
      if (stream._state === 'errored') return null;
      if (stream._state === 'closed') return 0;
      return stream._highWaterMark - stream._size;
    }

    enqueue(chunk) {
      this._stream._enqueue(chunk, 0);
    }

    close() {
      this._stream._close();
    }

    error(reason) {
      this._stream._error(reason);
    }
  }

  class ReadableStreamDefaultReader {
    constructor(stream) {
      this._stream = stream;
      stream._reader = this;
    }

    get closed() {
      return this._stream === null
        ? rejected(typeError('This reader has been released'))
        : this._stream._closed;
    }

    read() {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This reader has been released'));
      if (stream._state === 'closed') return resolved({ value: undefined, done: true });
      if (stream._state === 'errored') return rejected(stream._storedError);
      if (__sw_count(stream._queue) > 0) {
        return resolved({ value: stream._dequeue(0), done: false });
      }
      // Closed with nothing left: the stream is only now really finished, so a
      // read that arrives after the queue drained reports done here.
      if (stream._closeRequested) {
        stream._finish();
        return resolved({ value: undefined, done: true });
      }
      return new Promise((resolve, reject) => {
        stream._requests.push({ resolve, reject, max: 0 });
        stream._maybePull();
      });
    }

    cancel(reason) {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This reader has been released'));
      return stream._cancel(reason);
    }

    releaseLock() {
      const stream = this._stream;
      if (stream === null) return;
      this._stream = null;
      stream._reader = null;
      if (stream._requests.length > 0) {
        const requests = stream._requests;
        stream._requests = [];
        const error = typeError('The reader was released with reads pending');
        for (const request of requests) request.reject(error);
      }
    }
  }

  class ReadableStreamBYOBReader {
    constructor(stream) {
      this._stream = stream;
      stream._reader = this;
    }

    get closed() {
      return this._stream === null
        ? rejected(typeError('This reader has been released'))
        : this._stream._closed;
    }

    read(view) {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This reader has been released'));
      if (!isView(view) || view.byteLength === 0) {
        return rejected(typeError('The read view has to be a non-empty ArrayBufferView'));
      }
      if (!stream._byte) return rejected(typeError('This stream is not a byte stream'));
      if (stream._state === 'errored') return rejected(stream._storedError);
      if (stream._state === 'closed') return resolved({ value: emptyView(view), done: true });
      if (__sw_count(stream._queue) > 0) return resolved(fill(view, stream));
      if (stream._closeRequested) {
        stream._finish();
        return resolved({ value: emptyView(view), done: true });
      }
      return new Promise((resolve, reject) => {
        stream._requests.push({
          resolve: (result) => resolve(result.done ? result : fill(view, stream, result.value)),
          reject,
          max: view.byteLength,
          view,
        });
        stream._maybePull();
      });
    }

    cancel(reason) {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This reader has been released'));
      return stream._cancel(reason);
    }

    releaseLock() {
      const stream = this._stream;
      if (stream === null) return;
      this._stream = null;
      stream._reader = null;
      if (stream._requests.length > 0) {
        const requests = stream._requests;
        stream._requests = [];
        const error = typeError('The reader was released with reads pending');
        for (const request of requests) request.reject(error);
      }
    }
  }

  // The BYOB path: the bytes arrive as an ArrayBuffer, so they are copied into
  // the caller view, and the view handed back covers just what was filled.
  const fill = (view, stream, taken) => {
    const bytes = taken === undefined ? stream._dequeue(view.byteLength) : taken;
    const space = Math.min(bytes.length, view.byteLength);
    const target = new Uint8Array(view.buffer, view.byteOffset, view.byteLength);
    target.set(bytes.subarray(0, space));
    return { value: sizedView(view, space), done: false };
  };

  const sizedView = (view, length) => {
    if (length === view.byteLength) return view;
    if (view instanceof Uint8Array) return new Uint8Array(view.buffer, view.byteOffset, length);
    const unit = view.BYTES_PER_ELEMENT || 1;
    return new view.constructor(view.buffer, view.byteOffset, Math.floor(length / unit));
  };

  const emptyView = (view) => sizedView(view, 0);

  // ---------------------------------------------------------------------
  // WritableStream
  // ---------------------------------------------------------------------

  class WritableStream {
    constructor(sink, strategy) {
      if (sink === undefined || sink === null) sink = {};
      const settings = strategy || {};
      this._sink = sink;
      this._queue = __sw_open(false);
      this._queueOpen = true;
      this._state = 'writable';
      this._storedError = null;
      this._writer = null;
      this._highWaterMark = settings.highWaterMark === undefined ? 1 : settings.highWaterMark;
      this._strategySize = typeof settings.size === 'function' ? settings.size : null;
      this._pending = [];
      this._size = 0;
      this._busy = false;
      this._readyRequests = [];
      this._controller = new WritableStreamDefaultController(this);
      let settle;
      let fail;
      this._closed = new Promise((resolve, reject) => {
        settle = resolve;
        fail = reject;
      });
      this._closed.catch(() => {});
      this._settleClosed = settle;
      this._failClosed = fail;
      let started;
      const start = sink.start;
      if (typeof start === 'function') {
        try {
          started = start.call(sink, this._controller);
        } catch (error) {
          this._error(error);
        }
      }
      this._startedSettled = false;
      this._started = Promise.resolve(started);
      this._started.then(
        () => {
          this._startedSettled = true;
          this._busy = false;
          this._advance();
        },
        (error) => {
          this._startedSettled = true;
          this._error(error);
        }
      );
    }

    get locked() {
      return this._writer !== null;
    }

    abort(reason) {
      if (this._writer !== null) {
        return rejected(typeError('Cannot abort a stream that is locked to a writer'));
      }
      return this._abort(reason);
    }

    close() {
      if (this._writer !== null) {
        return rejected(typeError('Cannot close a stream that is locked to a writer'));
      }
      return this._close();
    }

    getWriter() {
      if (this._writer !== null) throw typeError('The stream is already locked to a writer');
      return new WritableStreamDefaultWriter(this);
    }

    _write(chunk) {
      let size = 1;
      if (this._strategySize !== null) {
        try {
          size = this._strategySize(chunk);
        } catch (error) {
          this._error(error);
          return rejected(error);
        }
      }
      return new Promise((resolve, reject) => {
        __sw_put(this._queue, chunk);
        this._pending.push({ size, resolve, reject });
        this._size += size;
        this._advance();
      });
    }

    // One sink.write at a time. Chunks wait in the native queue so nothing is
    // held in JS beyond the promise that resolves the caller.
    //
    // The loop matters for throughput: a sink whose write returns nothing is
    // finished with the chunk on the spot, so a whole queue drains without a
    // single promise. Without that, every chunk costs a handful of promise hops,
    // which is the most expensive thing this engine does.
    _advance() {
      if (this._busy) return;
      if (!this._startedSettled && this._pending.length > 0) {
        // The start promise settles into _advance on its own; this is only here
        // so a second call cannot overtake the first chunk.
        this._busy = true;
        return;
      }
      while (this._pending.length > 0) {
        if (this._state === 'errored') {
          this._rejectPending(this._storedError);
          return;
        }
        if (this._state !== 'writable' && this._state !== 'closing') {
          this._rejectPending(typeError('The stream is not writable'));
          return;
        }
        const item = this._pending[0];
        const chunk = __sw_take(this._queue, 0);
        const write = this._sink.write;
        let result;
        if (typeof write === 'function') {
          try {
            result = write.call(this._sink, chunk);
          } catch (error) {
            this._pending.shift();
            item.reject(error);
            this._error(error);
            return;
          }
        }
        this._pending.shift();
        if (result !== null && typeof result === 'object' && typeof result.then === 'function') {
          this._busy = true;
          Promise.resolve(result).then(
            () => {
              this._busy = false;
              this._size -= item.size;
              item.resolve();
              this._settleReady();
              this._advance();
            },
            (error) => {
              this._busy = false;
              item.reject(error);
              this._error(error);
            }
          );
          return;
        }
        this._size -= item.size;
        item.resolve();
      }
      this._settleReady();
      if (this._state === 'closing') this._finishClose();
    }

    _close() {
      if (this._state === 'closed') return rejected(typeError('The stream is already closed'));
      if (this._state === 'errored') return rejected(this._storedError);
      this._state = 'closing';
      if (this._pending.length === 0 && !this._busy) this._finishClose();
      return this._closed;
    }

    _finishClose() {
      if (this._state === 'closed') return;
      const close = this._sink.close;
      const done = () => {
        this._state = 'closed';
        this._releaseQueued();
        this._settleClosed();
        const requests = this._readyRequests;
        this._readyRequests = [];
        for (const request of requests) request.resolve();
      };
      if (typeof close !== 'function') {
        done();
        return;
      }
      this._started.then(() => close.call(this._sink)).then(done, (error) => this._error(error));
    }

    _abort(reason) {
      if (this._state === 'errored') return rejected(this._storedError);
      if (this._state === 'closed') return resolved(undefined);
      this._state = 'errored';
      this._storedError = reason;
      this._releaseQueued();
      this._rejectPending(reason);
      const abort = this._sink.abort;
      const settle = () => {
        this._settleClosed();
      };
      if (typeof abort !== 'function') {
        settle();
        return resolved(undefined);
      }
      return this._started
        .then(() => abort.call(this._sink, reason))
        .then(settle, (error) => {
          settle();
          throw error;
        });
    }

    _error(reason) {
      if (this._state === 'errored') return;
      this._state = 'errored';
      this._storedError = reason;
      this._releaseQueued();
      this._rejectPending(reason);
      this._failClosed(reason);
    }

    _rejectPending(reason) {
      const pending = this._pending;
      this._pending = [];
      this._size = 0;
      for (const item of pending) item.reject(reason);
      const requests = this._readyRequests;
      this._readyRequests = [];
      for (const request of requests) request.reject(reason);
    }

    _releaseQueued() {
      if (!this._queueOpen) return;
      this._queueOpen = false;
      __sw_reset(this._queue);
      __sw_drop(this._queue);
    }

    _ready() {
      if (this._state === 'errored') return rejected(this._storedError);
      if (this._storedError !== null) return rejected(this._storedError);
      if (this._highWaterMark - this._size > 0) return resolved(undefined);
      return new Promise((resolve, reject) => {
        this._readyRequests.push({ resolve, reject });
      });
    }

    _settleReady() {
      if (this._readyRequests.length === 0) return;
      if (this._highWaterMark - this._size <= 0) return;
      const requests = this._readyRequests;
      this._readyRequests = [];
      for (const request of requests) request.resolve();
    }
  }

  class WritableStreamDefaultController {
    constructor(stream) {
      this._stream = stream;
    }

    get signal() {
      if (this._abortController === undefined) this._abortController = new AbortController();
      return this._abortController.signal;
    }

    error(reason) {
      this._stream._error(reason);
    }
  }

  class WritableStreamDefaultWriter {
    constructor(stream) {
      this._stream = stream;
      stream._writer = this;
    }

    get closed() {
      return this._stream === null
        ? rejected(typeError('This writer has been released'))
        : this._stream._closed;
    }

    get desiredSize() {
      const stream = this._stream;
      if (stream === null) throw typeError('This writer has been released');
      if (stream._state === 'errored') return null;
      if (stream._state === 'closed') return 0;
      return stream._highWaterMark - stream._size;
    }

    get ready() {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This writer has been released'));
      return stream._ready();
    }

    write(chunk) {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This writer has been released'));
      if (stream._state === 'errored') return rejected(stream._storedError);
      if (stream._state !== 'writable') {
        return rejected(typeError('The stream is closing or closed'));
      }
      return stream._write(chunk);
    }

    close() {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This writer has been released'));
      return stream._close();
    }

    abort(reason) {
      const stream = this._stream;
      if (stream === null) return rejected(typeError('This writer has been released'));
      return stream._abort(reason);
    }

    releaseLock() {
      const stream = this._stream;
      if (stream === null) return;
      this._stream = null;
      stream._writer = null;
    }
  }

  // ---------------------------------------------------------------------
  // TransformStream
  // ---------------------------------------------------------------------

  class TransformStreamDefaultController {
    constructor(stream) {
      this._stream = stream;
    }

    get desiredSize() {
      return this._stream._readableController.desiredSize;
    }

    enqueue(chunk) {
      this._stream._readableController.enqueue(chunk);
    }

    error(reason) {
      this._stream._readableController.error(reason);
      this._stream._writeError = reason;
    }

    terminate() {
      this._stream._readableController.close();
    }
  }

  class TransformStream {
    constructor(transformer, writableStrategy, readableStrategy) {
      const settings = transformer || {};
      const self = this;
      this._transformer = settings;
      this._backpressureWaiters = [];
      this._backpressure = false;
      this._writeError = null;
      this._readable = new ReadableStream(
        {
          start(controller) {
            self._readableController = controller;
          },
          pull() {
            self._clearBackpressure();
          },
          cancel(reason) {
            if (typeof settings.cancel === 'function') return settings.cancel(reason);
            return undefined;
          },
        },
        readableStrategy
      );
      this._readable._onDrain = () => {
        self._clearBackpressure();
      };
      this._writable = new WritableStream(
        {
          write(chunk) {
            return self._transform(chunk);
          },
          close() {
            return self._flush();
          },
          abort(reason) {
            self._readableController.error(reason);
            if (typeof settings.cancel === 'function') return settings.cancel(reason);
            return undefined;
          },
        },
        writableStrategy
      );
    }

    get readable() {
      return this._readable;
    }

    get writable() {
      return this._writable;
    }

    _transform(chunk) {
      if (this._writeError !== null) return rejected(this._writeError);
      // The check comes before the transform, not after: with the default
      // readable strategy (a high water mark of zero) a transform that ran first
      // would enqueue into a queue nobody may read and then wait for itself.
      return this._waitForRoom().then(() => {
        const controller = new TransformStreamDefaultController(this);
        const transform = this._transformer.transform;
        const after = () => {
          if (this._readableController.desiredSize <= 0) this._backpressure = true;
        };
        if (typeof transform !== 'function') {
          this._readableController.enqueue(chunk);
          after();
          return undefined;
        }
        const result = transform.call(this._transformer, chunk, controller);
        return Promise.resolve(result).then(() => {
          after();
        });
      });
    }

    _flush() {
      const flush = this._transformer.flush;
      const controller = new TransformStreamDefaultController(this);
      const close = () => {
        this._readableController.close();
      };
      if (typeof flush !== 'function') {
        close();
        return resolved(undefined);
      }
      return Promise.resolve(flush.call(this._transformer, controller)).then(close);
    }

    // The readable side is the brake: a transform that outruns its reader waits
    // here until a read makes room.
    _waitForRoom() {
      if (!this._backpressure) return resolved(undefined);
      if (this._readable._requests.length > 0) {
        this._backpressure = false;
        return resolved(undefined);
      }
      return new Promise((resolve) => {
        this._backpressureWaiters.push(resolve);
      });
    }

    _clearBackpressure() {
      this._backpressure = false;
      if (this._backpressureWaiters.length === 0) return;
      const waiters = this._backpressureWaiters;
      this._backpressureWaiters = [];
      for (const waiter of waiters) waiter();
    }
  }
  // ---------------------------------------------------------------------
  // tee, pipeTo, pipeThrough
  // ---------------------------------------------------------------------
  //
  // These are methods on ReadableStream, declared above, and filled in here so
  // the class body stays readable.

  ReadableStream.prototype._tee = function () {
    const reader = this.getReader();
    const want = [false, false];
    const controllers = [null, null];
    const cancelled = [false, false];
    let reading = false;
    let finished = false;

    const pump = () => {
      if (reading || finished) return;
      if (!want[0] || !want[1]) return;
      reading = true;
      reader.read().then(
        (result) => {
          reading = false;
          if (result.done) {
            finished = true;
            controllers[0].close();
            controllers[1].close();
            reader.releaseLock();
            return;
          }
          // One chunk, both branches: the same reference reaches each reader.
          controllers[0].enqueue(result.value);
          controllers[1].enqueue(result.value);
          want[0] = false;
          want[1] = false;
          pump();
        },
        (error) => {
          reading = false;
          finished = true;
          controllers[0].error(error);
          controllers[1].error(error);
        }
      );
    };

    const branch = (index) =>
      new ReadableStream(
        {
          start(controller) {
            controllers[index] = controller;
          },
          pull() {
            want[index] = true;
            pump();
          },
          cancel(reason) {
            cancelled[index] = true;
            if (!cancelled[0] || !cancelled[1]) return undefined;
            finished = true;
            return reader.cancel(reason);
          },
        },
        { highWaterMark: 0 }
      );

    return [branch(0), branch(1)];
  };

  ReadableStream.prototype._pipeTo = function (destination, options) {
    const settings = options || {};
    const preventClose = !!settings.preventClose;
    const preventAbort = !!settings.preventAbort;
    const preventCancel = !!settings.preventCancel;
    const signal = settings.signal;
    if (destination instanceof ReadableStream || !destination) {
      return rejected(typeError('pipeTo needs a WritableStream'));
    }
    if (signal && signal.aborted) {
      return rejected(signal.reason === undefined ? typeError('aborted') : signal.reason);
    }
    const reader = this.getReader();
    let writer;
    try {
      writer = destination.getWriter();
    } catch (error) {
      reader.releaseLock();
      return rejected(error);
    }
    const stopBoth = (reason) => {
      const tasks = [];
      if (!preventCancel) tasks.push(reader.cancel(reason).catch(() => {}));
      if (!preventAbort) tasks.push(writer.abort(reason).catch(() => {}));
      return Promise.all(tasks).then(() => {
        throw reason;
      });
    };
    const pump = () =>
      reader.read().then(
        (result) => {
          if (result.done) {
            reader.releaseLock();
            if (preventClose) {
              writer.releaseLock();
              return undefined;
            }
            return writer.close().then(() => writer.releaseLock());
          }
          return writer.write(result.value).then(pump);
        },
        (error) => {
          reader.releaseLock();
          return stopBoth(error);
        }
      );
    const pumped = pump().then(
      () => undefined,
      (error) => {
        if (preventAbort) throw error;
        return writer.abort(error).then(() => {
          throw error;
        });
      }
    );
    if (!signal) return pumped;
    return new Promise((resolve, reject) => {
      const onAbort = () => {
        const reason = signal.reason === undefined ? typeError('aborted') : signal.reason;
        stopBoth(reason).catch(() => reject(reason));
      };
      signal.addEventListener('abort', onAbort, { once: true });
      pumped.then(
        (value) => {
          signal.removeEventListener('abort', onAbort);
          resolve(value);
        },
        (error) => {
          signal.removeEventListener('abort', onAbort);
          reject(error);
        }
      );
    });
  };
  globalThis.__stream_web = {
    ReadableStream,
    ReadableStreamDefaultReader,
    ReadableStreamBYOBReader,
    ReadableStreamDefaultController,
    ReadableByteStreamController,
    WritableStream,
    WritableStreamDefaultWriter,
    WritableStreamDefaultController,
    TransformStream,
    TransformStreamDefaultController,
    ByteLengthQueuingStrategy,
    CountQueuingStrategy,
  };
  globalThis.ReadableStream = ReadableStream;
  globalThis.ReadableStreamDefaultReader = ReadableStreamDefaultReader;
  globalThis.ReadableStreamBYOBReader = ReadableStreamBYOBReader;
  globalThis.WritableStream = WritableStream;
  globalThis.WritableStreamDefaultWriter = WritableStreamDefaultWriter;
  globalThis.TransformStream = TransformStream;
  globalThis.ByteLengthQueuingStrategy = ByteLengthQueuingStrategy;
  globalThis.CountQueuingStrategy = CountQueuingStrategy;
})();

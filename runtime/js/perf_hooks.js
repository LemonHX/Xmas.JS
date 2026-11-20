(function () {
  if (typeof EventTarget !== 'function') throw new Error('perf_hooks must be installed after events');

  // One native call per reading; nothing else in here crosses the boundary.
  const nowNs = __perf_now_ns;
  const millis = () => nowNs() / 1e6;

  class PerformanceEntry {
    constructor(name, entryType, startTime, duration) {
      this.name = String(name);
      this.entryType = String(entryType);
      this.startTime = startTime;
      this.duration = duration;
    }
    toJSON() {
      return {
        name: this.name,
        entryType: this.entryType,
        startTime: this.startTime,
        duration: this.duration,
      };
    }
  }

  class PerformanceMark extends PerformanceEntry {
    constructor(name, options) {
      const settings = options || {};
      super(name, 'mark', settings.startTime === undefined ? millis() : Number(settings.startTime), 0);
      this.detail = settings.detail === undefined ? null : settings.detail;
    }
    toJSON() {
      const json = super.toJSON();
      json.detail = this.detail;
      return json;
    }
  }

  class PerformanceMeasure extends PerformanceEntry {
    constructor(name, startTime, duration, detail) {
      super(name, 'measure', startTime, duration);
      this.detail = detail === undefined ? null : detail;
    }
    toJSON() {
      const json = super.toJSON();
      json.detail = this.detail;
      return json;
    }
  }

  class Performance extends EventTarget {
    constructor() {
      super();
      Object.defineProperty(this, '_entries', { value: [], enumerable: false });
      Object.defineProperty(this, '_observers', { value: [], enumerable: false });
    }

    now() {
      return millis();
    }

    get timeOrigin() {
      return __perf_origin();
    }

    mark(name, options) {
      const entry = new PerformanceMark(name, options);
      this._record(entry);
      return entry;
    }

    measure(name, startOrOptions, endMark) {
      const at = millis();
      let startTime;
      let endTime;
      let detail = null;
      if (startOrOptions !== null && typeof startOrOptions === 'object') {
        detail = startOrOptions.detail === undefined ? null : startOrOptions.detail;
        startTime = this._timeOf(startOrOptions.start, at);
        endTime =
          startOrOptions.duration === undefined
            ? this._timeOf(startOrOptions.end, at)
            : startTime + Number(startOrOptions.duration);
      } else {
        startTime = this._timeOf(startOrOptions, at);
        endTime = this._timeOf(endMark, at);
      }
      const entry = new PerformanceMeasure(name, startTime, Math.max(0, endTime - startTime), detail);
      this._record(entry);
      return entry;
    }

    getEntries() {
      return this._entries.slice();
    }

    // Plain loops, not filter: a closure per entry is 30 ns of the 30 us this
    // takes on a thousand marks.
    getEntriesByType(type) {
      const entries = this._entries;
      const wanted = String(type);
      const found = [];
      for (let index = 0; index < entries.length; index += 1) {
        if (entries[index].entryType === wanted) found.push(entries[index]);
      }
      return found;
    }

    getEntriesByName(name, type) {
      const entries = this._entries;
      const wanted = String(name);
      const wantedType = type === undefined ? undefined : String(type);
      const found = [];
      for (let index = 0; index < entries.length; index += 1) {
        const entry = entries[index];
        if (entry.name !== wanted) continue;
        if (wantedType !== undefined && entry.entryType !== wantedType) continue;
        found.push(entry);
      }
      return found;
    }

    clearMarks(name) {
      this._drop('mark', name);
    }

    clearMeasures(name) {
      this._drop('measure', name);
    }

    // The lists are defined non-writable, so removal walks backwards in place.
    _drop(type, name) {
      for (let index = this._entries.length - 1; index >= 0; index -= 1) {
        const entry = this._entries[index];
        if (entry.entryType !== type) continue;
        if (name === undefined || entry.name === String(name)) this._entries.splice(index, 1);
      }
    }

    toJSON() {
      return { timeOrigin: this.timeOrigin };
    }

    // Node's event loop utilization needs the loop's own accounting; report a
    // believable zero rather than throwing, so code that reads it keeps going.
    eventLoopUtilization() {
      return { idle: 0, active: 0, utilization: 0 };
    }

    _timeOf(value, fallback) {
      if (value === undefined) return fallback;
      if (typeof value === 'number') return value;
      const wanted = String(value);
      const entries = this._entries;
      for (let index = entries.length - 1; index >= 0; index -= 1) {
        const entry = entries[index];
        if (entry.entryType === 'mark' && entry.name === wanted) return entry.startTime;
      }
      throw new Error("Failed to execute 'measure': the mark '" + value + "' does not exist");
    }

    _record(entry) {
      this._entries.push(entry);
      for (const observer of this._observers) observer._offer(entry);
    }
  }

  // What an observer callback receives: Node hands over a list object, not an
  // array, and tooling calls getEntries on it.
  class PerformanceObserverEntryList {
    constructor(entries) {
      Object.defineProperty(this, '_entries', { value: entries, enumerable: false });
    }
    getEntries() {
      return this._entries.slice();
    }
    getEntriesByType(type) {
      return this._entries.filter((entry) => entry.entryType === String(type));
    }
    getEntriesByName(name, type) {
      const wanted = String(name);
      return this._entries.filter(
        (entry) => entry.name === wanted && (type === undefined || entry.entryType === String(type)),
      );
    }
  }

  class PerformanceObserver {
    constructor(callback) {
      if (typeof callback !== 'function') {
        throw new TypeError('The "callback" argument must be of type function');
      }
      this._callback = callback;
      this._types = [];
      this._buffered = false;
      this._pending = [];
      this._scheduled = false;
    }

    observe(options) {
      const settings = options || {};
      const types = settings.entryTypes || (settings.type === undefined ? [] : [settings.type]);
      if (types.length === 0) throw new TypeError('entryTypes or type is required');
      if (this._types.length === 0) performance._observers.push(this);
      this._types = types.map(String);
      this._buffered = !!settings.buffered;
      if (this._buffered) {
        for (const entry of performance.getEntries()) {
          if (this._types.includes(entry.entryType)) this._pending.push(entry);
        }
        this._schedule();
      }
    }

    disconnect() {
      const at = performance._observers.indexOf(this);
      if (at >= 0) performance._observers.splice(at, 1);
      this._types = [];
      this._pending = [];
    }

    takeRecords() {
      return this._pending.splice(0);
    }

    _offer(entry) {
      if (!this._types.includes(entry.entryType)) return;
      this._pending.push(entry);
      this._schedule();
    }

    _schedule() {
      if (this._scheduled) return;
      this._scheduled = true;
      queueMicrotask(() => {
        this._scheduled = false;
        const records = this.takeRecords();
        if (records.length === 0) return;
        this._callback(new PerformanceObserverEntryList(records), this);
      });
    }

    static get supportedEntryTypes() {
      return ['mark', 'measure'];
    }
  }

  const constants = {
    NODE_PERFORMANCE_GC_MAJOR: 4,
    NODE_PERFORMANCE_GC_MINOR: 1,
    NODE_PERFORMANCE_GC_INCREMENTAL: 8,
    NODE_PERFORMANCE_GC_WEAKCB: 16,
    NODE_PERFORMANCE_ENTRY_TYPE_GC: 'gc',
    NODE_PERFORMANCE_ENTRY_TYPE_HTTP: 'http',
    NODE_PERFORMANCE_ENTRY_TYPE_HTTP2: 'http2',
    NODE_PERFORMANCE_ENTRY_TYPE_NET: 'net',
    NODE_PERFORMANCE_ENTRY_TYPE_DNS: 'dns',
    NODE_PERFORMANCE_MILESTONE_START: 0,
    NODE_PERFORMANCE_MILESTONE_ENVIRONMENT: 1,
    NODE_PERFORMANCE_MILESTONE_NODE_START: 2,
    NODE_PERFORMANCE_MILESTONE_V8_START: 3,
    NODE_PERFORMANCE_MILESTONE_LOOP_START: 4,
    NODE_PERFORMANCE_MILESTONE_LOOP_EXIT: 5,
    NODE_PERFORMANCE_MILESTONE_BOOTSTRAP_COMPLETE: 6,
  };

  globalThis.performance = new Performance();
  globalThis.PerformanceObserver = PerformanceObserver;
  globalThis.__perf_hooks_module = {
    Performance,
    PerformanceEntry,
    PerformanceMark,
    PerformanceMeasure,
    PerformanceObserver,
    constants,
  };
})();

(function () {
  const text = (value) => {
    if (typeof value === 'string') return value;
    if (typeof value === 'object' && value !== null) {
      try { return JSON.stringify(value); } catch (error) { return String(value); }
    }
    return String(value);
  };
  const format = (args) => args.map(text).join(' ');
  const write = (level, args) => __console_write(level, format(args));

  const console = {
    log: (...args) => write('log', args),
    info: (...args) => write('info', args),
    debug: (...args) => write('debug', args),
    warn: (...args) => write('warn', args),
    error: (...args) => write('error', args),
    trace: (...args) => write('trace', args),
    dir: (value) => write('log', [value]),
    assert: (ok, ...args) => { if (!ok) write('error', ['Assertion failed:', ...args]); },
    clear: () => {},
    time: (label) => { globalThis.__console_times = globalThis.__console_times || {}; globalThis.__console_times[String(label)] = __now_us(); },
    timeEnd: (label) => {
      const times = globalThis.__console_times || {};
      const started = times[String(label)];
      if (started === undefined) { write('warn', ['no such label: ' + label]); return; }
      write('log', [label + ': ' + ((__now_us() - started) / 1000).toFixed(3) + 'ms']);
      delete times[String(label)];
    },
  };

  globalThis.console = console;
  globalThis.__console = console;
})();

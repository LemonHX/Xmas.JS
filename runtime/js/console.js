(function () {
  // Node's console writes what util.format makes of its arguments, so the
  // formatting lives there now: inspect for objects, % spec for the rest. util
  // installs after console, so it is looked up at call time and the fallback
  // below only runs if something logs during installation.

  const fallback = (args) =>
    args
      .map((value) => {
        if (typeof value === 'string') return value;
        if (typeof value === 'object' && value !== null) {
          try {
            return JSON.stringify(value);
          } catch (error) {
            return String(value);
          }
        }
        return String(value);
      })
      .join(' ');

  const format = (args) => {
    const util = globalThis.util;
    if (util && typeof util.format === 'function') return util.format(...args);
    return fallback(args);
  };

  const write = (level, args) => __console_write(level, format(args));

  const stack = () => {
    const error = new Error();
    const lines = String(error.stack || '').split('\n');
    return lines.slice(1).join('\n');
  };

  const console = {
    log: (...args) => write('log', args),
    info: (...args) => write('info', args),
    debug: (...args) => write('debug', args),
    warn: (...args) => write('warn', args),
    error: (...args) => write('error', args),
    trace: (...args) => write('error', ['Trace: ' + format(args), stack()]),
    dir: (value, options) => write('log', [globalThis.util ? globalThis.util.inspect(value, options) : fallback([value])]),
    assert: (ok, ...args) => {
      if (ok) return;
      write('error', ['Assertion failed' + (args.length ? ': ' + format(args) : '')]);
    },
    clear: () => {},
    time: (label) => {
      globalThis.__console_times = globalThis.__console_times || {};
      globalThis.__console_times[String(label)] = __now_us();
    },
    timeEnd: (label) => {
      const times = globalThis.__console_times || {};
      const started = times[String(label)];
      if (started === undefined) {
        write('warn', ['no such label: ' + label]);
        return;
      }
      write('log', [label + ': ' + ((__now_us() - started) / 1000).toFixed(3) + 'ms']);
      delete times[String(label)];
    },
  };

  globalThis.console = console;
  globalThis.__console = console;
})();

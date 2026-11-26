(function () {
  // process is an EventEmitter in Node, and so are its stdio streams. Everything
  // that does not change is read once here; the rest stays a host call.
  const meta = JSON.parse(__process_meta());
  const EE = globalThis.EventEmitter;

  const process = new EE();
  const ticks = [];
  let scheduled = false;

  const report = (error) => {
    if (process.listenerCount('uncaughtException') > 0) {
      process.emit('uncaughtException', error);
      return;
    }
    __process_write('stderr', String(error && error.stack ? error.stack : error) + '\n');
  };

  const drainTicks = () => {
    scheduled = false;
    // A tick that schedules another tick belongs to the next drain, so the
    // length is taken once instead of re-read.
    const pending = ticks.splice(0, ticks.length);
    for (let i = 0; i < pending.length; i += 1) {
      try {
        pending[i][0].apply(undefined, pending[i][1]);
      } catch (error) {
        report(error);
      }
    }
  };

  const nextTick = (callback, ...args) => {
    if (typeof callback !== 'function') throw new TypeError('process.nextTick needs a function');
    ticks.push([callback, args]);
    if (!scheduled) {
      scheduled = true;
      Promise.resolve().then(drainTicks);
    }
  };

  const hrtime = (previous) => {
    const now = __process_now_ns();
    const seconds = Math.floor(now / 1e9);
    const nanos = Math.floor(now - seconds * 1e9);
    if (previous === undefined) return [seconds, nanos];
    let differenceSeconds = seconds - previous[0];
    let differenceNanos = nanos - previous[1];
    if (differenceNanos < 0) {
      differenceNanos += 1e9;
      differenceSeconds -= 1;
    }
    return [differenceSeconds, differenceNanos];
  };
  hrtime.bigint = () => BigInt(Math.floor(__process_now_ns()));

  // process.env writes have to reach the native side, or a spawned child would
  // not see them. Reads stay plain property reads when Proxy is around.
  const snapshot = meta.env;
  let env = snapshot;
  if (typeof Proxy === 'function') {
    env = new Proxy(snapshot, {
      set(target, key, value) {
        const name = String(key);
        const text = String(value);
        target[name] = text;
        __process_set_env(name, text);
        return true;
      },
      deleteProperty(target, key) {
        const name = String(key);
        delete target[name];
        __process_del_env(name);
        return true;
      },
    });
  }

  const stdio = (fd, name) => {
    const stream = new EE();
    stream.fd = fd;
    stream.isTTY = __process_isatty(fd);
    stream.columns = 80;
    stream.rows = 24;
    stream.write = (chunk, encoding, callback) => {
      let text = chunk;
      if (typeof chunk !== 'string') {
        text = globalThis.Buffer && globalThis.Buffer.isBuffer(chunk)
          ? chunk.toString(typeof encoding === 'string' ? encoding : 'utf8')
          : String(chunk);
      }
      __process_write(name, text);
      const done = typeof encoding === 'function' ? encoding : callback;
      if (typeof done === 'function') done();
      return true;
    };
    stream.end = (chunk) => {
      if (chunk !== undefined) stream.write(chunk);
    };
    return stream;
  };

  const exit = (code) => {
    const status = code === undefined ? 0 : Number(code) | 0;
    process.exitCode = status;
    process.emit('exit', status);
    // Returns only when the host captures output, where exiting would take the
    // harness down with it; in that case it throws instead.
    __process_exit(status);
  };

  process.argv = meta.argv;
  process.argv0 = meta.argv.length > 0 ? meta.argv[0] : '';
  process.execPath = meta.execPath;
  if (process.execArgv === undefined) process.execArgv = [];
  if (process.title === undefined) process.title = 'xmas';
  if (process.release === undefined) process.release = { name: 'xmas' };
  if (process.emitWarning === undefined) process.emitWarning = (warning) => { if (globalThis.console) console.error('Warning:', warning && warning.message ? warning.message : warning); };
  if (process.umask === undefined) process.umask = () => 0o22;
  if (process.kill === undefined) process.kill = () => { throw new Error('process.kill is not supported'); };
  process.env = env;
  process.platform = meta.platform;
  process.arch = meta.arch;
  process.pid = meta.pid;
  process.ppid = meta.ppid;
  process.version = meta.version;
  process.versions = meta.versions;
  process.title = 'xmas';
  process.cwd = () => __process_cwd();
  process.chdir = (path) => {
    __process_chdir(String(path));
  };
  process.nextTick = nextTick;
  process.hrtime = hrtime;
  process.uptime = () => __process_uptime();
  process.memoryUsage = () => JSON.parse(__process_memory());
  process.memoryUsage.rss = () => JSON.parse(__process_memory()).rss;
  process.stdout = stdio(1, 'stdout');
  process.stderr = stdio(2, 'stderr');
  process.exit = exit;
  process.abort = () => {
    throw new Error('process.abort is not supported');
  };
  process.emitWarning = (warning) => {
    const text = warning && warning.message ? warning.message : String(warning);
    process.emit('warning', warning);
    __process_write('stderr', '(Warning) ' + text + '\n');
  };

  globalThis.process = process;
})();

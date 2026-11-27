// node:child_process: the Node shape over the host in src/modules/child_process.rs.
//
// spawn() gives back a ChildProcess with two Readables and a Writable built out of the same
// read/write/wait promises `$` uses, exec() and execFile() collect that into one callback, and the
// three *Sync calls block the realm thread the way a synchronous call has to.
//
// Known gap, written down in docs/child-process.md: only SIGKILL can be sent. stdio is honoured
// for 'inherit', 'pipe' and 'ignore'; 'ipc' and file descriptors are not.

(function (global) {
  'use strict';

  const EventEmitter = global.EventEmitter;
  const stream = global.__stream_module;
  const Readable = stream.Readable;
  const Writable = stream.Writable;

  let ids = 1;

  const bytesOf = (base64) => new Uint8Array(__text_to_bytes(base64, 'base64'));
  const textOf = (bytes) => new TextDecoder().decode(bytes);

  function typeError(message) {
    return new TypeError(message);
  }

  class ChildProcess extends EventEmitter {
    constructor() {
      super();
      this.pid = -1;
      this.exitCode = null;
      this.signalCode = null;
      this.killed = false;
      this.spawnfile = '';
      this.spawnargs = [];
      this.stdin = null;
      this.stdout = null;
      this.stderr = null;
      this._id = 0;
    }

    kill(signal) {
      if (this._id === 0) return false;
      const sent = __cp_kill(this._id, signal === undefined ? '' : String(signal));
      this.killed = true;
      return sent;
    }
  }

  // One pipe, read on demand: a read is a promise, so the stream pulls only when someone wants
  // bytes, and pushes null when the host says the pipe closed.
  function reader(id, which) {
    let reading = false;
    let ended = false;
    const readable = new Readable({
      read() {
        if (reading || ended) return;
        reading = true;
        __cp_read(id, which).then(
          (chunk) => {
            reading = false;
            if (typeof chunk === 'string') {
              ended = true;
              readable.push(null);
              return;
            }
            readable.push(Buffer.from(new Uint8Array(chunk)));
          },
          (error) => {
            reading = false;
            ended = true;
            readable.destroy(error);
          },
        );
      },
    });
    return readable;
  }

  function writer(id) {
    return new Writable({
      write(chunk, encoding, done) {
        __cp_write(id, Buffer.from(chunk)).then(() => done(), done);
      },
      final(done) {
        __cp_end(id);
        done();
      },
    });
  }

  function encodeArguments(args) {
    const list = [];
    for (const argument of args) list.push(String(argument));
    return JSON.stringify(list);
  }

  // Node's stdio option has many shapes; what a caller means by the ones that matter here is one
  // answer for all three streams. A string ('inherit', 'ignore', 'pipe') is that answer, and an
  // array is when every entry agrees -- a mix keeps the pipes, which is what can be read back.
  function stdioMode(setting) {
    const one = (value) => (value === 1 || value === 2 ? 'inherit' : value);
    if (typeof setting === 'string') return one(setting);
    if (Array.isArray(setting)) {
      const names = setting.map(one);
      const all = names.every((name) => name === names[0]);
      return all ? names[0] : 'pipe';
    }
    return 'pipe';
  }

  function spawn(command, args, options) {
    let arguments_ = args;
    let settings = options;
    if (arguments_ === undefined || arguments_ === null) arguments_ = [];
    else if (!Array.isArray(arguments_)) {
      settings = arguments_;
      arguments_ = [];
    }
    settings = settings || {};

    const child = new ChildProcess();
    const id = ids++;
    const throughShell = settings.shell === true;
    const line = throughShell
      ? [String(command)].concat(arguments_.map(String)).join(' ')
      : String(command);
    const cwd = settings.cwd === undefined ? '' : String(settings.cwd);
    const env = JSON.stringify(settings.env || {});
    child._id = id;
    child.spawnfile = String(command);
    child.spawnargs = throughShell ? [line] : [String(command)].concat(arguments_.map(String));

    try {
      __cp_spawn(id, line, throughShell ? '[]' : encodeArguments(arguments_), cwd, env, throughShell, stdioMode(settings.stdio));
    } catch (error) {
      // Node reports a failed spawn as an 'error' event on the returned object, not as a throw, and
      // the object still has its three streams (empty ones), which callers attach listeners to.
      const m = /^([A-Z][A-Z0-9]{1,15}): /.exec(String(error && error.message));
      const failure = error instanceof Error ? error : new Error(String(error));
      if (failure.code === undefined) failure.code = m ? m[1] : 'ENOENT';
      failure.errno = failure.code === 'ENOENT' ? -2 : failure.errno;
      failure.syscall = 'spawn ' + String(command);
      // Node's text: 'spawn <command> ENOENT', and a stack that starts with it (callers log the stack).
      failure.message = 'spawn ' + String(command) + ' ' + failure.code;
      failure.stack = 'Error: ' + failure.message + '\n' + String(failure.stack || '').split('\n').filter((l) => /^\s+at /.test(l)).join('\n');
      failure.path = String(command);
      failure.spawnargs = arguments_.map(String);
      const empty = () => new Readable({ read() { this.push(null); } });
      child.stdout = empty();
      child.stderr = empty();
      child.stdin = new Writable({ write(chunk, enc, done) { done(); } });
      queueMicrotask(() => {
        child.emit('error', failure);
        child.emit('close', -2, null);
      });
      return child;
    }

    const inherited = stdioMode(settings.stdio) === 'inherit';
    if (inherited) {
      // The child writes to this process's own descriptors; reading them back is not possible.
      const empty = () => new Readable({ read() { this.push(null); } });
      child.stdout = empty();
      child.stderr = empty();
    } else {
      child.stdout = reader(id, 'stdout');
      child.stderr = reader(id, 'stderr');
    }
    child.stdin = writer(id);
    child.pid = -1;

    __cp_wait(id).then(
      (report) => {
        const status = JSON.parse(report);
        if (status.signal !== null && status.signal !== undefined && /^\d+$/.test(String(status.signal))) {
          const names = globalThis.__os.constants.signals;
          const found = Object.keys(names).find((name) => names[name] === Number(status.signal));
          if (found) status.signal = found;
        }
        child.exitCode = status.code;
        child.signalCode = status.signal;
        child.emit('exit', status.code, status.signal);
        // 'close' comes after the pipes are done, which is what lets a caller read the last bytes
        // out of stdout without racing the exit.
        // An inherited stream was never a pipe of ours: nothing was written into it, so waiting
        // for it to end would be waiting for an event that has no reason to come.
        const pending = [];
        if (inherited) {
          // nothing to drain
        } else {
          if (child.stdout && !child.stdout.readableEnded) pending.push(ended(child.stdout));
          if (child.stderr && !child.stderr.readableEnded) pending.push(ended(child.stderr));
        }
        Promise.all(pending).then(() => {
          __cp_close(id);
          child.emit('close', status.code, status.signal);
        });
      },
      (error) => {
        __cp_close(id);
        child.emit('error', error);
      },
    );

    child.emit('spawn');
    return child;
  }

  function ended(readable) {
    return new Promise((resolve) => {
      if (readable.readableEnded) return resolve();
      readable.once('end', () => resolve());
      readable.once('close', () => resolve());
      readable.once('error', () => resolve());
      readable.resume();
    });
  }

  // Collect everything a child says, then hand it to the callback the way Node does.
  function collect(child, options, callback) {
    const encoding = options && options.encoding ? String(options.encoding) : null;
    const out = [];
    const err = [];
    child.stdout.on('data', (chunk) => out.push(chunk));
    child.stderr.on('data', (chunk) => err.push(chunk));
    child.on('error', (error) => callback(error, null, null));
    child.on('close', (code, signal) => {
      const stdout = decode(concat(out), encoding);
      const stderr = decode(concat(err), encoding);
      if (code !== 0 || signal !== null) {
        const error = new Error(
          'Command failed: ' + child.spawnargs.join(' ') + (stderr === '' ? '' : '\n' + stderr),
        );
        error.code = code;
        error.signal = signal;
        error.stdout = stdout;
        error.stderr = stderr;
        callback(error, stdout, stderr);
        return;
      }
      callback(null, stdout, stderr);
    });
  }

  function concat(chunks) {
    let length = 0;
    for (const chunk of chunks) length += chunk.length;
    const all = Buffer.alloc(length);
    let at = 0;
    for (const chunk of chunks) {
      all.set(chunk, at);
      at += chunk.length;
    }
    return all;
  }

  function decode(bytes, encoding) {
    if (encoding === null || encoding === 'buffer') return bytes;
    return textOf(bytes);
  }

  function exec(command, options, callback) {
    let settings = options;
    let done = callback;
    if (typeof settings === 'function') {
      done = settings;
      settings = undefined;
    }
    if (typeof done !== 'function') throw typeError('exec needs a callback');
    const child = spawn(String(command), [], Object.assign({}, settings, { shell: true }));
    collect(child, settings, done);
    return child;
  }

  function execFile(file, args, options, callback) {
    let arguments_ = args;
    let settings = options;
    let done = callback;
    if (typeof arguments_ === 'function') {
      done = arguments_;
      arguments_ = [];
      settings = undefined;
    } else if (typeof settings === 'function') {
      done = settings;
      settings = undefined;
    }
    if (typeof done !== 'function') throw typeError('execFile needs a callback');
    const child = spawn(String(file), arguments_ || [], settings);
    collect(child, settings, done);
    return child;
  }

  // The blocking calls: the host runs the command to its end and answers with everything it said.
  function capture(command, args, options, throughShell) {
    const settings = options || {};
    const cwd = settings.cwd === undefined ? '' : String(settings.cwd);
    const env = JSON.stringify(settings.env || {});
    const maxBuffer = settings.maxBuffer === undefined ? 1024 * 1024 : Number(settings.maxBuffer);
    const line = throughShell
      ? [String(command)].concat((args || []).map(String)).join(' ')
      : String(command);
    const report = JSON.parse(
      __cp_capture(line, throughShell ? '[]' : encodeArguments(args || []), cwd, env, throughShell, maxBuffer),
    );
    if (report.overflow) {
      const error = new Error('maxBuffer length exceeded');
      error.code = 'ERR_CHILD_PROCESS_STDIO_MAXBUFFER';
      throw error;
    }
    const stdout = bytesOf(report.stdout);
    const stderr = bytesOf(report.stderr);
    const encoding = settings.encoding ? String(settings.encoding) : null;
    const output = {
      status: report.status.code,
      signal: report.status.signal,
      stdout: decode(stdout, encoding),
      stderr: decode(stderr, encoding),
    };
    if (settings.failOnError !== false && (output.status !== 0 || output.signal !== null)) {
      const error = new Error('Command failed: ' + line + (output.stderr === '' ? '' : '\n' + textOf(stderr)));
      error.status = output.status;
      error.signal = output.signal;
      error.stdout = output.stdout;
      error.stderr = output.stderr;
      throw error;
    }
    return output;
  }

  function execSync(command, options) {
    return capture(command, [], options, true).stdout;
  }

  function execFileSync(file, args, options) {
    let arguments_ = args;
    let settings = options;
    if (settings === undefined && args !== undefined && !Array.isArray(args)) {
      settings = args;
      arguments_ = [];
    }
    return capture(file, arguments_ || [], settings, false).stdout;
  }

  function spawnSync(command, args, options) {
    let arguments_ = args;
    let settings = options;
    if (settings === undefined && args !== undefined && !Array.isArray(args)) {
      settings = args;
      arguments_ = [];
    }
    settings = Object.assign({}, settings, { failOnError: false });
    const throughShell = settings.shell === true;
    const output = capture(command, arguments_ || [], settings, throughShell);
    return {
      pid: -1,
      status: output.status,
      signal: output.signal,
      stdout: output.stdout,
      stderr: output.stderr,
      output: [null, output.stdout, output.stderr],
    };
  }

  const api = {
    spawn,
    exec,
    execFile,
    execSync,
    execFileSync,
    spawnSync,
    ChildProcess,
  };
  api.default = api;
  globalThis.__child_process_module = api;
})(globalThis);

// `$`: run a command, the way zx's `$` does it, with the pieces this runtime has.
//
// A template is one line of shell: pieces are literal, interpolated values are quoted for that
// shell, and the whole line goes to the platform's own shell. The native side owns the process
// (src/host/exec.rs); this file owns the quoting and the promises.
//
// What this first version does not have: piping (`| grep foo`), stdin, and the `cd`/`within`
// helpers. A command that needs input writes it with the shell's own redirection.

(function (global) {
  'use strict';

  const WINDOWS = typeof process === 'object' && process !== null && process.platform === 'win32';
  const RAW = Symbol('raw');

  let ids = 1;

  class ProcessOutput {
    constructor(stdout, stderr, exitCode, signal) {
      this.stdout = stdout;
      this.stderr = stderr;
      this.exitCode = exitCode;
      this.signal = signal;
    }

    get ok() {
      return this.exitCode === 0;
    }

    toString() {
      return this.stdout;
    }
  }

  class ProcessError extends Error {
    constructor(command, output, reason) {
      const how = reason ?? (output.signal === null
        ? 'exited with ' + output.exitCode
        : 'was killed by signal ' + output.signal);
      super(command + ' ' + how);
      this.name = 'ProcessError';
      this.command = command;
      this.stdout = output.stdout;
      this.stderr = output.stderr;
      this.exitCode = output.exitCode;
      this.signal = output.signal;
    }
  }

  // One value, made safe for the shell that will parse the line.
  function quote(value) {
    const text = String(value);
    if (WINDOWS) {
      if (text === '') return '""';
      if (!/[ \t"&|<>^()%!]/.test(text)) return text;
      return '"' + text.split('"').join('""') + '"';
    }
    return "'" + text.split("'").join("'\\''") + "'";
  }

  function textOf(chunks) {
    let length = 0;
    for (const chunk of chunks) length += chunk.length;
    if (length === 0) return '';
    const all = new Uint8Array(length);
    let at = 0;
    for (const chunk of chunks) {
      all.set(chunk, at);
      at += chunk.length;
    }
    return new TextDecoder().decode(all);
  }

  class ProcessPromise {
    constructor(command) {
      this._command = command;
      this._options = {};
      this._promise = null;
    }

    nothrow() {
      this._options.nothrow = true;
      return this;
    }

    quiet() {
      this._options.quiet = true;
      return this;
    }

    timeout(ms) {
      this._options.timeout = Number(ms);
      return this;
    }

    cwd(directory) {
      this._options.cwd = String(directory);
      return this;
    }

    env(environment) {
      this._options.env = Object.assign({}, this._options.env, environment);
      return this;
    }

    then(onFulfilled, onRejected) {
      return this._run().then(onFulfilled, onRejected);
    }

    catch(onRejected) {
      return this._run().catch(onRejected);
    }

    finally(onFinally) {
      return this._run().finally(onFinally);
    }

    _run() {
      if (this._promise === null) this._promise = run(this._command, this._options);
      return this._promise;
    }
  }

  async function run(command, options) {
    const quiet = options.quiet === true || $.quiet === true;
    if (!quiet && $.verbose === true) console.log(command);

    const id = ids++;
    const cwd = options.cwd === undefined ? ($.cwd ?? '') : String(options.cwd);
    const environment = Object.assign({}, $.env, options.env);

    __exec_spawn(id, command, cwd, JSON.stringify(environment));

    let timedOut = false;
    let timer = 0;
    if (options.timeout) {
      timer = setTimeout(() => {
        timedOut = true;
        try {
          __exec_kill(id);
        } catch (error) {
          // The command finished on its own between the timer and the kill; nothing to stop.
        }
      }, options.timeout);
    }

    // Both pipes are read while the command runs: a command that fills a pipe before we ask for
    // its output would otherwise wait forever for a reader that is waiting for it to exit.
    const collected = { stdout: [], stderr: [] };
    const reading = (which) => (async () => {
      for (;;) {
        const chunk = await __exec_read(id, which);
        if (typeof chunk === 'string') return;
        collected[which].push(new Uint8Array(chunk));
      }
    })();

    let status;
    try {
      const waiting = __exec_wait(id);
      await Promise.all([reading('stdout'), reading('stderr'), waiting]);
      status = JSON.parse(await waiting);
    } finally {
      if (timer !== 0) clearTimeout(timer);
    }

    __exec_close(id);

    const output = new ProcessOutput(
      textOf(collected.stdout),
      textOf(collected.stderr),
      status.code,
      status.signal,
    );

    if (!quiet && $.verbose === true) {
      if (output.stdout !== '') console.log(output.stdout.replace(/\n$/, ''));
      if (output.stderr !== '') console.error(output.stderr.replace(/\n$/, ''));
    }

    if (timedOut) throw new ProcessError(command, output, 'timed out after ' + options.timeout + 'ms');
    if (!output.ok && options.nothrow !== true) throw new ProcessError(command, output);
    return output;
  }

  function $(pieces) {
    const values = Array.prototype.slice.call(arguments, 1);
    let command = '';
    for (let i = 0; i < pieces.length; i++) {
      command += pieces[i];
      if (i >= values.length) continue;
      const value = values[i];
      if (value !== null && typeof value === 'object' && value[RAW] === true) command += value.text;
      else command += quote(value);
    }
    return new ProcessPromise(command);
  }

  // A value that goes into the line as it is. For the pieces quoting would break: a pipe, a
  // redirection, a file name the shell already knows.
  $.raw = function (value) {
    return { [RAW]: true, text: String(value) };
  };

  $.quote = quote;
  $.shell = WINDOWS ? 'cmd' : '/bin/sh';
  $.cwd = null;
  $.env = {};
  $.verbose = true;
  $.quiet = false;
  $.ProcessOutput = ProcessOutput;
  $.ProcessError = ProcessError;
  $.ProcessPromise = ProcessPromise;

  global.$ = $;
})(globalThis);

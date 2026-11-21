// `$`: what zx gives a script, on top of what this runtime has.
//
// The global is a function and a preset at once: `$`line``` runs a line, `$({...})` makes a new `$`
// with other defaults, and `$.cwd` / `$.env` / `$.verbose` are the defaults it carries. Around it
// are the helpers zx documents: cd, within, syncProcessCwd, sleep, echo, stdin, question, retry,
// spinner, glob, which, tmpdir, tmpfile, minimist, argv, dotenv, chalk, fs, os, path, quote, ps,
// kill, useBash and friends.
//
// A template is one line of shell, and everything between the backticks is the line as the shell
// sees it: pipes, redirection and globs work, because the platform's own shell parses it. Values
// from the template are quoted for that shell, so a value with spaces or a semicolon goes in as one
// word. `$.raw(text)` is for the pieces quoting would break.
//
// Where it differs from zx is written down in docs/exec.md; the two that matter here: a pipe into
// another command buffers the left side (nothing streams yet), and `within` cannot follow an async
// chain the way AsyncLocalStorage does, so it restores its defaults when its callback's promise
// settles and any other task running meanwhile sees them too.

(function (global) {
  'use strict';

  const WINDOWS = typeof process === 'object' && process !== null && process.platform === 'win32';
  const RAW = Symbol('raw');

  let ids = 1;

  // ---------- quoting ----------

  function quote(value) {
    const text = String(value);
    if (WINDOWS) {
      if (text === '') return '""';
      if (!/[ \t"&|<>^()%!]/.test(text)) return text;
      return '"' + text.split('"').join('""') + '"';
    }
    return "'" + text.split("'").join("'\\''") + "'";
  }

  function quotePowerShell(value) {
    return "'" + String(value).split("'").join("''") + "'";
  }

  // ---------- what a command answers ----------

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

    get message() {
      return this.stderr || this.stdout;
    }

    toString() {
      return this.stdout;
    }
  }

  class ProcessError extends Error {
    constructor(command, output, reason) {
      const how = reason !== undefined && reason !== null
        ? reason
        : output.signal === null
          ? 'exited with ' + output.exitCode
          : 'was killed by signal ' + output.signal;
      super(command + ' ' + how);
      this.name = 'ProcessError';
      this.command = command;
      this.stdout = output.stdout;
      this.stderr = output.stderr;
      this.exitCode = output.exitCode;
      this.signal = output.signal;
      this.ok = false;
    }
  }

  // Both halves of a run: the line as it will be written, and the settings it runs with.
  class ProcessPromise {
    constructor(command, options, preset) {
      this._command = command;
      this._options = options || {};
      this._preset = preset;
      this._promise = null;
      this._started = false;
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
      this._options.timeout = duration(ms);
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

    stdin(input) {
      this._options.input = input;
      return this;
    }

    // `left.pipe`right`` and `left.pipe(right)`: the left side runs first and everything it said
    // becomes the right side's input. Buffered, not streamed — see docs/exec.md.
    pipe(destination) {
      let next = destination;
      let values = [];
      if (Array.isArray(destination)) {
        next = this._preset.apply(null, arguments);
        values = null;
      }
      if (!(next instanceof ProcessPromise)) throw new TypeError('pipe wants a ProcessPromise or a tag');
      const source = this;
      const chained = new ProcessPromise(next._command, next._options, next._preset);
      // The right side is a job of its own rather than next._run(): the promise this returns is
      // what _run() would hand back, and a promise cannot resolve with itself.
      chained._promise = (async () => {
        const produced = await source._run();
        const settings = Object.assign({}, next._options, { input: produced.stdout });
        return run(new ProcessPromise(next._command, settings, next._preset));
      })();
      return chained;
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
      if (this._promise === null) this._promise = run(this);
      return this._promise;
    }
  }

  // '1s', '500ms', '2m' and milliseconds: zx's Duration.
  function duration(value) {
    if (typeof value === 'number') return value;
    const match = /^([0-9.]+)\s*(ms|s|m|h)?$/.exec(String(value).trim());
    if (match === null) throw new TypeError('not a duration: ' + String(value));
    const scale = { ms: 1, s: 1000, m: 60000, h: 3600000 }[match[2] || 'ms'];
    return Number(match[1]) * scale;
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

  function bytesOf(base64) {
    return new Uint8Array(__text_to_bytes(base64, 'base64'));
  }

  function inputOf(value) {
    if (value === undefined || value === null) return undefined;
    if (typeof value === 'string') return Buffer.from(value, 'utf8');
    if (value instanceof ProcessOutput) return Buffer.from(value.stdout, 'utf8');
    if (value instanceof ProcessPromise) {
      throw new TypeError('input: await the command first');
    }
    if (value instanceof Uint8Array) return value;
    return Buffer.from(String(value), 'utf8');
  }

  // ---------- running one line ----------

  function environment(preset, options) {
    const merged = Object.assign({}, preset.env, options.env);
    if (options.preferLocal === false || preset.preferLocal === false) return merged;
    // zx's preferLocal: node_modules/.bin of the working directory first on PATH.
    const directory = options.cwd !== undefined ? String(options.cwd) : preset.cwd;
    if (!directory) return merged;
    const local = directory.replace(/\/+$/, '') + '/node_modules/.bin';
    const existing = merged.PATH !== undefined ? merged.PATH : merged.Path;
    if (existing !== undefined && String(existing).split(WINDOWS ? ';' : ':')[0] === local) return merged;
    const separator = WINDOWS ? ';' : ':';
    const current = existing !== undefined ? separator + String(existing) : '';
    const key = merged.PATH !== undefined || WINDOWS || merged.Path === undefined ? 'PATH' : 'Path';
    merged[key] = local + current;
    return merged;
  }

  async function run(job) {
    const preset = job._preset;
    const options = job._options;
    const command = job._command;
    const quiet = options.quiet === true || preset.quiet === true;
    if (!quiet && preset.verbose === true) console.log(command);

    const id = ids++;
    const cwd = options.cwd === undefined ? (preset.cwd === null ? '' : String(preset.cwd)) : String(options.cwd);
    const env = environment(preset, options);
    const input = inputOf(options.input !== undefined ? options.input : preset.input);

    __exec_spawn(id, command, cwd, JSON.stringify(env), input !== undefined);

    let timedOut = false;
    let timer = 0;
    const deadline = options.timeout !== undefined ? duration(options.timeout) : preset.timeout;
    if (deadline !== undefined && deadline > 0) {
      timer = setTimeout(() => {
        timedOut = true;
        try {
          __exec_kill(id);
        } catch (error) {
          // The command finished on its own between the timer and the kill; nothing to stop.
        }
      }, deadline);
    }

    // Both pipes are read while the command runs: a command that fills a pipe before we ask for its
    // output would otherwise wait forever for a reader that is waiting for it to exit.
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
      const writes = input !== undefined
        ? __exec_write(id, input).then(() => { __exec_end(id); }, () => { __exec_end(id); })
        : Promise.resolve();
      await Promise.all([reading('stdout'), reading('stderr'), waiting, writes]);
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

    if (!quiet && preset.verbose === true) {
      if (output.stdout !== '') console.log(output.stdout.replace(/\n$/, ''));
      if (output.stderr !== '') console.error(output.stderr.replace(/\n$/, ''));
    }

    if (timedOut) throw new ProcessError(command, output, 'timed out after ' + deadline + 'ms');
    if (!output.ok && options.nothrow !== true && preset.nothrow !== true) {
      throw new ProcessError(command, output);
    }
    return output;
  }

  // $.sync: the same line, run to its end before this returns.
  function runSync(job) {
    const preset = job._preset;
    const options = job._options;
    const command = job._command;
    const quiet = options.quiet === true || preset.quiet === true;
    if (!quiet && preset.verbose === true) console.log(command);

    const cwd = options.cwd === undefined ? (preset.cwd === null ? '' : String(preset.cwd)) : String(options.cwd);
    const env = environment(preset, options);
    const input = inputOf(options.input !== undefined ? options.input : preset.input);
    const report = JSON.parse(__exec_capture(command, cwd, JSON.stringify(env), input));
    const output = new ProcessOutput(
      new TextDecoder().decode(bytesOf(report.stdout)),
      new TextDecoder().decode(bytesOf(report.stderr)),
      report.status.code,
      report.status.signal,
    );
    if (!quiet && preset.verbose === true) {
      if (output.stdout !== '') console.log(output.stdout.replace(/\n$/, ''));
      if (output.stderr !== '') console.error(output.stderr.replace(/\n$/, ''));
    }
    if (!output.ok && options.nothrow !== true && preset.nothrow !== true) {
      throw new ProcessError(command, output);
    }
    return output;
  }

  // ---------- the $` presets ----------

  const DEFAULTS = {
    cwd: null,
    env: {},
    verbose: true,
    quiet: false,
    nothrow: false,
    timeout: undefined,
    shell: WINDOWS ? 'cmd' : '/bin/sh',
    quote: quote,
    prefix: '',
    postfix: '',
    preferLocal: true,
    halt: false,
    input: undefined,
    spawn: null,
    log: null,
  };

  function create$(defaults) {
    const preset = Object.assign({}, defaults);

    function tag(pieces) {
      // `$({...})`: a new `$` with these defaults, and an object is not a template.
      if (!Array.isArray(pieces)) {
        return create$(Object.assign({}, preset, pieces || {}));
      }
      const values = Array.prototype.slice.call(arguments, 1);
      let command = preset.prefix + pieces[0];
      for (let i = 0; i < values.length; i++) {
        const value = values[i];
        if (value !== null && typeof value === 'object' && value[RAW] === true) command += value.text;
        else if (value instanceof ProcessOutput) command += preset.quote(value.stdout.replace(/\n$/, ''));
        else command += preset.quote(value);
        command += pieces[i + 1];
      }
      command += preset.postfix;
      return new ProcessPromise(command, {}, tag);
    }

    tag.sync = function (pieces) {
      const values = Array.prototype.slice.call(arguments, 1);
      const job = tag.apply(null, [pieces].concat(values));
      return runSync(job);
    };

    const writable = ['cwd', 'env', 'verbose', 'quiet', 'nothrow', 'timeout', 'prefix', 'postfix', 'preferLocal', 'halt', 'input'];
    for (const name of writable) {
      Object.defineProperty(tag, name, {
        get() { return preset[name]; },
        set(value) { preset[name] = value; },
        enumerable: true,
      });
    }
    Object.defineProperty(tag, 'quote', {
      get() { return preset.quote; },
      set(value) { preset.quote = value; },
      enumerable: true,
    });
    Object.defineProperty(tag, 'shell', {
      get() { return preset.shell; },
      set(value) {
        preset.shell = value;
        // The shell is chosen by the platform, so a preset that sets one is only a promise about
        // what a line means; the host still runs sh -c or cmd /C.
      },
      enumerable: true,
    });

    tag.raw = function (value) {
      return { [RAW]: true, text: String(value) };
    };
    tag.ProcessOutput = ProcessOutput;
    tag.ProcessError = ProcessError;
    tag.ProcessPromise = ProcessPromise;
    tag.versions = { xmas: '0.10.0' };
    return tag;
  }

  const $ = create$(DEFAULTS);

  // A line built in code rather than from a template: same quoting, same preset.
  function line(text) {
    const pieces = [text];
    pieces.raw = pieces.slice();
    return $.apply(null, [pieces]);
  }

  // ---------- cd, within, syncProcessCwd ----------

  let followingCwd = false;

  function cd(directory) {
    // zx accepts a command's output here: cd(await $`mktemp -d`) with the newline trimmed.
    const value = directory instanceof ProcessOutput ? directory.stdout : String(directory);
    $.cwd = value.replace(/\n$/, '');
    if (followingCwd && typeof process === 'object' && process !== null && typeof process.chdir === 'function') {
      process.chdir($.cwd);
    }
  }

  function syncProcessCwd(enabled) {
    followingCwd = enabled !== false;
  }

  // Restore the defaults when the callback's promise settles. Not an async context: another task
  // running while that promise is pending sees these settings too, which is the one difference from
  // zx's within (it has AsyncLocalStorage, this runtime does not).
  function within(callback) {
    const saved = { cwd: $.cwd, env: Object.assign({}, $.env), verbose: $.verbose, quiet: $.quiet };
    const restore = () => {
      $.cwd = saved.cwd;
      $.env = saved.env;
      $.verbose = saved.verbose;
      $.quiet = saved.quiet;
    };
    let result;
    try {
      result = callback();
    } catch (error) {
      restore();
      throw error;
    }
    if (result !== null && typeof result === 'object' && typeof result.then === 'function') {
      return result.then(
        (value) => { restore(); return value; },
        (error) => { restore(); throw error; },
      );
    }
    restore();
    return result;
  }

  // ---------- small helpers ----------

  function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, duration(ms)));
  }

  function echo(first) {
    let text;
    if (Array.isArray(first) && first.raw) {
      text = first[0];
      for (let i = 1; i < arguments.length; i++) text += echoText(arguments[i]) + first[i];
    } else {
      const parts = [];
      for (let i = 0; i < arguments.length; i++) parts.push(echoText(arguments[i]));
      text = parts.join(' ');
    }
    console.log(text.replace(/\n$/, ''));
  }

  function echoText(value) {
    if (value instanceof ProcessOutput) return value.stdout.replace(/\n$/, '');
    if (value instanceof Error) return value.message;
    return String(value);
  }

  async function stdin() {
    const bytes = await __stdin_read_all();
    return new TextDecoder().decode(new Uint8Array(bytes));
  }

  // One line from this process's input. No raw mode and no key handling: there is no termios here,
  // so the terminal itself does the line editing and echoes what is typed.
  async function question(query, options) {
    const settings = options || {};
    if (query !== undefined && query !== null && query !== '') console.log(String(query));
    const choices = settings.choices;
    if (Array.isArray(choices) && choices.length > 0) {
      for (let i = 0; i < choices.length; i++) console.log('  ' + (i + 1) + ') ' + echoText(choices[i]));
    }
    for (;;) {
      const line = await __stdin_read_line();
      if (typeof line === 'string' && line === 'eof') return null;
      const answer = String(line).replace(/\r?\n$/, '');
      if (!Array.isArray(choices) || choices.length === 0) return answer;
      const picked = choices.find((choice, index) => echoText(choice) === answer || String(index + 1) === answer);
      if (picked !== undefined) return echoText(picked);
      console.log('Pick one of: ' + choices.map(echoText).join(', '));
    }
  }

  // 1s, 2s, 4s, ... for retry.
  function expBackoff() {
    let attempt = 0;
    return () => {
      attempt += 1;
      return Math.min(1000 * Math.pow(2, attempt - 1), 30000);
    };
  }

  async function retry(attempts, delay, callback) {
    let count = attempts;
    let wait = delay;
    let job = callback;
    if (typeof wait === 'function') {
      job = wait;
      wait = 0;
    }
    if (typeof job !== 'function') throw new TypeError('retry wants a callback');
    let last;
    for (let attempt = 0; attempt < count; attempt++) {
      try {
        return await job();
      } catch (error) {
        last = error;
        if (attempt + 1 === count) break;
        const pause = typeof wait === 'function' ? wait(attempt) : wait;
        if (pause > 0) await sleep(pause);
      }
    }
    throw last;
  }

  // A spinner on stderr while the callback runs; nothing at all when the output is not a terminal.
  async function spinner(message, callback) {
    let text = message;
    let job = callback;
    if (typeof text === 'function') {
      job = text;
      text = '';
    }
    if (typeof job !== 'function') throw new TypeError('spinner wants a callback');
    const frames = ['-', '\\\\', '|', '/'];
    const interactive = typeof process === 'object' && process !== null
      && process.env !== undefined && process.env.CI === undefined
      && typeof __is_tty === 'function' && __is_tty(2);
    if (!interactive) return job();
    let frame = 0;
    const draw = () => {
      process.stderr.write('\r' + frames[frame] + ' ' + text);
      frame = (frame + 1) % frames.length;
    };
    draw();
    const timer = setInterval(draw, 100);
    try {
      return await job();
    } finally {
      clearInterval(timer);
      process.stderr.write('\r' + ' '.repeat(text.length + 2) + '\r');
    }
  }

  // ---------- glob ----------

  // zx's glob is globby; this is the part of it a script uses: *, **, ?, [abc] and the brace form
  // {a,b}. Patterns are matched against paths relative to the working directory.
  function globToRegExp(pattern) {
    let out = '^';
    let at = 0;
    while (at < pattern.length) {
      const character = pattern[at];
      if (character === '*') {
        if (pattern[at + 1] === '*') {
          if (pattern[at + 2] === '/') {
            out += '(?:[^/]+/)*';
            at += 3;
          } else {
            out += '.*';
            at += 2;
          }
          continue;
        }
        out += '[^/]*';
        at += 1;
        continue;
      }
      if (character === '?') {
        out += '[^/]';
        at += 1;
        continue;
      }
      if (character === '[') {
        const close = pattern.indexOf(']', at + 1);
        if (close > at) {
          out += pattern.slice(at, close + 1);
          at = close + 1;
          continue;
        }
      }
      if (/[A-Za-z0-9_\\/-]/.test(character)) out += character;
      else out += '\\' + character;
      at += 1;
    }
    return new RegExp(out + '$');
  }

  function expandBraces(pattern) {
    const open = pattern.indexOf('{');
    if (open < 0) return [pattern];
    const close = pattern.indexOf('}', open);
    if (close < 0) return [pattern];
    const head = pattern.slice(0, open);
    const tail = pattern.slice(close + 1);
    const out = [];
    for (const choice of pattern.slice(open + 1, close).split(',')) {
      for (const expanded of expandBraces(head + choice + tail)) out.push(expanded);
    }
    return out;
  }

  // How many entries ** visits before it gives up: a project is a fine place to glob, a home
  // directory with a symlink loop is not.
  const GLOB_LIMIT = 20000;

  function walk(from, base, visited, found, matchers) {
    if (visited.count > GLOB_LIMIT) return;
    let entries;
    try {
      entries = globalThis.__fs_sync.readdirSync(from, { withFileTypes: true });
    } catch (error) {
      return;
    }
    for (const entry of entries) {
      if (entry.name === 'node_modules' || entry.name === '.git') continue;
      const full = from.replace(/\/+$/, '') + '/' + entry.name;
      const relative = base === '' ? entry.name : base + '/' + entry.name;
      visited.count += 1;
      for (const matcher of matchers) if (matcher.test(relative)) found.add(relative);
      if (entry.isDirectory()) walk(full, relative, visited, found, matchers);
    }
  }

  function globSync(patterns, options) {
    const list = Array.isArray(patterns) ? patterns : [patterns];
    const settings = options || {};
    const directory = settings.cwd === undefined
      ? ($.cwd === null ? process.cwd() : $.cwd)
      : String(settings.cwd);
    const matchers = [];
    for (const pattern of list) {
      for (const expanded of expandBraces(String(pattern))) {
        matchers.push(globToRegExp(expanded.replace(/^\.\//, '')));
      }
    }
    const found = new Set();
    walk(directory, '', { count: 0 }, found, matchers);
    const results = Array.from(found).sort();
    if (settings.absolute === true) {
      return results.map((relative) => directory.replace(/\/+$/, '') + '/' + relative);
    }
    return results;
  }

  function glob(patterns, options) {
    return Promise.resolve().then(() => globSync(patterns, options));
  }

  glob.sync = globSync;

  // ---------- which ----------

  function whichSync(program, options) {
    const settings = options || {};
    const name = String(program);
    const separator = WINDOWS ? ';' : ':';
    const extensions = WINDOWS ? ['.exe', '.cmd', '.bat', ''] : [''];
    const directories = [];
    const local = settings.preferLocal === false ? null : ($.preferLocal === false ? null : ($.cwd === null ? process.cwd() : $.cwd));
    if (local !== null) directories.push(local.replace(/\/+$/, '') + '/node_modules/.bin');
    const search = process.env.PATH !== undefined ? process.env.PATH : (process.env.Path || '');
    for (const entry of String(search).split(separator)) if (entry !== '') directories.push(entry);
    for (const directory of directories) {
      for (const extension of extensions) {
        const candidate = directory.replace(/\/+$/, '') + '/' + name + extension;
        try {
          if (globalThis.__fs_sync.statSync(candidate).isFile()) return candidate;
        } catch (error) {
          // Not in this directory; the next one.
        }
      }
    }
    if (settings.nothrow === true) return null;
    throw new Error('not found: ' + name);
  }

  function which(program, options) {
    return Promise.resolve().then(() => whichSync(program, options));
  }

  which.sync = whichSync;

  // ---------- tmpdir and tmpfile ----------

  let temporary = null;

  function temporaryDirectory() {
    if (temporary === null) {
      temporary = __os.tmpdir().replace(/\/+$/, '') + '/xmas-' + Math.random().toString(36).slice(2, 12);
      globalThis.__fs_sync.mkdirSync(temporary, { recursive: true });
    }
    return temporary;
  }

  function tmpdir() {
    let directory = temporaryDirectory();
    for (let i = 0; i < arguments.length; i++) {
      directory = directory.replace(/\/+$/, '') + '/' + String(arguments[i]);
      globalThis.__fs_sync.mkdirSync(directory, { recursive: true });
    }
    return directory.replace(/\/+$/, '') + '/';
  }

  function tmpfile(name, data) {
    const directory = temporaryDirectory();
    const path = directory + '/' + (name === undefined ? Math.random().toString(36).slice(2, 12) : String(name));
    globalThis.__fs_sync.writeFileSync(path, data === undefined ? '' : data);
    return path;
  }

  // ---------- minimist, and argv ----------

  // The rules of minimist, which zx hands back as argv: --name value, --name=value, --flag (true),
  // --no-flag (false), -abc (a, b, c), -n 5, and everything after -- as _.
  function minimist(args, options) {
    const settings = options || {};
    const booleans = new Set();
    const strings = new Set();
    const aliases = settings.alias || {};
    const toList = (value) => (value === undefined ? [] : Array.isArray(value) ? value : [value]);
    for (const name of toList(settings.boolean)) booleans.add(name);
    for (const name of toList(settings.string)) strings.add(name);
    for (const key of Object.keys(aliases)) {
      for (const other of toList(aliases[key])) {
        if (booleans.has(other)) booleans.add(key);
        if (strings.has(other)) strings.add(key);
      }
    }
    const looked = (name) => booleans.has(name) || strings.has(name);
    const out = { _: [] };
    const set = (name, value) => {
      out[name] = value;
      for (const other of toList(aliases[name])) out[other] = value;
    };
    const list = Array.isArray(args) ? args : [];
    let at = 0;
    while (at < list.length) {
      const argument = String(list[at]);
      if (argument === '--') {
        at += 1;
        while (at < list.length) out._.push(list[at++]);
        break;
      }
      if (argument.slice(0, 2) === '--') {
        const body = argument.slice(2);
        const equals = body.indexOf('=');
        if (equals >= 0) {
          set(body.slice(0, equals), coerce(body.slice(equals + 1)));
          at += 1;
          continue;
        }
        if (body.slice(0, 3) === 'no-' && !looked(body) && !looked(body.slice(3))) {
          set(body.slice(3), false);
          at += 1;
          continue;
        }
        if (booleans.has(body)) {
          set(body, true);
          at += 1;
          continue;
        }
        const value = list[at + 1];
        if (value === undefined || String(value).slice(0, 1) === '-') {
          set(body, looked(body) ? '' : true);
          at += 1;
          continue;
        }
        set(body, coerce(value));
        at += 2;
        continue;
      }
      if (argument.slice(0, 1) === '-' && argument.length > 1) {
        const letters = argument.slice(1);
        if (letters.length === 1 && !booleans.has(letters)) {
          const value = list[at + 1];
          if (value === undefined || String(value).slice(0, 1) === '-') {
            set(letters, strings.has(letters) ? '' : true);
            at += 1;
            continue;
          }
          set(letters, coerce(value));
          at += 2;
          continue;
        }
        if (looked(letters)) {
          const value = list[at + 1];
          if (value === undefined || String(value).slice(0, 1) === '-') {
            set(letters, booleans.has(letters) ? true : '');
            at += 1;
            continue;
          }
          set(letters, coerce(value));
          at += 2;
          continue;
        }
        for (const letter of letters) set(letter, true);
        at += 1;
        continue;
      }
      out._.push(coerce(argument));
      at += 1;
    }
    return out;
  }

  function coerce(value) {
    const text = String(value);
    if (text === 'true') return true;
    if (text === 'false') return false;
    if (/^-?[0-9]+$/.test(text)) return Number(text);
    if (/^-?[0-9]*\.[0-9]+$/.test(text)) return Number(text);
    return text;
  }

  const argv = minimist(process.argv.slice(2));

  // ---------- dotenv ----------

  const dotenv = {
    parse(source) {
      const out = {};
      for (const line of String(source).split(/\r?\n/)) {
        const trimmed = line.trim();
        if (trimmed === '' || trimmed.slice(0, 1) === '#') continue;
        const equals = trimmed.indexOf('=');
        if (equals < 0) continue;
        const name = trimmed.slice(0, equals).trim().replace(/^export\s+/, '');
        let value = trimmed.slice(equals + 1).trim();
        if (value.length >= 2
          && ((value.slice(0, 1) === '"' && value.slice(-1) === '"') || (value.slice(0, 1) === "'" && value.slice(-1) === "'"))) {
          value = value.slice(1, -1);
        }
        out[name] = value;
      }
      return out;
    },
    load(path) {
      return dotenv.parse(globalThis.__fs_sync.readFileSync(path === undefined ? '.env' : path));
    },
    config(path) {
      const values = dotenv.load(path);
      Object.assign(process.env, values);
      return values;
    },
  };

  // ---------- chalk ----------

  // chalk's surface, cut down: named colours, backgrounds and modifiers, chained or called, with
  // rgb()/hex(), and NO_COLOR / FORCE_COLOR / isTTY deciding whether anything is written at all.
  const STYLES = {
    reset: [0], bold: [1], dim: [2], italic: [3], underline: [4], inverse: [7],
    hidden: [8], strikethrough: [9],
    black: [30], red: [31], green: [32], yellow: [33], blue: [34], magenta: [35], cyan: [36], white: [37],
    gray: [90], grey: [90], blackBright: [90], redBright: [91], greenBright: [92], yellowBright: [93],
    blueBright: [94], magentaBright: [95], cyanBright: [96], whiteBright: [97],
    bgBlack: [40], bgRed: [41], bgGreen: [42], bgYellow: [43], bgBlue: [44], bgMagenta: [45],
    bgCyan: [46], bgWhite: [47],
  };

  const colourLevel = (() => {
    if (typeof process !== 'object' || process === null) return 0;
    if (process.env.FORCE_COLOR !== undefined) return process.env.FORCE_COLOR === '0' ? 0 : 1;
    if (process.env.NO_COLOR !== undefined) return 0;
    return process.stdout !== undefined && process.stdout.isTTY === true ? 1 : 0;
  })();

  function paintWith(codes) {
    const paint = (value) => {
      if (colourLevel === 0) return String(value);
      return '\u001b[' + codes.join(';') + 'm' + String(value) + '\u001b[0m';
    };
    const extend = (extra) => paintWith(codes.concat(extra));
    for (const name of Object.keys(STYLES)) {
      Object.defineProperty(paint, name, { get: () => extend(STYLES[name]) });
    }
    Object.defineProperty(paint, 'rgb', { get: () => (r, g, b) => extend(['38;2', r, g, b]) });
    Object.defineProperty(paint, 'bgRgb', { get: () => (r, g, b) => extend(['48;2', r, g, b]) });
    Object.defineProperty(paint, 'hex', {
      get: () => (hex) => {
        const value = String(hex).replace('#', '');
        const whole = parseInt(value.length === 3
          ? value.split('').map((character) => character + character).join('')
          : value, 16);
        return extend(['38;2', (whole >> 16) & 255, (whole >> 8) & 255, whole & 255]);
      },
    });
    Object.defineProperty(paint, 'level', { get: () => colourLevel });
    return paint;
  }

  const chalk = paintWith([]);

  // ---------- ps and kill ----------

  // Both go through the platform's own tools: listing processes and signalling one are the two
  // things this runtime will not do itself, because both mean libc.
  const ps = {
    async lookup(options) {
      const settings = options || {};
      const report = await line('ps -Ao pid,ppid,comm').nothrow();
      const rows = [];
      if (report.ok) {
        for (const row of report.stdout.split('\n')) {
          const trimmed = row.trim();
          if (trimmed === '') continue;
          const parts = trimmed.split(/\s+/);
          if (parts.length < 3 || !/^[0-9]+$/.test(parts[0])) continue;
          rows.push({ pid: Number(parts[0]), ppid: Number(parts[1]), command: parts.slice(2).join(' ') });
        }
      }
      if (settings.command === undefined) return rows;
      const wanted = String(settings.command);
      return rows.filter((row) => row.command.indexOf(wanted) >= 0);
    },
    async tree(options) {
      const settings = options || {};
      const rows = await ps.lookup();
      const out = [];
      const walkFrom = (parent, depth) => {
        for (const row of rows) {
          if (row.ppid !== parent) continue;
          out.push(row);
          if (settings.recursive === true || depth < 1) walkFrom(row.pid, depth + 1);
        }
      };
      walkFrom(Number(settings.pid), 0);
      return out;
    },
  };

  async function kill(pid, signal) {
    const target = Number(pid);
    if (!Number.isFinite(target)) throw new TypeError('kill wants a pid');
    const name = signal === undefined ? 'KILL' : String(signal).replace(/^SIG/, '');
    const report = WINDOWS
      ? await line('taskkill /F /PID ' + target).nothrow()
      : await line('kill -' + name + ' ' + target).nothrow();
    if (!report.ok) throw new ProcessError('kill ' + target, report);
  }

  // ---------- what a shell choice means ----------

  function useBash() {
    $.shell = 'bash';
    $.quote = quote;
  }

  function usePowerShell() {
    $.shell = WINDOWS ? 'powershell' : 'pwsh';
    $.quote = quotePowerShell;
  }

  function usePwsh() {
    $.shell = 'pwsh';
    $.quote = quotePowerShell;
  }

  // ---------- fs, os, path ----------

  const fs = Object.assign({}, globalThis.__fs_sync, globalThis.__fs_promises);
  fs.readJsonSync = (path) => JSON.parse(globalThis.__fs_sync.readFileSync(path));
  fs.writeJsonSync = (path, value, options) => {
    globalThis.__fs_sync.writeFileSync(path, JSON.stringify(value, null, (options && options.spaces) || 2) + '\n');
  };
  fs.ensureDirSync = (path) => globalThis.__fs_sync.mkdirSync(path, { recursive: true });
  fs.readJson = async (path) => JSON.parse(await globalThis.__fs_promises.readFile(path, 'utf8'));
  fs.writeJson = async (path, value, options) => {
    await globalThis.__fs_promises.writeFile(path, JSON.stringify(value, null, (options && options.spaces) || 2) + '\n');
  };
  fs.ensureDir = (path) => globalThis.__fs_promises.mkdir(path, { recursive: true });

  // ---------- the globals ----------

  const exported = {
    $, cd, within, syncProcessCwd, sleep, echo, stdin, question, retry, expBackoff, spinner,
    glob, which, tmpdir, tmpfile, minimist, argv, dotenv, chalk, ps, kill, fs,
    os: globalThis.__os, path: globalThis.path, quote, quotePowerShell,
    useBash, usePowerShell, usePwsh, versions: { xmas: '0.10.0' },
    ProcessOutput, ProcessError, ProcessPromise,
  };

  for (const name of Object.keys(exported)) global[name] = exported[name];

  // Both ways round: a global so a one-file script needs no import, and the same objects as
  // 'xmas:shell' for a script that would rather name where they come from.
  globalThis.__shell_module = exported;
})(globalThis);

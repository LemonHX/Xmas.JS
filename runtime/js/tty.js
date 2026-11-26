(function () {
  if (typeof EventTarget !== 'function') throw new Error('tty must be installed after events');

  const isatty = (fd) => __tty_isatty(Number(fd));

  // Node's colour depth heuristic, cut down to the parts that do not need a
  // terminal database: FORCE_COLOR wins, then NO_COLOR, then TERM.
  const colorDepth = () => {
    const env = (typeof process !== 'undefined' && process.env) || {};
    if (env.FORCE_COLOR !== undefined) {
      const forced = Number(env.FORCE_COLOR);
      return forced >= 3 ? 24 : forced === 2 ? 8 : forced === 1 ? 4 : 1;
    }
    if (env.NO_COLOR !== undefined) return 1;
    const term = String(env.TERM || '');
    if (term === 'dumb') return 1;
    if (env.COLORTERM === 'truecolor' || env.TERM_PROGRAM === 'iTerm.app') return 24;
    if (term.includes('256')) return 8;
    if (term !== '') return 4;
    return 1;
  };

  class WriteStream extends EventTarget {
    constructor(fd = 1, options) {
      super();
      const settings = options || {};
      this.fd = fd;
      this.isTTY = isatty(fd);
      this._options = settings;
      this._target = fd === 1 ? process.stdout : fd === 2 ? process.stderr : null;
      if (this._target === null) throw new Error('tty.WriteStream supports fd 1 and 2');
    }
    get columns() {
      const size = this.getWindowSize();
      return size[0];
    }
    get rows() {
      const size = this.getWindowSize();
      return size[1];
    }
    getWindowSize() {
      const size = JSON.parse(__tty_size(this.fd));
      return size === null ? [0, 0] : size;
    }
    getColorDepth() {
      const options = this && this._options;
      return !options || options.colorDepth === undefined ? colorDepth() : options.colorDepth;
    }
    hasColors(count = 16) {
      const palette = { 1: 2, 4: 16, 8: 256, 24: 16777216 };
      const available = palette[this.getColorDepth()] || 2;
      return available >= Number(count);
    }
    write(chunk, encoding, callback) {
      const done = typeof encoding === 'function' ? encoding : callback;
      this._target.write(chunk, typeof encoding === 'function' ? undefined : encoding, done);
      return true;
    }
    end(chunk) {
      if (chunk !== undefined) this.write(chunk);
      return this;
    }
    // dir 0 is cursor to end of line, 1 start of line to cursor, 2 the whole line.
    clearLine(dir = 0) {
      this._target.write('\u001b[' + (dir === 1 ? 1 : dir === 2 ? 2 : 0) + 'K');
      return this;
    }
    clearScreenDown() {
      this._target.write('\u001b[0J');
      return this;
    }
    cursorTo(x, y) {
      this._target.write('\u001b[' + (Number(y) + 1 || 1) + ';' + (Number(x) + 1) + 'H');
      return this;
    }
    moveCursor(dx, dy) {
      const across = Number(dx) || 0;
      const down = Number(dy) || 0;
      if (across !== 0) this._target.write('\u001b[' + Math.abs(across) + (across > 0 ? 'C' : 'D'));
      if (down !== 0) this._target.write('\u001b[' + Math.abs(down) + (down > 0 ? 'B' : 'A'));
      return this;
    }
  }

  // Reading a terminal needs raw mode and fd events; nothing in this runtime
  // has those yet, so a ReadStream exists, reports isTTY, and reads nothing.
  class ReadStream extends EventTarget {
    constructor(fd = 0, options) {
      super();
      this.fd = fd;
      this.isTTY = isatty(fd);
      this._options = options || {};
      this._raw = false;
      this.readable = true;
    }
    get isRaw() {
      return this._raw;
    }
    setRawMode(mode) {
      this._raw = !!mode;
      return this;
    }
    setEncoding() {
      return this;
    }
    pause() {
      return this;
    }
    resume() {
      return this;
    }
    destroy() {
      this.readable = false;
      return this;
    }
  }

  globalThis.__tty_module = { isatty, ReadStream, WriteStream };
})();

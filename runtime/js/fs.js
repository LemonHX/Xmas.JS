(function () {
  // What is read back is an ArrayBuffer; to get a Buffer, wrap it with Buffer.from
  const wrap = (buffer) => Buffer.from(buffer);
  const textOf = (buffer, encoding) => Buffer.from(buffer).toString(encoding || 'utf8');
  const pathOf = (path) => path instanceof URL ? path.pathname : String(path);
  const dataOf = (data, encoding) => typeof data === "string" ? Buffer.from(data, encoding || "utf8") : Buffer.from(data);

  const constants = JSON.parse(__fs_constants());
  class Stats {
    constructor(info) {
      Object.assign(this, info);
      this.atime = new Date(info.atimeMs);
      this.mtime = new Date(info.mtimeMs);
      this.ctime = new Date(info.ctimeMs);
      this.birthtime = new Date(info.birthtimeMs);
    }
    _is(type) { return (this.mode & constants.S_IFMT) === type; }
    isFile() { return this._is(constants.S_IFREG); }
    isDirectory() { return this._is(constants.S_IFDIR); }
    isSymbolicLink() { return this._is(constants.S_IFLNK); }
    isFIFO() { return this._is(0o010000); }
    isSocket() { return this._is(0o140000); }
    isCharacterDevice() { return this._is(0o020000); }
    isBlockDevice() { return this._is(0o060000); }
  }
  class Dirent {
    constructor(name, parentPath, item) { this.name = name; this.parentPath = parentPath; this.path = parentPath; this._item = item; }
    isFile() { return !this._item.dir && !this._item.link; }
    isDirectory() { return !!this._item.dir; }
    isSymbolicLink() { return !!this._item.link; }
    isFIFO() { return false; }
    isSocket() { return false; }
    isCharacterDevice() { return false; }
    isBlockDevice() { return false; }
  }
  const direntsOf = (json, parent) => JSON.parse(json).map((item) => new Dirent(item.name, parent, item));

  const readAll = (fd) => {
    const chunks = [];
    for (;;) {
      const chunk = new Uint8Array(__fs_read_sync(fd, 65536, -1));
      if (chunk.length === 0) break;
      chunks.push(chunk);
    }
    return Buffer.concat(chunks);
  };
  class FileHandle {
    constructor(fd) { this.fd = fd; }
    async read(buffer, offset = 0, length, position = null) {
      const bytesRead = sync.readSync(this.fd, buffer, offset, length, position);
      return { bytesRead, buffer };
    }
    async write(data, ...rest) {
      const bytesWritten = sync.writeSync(this.fd, data, ...rest);
      return { bytesWritten, buffer: data };
    }
    async readFile(options) {
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      const all = readAll(this.fd);
      return encoding ? all.toString(encoding) : all;
    }
    async writeFile(data, options) {
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      sync.writeSync(this.fd, dataOf(data, encoding));
    }
    async appendFile(data, options) { return this.writeFile(data, options); }
    async stat() { return sync.fstatSync(this.fd); }
    async close() { if (this.fd >= 0) { __fs_close_sync(this.fd); this.fd = -1; } }
    async [Symbol.asyncDispose]() { return this.close(); }
  }

  const promises = {
    readFile(path, options) {
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      return __fs_read_file(pathOf(path)).then((buffer) => encoding ? textOf(buffer, encoding) : wrap(buffer));
    },
    writeFile(path, data, options) {
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      return __fs_write_file(pathOf(path), dataOf(data), false, encoding || 'utf8');
    },
    appendFile(path, data, options) {
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      return __fs_write_file(pathOf(path), dataOf(data), true, encoding || 'utf8');
    },
    mkdir(path, options) {
      const recursive = !!(options && typeof options === 'object' && options.recursive);
      return __fs_mkdir(pathOf(path), recursive);
    },
    rm(path, options) {
      const settings = options || {};
      return __fs_rm(pathOf(path), !!settings.recursive, !!settings.force);
    },
    unlink(path) { return __fs_rm(pathOf(path), false, false); },
    rmdir(path, options) {
      return __fs_rm(pathOf(path), !!(options && options.recursive), false);
    },
    rename(from, to) { return __fs_rename(pathOf(from), pathOf(to)); },
    copyFile(from, to) { return __fs_copy(pathOf(from), pathOf(to)); },
    realpath(path) { return __fs_realpath(pathOf(path)); },
    readdir(path, options) {
      const withTypes = !!(options && typeof options === 'object' && options.withFileTypes);
      const parent = pathOf(path);
      return __fs_readdir(parent).then((json) => withTypes ? direntsOf(json, parent) : JSON.parse(json).map((item) => item.name));
    },
    stat(path) { return __fs_stat(pathOf(path)).then((json) => new Stats(JSON.parse(json))); },
    lstat(path) { return __fs_lstat(pathOf(path)).then((json) => new Stats(JSON.parse(json))); },
    async mkdtemp(prefix) {
      const chars = 'abcdefghijklmnopqrstuvwxyz0123456789';
      for (let attempt = 0; attempt < 20; attempt++) {
        let suffix = '';
        for (let i = 0; i < 6; i++) suffix += chars[Math.floor(Math.random() * chars.length)];
        const path = String(prefix) + suffix;
        try { await promises.stat(path); } catch { await promises.mkdir(path); return path; }
      }
      throw new Error('mkdtemp: could not find a free name for ' + prefix);
    },
    async chmod(path, mode) { __fs_chmod_sync(pathOf(path), (typeof mode === 'number' ? mode : parseInt(mode, 8)).toString(8)); },
    async symlink(target, path) { __fs_symlink_sync(String(target), pathOf(path)); },
    async readlink(path) { return __fs_readlink_sync(pathOf(path)); },
    async access(path) { await promises.stat(path); },
    async open(path, flags = 'r', mode = 0o666) {
      return new FileHandle(Number(__fs_open_sync(pathOf(path), String(flags), mode)));
    },
    async exists(path) {
      try { await promises.stat(path); return true; } catch { return false; }
    },
  };

  // Node's *Sync calls. They block the realm thread, which is what they are for: a script that wants
  // a file now should not have to become a state machine to get one.
  const modeOf = (mode) => (typeof mode === 'number' ? mode : parseInt(mode, 8));
  const sync = {
    readdirSync(path, options) {
      const withTypes = !!(options && typeof options === 'object' && options.withFileTypes);
      const parent = pathOf(path);
      const json = __fs_readdir_sync(parent);
      return withTypes ? direntsOf(json, parent) : JSON.parse(json).map((item) => item.name);
    },
    statSync(path, options) {
      try { return new Stats(JSON.parse(__fs_stat_sync(pathOf(path), true))); } catch (error) {
        if (options && options.throwIfNoEntry === false && /^ENOENT\b/.test(String(error && error.message))) return undefined;
        throw error;
      }
    },
    lstatSync(path, options) {
      try { return new Stats(JSON.parse(__fs_stat_sync(pathOf(path), false))); } catch (error) {
        if (options && options.throwIfNoEntry === false && /^ENOENT\b/.test(String(error && error.message))) return undefined;
        throw error;
      }
    },
    fstatSync(fd) { return new Stats(JSON.parse(__fs_fstat_sync(fd))); },
    existsSync(path) {
      try {
        __fs_stat_sync(pathOf(path), true);
        return true;
      } catch (error) {
        return false;
      }
    },
    mkdirSync(path, options) {
      const recursive = !!(options && typeof options === 'object' && options.recursive);
      __fs_mkdir_sync(pathOf(path), recursive);
    },
    writeFileSync(path, data, options) {
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      __fs_write_file_sync(pathOf(path), dataOf(data, encoding), options && options.flag === 'a');
    },
    appendFileSync(path, data, options) {
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      __fs_write_file_sync(pathOf(path), dataOf(data, encoding), true);
    },
    // Streams over the synchronous calls. Writable/Readable are looked up when called, because the
    // stream module is installed after this one.
    createWriteStream(path, options) {
      const target = pathOf(path);
      const flags = typeof options === 'object' && options ? options.flags : undefined;
      const encoding = typeof options === 'string' ? options : options && options.encoding;
      let append = flags === 'a' || flags === 'a+';
      return new globalThis.__stream_module.Writable({
        write(chunk, enc, callback) {
          try {
            __fs_write_file_sync(target, dataOf(chunk, typeof chunk === 'string' ? (encoding || enc) : undefined), append);
            append = true;
            callback();
          } catch (error) { callback(error); }
        },
        final(callback) {
          try { if (!append) __fs_write_file_sync(target, dataOf('', undefined), false); callback(); } catch (error) { callback(error); }
        },
      });
    },
    createReadStream(path) {
      const target = pathOf(path);
      let done = false;
      return new globalThis.__stream_module.Readable({
        read() {
          if (done) return;
          done = true;
          try { this.push(__fs_read_file_sync(target)); this.push(null); } catch (error) { this.destroy(error); }
        },
      });
    },
    realpathSync(path) { return __fs_realpath_sync(pathOf(path)); },
    renameSync(from, to) { __fs_rename_sync(pathOf(from), pathOf(to)); },
    rmSync(path, options) { const o = options || {}; __fs_rm_sync(pathOf(path), (o.recursive ? 'r' : '') + (o.force ? 'f' : '')); },
    symlinkSync(target, path) { __fs_symlink_sync(String(target), pathOf(path)); },
    readlinkSync(path) { return __fs_readlink_sync(pathOf(path)); },
    linkSync(from, to) { __fs_link_sync(pathOf(from), pathOf(to)); },
    mkdtempSync(prefix) {
      const chars = 'abcdefghijklmnopqrstuvwxyz0123456789';
      for (let attempt = 0; attempt < 20; attempt++) {
        let suffix = '';
        for (let i = 0; i < 6; i++) suffix += chars[Math.floor(Math.random() * chars.length)];
        const path = String(prefix) + suffix;
        if (!sync.existsSync(path)) { sync.mkdirSync(path); return path; }
      }
      throw new Error('mkdtempSync: no free name for ' + prefix);
    },
    openSync(path, flags = 'r', mode = 0o666) { return Number(__fs_open_sync(pathOf(path), String(flags), modeOf(mode))); },
    closeSync(fd) { __fs_close_sync(fd); },
    readSync(fd, buffer, offsetOrOptions, length, position) {
      let offset = offsetOrOptions;
      if (offset !== null && typeof offset === 'object') ({ offset = 0, length, position } = offset);
      offset = offset || 0;
      if (length === undefined || length === null) length = buffer.byteLength - offset;
      const where = position === undefined || position === null || position === -1 ? -1 : Number(position);
      const got = new Uint8Array(__fs_read_sync(fd, length, where));
      new Uint8Array(buffer.buffer, buffer.byteOffset + offset, got.length).set(got);
      return got.length;
    },
    writeSync(fd, data, a, b, c) {
      if (typeof data === 'string') {
        const position = typeof a === 'number' ? a : -1;
        return Number(__fs_write_fd_sync(fd, dataOf(data, typeof b === 'string' ? b : 'utf8'), position));
      }
      const view = new Uint8Array(data.buffer, data.byteOffset, data.byteLength);
      const offset = typeof a === 'number' ? a : 0;
      const length = typeof b === 'number' ? b : view.length - offset;
      const position = typeof c === 'number' ? c : -1;
      return Number(__fs_write_fd_sync(fd, view.slice(offset, offset + length), position));
    },
    chmodSync(path, mode) { __fs_chmod_sync(pathOf(path), (typeof mode === 'number' ? mode : parseInt(mode, 8)).toString(8)); },
    readFileSync(path) {
      // Text only: the synchronous read is the one text needs, and a Buffer is a promise the
      // caller can build from it.
      return __fs_read_file_sync(pathOf(path));
    },
  };

  // Host errors arrive as 'ENOENT: message'; Node's carry the same text as error.code and a name of Error.
  for (const key of Object.keys(sync)) {
    const original = sync[key];
    if (typeof original !== 'function' || key.startsWith('create')) continue;
    sync[key] = function (...args) {
      try { return original.apply(this, args); } catch (error) {
        if (error && typeof error.message === 'string' && error.code === undefined) {
          const m = /^([A-Z][A-Z0-9]{1,15}): /.exec(error.message);
          if (m) error.code = m[1];
        }
        throw error;
      }
    };
  }

  // For convenience in the probe: scripts can just use fs.readFile(...) directly. The official API goes through import.
  globalThis.fs = promises;
  globalThis.__fs_promises = promises;
  globalThis.__fs_sync = sync;
  promises.constants = constants;
  promises.FileHandle = FileHandle;
  globalThis.__fs_module = { promises, ...sync, ...promises, constants, Stats, Dirent };
  // `import fs from 'node:fs'` is the whole module, as in node; `fs.readFile` stays the promise one.
  globalThis.__fs_module.default = globalThis.__fs_module;
})();

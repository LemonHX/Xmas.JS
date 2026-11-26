(function () {
  // What is read back is an ArrayBuffer; to get a Buffer, wrap it with Buffer.from
  const wrap = (buffer) => Buffer.from(buffer);
  const textOf = (buffer, encoding) => Buffer.from(buffer).toString(encoding || 'utf8');
  const pathOf = (path) => path instanceof URL ? path.pathname : String(path);
  const dataOf = (data, encoding) => typeof data === "string" ? Buffer.from(data, encoding || "utf8") : Buffer.from(data);

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
      return __fs_readdir(pathOf(path)).then((json) => {
        const items = JSON.parse(json);
        return withTypes ? items.map((item) => ({
          name: item.name,
          isFile: () => !item.dir,
          isDirectory: () => item.dir,
          isSymbolicLink: () => item.link,
        })) : items.map((item) => item.name);
      });
    },
    stat(path) {
      return __fs_stat(pathOf(path)).then((json) => {
        const info = JSON.parse(json);
        info.isFile = () => !info.dir;
        info.isDirectory = () => !!info.dir;
        info.isSymbolicLink = () => !!info.link;
        info.mtime = new Date(info.mtimeMs);
        return info;
      });
    },
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
    async access(path) { await promises.stat(path); },
    lstat(path) { return promises.stat(path); },
    // A FileHandle over whole-file calls: enough for readFile/writeFile/appendFile/stat/close.
    async open(path, flags = 'r') {
      const target = pathOf(path);
      if (/^[wa]/.test(String(flags))) {
        if (String(flags).startsWith('w')) await promises.writeFile(target, '');
        else { try { await promises.stat(target); } catch { await promises.writeFile(target, ''); } }
      } else await promises.stat(target);
      return {
        fd: 0,
        readFile: (options) => promises.readFile(target, options),
        writeFile: (data, options) => promises.writeFile(target, data, options),
        appendFile: (data, options) => promises.appendFile(target, data, options),
        write: async (data) => { await promises.appendFile(target, data); return { bytesWritten: typeof data === 'string' ? data.length : data.byteLength, buffer: data }; },
        stat: () => promises.stat(target),
        sync: async () => {},
        datasync: async () => {},
        close: async () => {},
        [Symbol.asyncDispose]: async () => {},
      };
    },
    async exists(path) {
      try { await promises.stat(path); return true; } catch { return false; }
    },
  };

  // Node's *Sync calls. They block the realm thread, which is what they are for: a script that wants
  // a file now should not have to become a state machine to get one.
  const sync = {
    readdirSync(path, options) {
      const withTypes = !!(options && typeof options === 'object' && options.withFileTypes);
      const items = JSON.parse(__fs_readdir_sync(pathOf(path)));
      return withTypes ? items.map((item) => ({
        name: item.name,
        isFile: () => !item.dir,
        isDirectory: () => item.dir,
        isSymbolicLink: () => item.link,
      })) : items.map((item) => item.name);
    },
    statSync(path) {
      const info = JSON.parse(__fs_stat_sync(pathOf(path)));
      info.isFile = () => !info.dir;
      info.isDirectory = () => !!info.dir;
      info.isSymbolicLink = () => !!info.link;
      info.mtime = new Date(info.mtimeMs);
      return info;
    },
    existsSync(path) {
      try {
        __fs_stat_sync(pathOf(path));
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
    chmodSync(path, mode) { __fs_chmod_sync(pathOf(path), (typeof mode === 'number' ? mode : parseInt(mode, 8)).toString(8)); },
    readFileSync(path) {
      // Text only: the synchronous read is the one text needs, and a Buffer is a promise the
      // caller can build from it.
      return __fs_read_file_sync(pathOf(path));
    },
  };

  // For convenience in the probe: scripts can just use fs.readFile(...) directly. The official API goes through import.
  globalThis.fs = promises;
  globalThis.__fs_promises = promises;
  globalThis.__fs_sync = sync;
  globalThis.__fs_module = { promises, ...sync, ...promises };
  // `import fs from 'node:fs'` is the whole module, as in node; `fs.readFile` stays the promise one.
  globalThis.__fs_module.default = globalThis.__fs_module;
})();

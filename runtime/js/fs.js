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
  globalThis.__fs_module = { promises, default: promises, ...sync, ...promises };
})();

(function () {
  // node:module. There is no synchronous loader behind a runtime-made require, so createRequire
  // gives a function that knows the names the runtime ships and says so for anything else.
  const builtinModules = ['assert', 'buffer', 'child_process', 'crypto', 'diagnostics_channel', 'events', 'fs', 'fs/promises', 'os', 'path', 'path/posix', 'path/win32', 'perf_hooks', 'process', 'stream', 'stream/web', 'string_decoder', 'timers', 'timers/promises', 'tty', 'url', 'util', 'v8', 'zlib'];
  const isBuiltin = (name) => builtinModules.includes(String(name).replace(/^node:/, ''));
  // Every module the runtime ships keeps its API on the global object under one of these names.
  // They are read lazily, when require() is called: this prelude installs before fs, crypto and
  // stream do, so reading them now would find nothing.
  const ships = {
    'assert': () => globalThis.assert,
    'buffer': () => globalThis.__buffer_module,
    'child_process': () => globalThis.__child_process_module,
    'crypto': () => globalThis.crypto,
    'diagnostics_channel': () => globalThis.__diagnostics_channel_module,
    'events': () => globalThis.__events_module,
    'fs': () => globalThis.fs,
    'fs/promises': () => globalThis.__fs_promises,
    'os': () => globalThis.os,
    'path': () => globalThis.path,
    'path/posix': () => globalThis.path && globalThis.path.posix,
    'path/win32': () => globalThis.__path_win32,
    'perf_hooks': () => globalThis.__perf_hooks_module,
    'process': () => globalThis.process,
    'stream': () => globalThis.stream,
    'stream/web': () => globalThis.__stream_web,
    'string_decoder': () => globalThis.__string_decoder,
    'timers': () => globalThis.__timers_module,
    'timers/promises': () => globalThis.__timers_promises,
    'tty': () => globalThis.__tty_module,
    'url': () => globalThis.__url_module,
    'util': () => globalThis.util,
    'zlib': () => globalThis.__zlib_module,
  };

  // A runtime-made require serves the modules the runtime ships: a package that asks for "fs" by
  // name wants this runtime's fs, and there is no file on disk behind that name to scan for.
  // Anything else has no synchronous loader here, so it says so instead of guessing.
  const createRequire = (filename) => {
    const lookup = (name) => {
      const bare = String(name).replace(/^node:/, '');
      if (!Object.prototype.hasOwnProperty.call(ships, bare)) return undefined;
      return ships[bare]();
    };

    // The module a createRequire'd require is relative to: this is what makes 'package.json' and
    // './sibling.js' resolve the way they do next to the file that asked.
    const from = typeof filename === 'string' && filename.length > 0
      ? filename
      : (globalThis.__xmas_main_url || 'file:///');

    const where = (name) => {
      if (typeof __xmas_require_resolve !== 'function') return undefined;
      try {
        return __xmas_require_resolve(from, String(name));
      } catch {
        return undefined;
      }
    };

    const require = (name) => {
      const found = lookup(name);
      if (found !== undefined) return found;
      const path = where(name);
      if (path !== undefined) {
        // A file found on disk: the one synchronous load this runtime has is the module loader the
        // engine already runs for imports, so a path is handed back to it.
        const load = globalThis.__xmas_require_file;
        if (typeof load === 'function') return load(path);
        throw Object.assign(
          new Error("Cannot require '" + name + "': the file loader is not installed"),
          { code: 'MODULE_NOT_FOUND' },
        );
      }
      throw Object.assign(
        new Error("Cannot require '" + name + "': no such module"),
        { code: 'MODULE_NOT_FOUND' },
      );
    };
    require.resolve = (name) => {
      const path = where(name);
      if (path !== undefined) return path;
      if (lookup(name) !== undefined) return String(name).replace(/^node:/, '');
      throw Object.assign(new Error("Cannot resolve '" + name + "'"), { code: 'MODULE_NOT_FOUND' });
    };
    require.cache = Object.create(null);
    require.main = undefined;
    return require;
  };

  globalThis.__module_module = { builtinModules, isBuiltin, createRequire };
})();

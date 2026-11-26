(function () {
  // node:module. There is no synchronous loader behind a runtime-made require, so createRequire
  // gives a function that knows the names the runtime ships and says so for anything else.
  const builtinModules = ['assert', 'buffer', 'child_process', 'crypto', 'diagnostics_channel', 'events', 'fs', 'fs/promises', 'os', 'path', 'path/posix', 'path/win32', 'perf_hooks', 'process', 'stream', 'stream/web', 'string_decoder', 'timers', 'timers/promises', 'tty', 'url', 'util', 'v8', 'zlib'];
  const isBuiltin = (name) => builtinModules.includes(String(name).replace(/^node:/, ''));
  const createRequire = () => {
    const require = (name) => {
      throw Object.assign(new Error("Cannot require '" + name + "': this runtime resolves require by static scan; use import instead"), { code: 'MODULE_NOT_FOUND' });
    };
    require.resolve = (name) => { throw Object.assign(new Error("Cannot resolve '" + name + "'"), { code: 'MODULE_NOT_FOUND' }); };
    return require;
  };
  globalThis.__module_module = { builtinModules, isBuiltin, createRequire };
})();

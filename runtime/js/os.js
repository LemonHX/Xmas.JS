(function () {
  const facts = JSON.parse(__os_static());
  const cpus = [];
  for (let i = 0; i < facts.cpus; i += 1) {
    // Node reports a model and per-mode times per core; this runtime does not
    // have them, so they are empty and zeroed rather than invented.
    cpus.push({ model: '', speed: 0, times: { user: 0, nice: 0, sys: 0, idle: 0, irq: 0 } });
  }

  const os = {
    platform: () => facts.platform,
    arch: () => facts.arch,
    type: () => facts.type,
    release: () => facts.release,
    machine: () => facts.machine,
    hostname: () => facts.hostname,
    homedir: () => facts.homedir,
    tmpdir: () => facts.tmpdir,
    endianness: () => (new Uint8Array(new Uint16Array([1]).buffer)[0] === 1 ? 'LE' : 'BE'),
    cpus: () => cpus.slice(),
    availableParallelism: () => facts.cpus,
    totalmem: () => __os_totalmem(),
    freemem: () => __os_freemem(),
    uptime: () => __os_uptime(),
    loadavg: () => JSON.parse(__os_loadavg()),
    userInfo: () => ({
      uid: facts.uid,
      gid: facts.gid,
      username: facts.user,
      homedir: facts.homedir,
      shell: facts.shell || null,
    }),
    EOL: facts.eol,
    devNull: facts.platform === 'win32' ? '\\\\.\\nul' : '/dev/null',
    constants: {
      signals: JSON.parse(__os_signals()),
      errno: {},
      priority: {},
    },
  };

  globalThis.__os = os;
  globalThis.os = os;
})();

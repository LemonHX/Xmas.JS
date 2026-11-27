<div align="center">

![XMASJS LOGO](./Xmas.JS.svg)
> **The logo does not represent any religious affiliation.** 

# Xmas.JS

**A complete JavaScript runtime in one small binary. No JIT(except WASM).**


[![License: Apache-2.0 OR GPL-3.0](https://img.shields.io/badge/License-Apache%202.0%20OR%20GPL%203.0-blue.svg)](#license)
[![WinterTC](https://img.shields.io/badge/WinterTC-compatible-green.svg)](https://wintertc.org/)
[![Pure Rust](https://img.shields.io/badge/build-100%%20RUST-orange.svg)](#how-it-stays-small)

</div>

---

## TL;DR: ***EVERYTHING INCLUDED*** with Binary size **11.3 MiB**

One binary that runs TypeScript, serves HTTP, installs packages, and executes WebAssembly.

- **TypeScript and TSX run as they are.** oxc strips the types in-process. There is no build step.
- **A real server.** `export default { fetch }` and `xmas serve`. Trillium speaks HTTP/1.1, one realm per worker thread, with SSE and WebSocket upgrade.
- **WebAssembly.** The standard JS API (Module, Instance, Memory, Table, Global, streaming compile), backed by WNJR. `memory.buffer` is the wasm memory itself, not a copy.
- **Sockets.** WinterTC `connect()` for TCP, plus a `UDPSocket`.
- **Node-flavored modules.** buffer, crypto, fs, path, stream, zlib, child_process, util, assert, os, process and more, listed below.
- **CommonJS.** `require`, `module.exports`, `__dirname`, and `.cjs` files work next to ES modules.
- **A FAST package manager.** `xmas install`, `add`, `why`, `x`. It also runs your `package.json` scripts quickly.

Measured on one machine (Apple silicon macOS, release build, fat LTO, one codegen unit, symbols stripped):


| Runtime     | Cold Start  | Memory (Idle) | Best Use Case                             |
| ----------- | ----------- | ------------- | ----------------------------------------- |
| **Node.js** | ~100-200ms  | ~30-50MB      | Web servers, long-running apps            |
| **Deno** 🪦   | ~150-300ms  | ~40-60MB      | Secure web apps, TypeScript projects      |
| **Bun**     | ~50-100ms   | ~25-35MB      | Fast web development                      |
| **Xmas.JS** 🎄 | **~10-20ms** | **~10-17MB**    | **System scripts, CLI tools, serverless** |

---

## Hello

```ts
// app.ts
export default {
  fetch(request: Request) {
    return new Response('hello from ' + new URL(request.url).pathname + '\n');
  },
};
```

```bash
xmas serve app.ts --port 8000 --workers 4
```

### WebAssembly

```ts
const { instance } = await WebAssembly.instantiate(bytes, { env: { log: (n: number) => console.log(n) } });
instance.exports.run();
```

Memory is zero-copy and `memory.grow()` detaches the old `ArrayBuffer`, as the spec says. What is missing is listed in [runtime/docs/wasm.md](runtime/docs/wasm.md).

### Sockets

```ts
import { connect } from 'cloudflare:sockets';
```

TCP follows the WinterTC proposal. `startTls()` and `secureTransport: 'on'` throw a `TypeError`. They do not pretend. See [runtime/docs/sockets.md](runtime/docs/sockets.md).

### CommonJS

```js
const _ = require('lodash');
module.exports = { chunk: _.chunk };
```

`require` works by static scan: oxc finds every `require("literal")` before the file runs. `require(variable)` throws `MODULE_NOT_FOUND` and says why. See [runtime/docs/modules.md](runtime/docs/modules.md).

### Import from a URL

```ts
import * as rollup from 'https://unpkg.com/@rollup/browser/dist/es/rollup.browser.js';
```

we love web standard, sadly Deno passed away this year.

🪦 R.I.P. DENO


---

## Install

There is no installer yet. Build it:

```bash
git clone https://github.com/LemonHX/Xmas.JS
cd Xmas.JS
cargo build --release      # needs a recent nightly Rust, no C compiler
./target/release/xmas --help
```

## Commands

```
xmas [OPTIONS] [COMMAND]

  install  Install packages defined in package.json
  add      Add package to package.json
  remove   Remove package from package.json
  run      Run a script defined in package.json
  update   Prepare and save a newly planned lockfile
  upgrade  Update packages to the latest available version
  clean    Clean node_modules and cache
  exec     Execute a command (not a script)
  why      Find all uses of a given package
  create   Create new project from a starter kit
  x        Download and execute a package (like npx)
  serve    Serve a module as an HTTP app

Options:
  -v, --verbose
      --immutable                  Refuse to modify the lockfile
      --working-dir <WORKING_DIR>  Run in a custom working directory
```

`xmas serve <ENTRY>` takes `--port` (default 8000), `--host` (default 0.0.0.0) and `--workers` (default one per core).

There is no REPL and no `xmas file.ts` shortcut at the moment. Use `serve`, or `xmas run` for package scripts.

---


## How it stays small

- **No JS JIT.** The engine is [quickjs-oxide](https://github.com/0xfe-project/quickjs-oxide), a Rust QuickJS working cooperated with pocketjs team.
- **Zero C at build time.** TLS is rustls with a pure-Rust crypto provider. No OpenSSL, no libz, no system certificate store.
- **Tested for system libraries.** A test fails the build if openssl, native-tls or libz-sys show up in the runtime's dependency tree.
- **smol, not tokio.** The async runtime is smol everywhere, and tokio is not in the runtime's dependency tree today.

---

## Status

Pre-1.0. The WASM engine, the HTTP server and the package manager are young. Pin your versions.

Not done yet: `import.meta.resolve`, `new WebAssembly.Table()`/`Global()` constructors from JS, streaming zlib classes (`createGzip` and friends), and the benchmark comparison against other runtimes.

---

## License

Dual-licensed under **Apache-2.0 OR GPL-3.0**. Choose Apache-2.0 for almost everything. GPL-3.0 applies if you want to offer Xmas.JS as a managed service.

## Thanks

- [QuickJS](https://bellard.org/quickjs/) by Fabrice Bellard
- [quickjs-oxide](https://github.com/0xfe-project/quickjs-oxide) PocketJS TEAM

- [WNJR](https://github.com/0xfe-project/wnjr) WASM runtime.
- [Trillium](https://trillium.rs/) the web engine.
- [oxc](https://oxc-project.github.io/) the TS transpiler.

and [txiki.js](https://github.com/saghul/txiki.js) for the idea that a small runtime is worth having.

<div align="center">

Made by ❤️ LemonHX 🍋 and the Xmas.JS 🎄 team.

</div>

<div align="center">

![XMASJS LOGO](./Xmas.JS.svg)
> **The logo does not represent any religious affiliation.** It is a stylized "Xmas", as in "ECMAScript".

# Xmas.JS

**A complete JavaScript runtime in one small binary. No JIT, no C toolchain, no node_modules tax.**

[![License: Apache-2.0 OR GPL-3.0](https://img.shields.io/badge/License-Apache%202.0%20OR%20GPL%203.0-blue.svg)](#license)
[![WinterTC](https://img.shields.io/badge/WinterTC-compatible-green.svg)](https://wintertc.org/)
[![Pure Rust](https://img.shields.io/badge/build-zero%20C-orange.svg)](#how-it-stays-small)

</div>

---

## What is Xmas.JS?

One binary that runs TypeScript, serves HTTP, installs packages, and executes WebAssembly.

- **TypeScript and TSX run as they are.** oxc strips the types in-process. There is no build step.
- **A real server.** `export default { fetch }` and `xmas serve`. Trillium speaks HTTP/1.1, one realm per worker thread, with SSE and WebSocket upgrade.
- **WebAssembly.** The standard JS API (Module, Instance, Memory, Table, Global, streaming compile), backed by WNJR. `memory.buffer` is the wasm memory itself, not a copy.
- **Sockets.** WinterTC `connect()` for TCP, plus a `UDPSocket`.
- **Node-flavored modules.** buffer, crypto, fs, path, stream, zlib, child_process, util, assert, os, process and more, listed below.
- **CommonJS.** `require`, `module.exports`, `__dirname`, and `.cjs` files work next to ES modules.
- **A package manager.** `xmas install`, `add`, `why`, `x`. It also runs your `package.json` scripts.
- **Temporal.** Backed by temporal_rs.

Everything above is one process, written in Rust, with no system library underneath.

---

## Small on purpose

Measured on one machine (Apple silicon macOS, release build with the default profile, nothing tuned):

| What | Number |
| --- | --- |
| Binary size | **21.8 MiB** (22,831,088 bytes), runtime, package manager, TS compiler and WebAssembly engine included |
| Process start to first HTTP response | **20 to 62 ms**, five runs, median 28 ms (spawn, parse, compile, serve, one request) |
| Idle memory, hello-world server, 1 worker | **19 MB** resident (RSS, `ps`) |

These are not a comparison. Nobody else's runtime was measured with the same harness, so there is no "Nx faster" here. The comparison against LLRT, Node with Hono, and Bun is still to do, and it will go in this table when it exists.

The binary has no `[profile.release]` tuning yet (no LTO, no strip, no `opt-level = "z"`), so that size is the starting point, not the floor.

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

A handler can return a `Response` or a promise of one. A handler that returns anything else gets a 500 and a log line saying what it returned. A module that throws while loading stops the whole server with the file, the line and the reason, instead of listening and answering 500 to everyone.

You can also register the handler from inside the module:

```ts
import { serve } from 'xmas:server';

serve({ fetch: (request) => new Response('hi'), env: { GREETING: 'hi' } });
```

`env` is handed to your handler as its second argument, and the third is a `ctx` with a real `waitUntil`. Details are in [runtime/docs/server.md](runtime/docs/server.md).

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

## What is in the box

| Area | Modules |
| --- | --- |
| Web | fetch, Request/Response/Headers, URL, AbortController, events, WebSocket, streams (web), timers, console, crypto |
| Node-style | assert, async_hooks, buffer, child_process, exec, fs, os, path, perf_hooks, process, stream, string_decoder, tty, util, zlib |
| Network | sockets (TCP and UDP), the HTTP server |
| Time | Temporal |
| Code | WebAssembly, CommonJS, `import.meta` (`url`, `dir`, `dirname`, `path`, `filename`, `file`) |

`fetch('file://...')` reads from disk. Node refuses that and Bun and Deno allow it.

Each module has a doc in [runtime/docs](runtime/docs) that lists what it does not do. We would rather you read the gaps there than find them at 2 a.m.

### Left out on purpose

`node:net`, `node:tls`, `node:http`, `node:dgram`, `node:dns` and `node:intl`. Use `fetch` and sockets. There is no permission model: a script can reach what the process can reach. `os` and `process` report the runtime's own facts and read nothing from the host.

---

## How it stays small

- **No JIT.** The engine is [quickjs-oxide](https://github.com/0xfe-project/quickjs-oxide), a Rust QuickJS. Starting is cheap. Long CPU-heavy loops will lose to V8.
- **Zero C at build time.** TLS is rustls with a pure-Rust crypto provider. No OpenSSL, no libz, no system certificate store. Nothing in the build needs a C compiler or a system library, so it does not depend on what the host has installed.
- **Tested for system libraries.** A test fails the build if openssl, native-tls or libz-sys show up in the runtime's dependency tree.
- **smol, not tokio.** The async runtime is smol everywhere, and tokio is not in the runtime's dependency tree today.

---

## Status

Pre-1.0. The WASM engine, the HTTP server and the package manager are young. Pin your versions.

Not done yet: `https://` imports, `import.meta.main`/`env`/`resolve`, `new WebAssembly.Table()`/`Global()` constructors from JS, streaming zlib classes (`createGzip` and friends), and the benchmark comparison against other runtimes.

---

## License

Dual-licensed under **Apache-2.0 OR GPL-3.0**. Choose Apache-2.0 for almost everything. GPL-3.0 applies if you want to offer Xmas.JS as a managed service.

## Thanks

[QuickJS](https://bellard.org/quickjs/) by Fabrice Bellard, [quickjs-oxide](https://github.com/0xfe-project/quickjs-oxide), WNJR, [LLRT](https://github.com/awslabs/llrt), [Trillium](https://trillium.rs/), [smol](https://github.com/smol-rs/smol), [oxc](https://oxc-project.github.io/), [temporal_rs](https://github.com/boa-dev/temporal), and [txiki.js](https://github.com/saghul/txiki.js) for the idea that a small runtime is worth having.

<div align="center">

Made by LemonHX and the Xmas.JS team.

</div>

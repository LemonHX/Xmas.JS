
# WASM

WebAssembly comes in through WNJR (`wnjr-core`), embedded as a package. What WNJR has to grow,
what we do on our side, and what stays unsupported is in `runtime/docs/wnjr.md`.

# Networking API

`fetch` is trillium-client over smol with rustls, and `serve` (from `xmas:server`) is trillium's
server. Both are in the tree already (`runtime/src/host/http.rs`, `runtime/src/modules/server.rs`);
no hyper.

# bundler / minifier / optimizer / typescript / lint / etc...

OXC is great!

# sub commands
xmas: start repl

- [x] run <file>.<js/ts/mjs/cjs/jsx/tsx>
- [x] run also could behave like npx if not running a file

- [x] add : in project add to project.json dependencies, else add to global cache
- [x] install i : install all dependencies in project.json
- [x] update u : update all dependencies in project.json
- [x] list ls : list all dependencies in project.json, or global cache if not in project
- [x] execute exec <script> [args...] : execute script from project.json scripts

- [-] compile <input> <output>: gone. quickjs-oxide does not serialize bytecode.
- [ ] init (project): create a new xmas project
- [-] bundle <input> <output>: gone. There is no bundler; the loader resolves and compiles each
      module on its own.

# Winter TC API

[txiki.js](https://github.com/saghul/txiki.js) did a great job implementing TC API's,
i want to implement those API's as well. 
- [x] Abort
    - [x] AbortController
    - [x] AbortSignal

- [x] Crypto
- [x] CryptoKey

- [x] Blob
- [ ] ByteLengthQueuingStrategy
- [ ] CompressionStream
- [ ] CountQueuingStrategy
- [ ] DecompressionStream
- [x] DOMException

- [x] Event
    - [x] EventTarget

- [x] File
    - [x] async
    - [x] sync

- [x] FormData
- [x] Headers
- [ ] ReadableByteStreamController
- [ ] ReadableStream
- [ ] ReadableStreamBYOBReader
- [ ] ReadableStreamBYOBRequest
- [ ] ReadableStreamDefaultController
- [ ] ReadableStreamDefaultReader
- [x] Request
- [x] Response
- [x] SubtleCrypto
- [x] TextDecoder
- [ ] TextDecoderStream
- [x] TextEncoder
- [ ] TextEncoderStream
- [ ] TransformStream
- [ ] TransformStreamDefaultController
- [x] URL
- [x] URLSearchParams
- [ ] WritableStream
- [ ] WritableStreamDefaultController



Global methods / properties:
- [x] globalThis
- [x] globalThis.atob()
- [x] globalThis.btoa()
- [x] globalThis.console
    - i think i should use rust's log/tracing instead of printing
    - due to it is easier to connect with rust's logging ecosystem
    - and also easier to implement OTEL later
    - TODO: console.span(level: "info" | "debug" | "warn" | "error" | "trace", name: string, fn: (span) => void)
    - TODO: console.table
- [x] globalThis.crypto
- [x] globalThis.fetch()
- [x] globalThis.navigator.userAgent
- [x] globalThis.performance.now()
- [x] globalThis.performance.timeOrigin
- [x] globalThis.queueMicrotask()
- [x] globalThis.setTimeout() / globalThis.clearTimeout()
- [x] globalThis.setInterval() / globalThis.clearInterval()
- [x] globalThis.structuredClone()

---

- [ ] WASM
    - [ ] WebAssembly.Global
    - [ ] WebAssembly.Instance
    - [ ] WebAssembly.Memory
    - [ ] WebAssembly.Module
    - [ ] WebAssembly.Table
    - [ ] globalThis.WebAssembly.compile()
    - [ ] globalThis.WebAssembly.compileStreaming()
    - [ ] globalThis.WebAssembly.instantiate()
    - [ ] globalThis.WebAssembly.instantiateStreaming()
    - [ ] globalThis.WebAssembly.validate()

---

- [ ] globalThis
  - [ ] $ for shell commands
  - [ ] cli
    - [ ] prompt
    - [ ] ansi
    - [ ] command

- [x] Repl
  - [ ] complete method/property name
  - [ ] complete globalThis property name
  - [x] package manager commands

---

# Module loading

Done in the rewrite: `runtime/src/resolve.rs` turns a specifier into a path (relative, absolute,
bare through `node_modules`, `package.json` `exports`/`main`, extension probing), and
`runtime/src/host/modules.rs` loads and compiles each file once. See `runtime/docs/modules.md`.


---

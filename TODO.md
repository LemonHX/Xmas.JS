
# WASM

i decided to use WAMR,
because it is more lightweight and mature than other runtimes like wasmtime or wasmer.
in addition, WAMR has GC support and both JIT and AOT compilation modes.


# Networking API

i decided to use hyper for implementing networking API's like fetch,
because it is a mature and widely used HTTP library in rust ecosystem.

# Privilege API

like deno, i want to implement privilege API's,
so that scripts can request permissions for certain operations,
and the host application can grant or deny those permissions.

- fs
    - read
    - write
- os
    - process
- net
    - blacklist / whitelist
- env
    - read
    - write

for both main thread and worker threads.

# bundler / minifier / optimizer / typescript / lint / etc...

OXC is great!

# sub commands
xmas: start repl

- [ ] run <file>.<js/ts/mjs/cjs/jsx/tsx>
- [ ] run also could behave like npx if not running a file

- [x] add : in project add to project.json dependencies, else add to global cache
- [x] install i : install all dependencies in project.json
- [x] update u : update all dependencies in project.json
- [x] list ls : list all dependencies in project.json, or global cache if not in project
- [x] execute exec <script> [args...] : execute script from project.json scripts

- [ ] compile <input> <output> (compile to quickjs bytecode)
- [ ] init (project): create a new xmas project
- [ ] bundle <input> <output>

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

# Vsys (Virtual System Layer)


- [x] vsys crate 
- [x] FsVTable 
- [x] Permissions
- [x] ModuleLoaderVTable
- [x] modules/src/fs


- [ ] modules/src/module/package/resolver.rs
  - `fs::read`, `Path::is_file()`, `Path::is_dir()`, `Path::exists()`, `read_link()`
- [ ] modules/src/module/package/loader.rs - `std::fs::read`, `File::open`
- [ ] modules/src/module/module/require.rs - `fs::read_to_string`
- [ ] NetVTable
- [ ] EnvVTable

---

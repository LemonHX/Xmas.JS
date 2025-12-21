# WASM

i decided to use WAMR,
because it is more lightweight and mature than other runtimes like wasmtime or wasmer.
in addition, WAMR has GC support and both JIT and AOT compilation modes.

# shell for executing package.json scripts

`deno_task_shell` is a great example

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

# Package Manager

i want to implement a fast and lightweight package manager,
maybe i need to research at [cotton](https://github.com/danielhuang/cotton) which
does not currently support Git repositories or local paths as dependencies;
and i also want to support jsr and http package sources.

# bundler / minifier / optimizer / typescript / lint / etc...

OXC is great!

# Winter TC API

[txiki.js](https://github.com/saghul/txiki.js) did a great job implementing TC API's,
i want to implement those API's as well. 

- [ ] Abort
    - [ ] AbortController
    - [ ] AbortSignal

- [ ] Crypto
- [ ] CryptoKey

- [ ] Blob
- [ ] ByteLengthQueuingStrategy
- [ ] CompressionStream
- [ ] CountQueuingStrategy
- [ ] DecompressionStream
- [ ] DOMException

- [x] Event
    - [x] EventTarget

- [ ] File
- [ ] FormData
- [ ] Headers
- [ ] ReadableByteStreamController
- [ ] ReadableStream
- [ ] ReadableStreamBYOBReader
- [ ] ReadableStreamBYOBRequest
- [ ] ReadableStreamDefaultController
- [ ] ReadableStreamDefaultReader
- [ ] Request
- [ ] Response
- [ ] SubtleCrypto
- [ ] TextDecoder
- [ ] TextDecoderStream
- [ ] TextEncoder
- [ ] TextEncoderStream
- [ ] TransformStream
- [ ] TransformStreamDefaultController
- [ ] URL
- [ ] URLSearchParams
- [ ] WritableStream
- [ ] WritableStreamDefaultController

- [ ] WASM
    - [ ] WebAssembly.Global
    - [ ] WebAssembly.Instance
    - [ ] WebAssembly.Memory
    - [ ] WebAssembly.Module
    - [ ] WebAssembly.Table

Global methods / properties:
- [ ] globalThis
- [ ] globalThis.atob()
- [ ] globalThis.btoa()
- [x] globalThis.console
    - i think i should use rust's log/tracing instead of printing
    - due to it is easier to connect with rust's logging ecosystem
    - and also easier to implement OTEL later
    - TODO: console.span(level: "info" | "debug" | "warn" | "error" | "trace", name: string, fn: (span) => void)
    - TODO: console.table
- [ ] globalThis.crypto
- [ ] globalThis.fetch()
- [ ] globalThis.navigator.userAgent
- [ ] globalThis.performance.now()
- [ ] globalThis.performance.timeOrigin
- [ ] globalThis.queueMicrotask()
- [ ] globalThis.setTimeout() / globalThis.clearTimeout()
- [ ] globalThis.setInterval() / globalThis.clearInterval()
- [ ] globalThis.structuredClone()

- [ ] WASM
    - [ ] globalThis.WebAssembly.compile()
    - [ ] globalThis.WebAssembly.compileStreaming()
    - [ ] globalThis.WebAssembly.instantiate()
    - [ ] globalThis.WebAssembly.instantiateStreaming()
    - [ ] globalThis.WebAssembly.validate()
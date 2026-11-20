# The HTTP server

Trillium parses, we own the thread. A worker is one listener, one single-threaded executor, one
realm, and every connection that worker accepts is served on the thread the realm lives on.

## Why not trillium_smol::config().run()

That path works, and it is what the probe used for its first numbers, but it hands each accepted
connection to "the runtime's spawn". Outside a Linux reuseport worker that is the multi-threaded
global executor, and a connection future there can migrate between threads. A realm cannot follow
it: the engine is an Rc with a single-threaded core behind it, and a handler that runs on the wrong
thread is a crash or a corrupt heap, not an error you can catch.

So the accept loop is ours: one SO_REUSEPORT listener per worker, every connection carried by that
worker's `LocalExecutor`, and `HttpContext::run` (whose handler future has no Send bound) called
on the same thread. The realm and the connections it serves never leave the thread they were
created on, and no request pays for a thread hop.

What trillium still gives us, unchanged: the request head parser, chunked request bodies, keep-alive,
pipelining, trailers, 100-continue, graceful close, and the 400 it sends for a head that is not HTTP.

## Serving a module

`entry::serve_app(addr, workers, path)` is what a `serve` command does. Every worker builds its own
realm, installs the modules, evaluates the entry on that realm and resolves the handler from what
the entry exported. Three shapes work, in this order:

1. the entry called `Xmas.serve(handler)` or `Xmas.serve({ fetch, env })` at the top level,
2. the namespace has its own `fetch` (`export function fetch`),
3. the default export has one (`export default { fetch }`).

If none of the three is there the worker refuses to start. So does a worker whose entry throws while
it is evaluated: module evaluation is a promise, so the throw arrives as a rejection rather than an
error, and it is drained and reported instead of being mistaken for a missing handler.

A server only starts when every worker it asked for came up; otherwise `serve_app` stops the threads
that did start and returns the first reason. Half a server answering 500 on one worker in ten is
harder to read than a start that fails with the reason in it.

```ts
// app.ts
export default {
  fetch(request: Request): Response {
    return new Response('hello from ' + request.url + '\n');
  },
};
```

```sh
cargo run --example serve_app -- ./app.ts --port 8000 --workers 4
# or, from the cli
xmas serve ./app.ts --port 8000 --host 127.0.0.1 --workers 4
```

`--host` defaults to `0.0.0.0`, and a wildcard listener satisfies any hostname an entry asks for.

The address belongs to the listener, not to the entry: it is bound before any realm exists. An entry
that asks for a different port (`Xmas.serve({ port: 3000 })`) is refused at startup with both numbers
in the message, because the alternative is an app that answers on the wrong port and looks broken.
When the listener is on port 0 there is nothing to compare against and the check is skipped.

## The loader, for now

The engine asks its loader for a module by name and gets already-compiled source; that is all the
module table holds. The entry itself is put there before it is run, so a single-file app works, and
a TSX entry is compiled the same way the rest of the runtime compiles TSX. A relative or bare
import inside the entry fails with "Cannot find module", because resolving one needs the importing
module's path, which the engine's loader call does not carry. That resolution belongs in a pre-pass
over the module graph (rewrite each specifier to the path it resolves to, then load from disk), and
it is not written yet.

## The path of one request

1. `HttpContext::run` reads a request head and hands over a `Conn`.
2. The body is read into memory (see gaps below), the head is flattened into method, target and a
   NUL-joined header list.
3. A slot is reserved under a fresh id, and the realm's dispatcher is called with
   (id, method, target, headers, body). This is a direct call into JS, on this thread.
4. In JS, `runtime/js/server.js` builds a `Request` (the same class fetch uses), calls the app's
   handler and turns the answer into status, headers and bytes.
5. `__serve_respond` puts the answer in the slot. A handler that answered before it returned has
   its answer waiting when the call in step 3 comes back, so that request never awaits anything.
6. The connection future writes the response and loops for the next request on that connection.

An answer whose body is a `ReadableStream` does not go through step 5: `__serve_stream` settles the
head, and each chunk the handler produces is written with `__serve_chunk` as it is produced. An
answer that is a websocket handshake settles through `__serve_upgrade`, and the connection leaves
HTTP for the frame loop instead of going back to step 6.

A handler that goes async (a timer, a fetch, a file) calls `__serve_respond` later. The driver's
pump is what runs the handler's microtasks: `Pending::notify` right after the dispatch call is what
wakes it, instead of waiting for the next timer. A handler that never settles is answered with 504
after 30 seconds, and a connection that goes away mid-request drops its slot.

## Server-sent events

The handler returns a `Response` whose body is a `ReadableStream` and the chunks go out as the
handler makes them, which is what SSE needs and what a buffered answer cannot do:

```js
Xmas.serve(function (request) {
  const stream = new ReadableStream({
    start(controller) {
      const encoder = new TextEncoder();
      const timer = setInterval(() => {
        controller.enqueue(encoder.encode('data: ' + Date.now() + '\n\n'));
      }, 1000);
      request.signal.addEventListener('abort', () => {
        clearInterval(timer);
        controller.close();
      });
    },
  });
  return new Response(stream, { headers: { 'content-type': 'text/event-stream' } });
});
```

Three things make that work:

- The head is written before the body, framed chunked, because there is no length to declare.
- Chunks go through a bounded channel of four, so a slow client pushes back into the handler
  instead of the response growing in memory. The JS side sends one chunk at a time and waits for
  the connection to take it.
- `request.signal` is a real abort signal per request. It fires when the connection is gone, which
  is discovered when the next chunk cannot be written: the reader is dropped, the channel closes,
  and `__serve_chunk` resolves false. A handler that ignores it keeps its timer, so the listener in
  the example is not decoration.

A connection goes back to serving requests normally once a streamed body ends; the chunked framing
is what closes it off.

## WebSockets

A handler asks for the upgrade, gets a socket and a handshake response, and returns the response:

```js
Xmas.serve(function (request) {
  if (request.headers.get('upgrade') !== 'websocket') {
    return new Response('this endpoint wants a websocket');
  }
  const { socket, response } = Xmas.upgradeWebSocket(request);
  socket.onopen = () => socket.send('hello');
  socket.onmessage = (event) => socket.send('echo: ' + event.data);
  socket.onclose = (event) => console.log('closed', event.code);
  return response;
});
```

What happens under that:

- `__ws_new(key)` registers a socket under the client's `sec-websocket-key` and returns the id plus
  the accept value. The handshake response is a normal `Response` with status 101 and the three
  upgrade headers, so the handler can add headers of its own to it.
- The answer's body is an upgrade rather than bytes, so the connection stops being HTTP after the
  101 goes out: trillium's h1 loop hands its `Upgrade` back and `serve()` passes it to the worker's
  upgrade closure. That closure finds the socket by the client's key and starts the frame loop on
  the worker's own executor, on the same thread as the realm that holds the socket.
- The frame loop (`runtime/src/host/websocket.rs`) masks nothing on the way out, unmasks what comes
  in, assembles fragments, answers a ping with a pong, and echoes the peer's close before it ends.
  A message is handed to JS as one buffer: the opcode byte and then the payload.
- `runtime/js/websocket.js` is the object the handler sees: `readyState`, `onopen`, `onmessage`,
  `onerror`, `onclose`, `send`, `close`, plus `addEventListener`. It keeps one read outstanding
  and asks for the next message as soon as the last one is handled.
- Binary messages arrive as an `ArrayBuffer`, copies of a buffer the runtime reuses. Text arrives as
  a string.

## Numbers

macOS, M5, release, client and server on the same machine (8 client threads, keep-alive, one
outstanding request per connection), single worker, JSON answer from a JS handler:

| clients | requests | rps | p50 | p99 |
| --- | --- | --- | --- | --- |
| 1 | 2000 | 61k | 15.5 µs | 35.0 µs |
| 8 | 16000 | 162k | 44.9 µs | 121.1 µs |
| 32 | 16000 | 170k | 176.0 µs | 370.0 µs |

Answered from Rust instead of JS, same binary, same client: 141k to 177k rps, p50 43.7 µs, p99
103.6 µs. The crossing into the realm is not visible at this load; the client is. Two client
threads per core cannot push a server much past 170k requests per second while also being the
thing that generates them, so treat 170k as "not the server" rather than as a ceiling. The real
comparison has to be two machines, and that is what the benchmark step does.

Repeating the same run against 4 workers changes nothing measurable on macOS: SO_REUSEPORT binds
there, but the kernel does not distribute connections the way Linux does.

## Gaps, in the order they should be closed

- A request body is read into memory before the handler sees it. A 100 MB upload is 100 MB of
  heap, and the handler cannot start work while the body is still arriving.
- A streaming answer with nothing to send learns the client left only when it tries to send. A
  handler that waits a minute between chunks holds a dead connection for that minute.
- Backpressure is one chunk deep at a time: the JS side waits for each chunk to be taken before it
  reads the next one. A handler that produces faster than the wire can take it is slowed down, but
  the chunk it already handed over is not split.
- No TLS listener. The acceptor slot is there (`smol::net::TcpStream` in, `SmolTransport` out),
  but a certificate story is not.
- No HTTP/2: trillium-http has it, our accept loop only speaks h1.
- No graceful drain. Stopping the server drops the connections in flight instead of finishing them.
- One address per server, no unix sockets, no sniffing of the listener's own address into
  `request.url` (the host header is used).
- Imports inside the entry are not resolved (see the loader section above), and there is no watch
  mode: the realm is built once per worker and the entry is never re-evaluated.
- A websocket that the handler never answers parks one read on the socket and holds it; the entry is
  only released when that read is answered or the connection ends.
- Sending on a websocket is not a promise: a handler that produces faster than the wire can take it
  buffers in memory. There is no `bufferedAmount` and no `drain`.
- No permessage-deflate, no subprotocol negotiation beyond echoing nothing, and no server-side ping
  timer. A connection that goes silent stays open.
- A connection can only be upgraded when the request asked for it; the closure looks the socket up
  by `sec-websocket-key`, so a hand-made 101 with no registered socket logs and drops the
  connection.

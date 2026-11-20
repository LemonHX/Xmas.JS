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
head, and each chunk the handler produces is written with `__serve_chunk` as it is produced.

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

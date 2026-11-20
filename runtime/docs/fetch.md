# fetch

The web request API: fetch, Headers, Request, Response, FormData, File. Exported as
node:fetch as well, and installed as globals, because Response.body has to be the
same ReadableStream a script already knows (stream/web).

## Where the work happens

- host/http.rs is the transport: one trillium client per realm thread, so the
  connection pool belongs to the thread that made the request and no socket is
  shared between realms. HTTP/1.1 over http; over https, ALPN decides.
- modules/fetch.rs is the bridge: one host call per request, then one host call
  per body chunk. It also decompresses.
- js/fetch.js is the API: header folding and iteration, the body mixin, the
  constructors, and every promise.

TLS is rustls with the rustls-rustcrypto provider and the webpki roots compiled
in. No system certificate store is read, nothing in the build touches a C
compiler, and runtime/tests/purity.rs keeps it that way. A request to
https://example.com returns 200 with this setup.

## A body is a stream, and it waits in two places at once

A response body is a queue of two chunks between the socket and JS. The socket
side reads with a native read loop into that channel; the JS side asks for one
chunk per read through a promise settled on the realm thread. A slow consumer
therefore slows the socket instead of filling the heap, and a body nobody reads
costs one parked request.

The end of a body is a message rather than a closed channel. The realm thread
keeps a sender of its own so an abort can hand an error to a read that is already
waiting, and a channel with two senders never closes on its own.

## Decoding

gzip and deflate stream through flate2 as bytes arrive. gzip gets its own header
parser because the rust_backend build of flate2 has no gzip constructor for the
streaming decompressor: ten bytes, then the optional extra block, name, comment
and header checksum, then a bare deflate stream. A body that does not start with
the gzip magic goes to the inflater as it is, so a mislabelled body fails as a
decode error instead of disappearing.

brotli is collected whole and decoded at the end, because the brotli decoder
wants a reader. That is the one encoding that does not stream.

An encoding nobody here knows is handed over untouched, and the header is left
in place so the caller can see what it was. zstd is in that category.

## Redirects

redirect: 'follow' (the default) re-issues the request up to twenty times. 303
always becomes a GET, 301 and 302 do the same for a POST, and the body and its
content headers are dropped when that happens. Authorization is dropped when the
redirect leaves the origin, and the response's url is the original address, with
redirected set to true. redirect: 'manual' returns the 3xx as it is.

## Abort

An abort sets a flag, wakes the request out of whatever it is waiting for, and
pushes an error into the body channel so a waiting read rejects at once. A
request that has not been answered yet is abandoned rather than interrupted: the
future is dropped, which closes the connection.

## Cost

Measured with cargo run -p xmas-runtime --release --example fetch_bench on an M5,
500 requests to a keep-alive server on localhost, body read and thrown away:

    sequential, one at a time      201.1 us/request   4973 requests/s
    eight in flight                73.7 us/request  13573 requests/s

Those numbers are worth reading as a pair. The concurrent figure is what a
request costs when the loop is not the limit; the sequential one still carries
the cost of the driver noticing the result. Both used to be about fifteen times
worse for a reason that had nothing to do with fetch: the event loop waited for
host work by sleeping a millisecond. Pending now wakes the loop when a job
finishes, so a result is picked up as soon as it lands.

## Deliberate gaps

- A request body that is itself a ReadableStream is refused. Everything else
  (string, bytes, Blob, FormData, URLSearchParams) is read into memory first.
- Response.formData() only parses urlencoded bodies. Multipart parsing is not
  written yet; sending multipart works.
- No zstd.
- An unconsumed body keeps its connection busy until the realm ends. Undici
  behaves the same way until the collector runs, and there is no collector here.
- fetch does not read the http_proxy and https_proxy environment variables yet,
  although trillium-client can: with_proxy is not wired.
- Forbidden request headers are not enforced. There is no browser and no origin
  to protect, so the header syntax is checked and nothing else.

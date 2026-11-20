# sockets

connect() from the WinterCG sockets proposal, spelled the way cloudflare:sockets
spells it. Two names, one function: import { connect } from 'cloudflare:sockets'
and from 'sockets'.

    const socket = connect({ hostname: 'example.com', port: 80 });
    const writer = socket.writable.getWriter();
    await writer.write(new Uint8Array([...]));
    const reader = socket.readable.getReader();
    const { value, done } = await reader.read();
    await socket.close();
    await socket.closed;

A Socket is readable, writable, opened, closed, close() and startTls().

## Why this and not node:net

node:net, node:tls, node:http, node:dgram, node:dns and node:intl were dropped
from the plan. They are the part of Node where every platform difference, every
deprecation and every piece of hidden state lives, and this runtime is not
trying to be a Node host. What a script actually wants when it reaches for a
socket is a connection it can read and write bytes on, and that is what the
WinterCG proposal describes. Server work and the http layer are built here, on
top of this, not on a Node-compatible net module.

## Shape

There is no socket object in Rust. A socket is an id, and js/sockets.js holds
the two web streams, the opened promise and the closed promise. Rust holds the
connection.

A read happens because a reader asked: the ReadableStream has a highWaterMark of
0, so pull runs when a read is pending and not before. A 16 KiB buffer is filled
by one read from the socket and handed over as an ArrayBuffer; nothing is copied
twice and nothing is buffered that nobody asked for. A write happens because a
writer wrote: the bytes are copied on the realm thread, because the engine is not
reachable from the task that does the writing, and the promise resolves when the
socket took them.

Both are gated on opened, so a write issued immediately after connect() waits for
the connection instead of failing.

A refused connection rejects opened, and the two streams then reject on use.
opened reports remoteAddress and localAddress, the two ends of this connection.

writable.close() is a half close: the other side sees the end of the body and
whatever it still sends stays readable. socket.close() shuts both directions down
and resolves closed. Reaching the end of the readable side resolves closed as
well, so a script waiting on closed hears about a peer that hung up.

Name resolution is smol's, on a blocking thread. That is why there is no dns
module underneath: one call to connect is the whole story.

## Cost

64 MiB each way over a socket on localhost, cargo run -p xmas-runtime --release
--example sockets_bench on an M5:

    write through connect()    310.4 ms   206.2 MiB/s   18.95 us/4KiB chunk
    read through connect()      99.3 ms   644.8 MiB/s    6.06 us/4KiB chunk

Read chunks are 4 KiB here but the reader hands out 16 KiB at a time, so per host
call the two are the same number: about 20 us. That is the price of one round
trip through the event loop when a result has to come back from a thread that is
not the realm's, and it is the number to design against: a request that does one
read and one write pays it twice, and pay it a hundred times it shows.

## Deliberate gaps

- No TLS. secureTransport 'on' and startTls() are refused with a TypeError
  instead of being pretended. fetch has its own HTTPS client, using rustls.
- No UDP, no listening side, no unix sockets.
- connect() takes { hostname, port } only. The proposal also allows a
  "host:port" string; that is a five line change if it is wanted.
- allowHalfOpen and the rest of the options are not read.
- The Socket object is a plain object, not a class with getters.

# sockets

connect() from the WinterCG sockets proposal, spelled the way cloudflare:sockets
spells it, plus UDPSocket for the datagram side. Two names, one function: import
{ connect } from 'cloudflare:sockets' and from 'sockets'.

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

## UDPSocket

    const socket = new UDPSocket({ remoteAddress: '1.1.1.1', remotePort: 53 });
    const writer = socket.writable.getWriter();
    await writer.write(question);
    const answer = await socket.readable.getReader().read();

Note that the WinterCG proposal has no UDPSocket in it: it is TCP all the way
through, with one note saying datagram sockets would want message-oriented
streams. This is that note made real, in the same shape as connect(), because the
alternative was node:dgram and that is the API this runtime is not building.

Same members as a Socket: readable, writable, opened, closed, close(). No
startTls - there is no TLS for datagrams.

The options are remoteAddress and remotePort, plus optional localAddress and
localPort when something has to come from a known port. opened reports
localAddress and localPort, the address it actually bound.

There is no connection to open, so connect() the syscall is done at bind time:
the kernel then drops datagrams from anyone but that peer, which is what a socket
with one remote end means. The flip side is that this is a client-shaped API.
Reading from whoever happens to send is not in it, and neither are broadcast,
multicast or the ttl knobs.

Message edges are kept. One read is one datagram, exactly as it arrived: no
reassembly, no coalescing, and an empty datagram is still a datagram rather than
an end of stream. One write is one datagram, and a chunk over 65507 bytes is
refused with a RangeError instead of being split, because splitting would invent
edges the caller did not ask for.

There is also no end of stream to send: writable.close() does nothing on the
wire, and socket.close() is what ends the readable side - it wakes a read that is
waiting and reports done. A datagram socket has no EOF of its own, so the only
honest way a pending read can hear about a close is the socket telling it.

## Cost

64 MiB each way over a socket on localhost, plus 16384 datagrams each way,
cargo run -p xmas-runtime --release --example sockets_bench on an M5:

    write through connect()    321.6 ms   199.0 MiB/s   19.63 us/4096B
    read through connect()      95.4 ms   670.7 MiB/s    5.82 us/4096B
    datagram write             409.0 ms    19.6 MiB/s   24.97 us/512B
    datagram out and back      178.8 ms    11.2 MiB/s   43.66 us/512B

Read chunks are 4 KiB here but the reader hands out 16 KiB at a time, so per host
call the two are the same number: about 20 us. That is the price of one round
trip through the event loop when a result has to come back from a thread that is
not the realm's, and it is the number to design against: a request that does one
read and one write pays it twice, and pay it a hundred times it shows. A datagram
costs a whole round trip on its own because there is nothing to amortize: 25 us to
send one, 44 us to send one and get the answer.

Which is also why the datagram benchmark sends one and waits for the reply. The
first version of it fired 16384 datagrams at the loop as fast as it could and then
waited for 16384 back; it hung, because the receive buffer overflowed and the
missing ones were never coming. On a lossy transport a benchmark that does not
pace itself measures the kernel's queue, not the runtime.

## Deliberate gaps

- No TLS. secureTransport 'on' and startTls() are refused with a TypeError
  instead of being pretended. fetch has its own HTTPS client, using rustls.
- No UDP, no listening side, no unix sockets.
- connect() takes { hostname, port } only. The proposal also allows a
  "host:port" string; that is a five line change if it is wanted.
- allowHalfOpen and the rest of the options are not read.
- The Socket object is a plain object, not a class with getters.

# stream/web

The WHATWG streams API, shaped exactly as Node exposes it on node:stream/web and
as globals, since fetch needs these types to be the same ones a script already
knows.

## Where the line between Rust and JS falls

LLRT writes this module as 14,908 lines of rquickjs classes: the spec algorithms
run in Rust over rquickjs promises. That does not port: in this engine a promise
is resolved only from the thread holding the realm, so a WHATWG algorithm
written in Rust would hop to JS to settle something every other line, which is
slower and harder to keep correct.

The split is the one node:stream already uses here:

- stream_web.rs owns the queues: the byte buffer, the chunk offsets, the byte or
  chunk count, close and reset. Nine host functions, no state machine.
- stream_web.js owns the controllers, readers, writers and every promise.

What the native queue buys: a byte stream that has 40 chunks queued answers a
read with one ArrayBuffer of the right size, keeps a partially consumed chunk as
an offset instead of splicing it, and never puts the same bytes in the JS heap
twice. What it does not buy is the promise work, which is most of the cost; see
the numbers below.

## What is implemented

ReadableStream: source start/pull/cancel, high water mark and a size strategy,
default and byte sources, getReader for both modes, cancel, tee, pipeTo,
pipeThrough, values() and async iteration.

ReadableStreamDefaultReader, ReadableStreamBYOBReader, both controllers,
WritableStream with its writer and controller, TransformStream with transform and
flush, the identity transform, and both queuing strategies.

## Deliberate gaps

- byobRequest is always null. A source that wants to fill a view has to use
  autoAllocateChunkSize or enqueue itself. This is the one gap that would show up
  in code written against the spec's pull-into protocol.
- A BYOB read returns a view over the caller buffer instead of detaching it.
  Real code reads the value and moves on; code that checks for a detached buffer
  would notice.
- pipeTo takes preventClose, preventAbort, preventCancel and an AbortSignal. It
  does not implement the spec's "shutdown with an action" bookkeeping exactly,
  only the observable outcome.
- No ReadableStream.from, no ReadableStreamDefaultController size checks on
  enqueue after close (it throws a TypeError, which is the observable part).

## Cost

Measured with cargo run -p xmas-runtime --release --example web_stream_bench on
an M5, 20,000 chunks of 4 KiB, all figures from one run: absolute values drift
with clock scaling, so only the ratios mean anything.

    read (byte stream, default reader)   2.07 us/chunk   1885 MiB/s
    byob read (4 KiB view)               3.50 us/chunk   1116 MiB/s
    write (no-op sink, awaited)          1.50 us/chunk
    pipeTo                               10.10 us/chunk   387 MiB/s
    pipeTo through identity transform    36.36 us/chunk   107 MiB/s
    control: one await of a promise      1.33 us
    control: same array, no streams       0.73 us

Read the pipe numbers against the await control, not against node:stream. A pipe
costs what its promise hops cost: read returns a promise, write returns one, and
pipeTo waits for both per chunk, so about seven awaits show up in the ten
microseconds. The engine's await is 1.3 us here (0.68 us per job in the earlier
drain measurement), which is the floor this API can reach without promises that
resolve synchronously.

The identity transform doubles it, and that is the spec's default rather than a
bug: a TransformStream readable has a high water mark of zero, so the writable
side waits for the reader once per chunk. A plain pipeTo is the shape to reach
for when throughput matters, and node:stream is faster still (1.88 us per 4 KiB
chunk through one stage) because it has no promises on its path at all.

Two paths are cheap enough to use without thinking: a buffered read at 1.9 GiB/s
and a write into a sink that does not return a promise, which stays synchronous
inside the native queue.

## Gotchas worth writing down

- A readable that is closed with chunks still queued is not finished yet. A read
  that arrives after the queue drains has to report done itself, which is why
  every read path checks closeRequested after checking the queue.
- error() discards whatever is queued, per spec, so the first read after an error
  rejects rather than handing back the chunk that was already there.
- The writable loop resolves the caller synchronously when the sink's write
  returns nothing. Doing it with a promise per chunk cost 30% of the whole pipe.
- A transform checks backpressure before running the transform, not after: with a
  zero high water mark, a transform that ran first would enqueue into a queue
  nobody may read yet and then wait for itself.

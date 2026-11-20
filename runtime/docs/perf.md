# Stream and buffer performance

Measured with `cargo run -p xmas-runtime --release --example stream_bench`
(macOS arm64, M5, one process, one thread; compare numbers only within a run).

## Current numbers

| what | per chunk | throughput |
| --- | --- | --- |
| push a 4 KiB Buffer into a Readable | 482 ns | 8.1 GiB/s |
| drain it with read() | 1.72 µs | 2.3 GiB/s |
| push a 64 B Buffer | 402 ns | — |
| drain it with read() | 1.11 µs | — |
| 4 KiB through source -> writable | 1.88 µs | 2.0 GiB/s |
| 4 KiB through source -> PassThrough -> writable | 4.2 µs | 926 MiB/s |
| 4 KiB strings through the same pipeline | 5.2 µs | 755 MiB/s |
| 64 B through the same pipeline | 4.1 µs | — |

Reference costs in the same process:

| operation | cost |
| --- | --- |
| emit('x', value) with one listener | 163 ns |
| Buffer.from(arrayBuffer), a 4 KiB view | 831 ns |
| Buffer.alloc(4096) | 5.4 µs (724 MiB/s) |

## What it costs and why

The native core owns the chunk queues, so a queue holds raw `Vec<u8>` and
`readableLength()` is a counter. That part is cheap: 482 ns per 4 KiB push.

The behaviour layer is the floor. One pipeline stage costs about 1.9 µs of
plain JS: the push path, the write path, an emit, and the two closures a write
needs. Two stages therefore cost about 4.2 µs per chunk, and 64 B chunks are
bound by that fixed cost rather than by bytes. This is the engine's interpreter
speed, not something a different data structure fixes.

## Two things that were slow and are not any more

Encoding conversion used to go through base64 in JS on both sides. A 4 KiB
string chunk in a pipeline spent about 360 µs in the JS decoder, which is
85 ns per byte. Moving to an ArrayBuffer in and out of the native encoders
(`__text_to_bytes`, `__bytes_to_text` in buffer.rs) took that to 5.2 µs, and it
removed both JS codecs entirely.

`emit` used to copy the listener list on every call. Now it copies only for
event names that have a `once` wrapper registered, because only those can
remove a listener while the list is being walked.

The writable side used to queue every chunk in the native buffer and shift it
right back out. When the writer is idle the chunk now goes straight to `_write`,
which removes two host calls per write. `drain` is emitted only after a write
that came back false, instead of after every write.

## Numbers that are the engine, not us

`Buffer.alloc(4096)` costs 5.4 µs, which is the engine zeroing 4 KiB at about
700 MiB/s. A fresh Buffer per chunk is expensive by itself; streams should hand
out views over ArrayBuffers, which is what the native read path does.

The heap cap matters more than it looks: the collector is mark and sweep over
the whole realm, so a heap stuffed with live objects makes every collection
longer. Every realm should be created with a `max_bytes` that matches its
workload; the harness default is 64 MiB.

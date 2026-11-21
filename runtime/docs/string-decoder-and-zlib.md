# string_decoder and zlib

## string_decoder

Copied from `llrt_string_decoder/src/string_decoder.rs`, which adapted Node's
`src/string_decoder.cc`. The interesting part is not the decoding, it is the
chunk boundary: a character split across two writes must be held back, because
emitting half of it produces a replacement character that cannot be taken back.

The algorithm is line for line, comments included. What changed is the binding:

- `Encoder` (all of `llrt_encoding`) became a six-variant enum over
  `modules::buffer::decode`. The alias table is LLRT's, cut down to the labels a
  StringDecoder accepts, and it normalizes to the spelling Node reports on
  `decoder.encoding` (`utf-8` becomes `utf8`).
- `Ctx` disappeared: the decoder loop returns `String`s and the host functions
  turn them into JS values.
- `__sd_create` returns one string, `label\0id`, so a constructor is one host
  call instead of two.

`end()` flushes the pending bytes but does not destroy the decoder, so `write`
after `end` still works, which is Node's behaviour. Destroying it made a
legitimate Node pattern throw.

Encodings covered: utf8, utf16le, latin1/binary, ascii, hex, base64, base64url
and their aliases.

## zlib

The one-shot functions are LLRT's `llrt_zlib` plus `libs/llrt_compression`: the
same read adapters, and the same `maxOutputLength` handling, which reads one
byte past the limit instead of inflating the whole payload first.

Three departures:

- **zstd is not there.** There is no pure-Rust zstd encoder and this runtime
  links no C, so `zstdCompress`/`zstdDecompress` are absent rather than wrong.
- **Async work leaves the realm thread.** LLRT ran the conversion inline in
  `spawn_exit`; here the bytes are copied and the conversion runs on
  `smol::unblock`'s blocking pool, settled through `host::pending`. A 45 KiB gzip
  is 46 us, and blocking the realm for that inside a request costs more than the
  copy.
- **The JS layer owns the class shape.** `__zlib_run` returns a raw ArrayBuffer
  and the prelude wraps it in `Buffer.from`, which is a view, not a copy (0.5 us
  for 45 KiB). Errors: the engine's native errors all come out named `Error`
  here, so `RangeError` travels as a message prefix and the prelude rebuilds the
  class; anything else becomes `Error` with `code: 'Z_DATA_ERROR'`.

`crc32` is ours, not LLRT's. The bit-at-a-time CRC cost 301 us per 45 KiB;
the table-driven one costs 87 us in the same run.

Cost, `cargo run -p xmas-runtime --release --example zlib_bench` (M5, one run,
45,000 bytes of English text):

| case | us/call |
| --- | --- |
| gzipSync, 14 bytes | 7.7 |
| gunzipSync, 14 bytes | 4.0 |
| gzipSync, 45 KiB | 46.4 |
| gunzipSync, 45 KiB | 23.7 |
| brotliCompressSync, 45 KiB | 123.9 |
| brotliDecompressSync, 45 KiB | 19.0 |
| crc32, 45 KiB | 87.1 |
| Buffer.from(ArrayBuffer), 45 KiB | 0.5 |

Absolute numbers move with the machine's clock; only compare within one run.

## Not there yet

- The streaming classes: `createGzip`, `createDeflate`, `createGunzip`,
  `createInflate`, `createBrotliCompress` and their friends. They need a
  stateful compressor held in Rust (`flate2::Compress` with `FlushCompress`), and
  brotli's equivalent is a different shape again. Tracked as its own step.
- Options other than `level` and `maxOutputLength`: `windowBits`, `memLevel`,
  `strategy`, `dictionary`, `flush`, brotli's `params`. They are accepted and
  ignored.
- `zlib.codes`, `zlib.Z_MIN_CHUNK` style extras beyond the constants table.

## A wart worth knowing

An exception thrown by a host function while a **module** is being evaluated
does not come back out: `execute_module` returns Ok, the module's remaining
statements never run, and the thrown value is gone. Two of the tests here hit it
before the code was right, and the symptom is a module namespace that is
`undefined`. Throwing from a host function during plain script evaluation works
as expected.

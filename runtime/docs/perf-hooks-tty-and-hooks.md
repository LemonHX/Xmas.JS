# perf_hooks, tty, async_hooks and DOMException

## perf_hooks

LLRT's `llrt_perf_hooks` is 228 lines: a Performance class with `now()`,
`timeOrigin` and `toJSON()`, installed as a global and re-exported as a module.
Node also has marks, measures and PerformanceObserver, and those are the parts
tooling calls, so they are here. The class shape (an EventTarget subclass, the
module default being the same object as `globalThis.performance`) is LLRT's.

Only the clock crosses the boundary, as one host call returning an f64
(`__perf_now_ns`). `timeOrigin` is a second one, read once: the wall clock at
the moment the realm's clock started, which is what Node reports for the
process. Entry lists, lookups and observers are plain JS arrays.

Cost, `cargo run -p xmas-runtime --release --example perf_bench` (M5, one run;
an empty JS call is 26 ns and is the floor everything else sits on):

| case | ns/call |
| --- | --- |
| empty call (floor) | 25.6 |
| `performance.now()` | 101.4 |
| `performance.mark('m')` (with the list cleared after) | 682.2 |
| `measure()` with numeric start and duration | 736.7 |
| `measure()` between two mark names | 838.5 |
| `getEntriesByName` on 1000 marks | 16170 |
| `storage.run(store, fn)` then `getStore()` | 340.2 |
| `storage.bind(fn)` then call it | 542.5 |

`now()` is 75 ns above the floor, all of it the host call. Node's is nearer
30 ns because v8 answers it in the isolate. If a hot path ever needs the clock
more than a few thousand times per request, this is the number to beat.

Two things were made cheap on purpose:

- `getEntriesByName` and `getEntriesByType` are plain for loops, not `filter`.
  A closure call per entry turned 16 us of scanning into 30 us on a thousand
  marks, and a closure is 30 ns of pure overhead in this engine.
- `measure(name, startMark, endMark)` walks the entry list backwards instead of
  calling `getEntriesByName` and allocating a result array. 1.6 us became 0.84.

The entry list grows with every mark, and lookups walk it, so a long-running
process that marks per request keeps paying more. Node behaves the same way;
nothing here pretends otherwise.

## tty

LLRT's `llrt_tty` is `isatty` over libc. That is kept, with the unsafe call
moved into `host/sysinfo.rs` where the other libc FFI lives, plus
`window_size` (TIOCGWINSZ) for `columns`, `rows` and `getWindowSize`.

Node's `WriteStream` and `ReadStream` are shapes over the stdio that
process.js already holds: there is one console per realm, so fd 1 and fd 2
delegate to `process.stdout` and `process.stderr`. `clearLine`, `clearScreenDown`,
`cursorTo` and `moveCursor` write the ANSI sequences; `getColorDepth` reads
`FORCE_COLOR`, `NO_COLOR`, `COLORTERM` and `TERM` rather than consulting a
terminfo database.

Not there: raw mode, key events, and fd watching. `ReadStream` reports
`isTTY` correctly and reads nothing, because nothing in this runtime can watch a
descriptor yet. That is the same gap `net` will close.

## async_hooks

This is the one module where LLRT's implementation cannot be carried over.
LLRT rides on rquickjs's promise hook, which wraps C QuickJS's
`JS_SetPromiseHook`: every promise creation, before, after and resolve comes
back through Rust, and that is what makes `AsyncLocalStorage` follow an
`await`. This engine exposes no such hook (the API surface has none), so:

- `createHook` returns an object with `enable`/`disable` and the callbacks it
  was given. The callbacks never fire.
- `executionAsyncId`, `triggerAsyncId` and `currentId` return 0. LLRT returns 0
  from `currentId` for the same reason and says so in a comment.
- `AsyncLocalStorage` implements `run`, `enterWith`, `exit`, `getStore`,
  `disable`, `bind` and `snapshot`. A store survives synchronous code and
  anything `bind()` wrapped. It does not follow an `await`, because there is no
  way to know which store a resumed continuation belongs to: the continuation is
  a microtask the engine owns.
- `AsyncResource` is a shell with the right method names.

`run` restores by truncating the store stack to its previous depth instead of
copying it, which is correct because the stack is only ever pushed at the end.

If the goal is per-request state, this runtime has a better answer than a store:
one realm per request (t13). Two requests then share no globals at all, so the
question "which request am I in" does not come up. `AsyncLocalStorage` is here
for code written against Node, not because a server here needs it.

## DOMException

LLRT built it in Rust because rquickjs has class macros. Nothing in it is
native: the WebIDL name table, the legacy `code` numbers and the
`QuotaExceededError` subclass are data, so the whole module is the prelude
`js/exception.js` and `modules/exception.rs` only evaluates it.

Like LLRT, `DOMException` derives from `Error` so `instanceof Error` holds;
the spec says otherwise and Node follows the spec. This is a deliberate match
to LLRT, and one of the few places where Node compatibility was traded for the
thing being copied.

Installed as a global, not as a module: `DOMException` is a global in Node and
in every browser.

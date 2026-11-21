# util and assert

## util

The inspect walker is copied from LLRT (`libs/llrt_logging/src/lib.rs`,
`format_raw_inner` and `write_object`), and so is the styleText table
(`llrt_util/src/style_text.rs`). LLRT keeps the format spec walk in the same
file (`build_formatted_string`); here that part is in JS, because it is a scan
over an already-built string and doing it natively would only add a call.

What changed in the copy: reflection. LLRT reaches into values through rquickjs'
`Object`, `Type`, `Coerced` and a primordials cache it fills once per runtime.
This engine exposes the same information on the `Js` trait, which `Scope`
implements:

| LLRT | here |
| --- | --- |
| `value.type_of()` | `js.type_of(&value)` |
| `obj.keys()` | `js.own_property_keys` + `js.get_own_property` for `enumerable` |
| `PromiseState` | `js.promise_snapshot` (there is no Promise variant in `Value`) |
| `Symbol::description` | `js.symbol_description` |
| `get_class_name` | `value.constructor.name` through `get_property` |
| `Coerced<String>` | `js.to_js_string` |

Differences from LLRT worth knowing:

- A container that fits on one line is written on one line (Node's rule), because
  a container is rendered compactly first and measured. LLRT always indents an
  object at depth 0, so `{ a: 1 }` came out over three lines. The compact
  rendering is thrown away when it does not fit, which means an accessor on a
  nested object can run twice.
- Cycles are tracked against the current ancestors and popped on the way out, so
  the same object appearing twice side by side prints twice; LLRT keeps one set
  for the whole walk and calls the second one circular too.
- `Map` and `Set` print with their class name and no members, as LLRT does;
  Node prints `Map(1) { 'a' => 1 }`. Nothing needs it yet.

Not implemented: `util.getSystemErrorName` (Node's names come from libuv's errno
table; errors here carry their code as a string), `util.transferableAbortSignal`,
`util.TextEncoder`/`TextDecoder` (they are globals in Node and ours come from
t27). `util.inspect` honours `colors`; `depth`, `showHidden` and `breakLength` are
accepted and ignored, the walker's depth is LLRT's fixed four.

Cost, measured with `cargo run -p xmas-runtime --release --example inspect_bench`
(M5, per call, same-run comparison only):

| case | ns |
| --- | --- |
| inspect a string | 215 |
| inspect `{ a: 1, b: 'two' }` | 1,237 |
| inspect four levels deep | 3,565 |
| inspect an array of 100 numbers | 19,985 |
| `format('%s is %d', ...)` | 1,550 |
| `format('plain text')` | 449 |
| `isDeepStrictEqual` on two objects | 4,118 |
| an empty JS call, for scale | 14 |

Logging is not a hot path, so this is not optimised further; if it ever is, the
first thing to remove is the double render of containers that do not fit.

`console` formats with `util.format` now, as Node's console does, so
`console.log('a', 1, { b: 2 })` prints `a 1 { b: 2 }` instead of JSON.

## assert

LLRT's assert (`llrt_assert/src/lib.rs`) is 78 lines and implements
`assert.ok` only, so there was nothing to copy past its rules: only the falsy
values fail, a string message replaces the default, an Error message is thrown
as it is. The rest of Node's surface is in `runtime/js/assert.js`:
`equal`, `notEqual`, `strictEqual`, `notStrictEqual`, `deepEqual`,
`notDeepEqual`, `deepStrictEqual`, `notDeepStrictEqual`, `throws`,
`doesNotThrow`, `rejects`, `doesNotReject`, `match`, `doesNotMatch`,
`ifError`, `fail`, `AssertionError`, and `assert/strict` registered as the same
object.

It lives in JS because every check it makes reads arbitrary JS values and their
prototypes; doing it natively would be one host call per property for no gain.
The one native thing it uses is `util.inspect`, for the messages.

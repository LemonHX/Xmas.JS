# WebAssembly

The standard JS API, on top of WNJR. `runtime/docs/wnjr.md` is the feasibility
study and covers the engine side; this file is what the host actually does.

## Where the code is

| File | Holds |
| --- | --- |
| `runtime/src/modules/wasm.rs` | every slot, and every call into WNJR |
| `runtime/js/wasm.js` | the namespace, the classes, the argument shapes |
| `runtime/tests/wasm.rs` | 34 tests over the seam |

The split is the one every module in this runtime uses: JS owns what the spec
describes in JS terms, Rust owns state and calls the engine.

A `Module`, an `Instance`, a `Memory`, a `Table` and a `Global` each hold a host
object whose slot is the Rust side. An exported function is not a host object: it
is a plain JS closure over the instance and the export name, made at
instantiation, because the spec has no callable object of its own for these.

## Calling wasm, and wasm calling back

`__wasm_call` takes the arguments, converts them, runs `Instance::call`, and
converts the results back. `i64` crosses as a `BigInt` in both directions, as
the JS API says.

An imported function is a closure over a `CallableRef` and a `Scope` that is not
available when it runs: inside a host function the engine's runtime is borrowed,
so `Context::*` answers `Reentered` and there is no second `Scope` to be had. So
the host function that drives wasm parks its `Scope` in a thread-local
(`RUNNING`) for the length of the call, and the import closures read it back.
That is sound because wasm runs synchronously inside that host function: the
`Scope` outlives every import the run makes, on the same thread, and the
thread-local is cleared before the host function returns. It is the only unsafe
in this module.

`WasmError` carries a `&'static str`, so a real message from an import cannot ride
out through it. The message is parked in an `Rc<RefCell<Option<String>>>` on the
way out and the wasm boundary reports `an imported function threw`; the parked
string is what the JS side unwraps.

## The import object

Driven by the module's declarations, not by the object's keys. `Module::imports()`
says what the module wants; anything in the object the module does not want is
ignored, and anything wanted and missing is a `LinkError` naming both the module
and the field.

v1 takes function imports. A module that declares a memory, table, global or tag
import is refused with a `LinkError` that says which kind, because there is no JS
wrapper that could be handed in yet.

## Linear memory

WNJR reserves the maximum up front and commits pages in place, so growing does
not move the base address. What changes is the JS object that names those bytes:
the grow listener sets a flag on the memory slot, and the next time the realm's
thread is back in host code -- after an exported call returns, at the entry of an
imported function -- the flag is flushed, the old `ArrayBuffer` is detached, and a
new one is made over the same base at the new length. Nothing runs in the
listener itself; it holds no engine calls and no locks.

That is what makes `memory.buffer` after a grow a different object with the same
bytes, and a view on the pre-grow buffer a `TypeError` -- which is what the spec
asks for.

`memory.grow` past the maximum answers `-1`; it does not throw.

## Tables, globals, references

`table.get(i)` hands back a funcref, and a funcref is not a JS value. What comes
back is a closure over the reference and the instance that owns it; calling it
goes back through that instance. This needed `Instance::func_from_value` on the
WNJR side -- `Func::to_value` existed and its inverse did not -- which resolves a
reference through the world's function registry and refuses one whose owner is
not this instance, or one from another world.

Out-of-range `table.get` is a `RangeError`. `global.value` reads and writes, and
refuses a write to an immutable global with a `TypeError`; a global holding a
funcref reads back as a function the same way a table element does.

## Streaming

`compileStreaming` and `instantiateStreaming` collect the body and compile it in
one go. That is a deviance from the names and it is deliberate: the engine
compiles bytes, so reading a body chunk by chunk would add a state machine and
buy nothing. The `application/wasm` content type is still checked, because a
response of the wrong type is a mistake the caller wants to hear about.

## What v1 does not do

| Gap | Why |
| --- | --- |
| `new WebAssembly.Table(...)` and `new Global(...)` | the descriptor's element and value types need an engine-side constructor the host does not have; both throw a `TypeError` that says so |
| `Table.prototype.set` / `grow` | the same constructor gap, plus a funcref has no JS value to write in |
| `Module.customSections` | WNJR does not keep custom sections |
| `externref` | the engine knows the type, but no JS value converts to or from one yet, so a module that uses it is not exercised |
| grow notifications across agents | a memory grown on one agent's thread does not wake another's; v1 has one agent |
| `instantiate` on a `Module` with imports that are not functions | see the import object section |

## Tests

`cargo test -p xmas-runtime --test wasm`, 34 tests. The hand-written wasm bytes are
built by a `sec(id, payload)` helper that writes the ULEB length, because a
length typed by hand is wrong the first time and the error it produces
(`Malformed: unexpected end of input`) does not say which section.

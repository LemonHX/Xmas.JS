# WebAssembly: the standard JS API over WNJR

The `WebAssembly` namespace as browsers and Node expose it, backed by
`../Silverfir-nano` (WNJR). This is a plan, not a record of work: nothing below is
implemented. Every claim was read out of the two checkouts; WNJR line numbers are
from its HEAD unless stated otherwise.

Scope: `WebAssembly.compile`, `compileStreaming`, `instantiate`,
`instantiateStreaming`, `validate`, the five primitives `Module`, `Instance`,
`Memory`, `Table`, `Global`, and the three error classes `CompileError`,
`LinkError`, `RuntimeError`.

## What WNJR already gives us

- `Module::new(name, bin)` parses and fully validates, and does not compile
  (`wnjr-core/src/module/mod.rs:60`). `Module` is `Clone`, and carries the JIT
  code cache: `code: Arc<Mutex<Option<PrecompiledModule>>>`
  (`module/mod.rs:41`), filled at instantiation and reused by later instances.
- `Module` is `Send + Sync` (`module/mod.rs:52-54`), so a parse and validate can
  move to another thread.
- `Instance::from_module(engine, module, imports)` (`vm/instance.rs:299`), and
  `Instance::new` over a byte slice (`:290`). Both report
  `InstanceInstantiationError`, which separates "nothing happened" from "partly
  linked" (`vm/instance.rs:166`).
- `Instance::exports()` gives `(name, Extern)` pairs (`:253`), `get_export(name)`
  gives one (`:240`).
- `Instance::get_func(name)` resolves an export to a `Func` handle once (`:334`);
  `Instance::call(func, args, results)` calls it into a slice the caller owns
  (`:369`); `invoke(name, args)` is the by-name shortcut (`:331`).
- `Instance::get_global(name)` reads an exported global (`:454`).
- `Instance::memory()` hands out a `MemoryView` that derefs to `[u8]`
  (`vm/memory.rs:92`), `memory_mut()` the writable one (`:124`). Both keep the
  world from executing until they drop.
- Host imports take a closure, not a function pointer: `Import::func(module,
  name, f)` where `F: Fn(&mut Caller, &[Value], &mut [Value]) -> Result<(),
  WasmError> + 'static` (`vm/imports.rs:140`). Captured state is allowed and
  there is no `Send` bound. `Import::func_typed` adds a `FunctionType` that
  linking checks (`:164`). The bare `fn` pointer type `HostFn`
  (`vm/entities.rs:28`) is still there but is not the path an embedder uses.
- `Import::memory`/`memory64`/`memory_with_limits` (`:195`, `:208`, `:221`),
  `Import::table`/`table64`/`table_with_limits` (`:253`-`:271`),
  `Import::global` (`:186`).
- `Import::shared_memory(module, name, SharedMemHandle)` (`:240`) is the one
  place where the host creates the memory and keeps its identity.
  `SharedMemHandle::new(limits)` is public and `Send + Sync`
  (`vm/entities.rs:347-380`).
- `Import::tag_typed_with_handle` (`:309`) is the same pattern for tags: the
  host mints an identity and keeps the handle.
- `Engine::new(Config)` (`vm/engine.rs:82`), with budgets for the code arena,
  wasm memory pages, wasm stack, compiler RAM and parallel compilation
  (`config.rs:104-124`).

So the two-part shape the JS API needs is already there: a module is parsed and
validated once, and instantiated many times, and it crosses threads.

## API by API

| JS API | Backed by | Missing |
| --- | --- | --- |
| `validate(b)` | `Module::new("", b).is_ok()` | nothing |
| `compile(b)` | `Module::new` on a worker thread | a way to return a Rust value through `pending`; see W8 |
| `compileStreaming(r)` | read `Response.body` to bytes, check `application/wasm` | JS only |
| `new Module(b)` | `Module::new` inline | message text, W7 |
| `instantiate(bOrModule, imports)` | `Instance::from_module`, then build the exports object | import object to `Import` list; exports need kinds, W1/W2 |
| `instantiateStreaming(r, imports)` | the two above over a body | JS only |
| `new Instance(module, imports)` | `from_module` | as above, plus a `LinkError` that names the import, W1/W7 |
| `Module.exports(m)` | descriptor list | W1 |
| `Module.imports(m)` | descriptor list | W1 |
| `Module.customSections(m, name)` | raw section bytes | not exposed by the parser; skip in v1 |
| `instance.exports.f(...)` | `get_func` + `call` | parameter and result types on `Func`, W2 |
| `new Memory({initial, maximum})` | nothing | W3 |
| `memory.buffer` | nothing | over WNJR's reservation, needs the engine edited, W4 |
| `memory.grow(n)` | `MemInst` is crate private | W3, and commits pages under a stable base |
| `new Table({...})`, get/set/grow/length | `TableInst` is crate private | W5 |
| `new Global({value, mutable}, v)`, `.value` | import by value only | W6 |
| `CompileError` | `Malformed`, `Invalid` | W7 for a message with the offset |
| `LinkError` | `Unlinkable` | W7 for a message with the import name |
| `RuntimeError` | `Trap`, `Exhaustion`, `Exit`, `Exception` | mapping, plus `exception_fields` for payloads |
| shared memory and `Atomics` | `SharedMemHandle`, a reservation whose base never moves | W4 makes it reachable; `Atomics.wait` also needs a `Waiter` in the runtime |

## Changes on the WNJR side

**W1. Module introspection.** For each export: name and kind. For each import:
module, name, kind and type. Every accessor is `pub(crate)`
(`module/mod.rs:142-176`), so an embedder cannot see what it is about to link.
Needed by `Module.imports`/`Module.exports`, and needed by us to write a
`LinkError` that names the thing that is missing instead of "unlinkable".
Small.

**W2. Extern and Func accessors.** `Extern` is opaque (`vm/imports.rs:99`): no
way to ask what kind it is, no way to get at the handle, so `instance.exports`
cannot be built. `Func`'s fields are private (`vm/instance.rs:199-206`), so a JS
wrapper cannot tell that a parameter is `i64` and has to be handed over as a
BigInt. Needed: kind, typed conversions, and `Func`'s parameter and result
types. Small.

**W3. A memory the host can name.** `MemInst` is `pub(crate)`
(`vm/entities.rs:335`), `grow` is not exposed at all, and `Import::memory`
carries limits only, so the engine allocates a memory the host cannot name
afterwards. Both of WNJR's memory kinds are reservations whose base never moves:
`GuardPageMemory` takes 8 GiB + 64 KiB of address space up front and `grow`
only commits more pages (`vm/jit/runtime/guard_pages.rs:23-27`, the doc comment
at `:43-44`, `grow` at `:100-120`), and `SharedMemHandle` does the same
(`vm/jit/shared_memory.rs:76-109`). So what WNJR should hand out is one handle:
`(base, len, id, keep-alive)` plus `grow(delta_pages)`, and the base it reports
is the base it keeps for the memory's life. That single handle serves
`new WebAssembly.Memory` and `memory.buffer` both. Medium.

**W4. The bytes behind `memory.buffer`.** With the engine open, this stops being
the hard part: the guest's pages are a reservation with a stable base, so the
engine can hold a buffer over them instead of over its own allocation.

What the engine needs, all of it in `core/src/heap/buffer.rs`:

- `SharedBytes` holds `bytes: Box<[AtomicU8]>` and a length (`:15-18`). The bytes
  become a base pointer, a capacity, a length, a key, and an owner that is either
  `Own(Box<[AtomicU8]>)` or `Foreign(Arc<dyn Any + Send + Sync>)`. `byte(i)`
  (`:52`) is then the one place that dereferences, and every read, write and
  atomic operation already goes through it (`:120-124`, `:136-140`).
- `Store::Owned(Vec<u8>)` (`:60`) needs the same region type, because an unshared
  memory is an `ArrayBuffer` and that is the owned arm. The mechanical fallout is
  28 match sites in four files: `heap/buffer.rs`, `builtins/buffer.rs`,
  `builtins/agent.rs`, `builtins/atomics.rs`.
- The crate root says `#![forbid(unsafe_code)]`. Adopting foreign bytes is unsafe
  by definition, so that becomes `deny` plus `#[allow(unsafe_code)]` on this one
  module. There is no way around it, and keeping it to one file is worth the
  exception.
- Identity has to stop being the allocation address. `Atomics.wait` and
  `notify` key on `Arc::as_ptr(shared)` (`builtins/atomics.rs:287`); two realms
  wrapping the same WNJR region make two `Arc`s and two keys, so a wait in one
  never sees a notify from the other. The key comes from the region — W3's `id`.
- Two embed entries: `new_array_buffer_over(base, len, max, owner)` and
  `new_shared_array_buffer_over(...)`. Growth allocates a new buffer record over
  the same base at the new length and detaches the old one, which is what the
  spec asks for anyway (`Buffer.detached`, `:68`) and costs nothing.

`array_buffer_bytes` should keep returning `None` for these buffers. If the engine
never holds a `&[u8]` over pages the guest writes, there is no aliasing hole to
argue about, and access goes through `read`/`write`, where the shared arm already
does one relaxed atomic per byte. Bulk copies are byte-at-a-time until a fast path
is added; measured cost, not a correctness question.

What this buys: the base is stable on both sides, so `memory.grow` is WNJR
committing pages plus a fresh `ArrayBuffer` over the same base, with no copy
anywhere. `memory.buffer` is the guest's pages rather than a picture of them,
which is what browsers do and what the reservations in WNJR were built for.

What is dropped with it: the earlier plan to borrow an engine-allocated
`ArrayBuffer` and copy on growth, the aliasing hole that came with writing
through a pointer derived from `&[u8]`, and the WNJR-side entry that took an
external base of fixed length. None of them are needed once the engine can adopt
the guest's region instead.

The one edge left is honest: the guest writes those bytes with plain stores out
of JIT code while the engine reads them as atomics, which is a mixed
atomic/non-atomic race. Every engine that shares a linear memory with JS has this
shape. It is recorded, not hidden.

## Changes on our side

**O1. A module, in the shape every other one has.** `runtime/src/modules/wasm.rs`
with `const PRELUDE: &str = include_str!("../../js/wasm.js")` and
`pub fn install(driver)`, registered in `modules/mod.rs:41`. The namespace is a
global only; Node has no `node:wasm` module to mirror. It has to be installed
before `fetch` only if something in fetch wants it, which nothing does, so it
goes near the end.

**O2. A Module slot.** `scope.new_host_object(ModuleSlot { module }, Some(&proto))`
read back with `host_data::<ModuleSlot>`, the same shape `temporal.rs:357` and
`:373` use. `Module` is not `Rc`, but the slot is shared as one.

**O3. An instance table.** `HashMap<u32, Instance>` behind an `Rc<RefCell<_>>` in
the module's own state. The exports object holds the id and caches the `Func`
handles; `Func` is `Copy`.

**O4. Import closures route through a thread local.** `Import::func` wants
`Fn + 'static`, and the realm lives on the thread, so the closure cannot capture
anything from the realm. It captures an index and looks the realm up on the
calling thread. That is not a WNJR limitation, it is the shape a `'static`
closure forces.

**O5. `pending` needs to carry a Rust value.** `Payload` is `Bytes` or `Text`
(`runtime/src/host/pending.rs:17`), and `start` requires
`F: Future<Output = Reply> + Send + 'static` (`:132`). A compiled `Module` is
neither. Add a third payload that boxes `dyn Any + Send`, or a typed sibling of
`start`. Small, and it is useful beyond wasm.

**O6. i64 crosses as BigInt.** Our bridge already round-trips BigInt for
Temporal, but only through the constructor. Every wasm `i64` parameter and
result needs the same treatment, driven by the types from W2.

**O7. Exceptions.** `WasmError::Exception` carries a `RefValue` and a tag;
`Instance::exception_fields` resolves the payload. Thrown out of an exported
function it should become a JS exception, with the payload as the thrown value
when there is exactly one field.

**O8. A `Waiter`.** `Atomics.wait` and `notify` go through a coordinator the host
installs (`quickjs-oxide/core/src/vm/waiter.rs:41-59`, set with
`Runtime::set_waiter`, `core/src/runtime.rs:544`), and `wait` refuses to run
when none is installed (`core/src/builtins/atomics.rs:317`). Nothing in
`runtime/src` installs one, so today every other `Atomics` operation works and
`wait` throws. A `Mutex<HashMap<(u64, usize), Condvar>>` behind that trait is
enough. This is a prerequisite of shared memory being useful, not a nicety.

## The open question: a JS function as an import

This is the one part with no clean answer today, so it is worth stating before
any of the above is built.

To run `new WebAssembly.Instance(m, { env: { log: (x) => ... } })`, WNJR calls
our import closure in the middle of a wasm call. That wasm call was started from
inside one of our own host functions, which holds `&mut Scope`. Calling a JS
function needs a `Scope` (`Js::call`, `engine/api/core/ops.rs:1303`), and the
only `Scope` in the process is the one the outer frame is holding. Rust will not
lend it twice. The engine documents this boundary from the other side too: a
`Context` method called during a host function returns `Reentered`
(`engine/api/README.md:120`).

Three ways out, in the order I would take them.

1. Ask quickjs-oxide for a supported way to nest: a second `Scope` for the
   duration of a foreign callback. The pieces look like they are already there,
   since `Scope` holds a `NativeCx` that is reborrowed per operation
   (`engine/api/core/function.rs:19-28`), and `RuntimeError::Reentered` is a
   guard on specific entry points rather than on calling functions. This is the
   right fix: the engine owns the safety argument, not us.
2. A thread local raw pointer to the core state, used for exactly one purpose,
   in exactly one file. It would be the first `unsafe` in `runtime/src`, and
   `tests/purity.rs` fails the build on it today, which is the point of that
   test. If we go this way the guard grows an allowlist of one, and the safety
   comment has to say why the outer frame cannot have been dropped.
3. Do not support it in v1. Imports that are host values (memory, global, table)
   and host functions we write natively work; a JS closure as an import throws
   a clear error. Modules that only need math and memory run fine either way,
   and most non-wasm-bindgen modules are in that class.

I would build the first three steps with option 3 and settle this question with
evidence before writing any of O4's call path.

## Order

1. `validate`, `Module`, `compile` over a byte array, then `instantiate` and a
   call into an exported `add`. Proves parse, validate, instantiate and call.
2. The exports object with kinds (needs W1, W2), and an `i64` round trip as
   BigInt (needs O6).
3. Import objects with numeric values and host-provided functions we write
   ourselves. Proves the closure plumbing from O4 without the open question.
4. `Memory` (needs W3) and `memory.buffer` over WNJR's reservation (W4), growth
   included and free: a new `ArrayBuffer` over the same base. Shared memory in the
   same step once the runtime installs a `Waiter`.
5. `Table` and `Global` (needs W5, W6).
6. `compileStreaming` and `instantiateStreaming`, with the content-type check
   and the "the body is not a WebAssembly module" error.

## Deviations to declare up front

- `Module.customSections` is not implemented until the parser keeps raw
  sections.
- WebAssembly GC references are not exposed to JS beyond what `externref`
  needs; the JS API has no way to spell the rest either.
- `compile()` does not compile until W8 exists, so the first instantiation of a
  module pays the JIT even if the script called `compile()` first.
- `Atomics.wait` over a shared wasm memory throws until the runtime installs a
  `Waiter`; the other `Atomics` operations do not need one.

## Not verified

Nothing here was compiled or run, except the one probe described in W4's
predecessor (the Rust `cdylib`'s `memory.grow`), which was compiled and
disassembled. The claims that would change the plan if they turn out false: that
`Module::precompile` is cheap to add (W8); that the reservation base stays put
for both memory kinds on all three targets, which is documented for
`GuardPageMemory` and `SharedMemHandle` and is the whole foundation of W4; and
that quickjs-oxide can be given a nested `Scope` without breaking its own
reentrancy rules (the open question).

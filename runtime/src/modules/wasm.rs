//! WebAssembly, the standard JS API, on top of WNJR.
//!
//! The shape is the one every module in js/ uses: js/wasm.js owns the classes,
//! the prototypes and the argument shapes, and this file owns the state and the
//! calls into the engine.
//!
//! A Module, an Instance, a Memory, a Table and a Global each hold a host object
//! whose slot is the Rust side of it. A Func exported by an instance is not a
//! host object: it is a plain JS function created at instantiation that closes
//! over the instance and the export name.
//!
//! Growing a memory detaches the ArrayBuffer that was its `.buffer`. WNJR's grow
//! leaves the base address alone, so the bytes are the same bytes; only the JS
//! object that names them changes. The engine's grow listener sets a flag on the
//! memory slot, and the next moment the realm's thread is back in host code --
//! after an exported call returns, at the entry of an imported function -- the
//! flag is flushed: the old buffer is detached and a new one is made over the
//! same base at the new length. Nothing runs in the listener itself, which keeps
//! it free of engine calls and locks.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};

use quickjs_oxide::engine::api::core::{
    CallableRef, JsString, NativeErrorKind, ObjectRef, PropertyKey, RuntimeError, Scope, Value,
};
use wnjr_core::{Config, Engine, Extern, ExternKind, Instance, Module as WasmModule, Tier};

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/wasm.js");

/// The page size of a wasm linear memory, fixed by the spec.
const PAGE_SIZE: usize = 65_536;

/// One compiled module. WNJR's Module is Clone, so an embedding that instantiates
/// the same module twice shares this slot.
pub(crate) struct ModuleSlot {
    module: WasmModule,
}

/// One instantiated module, plus the slots of the memories it exports.
pub(crate) struct InstanceSlot {
    instance: RefCell<Instance>,
    engine: Engine,
    module: WasmModule,
    memories: RefCell<Vec<Rc<MemorySlot>>>,
}

/// A linear memory and the ArrayBuffer currently naming it.
pub(crate) struct MemorySlot {
    handle: wnjr_core::MemHandle,
    key: u64,
    shared: bool,
    stale: AtomicBool,
    buffer: RefCell<Option<Value>>,
}

pub(crate) struct TableSlot {
    export: Extern,
}

pub(crate) struct GlobalSlot {
    export: Extern,
}

/// What a host object in this module holds.
#[derive(Clone)]
pub(crate) enum Slot {
    Module(Rc<ModuleSlot>),
    Instance(Rc<InstanceSlot>),
    Memory(Rc<MemorySlot>),
    Table(Rc<TableSlot>),
    Global(Rc<GlobalSlot>),
}

fn host(scope: &mut Scope<'_>, value: &Value) -> Result<Option<Slot>, RuntimeError> {
    let Value::Object(object) = value else {
        return Ok(None);
    };
    Ok(scope.host_data::<Slot>(object)?.map(|slot| (*slot).clone()))
}

fn want(scope: &mut Scope<'_>, value: &Value, what: &str) -> Result<Slot, RuntimeError> {
    match host(scope, value)? {
        Some(slot) => Ok(slot),
        None => Err(type_error(scope, &format!("expected a WebAssembly.{what}"))),
    }
}

fn type_error(scope: &mut Scope<'_>, message: &str) -> RuntimeError {
    match scope.new_native_error(NativeErrorKind::Type, message) {
        Ok(value) => RuntimeError::Exception(value),
        Err(error) => error,
    }
}

/// A WebAssembly error: the three classes live on the `WebAssembly` object the
/// prelude builds, because the engine only knows the built-in error kinds.
fn wasm_error(scope: &mut Scope<'_>, class: &str, message: &str) -> RuntimeError {
    let constructor = match wasm_error_constructor(scope, class) {
        Ok(constructor) => constructor,
        Err(error) => return error,
    };
    let text = match JsString::try_from_utf8(message) {
        Ok(text) => Value::String(text),
        Err(error) => return RuntimeError::from(error),
    };
    match scope.construct(&constructor, &[text]) {
        Ok(value) => RuntimeError::Exception(value),
        Err(error) => error,
    }
}

/// `WebAssembly.<class>`, read off the global by name.
fn wasm_error_constructor(
    scope: &mut Scope<'_>,
    class: &str,
) -> Result<CallableRef, RuntimeError> {
    let global = scope.global_object()?;
    let key = PropertyKey::from_js_string(JsString::from_static("WebAssembly"));
    let Value::Object(namespace) = scope.get_property(&global, &key)? else {
        return Err(scope.throw_error(
            NativeErrorKind::Internal,
            "WebAssembly is not installed",
        ));
    };
    let key = PropertyKey::from_js_string(JsString::try_from_utf8(class)?);
    let Value::Object(constructor) = scope.get_property(&namespace, &key)? else {
        return Err(scope.throw_error(
            NativeErrorKind::Internal,
            &format!("WebAssembly.{class} is not a constructor"),
        ));
    };
    match scope.as_callable(&constructor)? {
        Some(callable) => Ok(callable),
        None => Err(scope.throw_error(
            NativeErrorKind::Internal,
            &format!("WebAssembly.{class} is not callable"),
        )),
    }
}

fn compile_error(scope: &mut Scope<'_>, error: wnjr_core::WasmError) -> RuntimeError {
    wasm_error(scope, "CompileError", &error.to_string())
}

fn link_error(scope: &mut Scope<'_>, message: &str) -> RuntimeError {
    wasm_error(scope, "LinkError", message)
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    driver
        .context()
        .define_function(&global, "__wasm_validate", 1, move |scope: &mut Scope<'_>| {
            let value = scope.arg(0);
            let bytes = bytes_of(scope, &value)?.ok_or_else(|| {
                type_error(scope, "expected a BufferSource of WebAssembly bytes")
            })?;
            // validate answers, it never throws: bad bytes are `false`.
            match WasmModule::new("", &bytes) {
                Ok(_) => Ok(Value::Bool(true)),
                Err(_) => Ok(Value::Bool(false)),
            }
        })
        .map_err(|error| format!("__wasm_validate: {error}"))?;

    driver
        .context()
        .define_function(&global, "__wasm_compile", 1, move |scope: &mut Scope<'_>| {
            let value = scope.arg(0);
            let bytes = bytes_of(scope, &value)?.ok_or_else(|| {
                type_error(scope, "expected a BufferSource of WebAssembly bytes")
            })?;
            let module = WasmModule::new("", &bytes).map_err(|error| compile_error(scope, error))?;
            let prototype = prototype_of(scope, "Module")?;
            let slot = Slot::Module(Rc::new(ModuleSlot { module }));
            Ok(Value::Object(scope.new_host_object(slot, Some(&prototype))?))
        })
        .map_err(|error| format!("__wasm_compile: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("wasm prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__wasm")
        .map_err(|error| format!("__wasm: {error}"))?;
    let exports = super::collect(driver, &module, &["WebAssembly", "default"])?;
    super::define_with_alias(driver, "wasm", exports)
}

/// The bytes a BufferSource names, copied: the engine's buffer may detach or be
/// written while the module is compiled.
fn bytes_of(scope: &mut Scope<'_>, value: &Value) -> Result<Option<Vec<u8>>, RuntimeError> {
    let Value::Object(object) = value else {
        return Ok(None);
    };
    if let Some(length) = scope.array_buffer_len(object)? {
        let mut bytes = vec![0; length];
        scope.with_array_buffer_bytes(object, |source| bytes.copy_from_slice(source))?;
        return Ok(Some(bytes));
    }
    let Some(view) = scope.typed_array_view(object)? else {
        return Ok(None);
    };
    // A view whose buffer detached reports no length; the spec treats that as
    // empty bytes rather than an error.
    let length = view.length.unwrap_or(0);
    let mut bytes = vec![0; length * view.kind.size()];
    if !bytes.is_empty() {
        let length = bytes.len();
        scope.with_array_buffer_bytes(&view.buffer, |source| {
            let start = view.byte_offset;
            bytes.copy_from_slice(&source[start..start + length]);
        })?;
    }
    Ok(Some(bytes))
}

/// `WebAssembly.<name>.prototype`, for the host objects this module makes.
fn prototype_of(scope: &mut Scope<'_>, name: &str) -> Result<ObjectRef, RuntimeError> {
    let global = scope.global_object()?;
    let key = PropertyKey::from_js_string(JsString::from_static("WebAssembly"));
    let Value::Object(namespace) = scope.get_property(&global, &key)? else {
        return Err(scope.throw_error(
            NativeErrorKind::Internal,
            "WebAssembly is not installed",
        ));
    };
    let key = PropertyKey::from_js_string(JsString::try_from_utf8(name)?);
    let Value::Object(constructor) = scope.get_property(&namespace, &key)? else {
        return Err(scope.throw_error(
            NativeErrorKind::Internal,
            &format!("WebAssembly.{name} is not a constructor"),
        ));
    };
    let key = PropertyKey::from_js_string(JsString::from_static("prototype"));
    let Value::Object(prototype) = scope.get_property(&constructor, &key)? else {
        return Err(scope.throw_error(
            NativeErrorKind::Internal,
            &format!("WebAssembly.{name}.prototype is not an object"),
        ));
    };
    Ok(prototype)
}

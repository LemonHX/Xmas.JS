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
    CallableRef, JsBigInt, JsString, NativeErrorKind, ObjectRef, PropertyKey, RuntimeError, Scope,
    Value,
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
        .context()
        .define_function(&global, "__wasm_instantiate", 1, move |scope: &mut Scope<'_>| {
            let value = scope.arg(0);
            let slot = match want(scope, &value, "Module") {
                Ok(slot) => slot,
                Err(error) => return Err(error),
            };
            let slot = match slot {
                Slot::Module(slot) => slot,
                _ => {
                    return Err(type_error(
                        scope,
                        "expected a WebAssembly.Module (the slot is not one)",
                    ))
                }
            };
            let engine = Engine::with_defaults();
            let instance = Instance::from_module(&engine, slot.module.clone(), &[])
                .map_err(|error| link_error(scope, &error.to_string()))?;
            let slot = Slot::Instance(Rc::new(InstanceSlot {
                instance: RefCell::new(instance),
                engine,
                module: slot.module.clone(),
                memories: RefCell::new(Vec::new()),
            }));
            let prototype = prototype_of(scope, "Instance")?;
            Ok(Value::Object(scope.new_host_object(slot, Some(&prototype))?))
        })
        .map_err(|error| format!("__wasm_instantiate: {error}"))?;

    driver
        .context()
        .define_function(&global, "__wasm_call", 3, move |scope: &mut Scope<'_>| {
            let instance = scope.arg(0);
            let name = scope.arg(1);
            let Slot::Instance(slot) = want(scope, &instance, "Instance")? else {
                return Err(type_error(scope, "expected a WebAssembly.Instance"));
            };
            let Value::String(name) = name else {
                return Err(type_error(scope, "expected an export name"));
            };
            let name = name.to_utf8_lossy();
            let args = scope.arg(2);
            let args = to_wasm_args(scope, &args)?;
            let results = slot
                .instance
                .borrow_mut()
                .invoke(&name, &args)
                .map_err(|error| wasm_error(scope, "RuntimeError", &error.to_string()))?;
            match results.len() {
                0 => Ok(Value::Undefined),
                1 => from_wasm_value(scope, &results[0]),
                _ => Err(type_error(
                    scope,
                    "multi-value results are not supported yet",
                )),
            }
        })
        .map_err(|error| format!("__wasm_call: {error}"))?;

    driver
        .context()
        .define_function(&global, "__wasm_exports", 1, move |scope: &mut Scope<'_>| {
            let value = scope.arg(0);
            let Slot::Instance(slot) = want(scope, &value, "Instance")? else {
                return Err(type_error(scope, "expected a WebAssembly.Instance"));
            };
            let names = slot
                .instance
                .borrow()
                .exports()
                .map_err(|error| wasm_error(scope, "RuntimeError", &error.to_string()))?;
            let mut pairs = Vec::with_capacity(names.len());
            for (name, export) in names {
                let kind = match export.kind() {
                    ExternKind::Func => "function",
                    ExternKind::Memory => "memory",
                    ExternKind::Table => "table",
                    ExternKind::Global => "global",
                    ExternKind::Tag => "tag",
                };
                pairs.push((name, kind));
            }
            pairs_to_array(scope, &pairs)
        })
        .map_err(|error| format!("__wasm_exports: {error}"))?;

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

/// An array of `[name, kind]` pairs, which is what the prelude walks.
fn pairs_to_array(scope: &mut Scope<'_>, pairs: &[(String, &str)]) -> Result<Value, RuntimeError> {
    let array = scope.new_array()?;
    for (index, (name, kind)) in pairs.iter().enumerate() {
        let pair = scope.new_array()?;
        let text = Value::String(JsString::try_from_utf8(name)?);
        scope.set_property(&pair, &PropertyKey::index(0), text.clone())?;
        let kind = Value::String(JsString::try_from_utf8(kind)?);
        scope.set_property(&pair, &PropertyKey::index(1), kind)?;
        scope.set_property(&array, &PropertyKey::index(index as u32), Value::Object(pair))?;
    }
    Ok(Value::Object(array))
}

/// The JS arguments of an exported call, as wasm values.
///
/// An i32 is a JS number, an i64 is a BigInt, and the floats are numbers. The
/// engine sees no other kind from JS: a reference needs a table and a function
/// reference needs a closure, and neither is in this version.
fn to_wasm_args(scope: &mut Scope<'_>, args: &Value) -> Result<Vec<wnjr_core::Value>, RuntimeError> {
    let Value::Object(array) = args else {
        return Err(type_error(scope, "expected an array of arguments"));
    };
    let mut values = Vec::new();
    for index in 0..64u32 {
        let key = PropertyKey::index(index);
        let value = scope.get_property(array, &key)?;
        if matches!(value, Value::Undefined) {
            let next = PropertyKey::index(index + 1);
            if matches!(scope.get_property(array, &next)?, Value::Undefined) {
                break;
            }
            values.push(wnjr_core::Value::I32(0));
            continue;
        }
        values.push(to_wasm_value(scope, value, index)?);
    }
    Ok(values)
}

fn to_wasm_value(
    scope: &mut Scope<'_>,
    value: Value,
    index: u32,
) -> Result<wnjr_core::Value, RuntimeError> {
    match value {
        Value::Int(number) => Ok(wnjr_core::Value::I32(number as i32)),
        Value::Float(number) => Ok(wnjr_core::Value::F64(number)),
        Value::Bool(flag) => Ok(wnjr_core::Value::I32(i32::from(flag))),
        _ => Err(type_error(
            scope,
            &format!(
                "argument {index} is not a number; i64 arguments need BigInt, which this engine \
                 cannot convert yet"
            ),
        )),
    }
}

fn from_wasm_value(scope: &mut Scope<'_>, value: &wnjr_core::Value) -> Result<Value, RuntimeError> {
    match *value {
        wnjr_core::Value::I32(number) => Ok(Value::Int(number)),
        // An i64 crosses as a BigInt, as the JS API says. There is no i64
        // constructor on JsBigInt, so the decimal text is what it parses.
        wnjr_core::Value::I64(number) => {
            let text = number.to_string();
            let big = JsBigInt::parse_js_string(&text)
                .map_err(|_| RuntimeError::Invariant("an i64's decimal text is a BigInt"))?;
            Ok(Value::BigInt(big))
        }
        wnjr_core::Value::F32(number) => Ok(Value::Float(f64::from(number))),
        wnjr_core::Value::F64(number) => Ok(Value::Float(number)),
        _ => Err(type_error(scope, "this result type is not supported yet")),
    }
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

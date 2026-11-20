//! util: inspect and styleText are native, the rest is the JS layer.
//!
//! The value walker is LLRT's, from libs/llrt_logging/src/lib.rs
//! (format_raw_inner and write_object), with the layer around it swapped: LLRT
//! reaches into values through rquickjs' Object/Type/Coerced and a set of
//! primordials it caches once per runtime; this engine hands the same
//! information through the Js trait, which a Scope implements, so the walker
//! takes a mut ref to something implementing Js and calls the same operations.
//!
//! What LLRT gets from a primordials cache we get from the engine directly:
//! type_of instead of Value::type_of, own_property_keys plus
//! get_own_property instead of Object::keys, promise_snapshot instead of
//! PromiseState (there is no Promise variant in Value), symbol_description
//! instead of Symbol::description. The output rules, the indentation, the
//! circular marker, the 100 item cap and the class handling are copied.

use quickjs_oxide::engine::api::core::{
    CompleteOrdinaryPropertyDescriptor, Js, JsString, ObjectRef, PromiseState, PropertyKey,
    RuntimeError, Scope, Value,
};

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/util.js");

const SPACING: char = ' ';
const CIRCULAR: &str = "[Circular]";
const MAX_INDENTATION_LEVEL: usize = 4;
const MAX_EXPANSION_DEPTH: usize = 4;
const INDENTATION_LOOKUP: [&str; MAX_INDENTATION_LEVEL + 1] =
    ["", "  ", "    ", "        ", "                "];

const RESET: &str = "\x1b[0m";
const BOLD: &str = "\x1b[1m";
const BLACK: &str = "\x1b[30m";
const RED: &str = "\x1b[31m";
const GREEN: &str = "\x1b[32m";
const YELLOW: &str = "\x1b[33m";
const MAGENTA: &str = "\x1b[35m";
const CYAN: &str = "\x1b[36m";

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    {
        let context = driver.context();
        context
            .define_function(
                &global,
                "__util_inspect",
                2,
                |scope: &mut Scope<'_>, value: Value, colors: bool| inspect(scope, value, colors),
            )
            .map_err(|error| format!("__util_inspect: {error}"))?;
    }

    {
        let context = driver.context();
        context
            .define_function(
                &global,
                "__util_ansi",
                1,
                |scope: &mut Scope<'_>, format: String| match ansi_codes(&format) {
                    Some((open, close)) => {
                        let mut answer = String::with_capacity(open.len() + close.len() + 1);
                        answer.push_str(open);
                        answer.push('\0');
                        answer.push_str(close);
                        text(&answer)
                    }
                    None => Err(scope.throw_error(
                        quickjs_oxide::engine::api::core::NativeErrorKind::Type,
                        &format!(
                            "The argument 'format' must be a valid style. Received '{format}'"
                        ),
                    )),
                },
            )
            .map_err(|error| format!("__util_ansi: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("util prelude: {error}"))?;

    let util = driver
        .eval("globalThis.__util_module")
        .map_err(|error| format!("util object: {error}"))?;
    let names = [
        "format",
        "formatWithOptions",
        "inspect",
        "inherits",
        "promisify",
        "callbackify",
        "deprecate",
        "debuglog",
        "debug",
        "types",
        "styleText",
        "stripVTControlCharacters",
        "isDeepStrictEqual",
        "toUSVString",
        "getCallSites",
        "isArray",
        "parseArgs",
    ];
    let mut exports = vec![("default".to_string(), util.clone())];
    exports.extend(super::collect(driver, &util, &names)?);
    super::define_with_alias(driver, "util", exports)?;
    Ok(())
}

fn inspect(scope: &mut Scope<'_>, value: Value, colors: bool) -> Result<Value, RuntimeError> {
    let mut out = String::with_capacity(48);
    let mut visited: Vec<ObjectRef> = Vec::new();
    walk(scope, &mut out, value, colors, &mut visited, 0, false)?;
    text(&out)
}

fn text(body: &str) -> Result<Value, RuntimeError> {
    let body =
        JsString::try_from_utf8(body).map_err(|error| RuntimeError::Internal(error.to_string()))?;
    Ok(Value::String(body))
}

fn key(text: &str) -> Option<PropertyKey> {
    JsString::try_from_utf8(text)
        .ok()
        .map(PropertyKey::from_js_string)
}

fn key_text(property: &PropertyKey) -> Option<String> {
    if property.as_symbol().is_some() {
        return None;
    }
    property.to_js_string().map(|text| text.to_utf8_lossy())
}

fn colour(out: &mut String, code: &str, on: bool) {
    if on {
        out.push_str(code);
    }
}

fn reset(out: &mut String, on: bool) {
    if on {
        out.push_str(RESET);
    }
}

/// One value, the way Node writes it: primitives coloured and quoted, objects
/// expanded up to four levels, with the ancestors of the current value tracked
/// so a cycle prints as [Circular].
fn walk(
    js: &mut impl Js,
    out: &mut String,
    value: Value,
    color: bool,
    visited: &mut Vec<ObjectRef>,
    depth: usize,
    compact: bool,
) -> Result<(), RuntimeError> {
    let is_root = depth == 0;

    match &value {
        Value::Null => {
            colour(out, BOLD, color);
            out.push_str("null");
        }
        Value::Undefined => {
            colour(out, BLACK, color);
            out.push_str("undefined");
        }
        Value::Bool(flag) => {
            colour(out, YELLOW, color);
            out.push_str(if *flag { "true" } else { "false" });
        }
        Value::Int(number) => {
            colour(out, YELLOW, color);
            out.push_str(&number.to_string());
        }
        Value::Float(_) => {
            colour(out, YELLOW, color);
            out.push_str(&js.to_js_string(&value)?.to_utf8_lossy());
        }
        Value::BigInt(_) => {
            colour(out, YELLOW, color);
            out.push_str(&js.to_js_string(&value)?.to_utf8_lossy());
            out.push('n');
        }
        Value::String(body) => {
            write_string(out, &body.to_utf8_lossy(), !is_root, color);
        }
        Value::Symbol(symbol) => {
            colour(out, YELLOW, color);
            out.push_str("Symbol(");
            if let Some(description) = js.symbol_description(symbol)? {
                out.push_str(&description.to_utf8_lossy());
            }
            out.push(')');
        }
        Value::Object(object) => {
            write_value(js, out, object, &value, color, visited, depth, compact)?;
            return Ok(());
        }
    }

    reset(out, color);
    Ok(())
}

fn write_string(out: &mut String, body: &str, quoted: bool, color: bool) {
    if quoted {
        colour(out, GREEN, color);
        out.push('\'');
    }
    out.push_str(body);
    if quoted {
        out.push('\'');
    }
}

fn write_value(
    js: &mut impl Js,
    out: &mut String,
    object: &ObjectRef,
    value: &Value,
    color: bool,
    visited: &mut Vec<ObjectRef>,
    depth: usize,
    compact: bool,
) -> Result<(), RuntimeError> {
    if js.type_of(value)? == "function" {
        colour(out, CYAN, color);
        let mut name = property_text(js, object, "name")?.unwrap_or_default();
        if name.is_empty() {
            name.push_str("(anonymous)");
        }
        // A class is a function whose prototype is not writable.
        let is_class = match own(js, object, "prototype")? {
            Some(CompleteOrdinaryPropertyDescriptor::Data { writable, .. }) => !writable,
            _ => false,
        };
        out.push_str(if is_class { "[class: " } else { "[function: " });
        out.push_str(&name);
        out.push(']');
        reset(out, color);
        return Ok(());
    }

    let class_name = class_name(js, object)?;

    if js.is_error_object(object)? {
        let name = property_text(js, object, "name")?.unwrap_or_default();
        let message = property_text(js, object, "message")?.unwrap_or_default();
        out.push_str(&name);
        out.push_str(": ");
        out.push_str(&message);
        colour(out, BLACK, color);
        if let Some(stack) = property_text(js, object, "stack")? {
            for line in stack.trim().split('\n') {
                out.push('\n');
                push_indentation(out, depth + 1);
                out.push_str(line);
            }
        }
        reset(out, color);
        return Ok(());
    }

    match class_name.as_deref() {
        Some("Date") => {
            colour(out, MAGENTA, color);
            if let Some(iso) = call_method(js, object, "toISOString")? {
                out.push_str(&iso);
            }
            reset(out, color);
            return Ok(());
        }
        Some("RegExp") => {
            colour(out, RED, color);
            let source = property_text(js, object, "source")?.unwrap_or_default();
            let flags = property_text(js, object, "flags")?.unwrap_or_default();
            out.push('/');
            out.push_str(&source);
            out.push('/');
            out.push_str(&flags);
            reset(out, color);
            return Ok(());
        }
        Some("Promise") => {
            write_promise(js, out, object, color, visited, depth, compact)?;
            return Ok(());
        }
        _ => {}
    }

    if depth >= MAX_EXPANSION_DEPTH {
        colour(out, CYAN, color);
        let is_array = js.is_array_object(object)?;
        out.push_str(if is_array { "[Array]" } else { "[Object]" });
        reset(out, color);
        return Ok(());
    }

    let is_array = js.is_array_object(object)?;
    // LLRT only names the class for objects that are not arrays, and Node
    // does not name a plain Array or Object either.
    if let Some(name) = class_name.as_deref() {
        if !is_array && !matches!(name, "" | "Object" | "Array") {
            out.push_str(name);
            out.push(SPACING);
        }
    }
    let is_array = is_array || class_name.as_deref().map(is_typed_array).unwrap_or(false);

    if visited.contains(object) {
        colour(out, CYAN, color);
        out.push_str(CIRCULAR);
        reset(out, color);
        return Ok(());
    }
    // Node keeps a container on one line when it fits; the next line is where
    // that is decided, by writing it compactly first and measuring.
    if !compact {
        let mut scratch = String::with_capacity(32);
        visited.push(object.clone());
        let one_line = write_members(
            js,
            &mut scratch,
            object,
            color,
            visited,
            depth,
            is_array,
            true,
        );
        visited.pop();
        one_line?;
        if !scratch.contains('\n') && out.len() + scratch.len() <= 72 {
            out.push_str(&scratch);
            return Ok(());
        }
    }

    visited.push(object.clone());
    let outcome = write_members(js, out, object, color, visited, depth, is_array, compact);
    visited.pop();
    outcome
}

fn write_promise(
    js: &mut impl Js,
    out: &mut String,
    object: &ObjectRef,
    color: bool,
    visited: &mut Vec<ObjectRef>,
    depth: usize,
    compact: bool,
) -> Result<(), RuntimeError> {
    out.push_str("Promise {");
    let Some(snapshot) = js.promise_snapshot(object)? else {
        out.push('}');
        return Ok(());
    };
    let pending = snapshot.state() == PromiseState::Pending;
    let indent = depth < 2 && !pending;
    write_sep(out, false, indent);
    if indent {
        push_indentation(out, depth + 1);
    }
    if pending {
        colour(out, CYAN, color);
        out.push_str("<pending>");
        reset(out, color);
    } else {
        if snapshot.state() == PromiseState::Rejected {
            colour(out, RED, color);
            out.push_str("<rejected> ");
            reset(out, color);
        }
        let result = snapshot.result().clone();
        walk(js, out, result, color, visited, depth + 1, compact)?;
    }
    write_sep(out, false, indent);
    if indent {
        push_indentation(out, depth);
    }
    out.push('}');
    Ok(())
}

fn is_typed_array(name: &str) -> bool {
    matches!(
        name,
        "Int8Array"
            | "Uint8Array"
            | "Uint8ClampedArray"
            | "Int16Array"
            | "Uint16Array"
            | "Int32Array"
            | "Uint32Array"
            | "Int64Array"
            | "Uint64Array"
            | "Float32Array"
            | "Float64Array"
            | "Buffer"
    )
}

fn write_members(
    js: &mut impl Js,
    out: &mut String,
    object: &ObjectRef,
    color: bool,
    visited: &mut Vec<ObjectRef>,
    depth: usize,
    is_array: bool,
    compact: bool,
) -> Result<(), RuntimeError> {
    out.push(if is_array { '[' } else { '{' });

    // Own enumerable string keys, in the order the language gives them.
    let keys = js.own_property_keys(object)?;
    let mut members: Vec<(String, Value)> = Vec::with_capacity(keys.len());
    for property in keys {
        let Some(name) = key_text(&property) else {
            continue;
        };
        match js.get_own_property(object, &property)? {
            Some(CompleteOrdinaryPropertyDescriptor::Data {
                value, enumerable, ..
            }) if enumerable => members.push((name, value)),
            Some(CompleteOrdinaryPropertyDescriptor::Accessor { enumerable, .. }) if enumerable => {
                members.push((name, js.get_property(object, &property)?))
            }
            _ => {}
        }
    }

    let apply_indentation = !compact && !is_array && depth < 2;
    let length = members.len();
    let mut first = false;
    for (index, (name, value)) in members.into_iter().enumerate() {
        let numeric_key = name.parse::<f64>().is_ok();
        write_sep(out, first, apply_indentation);
        if apply_indentation {
            push_indentation(out, depth + 1);
        }
        if !compact && depth > MAX_INDENTATION_LEVEL - 1 {
            out.push(SPACING);
        }
        if !is_array {
            write_string(out, &name, numeric_key, numeric_key && color);
            if numeric_key {
                reset(out, color);
            }
            out.push(':');
            out.push(SPACING);
        }
        walk(js, out, value, color, visited, depth + 1, compact)?;
        first = true;
        if index > 99 {
            out.push_str("... ");
            out.push_str(&(length - index).to_string());
            out.push_str(" more items");
            break;
        }
    }

    if first {
        if apply_indentation {
            out.push('\n');
            push_indentation(out, depth);
        } else {
            out.push(SPACING);
        }
    }

    out.push(if is_array { ']' } else { '}' });
    Ok(())
}

fn write_sep(out: &mut String, add_comma: bool, has_indentation: bool) {
    if add_comma {
        out.push(',');
    }
    if has_indentation {
        out.push('\n');
    } else {
        out.push(SPACING);
    }
}

fn push_indentation(out: &mut String, depth: usize) {
    out.push_str(INDENTATION_LOOKUP[depth.min(MAX_INDENTATION_LEVEL)]);
}

fn own(
    js: &mut impl Js,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<CompleteOrdinaryPropertyDescriptor>, RuntimeError> {
    let Some(property) = key(name) else {
        return Ok(None);
    };
    js.get_own_property(object, &property)
}

fn property_text(
    js: &mut impl Js,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<String>, RuntimeError> {
    let Some(property) = key(name) else {
        return Ok(None);
    };
    let value = js.get_property(object, &property)?;
    match value {
        Value::Undefined | Value::Null => Ok(None),
        other => Ok(Some(js.to_js_string(&other)?.to_utf8_lossy())),
    }
}

/// object[name](), the call Node makes for a Date's toISOString; None when the
/// property is missing or not callable.
fn call_method(
    js: &mut impl Js,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<String>, RuntimeError> {
    let Some(property) = key(name) else {
        return Ok(None);
    };
    let value = js.get_property(object, &property)?;
    let Value::Object(callable_object) = &value else {
        return Ok(None);
    };
    let Some(callable) = js.as_callable(callable_object)? else {
        return Ok(None);
    };
    let answer = js.call(&callable, Value::Object(object.clone()), &[])?;
    match answer {
        Value::Undefined => Ok(None),
        other => Ok(Some(js.to_js_string(&other)?.to_utf8_lossy())),
    }
}

/// value.constructor.name, which is how LLRT finds the class.
fn class_name(js: &mut impl Js, object: &ObjectRef) -> Result<Option<String>, RuntimeError> {
    let Some(property) = key("constructor") else {
        return Ok(None);
    };
    let Ok(Value::Object(constructor)) = js.get_property(object, &property) else {
        return Ok(None);
    };
    let name = property_text(js, &constructor, "name")?;
    Ok(name.filter(|name| !name.is_empty()))
}

/// styleText's table, copied from llrt_util/src/style_text.rs.
fn ansi_codes(format: &str) -> Option<(&'static str, &'static str)> {
    Some(match format {
        "reset" => ("\x1b[0m", "\x1b[0m"),
        "bold" => ("\x1b[1m", "\x1b[22m"),
        "dim" => ("\x1b[2m", "\x1b[22m"),
        "italic" => ("\x1b[3m", "\x1b[23m"),
        "underline" => ("\x1b[4m", "\x1b[24m"),
        "blink" => ("\x1b[5m", "\x1b[25m"),
        "inverse" => ("\x1b[7m", "\x1b[27m"),
        "hidden" => ("\x1b[8m", "\x1b[28m"),
        "strikethrough" => ("\x1b[9m", "\x1b[29m"),
        "doubleunderline" => ("\x1b[21m", "\x1b[24m"),
        "black" => ("\x1b[30m", "\x1b[39m"),
        "red" => ("\x1b[31m", "\x1b[39m"),
        "green" => ("\x1b[32m", "\x1b[39m"),
        "yellow" => ("\x1b[33m", "\x1b[39m"),
        "blue" => ("\x1b[34m", "\x1b[39m"),
        "magenta" => ("\x1b[35m", "\x1b[39m"),
        "cyan" => ("\x1b[36m", "\x1b[39m"),
        "white" => ("\x1b[37m", "\x1b[39m"),
        "gray" => ("\x1b[90m", "\x1b[39m"),
        "bgBlack" => ("\x1b[40m", "\x1b[49m"),
        "bgRed" => ("\x1b[41m", "\x1b[49m"),
        "bgGreen" => ("\x1b[42m", "\x1b[49m"),
        "bgYellow" => ("\x1b[43m", "\x1b[49m"),
        "bgBlue" => ("\x1b[44m", "\x1b[49m"),
        "bgMagenta" => ("\x1b[45m", "\x1b[49m"),
        "bgCyan" => ("\x1b[46m", "\x1b[49m"),
        "bgWhite" => ("\x1b[47m", "\x1b[49m"),
        "bgGray" => ("\x1b[100m", "\x1b[49m"),
        "redBright" => ("\x1b[91m", "\x1b[39m"),
        "greenBright" => ("\x1b[92m", "\x1b[39m"),
        "yellowBright" => ("\x1b[93m", "\x1b[39m"),
        "blueBright" => ("\x1b[94m", "\x1b[39m"),
        "magentaBright" => ("\x1b[95m", "\x1b[39m"),
        "cyanBright" => ("\x1b[96m", "\x1b[39m"),
        "whiteBright" => ("\x1b[97m", "\x1b[39m"),
        "bgRedBright" => ("\x1b[101m", "\x1b[49m"),
        "bgGreenBright" => ("\x1b[102m", "\x1b[49m"),
        "bgYellowBright" => ("\x1b[103m", "\x1b[49m"),
        "bgBlueBright" => ("\x1b[104m", "\x1b[49m"),
        "bgMagentaBright" => ("\x1b[105m", "\x1b[49m"),
        "bgCyanBright" => ("\x1b[106m", "\x1b[49m"),
        "bgWhiteBright" => ("\x1b[107m", "\x1b[49m"),
        "framed" => ("\x1b[51m", "\x1b[54m"),
        "overlined" => ("\x1b[53m", "\x1b[55m"),
        _ => return None,
    })
}

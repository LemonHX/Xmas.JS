//! The unified entry point for bytes entering and leaving host functions.
//!
//! Previously everything went through base64 strings, which was wrong: an 8MB Buffer encoded as base64 is 11MB of
//! JS string, UTF-16 occupies 22MB on the heap, plus a copy of the original data, the 64MB heap limit directly OOMs.
//! Now binary goes through ArrayBuffer: on the way in it borrows memory from the engine (zero-copy Uint8Array),
//! on the way out, new_array_buffer. base64 is only left for cases where the caller wants a
//! base64 string in the first place.

use quickjs_oxide::engine::api::core::{Js, NativeErrorKind, RuntimeError, Value};

/// Lend a piece of bytes to f; f must not call back into the engine (the engine is still being borrowed).
pub(crate) fn with_bytes<R>(
    js: &mut impl Js,
    value: &Value,
    f: impl FnOnce(&[u8]) -> R,
) -> Result<R, RuntimeError> {
    match value {
        Value::String(text) => {
            let text = text.to_utf8_lossy();
            Ok(f(text.as_bytes()))
        }
        Value::Object(object) => {
            if let Some(view) = js.typed_array_view(object)? {
                let Some(length) = view.length else {
                    return Err(error(js, "view out of bounds or buffer already detached"));
                };
                let span = length * view.kind.size();
                let start = view.byte_offset;
                js.with_array_buffer_bytes(&view.buffer, |bytes| {
                    let end = (start + span).min(bytes.len());
                    f(&bytes[start.min(end)..end])
                })
            } else if js.array_buffer_len(object)?.is_some() {
                js.with_array_buffer_bytes(object, |bytes| f(bytes))
            } else {
                Err(error(js, "expected ArrayBuffer or typed array"))
            }
        }
        _ => Err(error(js, "expected string or bytes")),
    }
}

/// Make a copy, suitable for cases where data needs to be sent across threads.
pub(crate) fn take(js: &mut impl Js, value: &Value) -> Result<Vec<u8>, RuntimeError> {
    with_bytes(js, value, |bytes| bytes.to_vec())
}

/// Turn the bytes into an ArrayBuffer and hand it to JS.
pub(crate) fn give(js: &mut impl Js, bytes: &[u8]) -> Result<Value, RuntimeError> {
    Ok(Value::Object(js.new_array_buffer(bytes)?))
}

/// The TypeError thrown when argument types are incorrect.
pub(crate) fn error(js: &mut impl Js, message: &str) -> RuntimeError {
    match js.new_native_error(NativeErrorKind::Type, message) {
        Ok(value) => RuntimeError::Exception(value),
        Err(error) => error,
    }
}

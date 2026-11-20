//! buffer / blob: Buffer (a subclass of Uint8Array), Blob, atob/btoa.
//!
//! The old modules/src/buffer was an rquickjs Class wrapping Vec<u8>, and every byte read went through a host call.
//! Here Buffer is just a JS subclass of Uint8Array; reads and writes all happen in the engine, and only encoding conversion crosses the bridge:
//!   __text_to_bytes(text, encoding) -> ArrayBuffer   text becomes bytes
//!   __bytes_to_text(bytes, encoding) -> string       bytes become text
//! The bytes travel as an ArrayBuffer, not as a base64 string: base64 inflates
//! by a third and needs a codec on both sides. A 4 KiB string chunk used to
//! spend ~360 us in the JS base64 decoder; the ArrayBuffer path is a memcpy.

use quickjs_oxide::engine::api::core::{JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/buffer.js");
/// TextEncoder and TextDecoder are globals every handler expects, and they convert through the
/// same host call Buffer does.
const TEXT: &str = include_str!("../../js/text.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    driver
        .context()
        .define_function(
            &global,
            "__text_to_bytes",
            2,
            |scope: &mut Scope<'_>,
             text: String,
             encoding: String|
             -> Result<Value, RuntimeError> {
                let encoded =
                    encode(&text, &encoding).map_err(|error| super::throw(scope, &error))?;
                bytes::give(scope, &encoded)
            },
        )
        .map_err(|error| format!("__text_to_bytes: {error}"))?;

    driver
        .context()
        .define_function(
            &global,
            "__bytes_to_text",
            2,
            |scope: &mut Scope<'_>,
             value: Value,
             encoding: String|
             -> Result<Value, RuntimeError> {
                let decoded = bytes::with_bytes(scope, &value, |raw| decode(raw, &encoding))?;
                let text = decoded.map_err(|error| super::throw(scope, &error))?;
                Ok(Value::String(JsString::try_from_utf8(&text).map_err(
                    |error| RuntimeError::Internal(error.to_string()),
                )?))
            },
        )
        .map_err(|error| format!("__bytes_to_text: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("buffer prelude: {error}"))?;
    driver
        .eval(TEXT)
        .map_err(|error| format!("text prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__buffer_module")
        .map_err(|error| format!("buffer module: {error}"))?;
    let exports = super::collect(
        driver,
        &module,
        &["Buffer", "Blob", "atob", "btoa", "default"],
    )?;
    super::define_with_alias(driver, "buffer", exports.clone())?;
    let blob = super::collect(driver, &module, &["Blob"])?;
    super::define_with_alias(driver, "blob", blob)
}

fn encode(text: &str, encoding: &str) -> Result<Vec<u8>, String> {
    match encoding {
        "utf8" => Ok(text.as_bytes().to_vec()),
        "latin1" | "ascii" => Ok(text.chars().map(|c| c as u32 as u8).collect()),
        "utf16le" => {
            let mut out = Vec::with_capacity(text.len() * 2);
            for unit in text.encode_utf16() {
                out.extend_from_slice(&unit.to_le_bytes());
            }
            Ok(out)
        }
        "hex" => {
            let digits: Vec<u8> = text
                .bytes()
                .filter(|byte| !byte.is_ascii_whitespace())
                .collect();
            if digits.len() % 2 != 0 {
                return Err("hex length must be even".into());
            }
            let mut out = Vec::with_capacity(digits.len() / 2);
            for pair in digits.chunks(2) {
                let high = hex_value(pair[0]).ok_or("hex contains a non-hexadecimal character")?;
                let low = hex_value(pair[1]).ok_or("hex contains a non-hexadecimal character")?;
                out.push((high << 4) | low);
            }
            Ok(out)
        }
        "base64" => Ok(base64_decode(text).to_vec()),
        other => Err(format!("unknown encoding: {other}")),
    }
}

pub(crate) fn decode(bytes: &[u8], encoding: &str) -> Result<String, String> {
    match encoding {
        "utf8" => Ok(String::from_utf8_lossy(bytes).into_owned()),
        "latin1" | "ascii" => Ok(bytes.iter().map(|byte| *byte as char).collect()),
        "utf16le" => {
            let units: Vec<u16> = bytes
                .chunks_exact(2)
                .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
                .collect();
            Ok(String::from_utf16_lossy(&units))
        }
        "hex" => {
            let mut out = String::with_capacity(bytes.len() * 2);
            for byte in bytes {
                out.push_str(&format!("{byte:02x}"));
            }
            Ok(out)
        }
        "base64" => Ok(base64_encode(bytes)),
        "base64url" => Ok(base64_encode(bytes)
            .replace('+', "-")
            .replace('/', "_")
            .trim_end_matches('=')
            .to_string()),
        other => Err(format!("unknown encoding: {other}")),
    }
}

fn hex_value(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Hexadecimal, used for crypto's uuid and hex output.
pub(crate) fn buffer_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push(char::from_digit((byte >> 4) as u32, 16).expect("hex"));
        out.push(char::from_digit((byte & 0x0f) as u32, 16).expect("hex"));
    }
    out
}

fn base64_encode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let a = chunk[0];
        let b = *chunk.get(1).unwrap_or(&0);
        let c = *chunk.get(2).unwrap_or(&0);
        out.push(ALPHABET[(a >> 2) as usize] as char);
        out.push(ALPHABET[(((a & 3) << 4) | (b >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(((b & 15) << 2) | (c >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[(c & 63) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

fn base64_decode(text: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut buffer = 0u32;
    let mut bits = 0u32;
    for byte in text.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => continue,
        };
        buffer = (buffer << 6) | value as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
            buffer &= (1 << bits) - 1;
        }
    }
    out
}

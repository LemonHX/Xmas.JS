//! string_decoder: the byte-boundary logic is Node's, carried over from LLRT
//! (llrt_string_decoder/src/string_decoder.rs), which in turn adapted it from
//! node/src/string_decoder.cc.
//!
//! Only the binding changed. LLRT stores an llrt_encoding::Encoder, decodes
//! through it and raises errors with a Ctx; here the encoding is a small enum
//! over buffer's decode and the errors are Strings that install() turns into
//! exceptions.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::{JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/string_decoder.js");

/// The encodings Node's StringDecoder understands, after label normalization.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Utf8,
    Utf16le,
    Base64,
    Base64Url,
    Latin1,
    Ascii,
    Hex,
}

/// The alias table from llrt_encoding, cut down to the labels a StringDecoder
/// accepts. Node's normalizeEncoding spells the answer in the canonical form
/// below, and decoder.encoding shows that.
fn canonical(label: &str) -> Option<Encoding> {
    let label = label.to_ascii_lowercase();
    Some(match label.as_str() {
        "buffer" | "unicode-1-1-utf-8" | "unicode11utf8" | "unicode20utf8" | "utf-8" | "utf8"
        | "x-unicode20utf8" => Encoding::Utf8,
        "csunicode" | "iso-10646-ucs-2" | "ucs-2" | "ucs2" | "unicode" | "unicodefeff"
        | "utf-16le" | "utf16le" => Encoding::Utf16le,
        "base64" => Encoding::Base64,
        "base64url" => Encoding::Base64Url,
        "latin1" | "binary" | "iso-8859-1" | "iso8859-1" | "iso88591" | "iso_8859-1"
        | "iso_8859-1:1987" | "l1" | "cp819" | "ibm819" | "csisolatin1" => Encoding::Latin1,
        "ascii" | "us-ascii" | "ansi_x3.4-1968" | "iso-ir-6" | "iso646-us" | "us" | "cp367"
        | "ibm367" | "csascii" => Encoding::Ascii,
        "hex" => Encoding::Hex,
        _ => return None,
    })
}

impl Encoding {
    fn label(self) -> &'static str {
        match self {
            Self::Utf8 => "utf8",
            Self::Utf16le => "utf16le",
            Self::Base64 => "base64",
            Self::Base64Url => "base64url",
            Self::Latin1 => "latin1",
            Self::Ascii => "ascii",
            Self::Hex => "hex",
        }
    }

    fn decode(self, data: &[u8]) -> String {
        super::buffer::decode(data, self.label()).unwrap_or_default()
    }
}

pub struct Decoder {
    encoding: Encoding,
    buffer: Vec<u8>,
    buffered_bytes: usize,
    missing_bytes: usize,
}

impl Decoder {
    fn new(encoding: Encoding) -> Self {
        Self {
            encoding,
            buffer: Vec::new(),
            buffered_bytes: 0,
            missing_bytes: 0,
        }
    }

    fn make_string(&self, data: &[u8]) -> String {
        self.encoding.decode(data)
    }

    /// Try to decode the given buffer and store the incomplete bytes.
    ///
    /// This is Node's algorithm line for line, including the comments: it keeps
    /// the bytes of a character that straddles a chunk boundary and only emits
    /// it once the rest arrives, because a decoder that emits half a character
    /// produces a replacement character that cannot be taken back.
    fn decode_data(&mut self, mut data: &[u8]) -> String {
        let mut result = String::new();

        if matches!(
            self.encoding,
            Encoding::Utf8 | Encoding::Utf16le | Encoding::Base64 | Encoding::Base64Url
        ) {
            // See if we want bytes to finish a character from the previous
            // chunk; if so, copy the new bytes to the missing bytes buffer
            // and create a string from it that is to be prepended to the main body.
            if self.missing_bytes > 0 {
                if self.encoding == Encoding::Utf8 {
                    // For UTF-8, we need special alignment treatment:
                    // If an incomplete character is found at a chunk boundary, we use
                    // its remainder and try to decode it.
                    let mut i = 0;
                    while i < data.len() && i < self.missing_bytes {
                        if (data[i] & 0xC0) != 0x80 {
                            // This byte is not a continuation byte even though it should have
                            // been one. We stop decoding of the incomplete character at this
                            // point (but still use the rest of the incomplete bytes from this
                            // chunk) and assume that the new, unexpected byte starts a new one.
                            self.missing_bytes = 0;
                            self.buffer.extend_from_slice(&data[..i]);
                            self.buffered_bytes += i;
                            data = &data[i..];
                            break;
                        }
                        i += 1;
                    }
                } else if self.encoding == Encoding::Utf16le {
                    // For UTF-16le, we need special alignment treatment:
                    // If we have a high surrogate we need to extend the missing bytes
                    // to 3 to get the low surrogate.
                    let mut i = 0;
                    while i < data.len() && i < self.missing_bytes {
                        if (data[i] & 0xFC) == 0xD8 {
                            self.missing_bytes = 3;
                            break;
                        }
                        i += 1;
                    }
                }

                let found_bytes = std::cmp::min(data.len(), self.missing_bytes);
                self.buffer.extend_from_slice(&data[..found_bytes]);

                data = &data[found_bytes..];

                self.missing_bytes -= found_bytes;
                self.buffered_bytes += found_bytes;
                if self.missing_bytes == 0 {
                    // We have enough bytes to decode the buffered character
                    result = self.make_string(&self.buffer);
                    self.buffer.clear();
                    self.buffered_bytes = 0;
                }
            }

            // It could be that trying to finish the previous chunk already
            // consumed all data that we received in this chunk.
            if data.is_empty() {
                return result;
            }

            // If not, that means is no character left to finish at this point.

            // See whether there is a character that we may have to cut off and
            // finish when receiving the next chunk.
            if self.encoding == Encoding::Utf8 && (data[data.len() - 1] & 0x80) != 0 {
                let mut i = data.len() - 1;
                loop {
                    self.buffered_bytes += 1;
                    if (data[i] & 0xC0) == 0x80 {
                        // This byte does not start a character (a "trailing" byte).
                        if self.buffered_bytes >= 4 || i == 0 {
                            // We either have more then 4 trailing bytes (which means
                            // the current character would not be inside the range for
                            // valid Unicode, and in particular cannot be represented
                            // through JavaScript's UTF-16-based approach to strings), or the
                            // current buffer does not contain the start of an UTF-8 character
                            // at all. Either way, this is invalid UTF8 and we can just
                            // let the engine's decoder handle it.
                            self.buffer.clear();
                            self.buffered_bytes = 0;
                            break;
                        }
                    } else {
                        // Found the first byte of a UTF-8 character. By looking at the
                        // upper bits we can tell how long the character *should* be.
                        if (data[i] & 0xE0) == 0xC0 {
                            self.missing_bytes = 2;
                        } else if (data[i] & 0xF0) == 0xE0 {
                            self.missing_bytes = 3;
                        } else if (data[i] & 0xF8) == 0xF0 {
                            self.missing_bytes = 4;
                        } else {
                            // This lead byte would indicate a character outside of the
                            // representable range.
                            self.buffered_bytes = 0;
                            break;
                        }

                        if self.buffered_bytes >= self.missing_bytes {
                            // Received more or exactly as many trailing bytes than the lead
                            // character would indicate. In the "==" case, we have valid
                            // data and don't need to slice anything off;
                            // in the ">" case, this is invalid UTF-8 anyway.
                            self.missing_bytes = 0;
                            self.buffered_bytes = 0;
                        }

                        self.missing_bytes -= self.buffered_bytes;
                        break;
                    }
                    i -= 1;
                }
            } else if self.encoding == Encoding::Utf16le {
                // For UTF-16LE we deviate from the specification when an invalid
                // high surrogate is found: the spec says to keep it as is, but
                // there is no way to encode it in UTF-8, which is what the engine
                // hands to JS. It becomes a replacement character on flush.
                if (data.len() % 2) == 1 {
                    // We got half a codepoint, and need the second byte of it.
                    // But we need to avoid rendering high surrogates before we
                    // have the full character.
                    if data.len() >= 3 && (data[data.len() - 2] & 0xFC) == 0xD8 {
                        self.buffered_bytes = 3;
                        self.missing_bytes = 1;
                    } else {
                        self.buffered_bytes = 1;
                        self.missing_bytes = 1;
                    }
                } else if (data[data.len() - 1] & 0xFC) == 0xD8 {
                    // Half a split UTF-16 character.
                    self.buffered_bytes = 2;
                    self.missing_bytes = 2;
                }
            } else if matches!(self.encoding, Encoding::Base64 | Encoding::Base64Url) {
                // base64 and base64url work on groups of three bytes.
                self.buffered_bytes = data.len() % 3;
                if self.buffered_bytes > 0 {
                    self.missing_bytes = 3 - self.buffered_bytes;
                }
            }

            if self.buffered_bytes > 0 {
                // Copy the requested number of buffered bytes from the end of the
                // input into the incomplete character buffer.
                self.buffer
                    .extend_from_slice(&data[data.len() - self.buffered_bytes..]);
                data = &data[..data.len() - self.buffered_bytes];
            }

            if !data.is_empty() {
                result.push_str(&self.make_string(data));
            }

            result
        } else {
            // For ASCII, HEX, and LATIN1, we can decode everything directly
            self.make_string(data)
        }
    }

    fn flush(&mut self) -> String {
        let odd_utf16_byte = self.encoding == Encoding::Utf16le && self.buffered_bytes % 2 == 1;

        if odd_utf16_byte {
            // Ignore a single trailing byte, like the JS decoder does.
            self.missing_bytes -= 1;
            self.buffered_bytes -= 1;
        }

        if self.buffered_bytes == 0 {
            return String::new();
        }

        let decode_len = self.buffer.len() - usize::from(odd_utf16_byte);

        let mut res = self.make_string(&self.buffer[..decode_len]);

        if odd_utf16_byte && decode_len >= 2 {
            let code_unit =
                u16::from_le_bytes([self.buffer[decode_len - 2], self.buffer[decode_len - 1]]);
            if (0xD800..=0xDBFF).contains(&code_unit) {
                res.push(char::REPLACEMENT_CHARACTER);
            }
        }

        self.missing_bytes = 0;
        self.buffered_bytes = 0;
        self.buffer.clear();

        res
    }

    fn end(&mut self, data: Option<&[u8]>) -> String {
        let mut output = data.map(|data| self.decode_data(data)).unwrap_or_default();
        output.push_str(&self.flush());
        output
    }
}

/// A Rust String as a JS value.
fn text_value(text: &str) -> Result<Value, RuntimeError> {
    Ok(Value::String(JsString::try_from_utf8(text).map_err(
        |error| RuntimeError::Internal(error.to_string()),
    )?))
}

#[derive(Default)]
struct State {
    decoders: HashMap<u64, Decoder>,
    next: u64,
}

impl State {
    fn take_id(&mut self) -> u64 {
        self.next += 1;
        self.next
    }
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    use quickjs_oxide::engine::api::core::Js;

    let state = Rc::new(RefCell::new(State::default()));
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sd_create",
                1,
                move |scope: &mut Scope<'_>, label: String| {
                    let Some(encoding) = canonical(&label) else {
                        return Err(super::throw(scope, &format!("Unknown encoding: {label}")));
                    };
                    let mut state = state.borrow_mut();
                    let id = state.take_id();
                    state.decoders.insert(id, Decoder::new(encoding));
                    // One string instead of two calls: the label the decoder reports
                    // and the handle it keeps.
                    Ok(format!("{}\u{0}{id}", encoding.label()))
                },
            )
            .map_err(|error| format!("__sd_create: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sd_write",
                2,
                move |scope: &mut Scope<'_>, id: f64, data: Value| {
                    let mut state = state.borrow_mut();
                    let Some(decoder) = state.decoders.get_mut(&(id as u64)) else {
                        return Err(super::throw(scope, "decoder is gone"));
                    };
                    let text = bytes::with_bytes(scope, &data, |bytes| decoder.decode_data(bytes))?;
                    text_value(&text)
                },
            )
            .map_err(|error| format!("__sd_write: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__sd_end",
                2,
                move |scope: &mut Scope<'_>, id: f64, data: Value| {
                    let mut state = state.borrow_mut();
                    // end() flushes what is left but does not destroy the decoder:
                    // Node lets you keep writing to it afterwards.
                    let Some(decoder) = state.decoders.get_mut(&(id as u64)) else {
                        return Err(super::throw(scope, "decoder is gone"));
                    };
                    let text = if matches!(data, Value::Undefined) {
                        decoder.end(None)
                    } else {
                        bytes::with_bytes(scope, &data, |bytes| decoder.end(Some(bytes)))?
                    };
                    text_value(&text)
                },
            )
            .map_err(|error| format!("__sd_end: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("string_decoder prelude: {error}"))?;

    let module = driver
        .context()
        .eval("globalThis.__string_decoder")
        .map_err(|error| format!("__string_decoder: {error}"))?;
    let exports = super::collect(driver, &module, &["StringDecoder", "default"])?;
    super::define_with_alias(driver, "string_decoder", exports)
}

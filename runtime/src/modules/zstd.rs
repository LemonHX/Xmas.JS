//! zstd: frames, stored.
//!
//! The decompressor is ruzstd, which is a decoder and nothing else. There is no pure-Rust encoder
//! for the format and this runtime does not link C, so the compressor writes a frame whose blocks
//! are **stored**: a legal zstd stream that every decoder reads, and one that leaves the payload
//! exactly as long as the input plus the framing.
//!
//! That is a real encoder for the thing it is used for here -- carrying bytes across a boundary as
//! a self-describing stream -- and it is not one for making data smaller. A caller who wants the
//! ratio has to bring an encoder; what this promises is that a frame written here is read by
//! `zstd -d` and by ruzstd alike.

use std::io::{Cursor, Read};

use quickjs_oxide::engine::api::core::{Scope, Value};
use ruzstd::decoding::StreamingDecoder;

use crate::driver::Driver;
use crate::modules::bytes;

/// Every zstd stream starts with this, little-endian.
const MAGIC: [u8; 4] = [0x28, 0xB5, 0x2F, 0xFD];

/// One stored block carries at most this much: a block header gives the size in 21 bits, and the
/// format takes a block as at most 2^17 bytes.
const MAX_BLOCK: usize = 128 * 1024;

/// Frame header descriptor: bit 5 is single-segment, bits 7-6 say how many bytes the content size
/// takes. A single-segment frame leaves out the window descriptor, because its content size is the
/// window: everything is regenerated in one buffer.
const SINGLE_SEGMENT: u8 = 0b0010_0000;
const SIZE_ONE_BYTE: u8 = 0b0000_0000;
const SIZE_TWO_BYTES: u8 = 0b0100_0000;
const SIZE_FOUR_BYTES: u8 = 0b1000_0000;

/// A block header is three bytes of bit fields: bit 0 is last, bits 1-2 the kind (0 = stored),
/// bits 3-23 the size.
fn block_header(size: usize, last: bool, kind: u32) -> [u8; 3] {
    let value: u32 = u32::from(last) | (kind << 1) | ((size as u32) << 3);
    [value as u8, (value >> 8) as u8, (value >> 16) as u8]
}

/// Encode `src` as one frame of stored blocks.
///
/// The level a caller passes changes nothing: a stored block is stored whatever it is asked for.
fn compress(src: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(src.len() + 16 + src.len() / MAX_BLOCK * 3);
    out.extend_from_slice(&MAGIC);

    let size = src.len();
    if size < 256 {
        out.push(SINGLE_SEGMENT | SIZE_ONE_BYTE);
        out.push(size as u8);
    } else if size < 256 + 65536 {
        // The two-byte field holds the size minus 256, which is why it starts at 256.
        out.push(SINGLE_SEGMENT | SIZE_TWO_BYTES);
        out.extend_from_slice(&((size - 256) as u16).to_le_bytes());
    } else {
        out.push(SINGLE_SEGMENT | SIZE_FOUR_BYTES);
        out.extend_from_slice(&(size as u32).to_le_bytes());
    }

    if src.is_empty() {
        // A stream ends at a last block, and an empty one still needs that block.
        out.extend_from_slice(&block_header(0, true, 0));
        return out;
    }
    let mut rest = src;
    while !rest.is_empty() {
        let take = rest.len().min(MAX_BLOCK);
        let last = take == rest.len();
        out.extend_from_slice(&block_header(take, last, 0));
        out.extend_from_slice(&rest[..take]);
        rest = &rest[take..];
    }
    out
}

/// Decode a whole stream, refusing to go past `max`.
fn decompress(src: &[u8], max: Option<usize>) -> Result<Vec<u8>, String> {
    let mut decoder = StreamingDecoder::new(Cursor::new(src)).map_err(|error| error.to_string())?;
    let mut out = Vec::with_capacity(src.len().saturating_mul(2));
    match max {
        // One byte past the limit is enough to know the answer is no.
        Some(limit) => decoder
            .by_ref()
            .take(limit as u64 + 1)
            .read_to_end(&mut out),
        None => decoder.read_to_end(&mut out),
    }
    .map_err(|error| error.to_string())?;
    if let Some(limit) = max
        && out.len() > limit
    {
        return Err(format!(
            "RangeError: Cannot create a Buffer larger than {limit} bytes"
        ));
    }
    Ok(out)
}

fn run(compress_it: bool, src: &[u8], max: Option<usize>) -> Result<Vec<u8>, String> {
    if compress_it {
        Ok(compress(src))
    } else {
        decompress(src, max)
    }
}

/// A limit of zero or less means no limit, which is how the JS layer passes an absent
/// maxOutputLength.
fn limit_of(max: f64) -> Option<usize> {
    if !max.is_finite() || max <= 0.0 {
        return None;
    }
    Some(max as usize)
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    driver
        .context()
        .define_function(
            &global,
            "__zstd_run",
            3,
            |scope: &mut Scope<'_>, compress_it: bool, data: Value, max: f64| {
                let max = limit_of(max);
                let outcome = bytes::with_bytes(scope, &data, |raw| run(compress_it, raw, max))?;
                match outcome {
                    Ok(out) => bytes::give(scope, &out),
                    Err(message) => Err(super::throw(scope, &message)),
                }
            },
        )
        .map_err(|error| format!("__zstd_run: {error}"))?;

    Ok(())
}

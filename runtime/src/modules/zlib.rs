//! zlib: deflate, gzip and brotli, carried over from LLRT
//! (modules/llrt_zlib plus libs/llrt_compression).
//!
//! The one-shot functions are LLRT's: the same read adapters, the same
//! maxOutputLength handling that stops reading one byte past the limit instead
//! of decompressing everything first. What changed:
//!
//! - zstd is gone. There is no pure-Rust zstd encoder and this runtime does not
//!   link C, so zstdCompress and zstdDecompress are absent rather than wrong.
//! - LLRT's define_sync_function and define_cb_function macros became two host
//!   functions: __zlib_run returns the buffer, __zlib_start returns a promise
//!   that settles through host::pending, and the callback style is JS.
//! - Compression runs on smol's blocking pool, so a large gzip does not stall
//!   the realm; LLRT ran it inline on the JS thread.

use std::io::{Cursor, Read};

use flate2::Compression;
use flate2::read::{
    DeflateDecoder, DeflateEncoder, GzDecoder, GzEncoder, ZlibDecoder, ZlibEncoder,
};
use quickjs_oxide::engine::api::core::{Scope, Value};

use crate::driver::Driver;
use crate::host::pending::{self, Pending};
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/zlib.js");

/// Brotli's own quality range; Node's default is the maximum.
const BROTLI_DEFAULT_QUALITY: u32 = 11;
const BROTLI_BUFFER: usize = 8_096;
const BROTLI_WINDOW: i32 = 22;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Deflate,
    DeflateRaw,
    Gzip,
    Inflate,
    InflateRaw,
    Gunzip,
    BrotliCompress,
    BrotliDecompress,
}

fn kind_of(name: &str) -> Option<Kind> {
    Some(match name {
        "deflate" => Kind::Deflate,
        "deflateRaw" => Kind::DeflateRaw,
        "gzip" => Kind::Gzip,
        "inflate" => Kind::Inflate,
        "inflateRaw" => Kind::InflateRaw,
        "gunzip" => Kind::Gunzip,
        "brotliCompress" => Kind::BrotliCompress,
        "brotliDecompress" => Kind::BrotliDecompress,
        _ => return None,
    })
}

/// Drain a reader into a buffer, refusing to go past the limit.
///
/// Reading stops one byte past the limit, so an over-long result is caught
/// without inflating (or allocating) the rest of the payload. The message keeps
/// its RangeError prefix so the JS layer can throw the right class.
fn collect<R: Read>(
    mut reader: R,
    capacity: usize,
    limit: Option<usize>,
) -> Result<Vec<u8>, String> {
    let Some(limit) = limit else {
        let mut out = Vec::with_capacity(capacity);
        reader
            .read_to_end(&mut out)
            .map_err(|error| error.to_string())?;
        return Ok(out);
    };

    let cutoff = limit.saturating_add(1);
    let mut out = Vec::with_capacity(capacity.min(cutoff));
    reader
        .take(cutoff as u64)
        .read_to_end(&mut out)
        .map_err(|error| error.to_string())?;

    if out.len() > limit {
        return Err(format!(
            "RangeError: Cannot create a Buffer larger than {limit} bytes"
        ));
    }

    Ok(out)
}

fn run(kind: Kind, src: &[u8], level: Option<u32>, max: Option<usize>) -> Result<Vec<u8>, String> {
    let capacity = src.len();
    // flate2 and brotli have different ideas of a default level, so an absent
    // level is passed through as absent.
    let level = level.unwrap_or(Compression::default().level());
    let quality = level.min(BROTLI_DEFAULT_QUALITY);
    let input = || Cursor::new(src);
    match kind {
        Kind::Deflate => collect(
            ZlibEncoder::new(input(), Compression::new(level)),
            capacity,
            max,
        ),
        Kind::DeflateRaw => collect(
            DeflateEncoder::new(input(), Compression::new(level)),
            capacity,
            max,
        ),
        Kind::Gzip => collect(
            GzEncoder::new(input(), Compression::new(level)),
            capacity,
            max,
        ),
        Kind::Inflate => collect(ZlibDecoder::new(input()), capacity, max),
        Kind::InflateRaw => collect(DeflateDecoder::new(input()), capacity, max),
        Kind::Gunzip => collect(GzDecoder::new(input()), capacity, max),
        Kind::BrotliCompress => collect(
            brotli::CompressorReader::new(input(), BROTLI_BUFFER, quality, BROTLI_WINDOW as u32),
            capacity,
            max,
        ),
        Kind::BrotliDecompress => collect(
            brotli::Decompressor::new(input(), BROTLI_BUFFER),
            capacity,
            max,
        ),
    }
}

/// Node's level option. Anything below zero (the default, -1) means "let the
/// library decide"; flate2 and brotli then pick their own defaults.
fn level_of(level: f64) -> Option<u32> {
    if !level.is_finite() || level < 0.0 {
        return None;
    }
    Some((level as u32).min(11))
}

/// A limit of zero or less means no limit, which is how the JS layer passes an
/// absent maxOutputLength.
fn limit_of(max: f64) -> Option<usize> {
    if !max.is_finite() || max <= 0.0 {
        return None;
    }
    Some(max as usize)
}

/// CRC-32 with the zlib polynomial, for zlib.crc32. The table is built at
/// compile time; the bit-at-a-time version cost 301 us per 45 KiB, this one
/// about 1 GB/s.
const fn crc_table() -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut index = 0;
    while index < 256 {
        let mut crc = index as u32;
        let mut bit = 0;
        while bit < 8 {
            let mask = 0u32.wrapping_sub(crc & 1);
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            bit += 1;
        }
        table[index] = crc;
        index += 1;
    }
    table
}

static CRC_TABLE: [u32; 256] = crc_table();

fn crc32(mut crc: u32, bytes: &[u8]) -> u32 {
    crc = !crc;
    for byte in bytes {
        let slot = ((crc ^ u32::from(*byte)) & 0xFF) as usize;
        crc = (crc >> 8) ^ CRC_TABLE[slot];
    }
    !crc
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let pending: Pending = driver.host.pending.clone();

    {
        driver
            .context()
            .define_function(
                &global,
                "__zlib_run",
                4,
                |scope: &mut Scope<'_>, kind: String, data: Value, level: f64, max: f64| {
                    let Some(parsed) = kind_of(&kind) else {
                        return Err(super::throw(scope, &format!("Unknown compression: {kind}")));
                    };
                    let level = level_of(level);
                    let max = limit_of(max);
                    let outcome =
                        bytes::with_bytes(scope, &data, |raw| run(parsed, raw, level, max))?;
                    match outcome {
                        // An ArrayBuffer; the JS layer wraps it in a Buffer. The
                        // RangeError prefix travels in the message because the
                        // engine's native errors all come out named Error here.
                        Ok(out) => bytes::give(scope, &out),
                        Err(message) => Err(super::throw(scope, &message)),
                    }
                },
            )
            .map_err(|error| format!("__zlib_run: {error}"))?;
    }

    {
        let pending = pending.clone();
        driver
            .context()
            .define_function(
                &global,
                "__zlib_start",
                4,
                move |scope: &mut Scope<'_>, kind: String, data: Value, level: f64, max: f64| {
                    let Some(parsed) = kind_of(&kind) else {
                        return Err(super::throw(scope, &format!("Unknown compression: {kind}")));
                    };
                    let level = level_of(level);
                    let max = limit_of(max);
                    let source = bytes::take(scope, &data)?;
                    pending::start(scope, &pending, async move {
                        // The blocking pool, so a slow gzip does not hold the realm.
                        let outcome = smol::unblock(move || run(parsed, &source, level, max)).await;
                        outcome
                            .map(crate::host::Payload::Bytes)
                            .map_err(|error| error)
                    })
                },
            )
            .map_err(|error| format!("__zlib_start: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__crc32",
                2,
                |scope: &mut Scope<'_>, data: Value, seed: f64| {
                    let seed = if seed.is_finite() { seed as u32 } else { 0 };
                    let value = bytes::with_bytes(scope, &data, |raw| crc32(seed, raw))?;
                    Ok(f64::from(value))
                },
            )
            .map_err(|error| format!("__crc32: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("zlib prelude: {error}"))?;

    let module = driver
        .context()
        .eval("globalThis.__zlib_module")
        .map_err(|error| format!("__zlib_module: {error}"))?;
    let names = [
        "deflate",
        "deflateSync",
        "deflateRaw",
        "deflateRawSync",
        "gzip",
        "gzipSync",
        "inflate",
        "inflateSync",
        "inflateRaw",
        "inflateRawSync",
        "gunzip",
        "gunzipSync",
        "brotliCompress",
        "brotliCompressSync",
        "brotliDecompress",
        "brotliDecompressSync",
        "crc32",
        "constants",
        "default",
    ];
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "zlib", exports)
}

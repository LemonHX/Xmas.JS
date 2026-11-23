//! fetch: the web request API. The socket work lives in host/http.rs, this file is the bridge.
//!
//! A request is one host call that hands the answer back as a flat description; the body is
//! read afterwards, one chunk per read, so a response body streams at the caller's pace and a
//! body that is never read costs nothing. A body waits in a channel of two chunks between the
//! socket and the JS side, which is what keeps a slow reader from buffering a whole download.
//!
//! Decompression happens on the way through: gzip and deflate stream through flate2, brotli is
//! collected first because its decoder wants a reader, and an encoding nobody here knows is
//! handed over exactly as it arrived.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Read;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use flate2::{Decompress, FlushDecompress, Status};
use futures_lite::AsyncReadExt;
use quickjs_oxide::engine::api::core::{
    Context, JsString, NativeErrorKind, PromiseCapability, RuntimeError, Scope, Value,
};

use crate::driver::Driver;
use crate::host::Payload;
use crate::host::http::{self, ABORTED, Encoding};
use crate::host::pending::{Job, Reply};
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/fetch.js");

/// What a finished body sends instead of bytes.
const EOF: &str = "eof";

/// One piece of a body: bytes, the end of it, or the reason there will be no more.
///
/// The end is a message rather than a closed channel because the realm thread keeps a sender
/// of its own to hand an abort to a waiting read; a channel with two senders never closes.
enum Chunk {
    Data(Vec<u8>),
    End,
    Error(String),
}

/// A body as the realm thread sees it: a queue of read requests going out, and the abort flag.
struct Body {
    requests: smol::channel::Sender<Job>,
    chunks: smol::channel::Sender<Chunk>,
    cancel: Arc<AtomicBool>,
    /// Wakes the request out of whatever it is waiting for when the caller aborts.
    stop: smol::channel::Sender<()>,
}

/// The bodies of this realm. Ids come from JS so an abort can name a request before its
/// response exists.
#[derive(Default)]
struct State {
    bodies: RefCell<BTreeMap<u64, Rc<Body>>>,
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let pending = driver.host.pending.clone();
    let state = Rc::new(State::default());

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__fetch",
                6,
                move |scope: &mut Scope<'_>,
                      id: f64,
                      method: String,
                      url: String,
                      headers: String,
                      redirect: String,
                      body: Value|
                      -> Result<Value, RuntimeError> {
                    let body = match body {
                        Value::Undefined | Value::Null => None,
                        other => Some(bytes::take(scope, &other)?),
                    };
                    let key = id as u64;
                    let request = http::Request {
                        method,
                        url,
                        headers: pairs(&headers),
                        body,
                    };
                    let follow = redirect != "manual";
                    let cancel = Arc::new(AtomicBool::new(false));
                    let (chunk_tx, chunk_rx) = smol::channel::bounded::<Chunk>(2);
                    let (request_tx, request_rx) = smol::channel::unbounded::<Job>();
                    let (stop_tx, stop_rx) = smol::channel::bounded::<()>(1);
                    state.bodies.borrow_mut().insert(
                        key,
                        Rc::new(Body {
                            requests: request_tx,
                            chunks: chunk_tx.clone(),
                            cancel: Arc::clone(&cancel),
                            stop: stop_tx,
                        }),
                    );

                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();
                    let state_of_settle = Rc::clone(&state);
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            if reply.is_err() {
                                state_of_settle.bodies.borrow_mut().remove(&key);
                            }
                            settle(context, capability, reply);
                        }));

                    // cloned before the request takes one, so the pump can watch the same flag
                    let abort = Arc::clone(&cancel);
                    smol::spawn(async move {
                        // An abort has to reach a request that is still waiting for its
                        // response, so the wait races the stop signal instead of polling a flag.
                        let sent = futures_lite::future::or(
                            http::send(request, follow, cancel),
                            async move {
                                let _ = stop_rx.recv().await;
                                Err(ABORTED.to_string())
                            },
                        );
                        match sent.await {
                            Ok(response) => {
                                let description = describe(&response);
                                let encoding = response.encoding;
                                let body = response.body;
                                smol::spawn(
                                    async move { pump(body, encoding, chunk_tx, abort).await },
                                )
                                .detach();
                                smol::spawn(async move { serve(request_rx, chunk_rx).await })
                                    .detach();
                                parked.finish(Ok(Payload::Text(description)));
                            }
                            Err(message) => parked.finish(Err(message)),
                        }
                    })
                    .detach();

                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__fetch: {error}"))?;
    }

    {
        let pending = pending.clone();
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__fetch_read",
                1,
                move |scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let key = id as u64;
                    let Some(body) = state.bodies.borrow().get(&key).cloned() else {
                        return Err(bytes::error(scope, "the body is no longer readable"));
                    };
                    let capability = scope.new_promise()?;
                    let promise = capability.promise.clone();
                    let state_of_settle = Rc::clone(&state);
                    let parked =
                        pending.park(Box::new(move |context: &mut Context, reply: Reply| {
                            if matches!(&reply, Ok(Payload::Text(text)) if text == EOF) {
                                state_of_settle.bodies.borrow_mut().remove(&key);
                            }
                            settle(context, capability, reply);
                        }));
                    // A failed send means the body is gone, which is the same answer as EOF.
                    if body.requests.try_send(parked).is_err() {
                        let state_of_settle = Rc::clone(&state);
                        state_of_settle.bodies.borrow_mut().remove(&key);
                    }
                    Ok(Value::Object(promise))
                },
            )
            .map_err(|error| format!("__fetch_read: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__fetch_abort",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let Some(body) = state.bodies.borrow().get(&(id as u64)).cloned() else {
                        return Ok(Value::Undefined);
                    };
                    body.cancel.store(true, Ordering::Relaxed);
                    let _ = body.stop.try_send(());
                    // The error goes through the same channel as the chunks, so a read that is
                    // already waiting rejects now instead of waiting for a byte that never comes.
                    let chunks = body.chunks.clone();
                    smol::spawn(async move {
                        let _ = chunks.send(Chunk::Error(ABORTED.to_string())).await;
                    })
                    .detach();
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__fetch_abort: {error}"))?;
    }

    {
        let state = Rc::clone(&state);
        driver
            .context()
            .define_function(
                &global,
                "__fetch_free",
                1,
                move |_scope: &mut Scope<'_>, id: f64| -> Result<Value, RuntimeError> {
                    let body = state.bodies.borrow_mut().remove(&(id as u64));
                    if let Some(body) = body {
                        body.cancel.store(true, Ordering::Relaxed);
                    }
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__fetch_free: {error}"))?;
    }

    driver
        .context()
        .eval(PRELUDE)
        .map_err(|error| format!("fetch prelude: {error}"))?;
    let module = driver
        .context()
        .eval("__fetch_module")
        .map_err(|error| format!("__fetch_module: {error}"))?;
    let exports = super::collect(
        driver,
        &module,
        &[
            "fetch", "Headers", "Request", "Response", "FormData", "File", "Blob",
        ],
    )?;
    super::define_with_alias(driver, "fetch", exports)
}

/// Split the NUL-joined header list JS hands over.
fn pairs(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut parts = text.split('\0');
    while let (Some(name), Some(value)) = (parts.next(), parts.next()) {
        if !name.is_empty() {
            out.push((name.to_string(), value.to_string()));
        }
    }
    out
}

/// The answer as one flat string: status, status text, url, whether it was redirected, then
/// the header pairs. NUL is the separator, which is safe because no HTTP header can hold one.
fn describe(response: &http::Response) -> String {
    let mut out = String::with_capacity(128);
    out.push_str(&response.status.to_string());
    out.push('\0');
    out.push_str(&response.status_text);
    out.push('\0');
    out.push_str(&response.url);
    out.push('\0');
    out.push(if response.redirected { '1' } else { '0' });
    for (name, value) in &response.headers {
        out.push('\0');
        out.push_str(name);
        out.push('\0');
        out.push_str(value);
    }
    out
}

fn settle(context: &mut Context, capability: PromiseCapability, reply: Reply) {
    match reply {
        Ok(payload) => {
            let value = match payload {
                Payload::Bytes(bytes) => bytes::give(context, &bytes).unwrap_or(Value::Undefined),
                Payload::Text(text) if text == EOF => Value::String(JsString::from_static(EOF)),
                Payload::Text(text) => match JsString::try_from_utf8(&text) {
                    Ok(text) => Value::String(text),
                    Err(_) => Value::Undefined,
                },
            };
            let _ = capability.resolve(context, value);
        }
        Err(message) => {
            let value = match context.new_native_error(NativeErrorKind::Type, &message) {
                Ok(value) => value,
                Err(_) => Value::Undefined,
            };
            let _ = capability.reject(context, value);
        }
    }
}

/// Hand the chunks to whoever asks for them, one read at a time.
async fn serve(requests: smol::channel::Receiver<Job>, chunks: smol::channel::Receiver<Chunk>) {
    while let Ok(job) = requests.recv().await {
        let reply = match chunks.recv().await {
            Ok(Chunk::Data(bytes)) => Ok(Payload::Bytes(bytes)),
            Ok(Chunk::End) => Ok(Payload::Text(EOF.to_string())),
            Ok(Chunk::Error(message)) => Err(message),
            Err(_) => Ok(Payload::Text(EOF.to_string())),
        };
        job.finish(reply);
    }
}

/// Read the socket and put decoded bytes into the channel. Stops at EOF, on an error, or when
/// the abort flag is set.
async fn pump(
    mut body: http::Body,
    encoding: Encoding,
    chunks: smol::channel::Sender<Chunk>,
    cancel: Arc<AtomicBool>,
) {
    let mut decoder = Decoder::new(encoding);
    let mut buffer = vec![0u8; 16 * 1024];

    loop {
        if cancel.load(Ordering::Relaxed) {
            let _ = chunks.send(Chunk::Error(ABORTED.to_string())).await;
            return;
        }
        let read = match body.read(&mut buffer).await {
            Ok(0) => break,
            Ok(read) => read,
            Err(error) => {
                let _ = chunks.send(Chunk::Error(error.to_string())).await;
                return;
            }
        };
        let ended = match decoder.push(&buffer[..read]) {
            Ok((bytes, ended)) => {
                if !bytes.is_empty() && chunks.send(Chunk::Data(bytes)).await.is_err() {
                    return;
                }
                ended
            }
            Err(message) => {
                let _ = chunks.send(Chunk::Error(message)).await;
                return;
            }
        };
        if ended {
            let _ = chunks.send(Chunk::End).await;
            return;
        }
    }

    match decoder.finish() {
        Ok(bytes) => {
            if !bytes.is_empty() {
                let _ = chunks.send(Chunk::Data(bytes)).await;
            }
            let _ = chunks.send(Chunk::End).await;
        }
        Err(message) => {
            let _ = chunks.send(Chunk::Error(message)).await;
        }
    }
}

/// Turns what came off the socket into what the caller should see.
enum Decoder {
    Identity,
    /// deflate is zlib-wrapped, so the zlib header is expected.
    Deflate(Decompress),
    Gzip(Gzip),
    /// Brotli is collected whole: this build of flate2 has no gzip or window-bit
    /// constructor, and the brotli decoder wants a reader anyway.
    Brotli(Vec<u8>),
}

struct Gzip {
    /// Header bytes seen so far, kept until the header is complete.
    header: Vec<u8>,
    started: bool,
    inflate: Decompress,
}

impl Decoder {
    fn new(encoding: Encoding) -> Decoder {
        match encoding {
            Encoding::Gzip => Decoder::Gzip(Gzip {
                header: Vec::new(),
                started: false,
                // after the header what follows is a bare deflate stream
                inflate: Decompress::new(false),
            }),
            Encoding::Deflate => Decoder::Deflate(Decompress::new(true)),
            Encoding::Brotli => Decoder::Brotli(Vec::new()),
            Encoding::Identity | Encoding::Unknown => Decoder::Identity,
        }
    }

    /// Feed one piece of the body. Returns the bytes it produced and whether the
    /// compressed stream says it has ended.
    fn push(&mut self, input: &[u8]) -> Result<(Vec<u8>, bool), String> {
        match self {
            Decoder::Identity => Ok((input.to_vec(), false)),
            Decoder::Deflate(inflate) => {
                let mut out = Vec::new();
                let ended = inflate_into(inflate, input, &mut out, FlushDecompress::None)?;
                Ok((out, ended))
            }
            Decoder::Gzip(gzip) => {
                if !gzip.started {
                    gzip.header.extend_from_slice(input);
                    let Some(offset) = gzip_offset(&gzip.header) else {
                        return Ok((Vec::new(), false));
                    };
                    let rest = gzip.header[offset..].to_vec();
                    gzip.header = Vec::new();
                    gzip.started = true;
                    let mut out = Vec::new();
                    let ended =
                        inflate_into(&mut gzip.inflate, &rest, &mut out, FlushDecompress::None)?;
                    return Ok((out, ended));
                }
                let mut out = Vec::new();
                let ended =
                    inflate_into(&mut gzip.inflate, input, &mut out, FlushDecompress::None)?;
                Ok((out, ended))
            }
            Decoder::Brotli(raw) => {
                raw.extend_from_slice(input);
                Ok((Vec::new(), false))
            }
        }
    }

    /// Whatever is left at the end of the body.
    fn finish(&mut self) -> Result<Vec<u8>, String> {
        match self {
            Decoder::Identity => Ok(Vec::new()),
            Decoder::Deflate(inflate) => {
                let mut out = Vec::new();
                inflate_into(inflate, &[], &mut out, FlushDecompress::Finish)?;
                Ok(out)
            }
            Decoder::Gzip(gzip) => {
                let mut out = Vec::new();
                inflate_into(&mut gzip.inflate, &[], &mut out, FlushDecompress::Finish)?;
                Ok(out)
            }
            Decoder::Brotli(raw) => {
                let mut decoded = Vec::new();
                brotli::Decompressor::new(&raw[..], 4096)
                    .read_to_end(&mut decoded)
                    .map_err(|error| error.to_string())?;
                Ok(decoded)
            }
        }
    }
}

/// Push input through a decompressor until it stops making progress. Returns whether the
/// stream reached its end.
fn inflate_into(
    inflate: &mut Decompress,
    input: &[u8],
    out: &mut Vec<u8>,
    flush: FlushDecompress,
) -> Result<bool, String> {
    let mut produced = vec![0u8; 32 * 1024];
    let mut offset = 0usize;
    for _ in 0..64 {
        let before_in = inflate.total_in();
        let before_out = inflate.total_out();
        let status = inflate
            .decompress(&input[offset..], &mut produced, flush)
            .map_err(|error| error.to_string())?;
        let used = (inflate.total_in() - before_in) as usize;
        let made = (inflate.total_out() - before_out) as usize;
        offset += used;
        if made > 0 {
            out.extend_from_slice(&produced[..made]);
        }
        match status {
            Status::StreamEnd => return Ok(true),
            Status::Ok | Status::BufError => {
                if (used == 0 && made == 0) || offset >= input.len() {
                    return Ok(false);
                }
            }
        }
    }
    Ok(false)
}

/// How many bytes of a gzip header are here, if the whole header is here.
///
/// The optional fields (an extra block, a name, a comment, a header checksum) are what make
/// this more than ten bytes. A stream that does not start with the gzip magic is handed to the
/// inflater as it is, so a mislabelled body fails as a deflate error instead of vanishing.
fn gzip_offset(bytes: &[u8]) -> Option<usize> {
    if bytes.len() < 2 {
        return None;
    }
    if bytes[0] != 0x1f || bytes[1] != 0x8b {
        return Some(0);
    }
    if bytes.len() < 10 {
        return None;
    }
    let flags = bytes[3];
    let mut offset = 10;
    if flags & 0x04 != 0 {
        if bytes.len() < offset + 2 {
            return None;
        }
        let extra = u16::from_le_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        offset += 2 + extra;
    }
    if flags & 0x08 != 0 {
        offset = past_nul(bytes, offset)?;
    }
    if flags & 0x10 != 0 {
        offset = past_nul(bytes, offset)?;
    }
    if flags & 0x02 != 0 {
        offset += 2;
    }
    if bytes.len() < offset {
        None
    } else {
        Some(offset)
    }
}

fn past_nul(bytes: &[u8], from: usize) -> Option<usize> {
    if from >= bytes.len() {
        return None;
    }
    bytes[from..]
        .iter()
        .position(|byte| *byte == 0)
        .map(|index| from + index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    const GZIPPED: [u8; 28] = [
        0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x13, 0xcb, 0x48, 0xcd, 0xc9, 0xc9,
        0x57, 0xc8, 0x40, 0x90, 0x00, 0x80, 0x88, 0xf9, 0xe5, 0x11, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn a_known_gzip_stream_decodes() {
        let mut decoder = Decoder::new(Encoding::Gzip);
        let (bytes, ended) = decoder.push(&GZIPPED).expect("push");
        assert!(ended, "the stream should end inside the one chunk");
        assert_eq!(bytes, b"hello hello hello");
    }

    #[test]
    fn a_gzip_header_split_across_chunks_still_decodes() {
        let mut decoder = Decoder::new(Encoding::Gzip);
        let mut out = Vec::new();
        // three bytes at a time, so the optional-field parser has to wait for more
        for piece in GZIPPED.chunks(3) {
            let (bytes, _ended) = decoder.push(piece).expect("push");
            out.extend_from_slice(&bytes);
        }
        out.extend_from_slice(&decoder.finish().expect("finish"));
        assert_eq!(out, b"hello hello hello");
    }

    #[test]
    fn a_body_that_is_not_gzip_fails_loudly() {
        let mut decoder = Decoder::new(Encoding::Gzip);
        let result = decoder.push(b"not gzip at all");
        assert!(result.is_err());
    }
}

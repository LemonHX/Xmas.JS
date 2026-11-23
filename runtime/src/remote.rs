//! Remote module bodies: the bytes behind an `https:` or `data:` import.
//!
//! The engine asks the loader for a module synchronously, in the middle of evaluation, and cannot
//! be suspended. So this is a blocking fetch on the realm thread, not a promise: see docs/remote.md
//! for why there is no other choice. It runs rarely -- once per URL per machine, not per run --
//! because what it fetches is cached on disk.

use std::fs::create_dir_all;
use std::path::PathBuf;

use sha2::{Digest, Sha256};

/// What a module body turned out to be.
#[derive(Debug)]
pub struct Fetched {
    pub bytes: Vec<u8>,
    /// The media type the response carried, lowercased and without parameters. Empty when the
    /// response said nothing, and `data:` URLs carry theirs in the URL.
    pub media_type: String,
}

/// The bytes for `url`, from the cache if they are there and from the network if they are not.
///
/// A failure is never written to the cache: a 404 today can be a file tomorrow, and caching it
/// would make the runtime refuse a URL that works.
pub fn body(url: &str) -> Result<Fetched, String> {
    if let Some(rest) = url.strip_prefix("data:") {
        return data_url(url, rest);
    }
    let (path, type_path) = cache_paths(url)?;
    if let Some(bytes) = std::fs::read(&path).ok() {
        let media_type = std::fs::read_to_string(&type_path)
            .unwrap_or_default()
            .trim()
            .to_owned();
        return Ok(Fetched { bytes, media_type });
    }
    let (bytes, media_type) = download(url)?;
    // Best effort: a cache that cannot be written is not a reason for the import to fail.
    let _ = std::fs::write(&path, &bytes);
    let _ = std::fs::write(&type_path, &media_type);
    Ok(Fetched { bytes, media_type })
}

/// The cache lives under `.xmas/remote` in the working directory, beside the package store.
///
/// The file name is the sha256 of the URL, not the URL itself: a URL carries `?`, `#` and `/`,
/// which are not a file name on every platform this runtime targets. The media type goes in a
/// sidecar so the loader knows what it has without sniffing the bytes again.
fn cache_paths(url: &str) -> Result<(PathBuf, PathBuf), String> {
    let digest = Sha256::digest(url.as_bytes());
    let name: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let dir = PathBuf::from(".xmas").join("remote");
    create_dir_all(&dir).map_err(|error| format!("{}: {error}", dir.display()))?;
    let path = dir.join(&name);
    let type_path = dir.join(format!("{name}.type"));
    Ok((path, type_path))
}

fn download(url: &str) -> Result<(Vec<u8>, String), String> {
    use futures_lite::AsyncReadExt;

    // identity: the body is compiled as it arrives, and a compressed body would mean a decoder on
    // the realm thread for a module that is already going to sit in a file.
    let request = crate::host::http::Request {
        method: "GET".into(),
        url: url.to_owned(),
        headers: vec![("accept-encoding".into(), "identity".into())],
        body: None,
    };
    // Follow redirects: a package registry moves files around, and the URL in the source is the
    // name, not necessarily the address. Nothing can cancel this one -- the engine is waiting on
    // it and cannot be given a signal -- so the flag is a fresh false.
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let response = smol::block_on(crate::host::http::send(request, true, cancel))
        .map_err(|error| format!("cannot fetch {url}: {error}"))?;
    if !(200..300).contains(&response.status) {
        return Err(format!(
            "cannot fetch {url}: HTTP {} {}",
            response.status, response.status_text
        ));
    }
    let media_type = response
        .headers
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case("content-type"))
        .map(|(_, value)| media_type_of(value))
        .unwrap_or_default();
    let mut body = response.body;
    let mut bytes = Vec::new();
    smol::block_on(body.read_to_end(&mut bytes))
        .map_err(|error| format!("cannot read the body of {url}: {error}"))?;
    Ok((bytes, media_type))
}

/// The media type without its parameters: `text/javascript; charset=utf-8` is `text/javascript`.
fn media_type_of(value: &str) -> String {
    value
        .split(';')
        .next()
        .unwrap_or("")
        .trim()
        .to_ascii_lowercase()
}

/// `data:[<media type>][;base64],<data>`
fn data_url(url: &str, rest: &str) -> Result<Fetched, String> {
    let (head, payload) = rest
        .split_once(',')
        .ok_or_else(|| format!("{url}: a data URL has no comma"))?;
    let mut media_type = String::new();
    let mut base64 = false;
    for (index, part) in head.split(';').enumerate() {
        if part.eq_ignore_ascii_case("base64") {
            base64 = true;
        } else if index == 0 && !part.is_empty() {
            media_type = part.to_ascii_lowercase();
        }
    }
    let bytes = if base64 {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .map_err(|error| format!("{url}: {error}"))?
    } else {
        percent_decode(payload)
    };
    Ok(Fetched { bytes, media_type })
}

fn percent_decode(text: &str) -> Vec<u8> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut at = 0;
    while at < bytes.len() {
        if bytes[at] == b'%' && at + 2 < bytes.len() {
            if let (Some(high), Some(low)) = (
                crate::host::http::hex(bytes[at + 1]),
                crate::host::http::hex(bytes[at + 2]),
            ) {
                out.push(high * 16 + low);
                at += 3;
                continue;
            }
        }
        out.push(bytes[at]);
        at += 1;
    }
    out
}

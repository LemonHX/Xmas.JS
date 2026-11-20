//! crypto: the non-subtle half so far (hash, hmac, random numbers, uuid).
//!
//! WebCrypto's subtle is another big chunk (the old 3412 lines); it waits until this layer
//! is stable and then migrates separately. Digest state stays in Rust's table and JS only
//! holds an id, so each update crosses the bridge with just its own bytes, the way Node's
//! streaming semantics want, and no chunk is accumulated in JS.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use hmac::{Hmac, Mac};
use md5::Md5;
use sha1::Sha1;
use sha2::{Digest, Sha256, Sha384, Sha512};

use quickjs_oxide::engine::api::core::{JsString, RuntimeError, Scope, Value};

use crate::driver::Driver;
use crate::modules::bytes;

const PRELUDE: &str = include_str!("../../js/crypto.js");

fn unknown(scope: &mut Scope<'_>, message: &str) -> RuntimeError {
    super::throw(scope, message)
}

enum Hasher {
    Sha1(Sha1),
    Sha256(Sha256),
    Sha384(Sha384),
    Sha512(Sha512),
    Md5(Md5),
}

impl Hasher {
    fn new(name: &str) -> Option<Self> {
        Some(match name {
            "sha1" | "sha-1" => Self::Sha1(Sha1::new()),
            "sha256" | "sha-256" => Self::Sha256(Sha256::new()),
            "sha384" | "sha-384" => Self::Sha384(Sha384::new()),
            "sha512" | "sha-512" => Self::Sha512(Sha512::new()),
            "md5" => Self::Md5(Md5::new()),
            _ => return None,
        })
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(hasher) => hasher.update(data),
            Self::Sha256(hasher) => hasher.update(data),
            Self::Sha384(hasher) => hasher.update(data),
            Self::Sha512(hasher) => hasher.update(data),
            Self::Md5(hasher) => hasher.update(data),
        }
    }

    fn finish(self) -> Vec<u8> {
        match self {
            Self::Sha1(hasher) => hasher.finalize().to_vec(),
            Self::Sha256(hasher) => hasher.finalize().to_vec(),
            Self::Sha384(hasher) => hasher.finalize().to_vec(),
            Self::Sha512(hasher) => hasher.finalize().to_vec(),
            Self::Md5(hasher) => hasher.finalize().to_vec(),
        }
    }

    fn clone_state(&self) -> Self {
        match self {
            Self::Sha1(hasher) => Self::Sha1(hasher.clone()),
            Self::Sha256(hasher) => Self::Sha256(hasher.clone()),
            Self::Sha384(hasher) => Self::Sha384(hasher.clone()),
            Self::Sha512(hasher) => Self::Sha512(hasher.clone()),
            Self::Md5(hasher) => Self::Md5(hasher.clone()),
        }
    }
}

enum Macer {
    Sha1(Hmac<Sha1>),
    Sha256(Hmac<Sha256>),
    Sha384(Hmac<Sha384>),
    Sha512(Hmac<Sha512>),
    Md5(Hmac<Md5>),
}

impl Macer {
    fn new(name: &str, key: &[u8]) -> Option<Self> {
        Some(match name {
            "sha1" | "sha-1" => Self::Sha1(Hmac::new_from_slice(key).ok()?),
            "sha256" | "sha-256" => Self::Sha256(Hmac::new_from_slice(key).ok()?),
            "sha384" | "sha-384" => Self::Sha384(Hmac::new_from_slice(key).ok()?),
            "sha512" | "sha-512" => Self::Sha512(Hmac::new_from_slice(key).ok()?),
            "md5" => Self::Md5(Hmac::new_from_slice(key).ok()?),
            _ => return None,
        })
    }

    fn update(&mut self, data: &[u8]) {
        match self {
            Self::Sha1(mac) => mac.update(data),
            Self::Sha256(mac) => mac.update(data),
            Self::Sha384(mac) => mac.update(data),
            Self::Sha512(mac) => mac.update(data),
            Self::Md5(mac) => mac.update(data),
        }
    }

    fn finish(self) -> Vec<u8> {
        match self {
            Self::Sha1(mac) => mac.finalize().into_bytes().to_vec(),
            Self::Sha256(mac) => mac.finalize().into_bytes().to_vec(),
            Self::Sha384(mac) => mac.finalize().into_bytes().to_vec(),
            Self::Sha512(mac) => mac.finalize().into_bytes().to_vec(),
            Self::Md5(mac) => mac.finalize().into_bytes().to_vec(),
        }
    }
}

#[derive(Default)]
struct State {
    next: u64,
    hashes: HashMap<u64, Hasher>,
    macs: HashMap<u64, Macer>,
}

impl State {
    fn take(&mut self) -> u64 {
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
                "__hash_new",
                1,
                move |_scope: &mut Scope<'_>, algorithm: String| {
                    let Some(hasher) = Hasher::new(&algorithm) else {
                        return Ok(0.0);
                    };
                    let mut state = state.borrow_mut();
                    let id = state.take();
                    state.hashes.insert(id, hasher);
                    Ok(id as f64)
                },
            )
            .map_err(|error| format!("__hash_new: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__hash_update",
                2,
                move |scope: &mut Scope<'_>, id: f64, data: Value| {
                    let mut state = state.borrow_mut();
                    let Some(hasher) = state.hashes.get_mut(&(id as u64)) else {
                        return Err(unknown(scope, "hash is gone"));
                    };
                    bytes::with_bytes(scope, &data, |bytes| hasher.update(bytes))?;
                    Ok(true)
                },
            )
            .map_err(|error| format!("__hash_update: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__hash_digest",
                1,
                move |scope: &mut Scope<'_>, id: f64| {
                    let mut state = state.borrow_mut();
                    let Some(hasher) = state.hashes.remove(&(id as u64)) else {
                        return Err(unknown(scope, "hash is gone"));
                    };
                    let digest = hasher.finish();
                    bytes::give(scope, &digest)
                },
            )
            .map_err(|error| format!("__hash_digest: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__hash_copy",
                1,
                move |scope: &mut Scope<'_>, id: f64| {
                    let mut state = state.borrow_mut();
                    let Some(hasher) = state.hashes.get(&(id as u64)) else {
                        return Err(unknown(scope, "hash is gone"));
                    };
                    let copy = hasher.clone_state();
                    let new_id = state.take();
                    state.hashes.insert(new_id, copy);
                    Ok(new_id as f64)
                },
            )
            .map_err(|error| format!("__hash_copy: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__hash_once",
                2,
                move |scope: &mut Scope<'_>, algorithm: String, data: Value| {
                    let Some(mut hasher) = Hasher::new(&algorithm) else {
                        return Err(unknown(
                            scope,
                            &format!("Digest method not supported: {algorithm}"),
                        ));
                    };
                    bytes::with_bytes(scope, &data, |bytes| hasher.update(bytes))?;
                    let digest = hasher.finish();
                    bytes::give(scope, &digest)
                },
            )
            .map_err(|error| format!("__hash_once: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__hmac_new",
                2,
                move |scope: &mut Scope<'_>, algorithm: String, key: Value| {
                    let key = bytes::take(scope, &key)?;
                    let Some(mac) = Macer::new(&algorithm, &key) else {
                        return Ok(0.0);
                    };
                    let mut state = state.borrow_mut();
                    let id = state.take();
                    state.macs.insert(id, mac);
                    Ok(id as f64)
                },
            )
            .map_err(|error| format!("__hmac_new: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__hmac_update",
                2,
                move |scope: &mut Scope<'_>, id: f64, data: Value| {
                    let mut state = state.borrow_mut();
                    let Some(mac) = state.macs.get_mut(&(id as u64)) else {
                        return Err(unknown(scope, "hmac is gone"));
                    };
                    bytes::with_bytes(scope, &data, |bytes| mac.update(bytes))?;
                    Ok(true)
                },
            )
            .map_err(|error| format!("__hmac_update: {error}"))?;
    }

    {
        let state = state.clone();
        driver
            .context()
            .define_function(
                &global,
                "__hmac_digest",
                1,
                move |scope: &mut Scope<'_>, id: f64| {
                    let mut state = state.borrow_mut();
                    let Some(mac) = state.macs.remove(&(id as u64)) else {
                        return Err(unknown(scope, "hmac is gone"));
                    };
                    let digest = mac.finish();
                    bytes::give(scope, &digest)
                },
            )
            .map_err(|error| format!("__hmac_digest: {error}"))?;
    }

    driver
        .context()
        .define_function(
            &global,
            "__random_bytes",
            1,
            move |scope: &mut Scope<'_>, length: f64| {
                let length = length.max(0.0) as usize;
                let mut bytes = vec![0u8; length];
                if let Err(error) = getrandom::fill(&mut bytes) {
                    return Err(unknown(
                        scope,
                        &format!("random numbers unavailable: {error}"),
                    ));
                }
                bytes::give(scope, &bytes)
            },
        )
        .map_err(|error| format!("__random_bytes: {error}"))?;

    driver
        .context()
        .define_function(
            &global,
            "__random_int",
            2,
            move |scope: &mut Scope<'_>, low: f64, high: f64| {
                let (low, high) = (low as i64, high as i64);
                let span = (high as i128 - low as i128) as u128;
                if span == 0 {
                    return Ok(Value::Float(low as f64));
                }
                // Rejection sampling，discard the bias on these numbers just like Node
                let limit = u128::MAX - (u128::MAX % span);
                for _ in 0..64 {
                    let mut bytes = [0u8; 16];
                    if let Err(error) = getrandom::fill(&mut bytes) {
                        return Err(unknown(
                            scope,
                            &format!("random numbers unavailable: {error}"),
                        ));
                    }
                    let sample = u128::from_le_bytes(bytes);
                    if sample < limit {
                        return Ok(Value::Float((low as i128 + (sample % span) as i128) as f64));
                    }
                }
                Err(unknown(scope, "randomInt: unable to sample"))
            },
        )
        .map_err(|error| format!("__random_int: {error}"))?;

    driver
        .context()
        .define_function(&global, "__random_uuid", 0, move |scope: &mut Scope<'_>| {
            let mut bytes = [0u8; 16];
            if let Err(error) = getrandom::fill(&mut bytes) {
                return Err(unknown(
                    scope,
                    &format!("random numbers unavailable: {error}"),
                ));
            }
            bytes[6] = (bytes[6] & 0x0f) | 0x40;
            bytes[8] = (bytes[8] & 0x3f) | 0x80;
            let hex = super::buffer_hex(&bytes);
            let uuid = format!(
                "{}-{}-{}-{}-{}",
                &hex[0..8],
                &hex[8..12],
                &hex[12..16],
                &hex[16..20],
                &hex[20..32]
            );
            // Zero-argument host closures must return Value，otherwise HostFunction's bound is not satisfied
            match JsString::try_from_utf8(&uuid) {
                Ok(text) => Ok(Value::String(text)),
                Err(_) => Err(quickjs_oxide::engine::api::core::RuntimeError::Invariant(
                    "uuid cannot be converted to a string",
                )),
            }
        })
        .map_err(|error| format!("__random_uuid: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("crypto prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__crypto_module")
        .map_err(|error| format!("crypto module: {error}"))?;
    let names = [
        "createHash",
        "createHmac",
        "hash",
        "randomBytes",
        "randomFillSync",
        "randomUUID",
        "randomInt",
        "getRandomValues",
        "timingSafeEqual",
        "webcrypto",
    ];
    let exports = super::collect(driver, &module, &names)?;
    super::define_with_alias(driver, "crypto", exports)
}

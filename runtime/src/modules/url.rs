//! url：URL / URLSearchParams。
//!
//! The old modules/src/url/ was 1420 lines of rquickjs Class (url_class 328 + url_search_params 898).
//! This is split into two halves: parsing and normalization are delegated to Rust's url crate (the WHATWG rules are not worth rewriting),
//! Property access, searchParams, and iterators are all JS.
//!
//! The bridge has only two functions: __url_parse(input, base) and __url_set(href, field, value),
//! Both return a JSON string, and the JS side uses JSON.parse to turn it into a property bag.

use quickjs_oxide::engine::api::core::{Js, JsString, RuntimeError, Scope, Value};
use url::Url;

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/url.js");

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    driver
        .context()
        .define_function(
            &global,
            "__url_parse",
            2,
            |scope: &mut Scope<'_>, input: String, base: Value| -> Result<Value, RuntimeError> {
                let base = match &base {
                    Value::String(text) => Some(text.to_utf8_lossy()),
                    _ => None,
                };
                let url = match &base {
                    Some(base) => Url::parse(&base)
                        .and_then(|base| base.join(&input))
                        .map_err(|error| format!("{error}")),
                    None => Url::parse(&input).map_err(|error| format!("{error}")),
                }
                .map_err(|error| super::throw(scope, &error.to_string()))?;
                Ok(Value::String(text(&parts(&url))?))
            },
        )
        .map_err(|error| format!("__url_parse: {error}"))?;

    driver
        .context()
        .define_function(
            &global,
            "__url_set",
            3,
            |scope: &mut Scope<'_>,
             href: String,
             component: String,
             value: String|
             -> Result<Value, RuntimeError> {
                let mut url = Url::parse(&href).map_err(|error| super::throw(scope, &error.to_string()))?;
                set(&mut url, &component, &value).map_err(|error| super::throw(scope, &error))?;
                Ok(Value::String(text(&parts(&url))?))
            },
        )
        .map_err(|error| format!("__url_set: {error}"))?;

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("url prelude: {error}"))?;
    let module = driver
        .eval("globalThis.__url_module")
        .map_err(|error| format!("url module: {error}"))?;
    let exports = super::collect(driver, &module, &["URL", "URLSearchParams", "default"])?;
    super::define_with_alias(driver, "url", exports)
}

fn text(source: &str) -> Result<JsString, RuntimeError> {
    JsString::try_from_utf8(source).map_err(|error| RuntimeError::Internal(error.to_string()))
}

/// The public fields of a URL, flattened into JSON.
fn parts(url: &Url) -> String {
    let port = url.port().map(|port| port.to_string()).unwrap_or_default();
    let host = url.host_str().unwrap_or_default();
    let host_field = if port.is_empty() {
        host.to_string()
    } else {
        format!("{host}:{port}")
    };
    let path = url.path();
    let search = url.query().map(|query| format!("?{query}")).unwrap_or_default();
    let hash = url.fragment().map(|fragment| format!("#{fragment}")).unwrap_or_default();
    format!(
        "{{\"href\":{},\"protocol\":{},\"username\":{},\"password\":{},\"host\":{},\"hostname\":{},\"port\":{},\"pathname\":{},\"search\":{},\"hash\":{},\"origin\":{}}}",
        quote(url.as_str()),
        quote(&format!("{}:", url.scheme())),
        quote(url.username()),
        quote(url.password().unwrap_or_default()),
        quote(&host_field),
        quote(host),
        quote(&port),
        quote(path),
        quote(&search),
        quote(&hash),
        quote(&url.origin().ascii_serialization()),
    )
}

fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for character in text.chars() {
        match character {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            other if (other as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", other as u32)),
            other => out.push(other),
        }
    }
    out.push('"');
    out
}

fn set(url: &mut Url, component: &str, value: &str) -> Result<(), String> {
    match component {
        "protocol" => url
            .set_scheme(value.trim_end_matches(':'))
            .map_err(|_| "protocol is invalid".to_string()),
        "username" => url.set_username(value).map_err(|_| "username is invalid".to_string()),
        "password" => url
            .set_password(if value.is_empty() { None } else { Some(value) })
            .map_err(|_| "password is invalid".to_string()),
        "host" | "hostname" => url
            .set_host(if value.is_empty() { None } else { Some(value) })
            .map_err(|_| "host is invalid".to_string()),
        "port" => url
            .set_port(if value.is_empty() {
                None
            } else {
                Some(value.parse::<u16>().map_err(|_| "port is invalid".to_string())?)
            })
            .map_err(|_| "port is invalid".to_string()),
        "pathname" => {
            url.set_path(value);
            Ok(())
        }
        "search" => {
            let query = value.strip_prefix('?').unwrap_or(value);
            url.set_query(if query.is_empty() { None } else { Some(query) });
            Ok(())
        }
        "hash" => {
            let fragment = value.strip_prefix('#').unwrap_or(value);
            url.set_fragment(if fragment.is_empty() { None } else { Some(fragment) });
            Ok(())
        }
        other => Err(format!("unknown field: {other}")),
    }
}

//! Intl: DateTimeFormat over CLDR pattern data, and nothing else yet.
//!
//! There is no ICU here and no C. The zone rules and the calendar come from temporal_rs, which
//! this runtime already carries for Temporal; the rendering is the CLDR table in cldr_data.rs.
//! Two host functions are the whole boundary -- __intl_format and __intl_zone_names -- and the
//! classes, the option reading, the locale negotiation and the Date patches are in js/intl.js.
//!
//! What is missing is listed in docs/intl.md: no PluralRules, Collator, NumberFormat,
//! ListFormat, RelativeTimeFormat or DisplayNames, and a locale outside the table falls back to
//! the language it shares, then to en-US.

mod cldr_data;

// The timezone_provider singleton macro expands to `alloc::borrow::Cow`, which resolves only
// where a crate names alloc itself, is `no_std`, or uses edition 2018 (this is 2024).
extern crate alloc;

use quickjs_oxide::engine::api::core::{
    JsString, ObjectRef, PropertyKey, RuntimeError, Scope, Value,
};
use temporal_rs::{Instant, TimeZone, ZonedDateTime};
use timezone_provider::provider::TimeZoneProvider;
use timezone_provider::tzif::CompiledTzdbProvider;

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../../js/intl.js");

/// The fields one formatted value is built from: what the JS side already read off the options
/// object and resolved, so the formatting itself is one call.
pub(super) struct Fields {
    /// The locale tag the caller asked for; the table maps it to the one it carries.
    pub locale: String,
    pub month: Option<String>,
    pub day: Option<String>,
    pub year: Option<String>,
    pub hour: Option<String>,
    pub minute: Option<String>,
    pub second: Option<String>,
    pub weekday: Option<String>,
    pub hour12: Option<bool>,
    pub time_zone_name: Option<String>,
    pub date_style: Option<String>,
    pub time_style: Option<String>,
    /// The second timeZoneName option, which only a dateStyle/timeStyle pair can carry.
    pub second_zone: Option<String>,
}

impl Fields {
    fn empty() -> Self {
        Self {
            locale: "en-US".to_string(),
            month: None,
            day: None,
            year: None,
            hour: None,
            minute: None,
            second: None,
            weekday: None,
            hour12: None,
            time_zone_name: None,
            date_style: None,
            time_style: None,
            second_zone: None,
        }
    }
}

// The compiled IANA table: the normalizer singleton parks its data here, and
// available_identifiers reads the normalized spellings straight out of it. Going through the
// singleton keeps the table in the macro's own allocator, which this crate does not re-export.
timezone_provider::iana_normalizer_singleton!(IANA);

/// Every identifier the compiled normalizer carries, built once.
fn available_identifiers() -> Vec<String> {
    static IDS: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    IDS.get_or_init(|| {
        let table = &IANA.normalized_identifiers;
        (0..table.len())
            .filter_map(|index| table.get(index).map(|name| name.to_string()))
            .collect()
    })
    .clone()
}

/// Every IANA name the compiled tzdb knows: what Intl.supportedValuesOf('timeZone') answers.
fn time_zone_names() -> &'static Vec<String> {
    static NAMES: std::sync::OnceLock<Vec<String>> = std::sync::OnceLock::new();
    NAMES.get_or_init(|| {
        let provider = CompiledTzdbProvider::default();
        let mut names: Vec<String> = available_identifiers()
            .into_iter()
            .filter_map(|name| provider.get(name.as_bytes()).ok())
            .filter_map(|id| provider.identifier(id).ok())
            .map(|name| name.to_string())
            .collect();
        names.sort();
        names.dedup();
        names
    })
}

/// Resolve a time zone option to its canonical identifier, the way the constructor does; None is
/// an unknown zone, which the prelude reports as a RangeError.
fn resolve_time_zone(id: &str) -> Option<String> {
    if id.eq_ignore_ascii_case("utc") {
        return Some("UTC".to_string());
    }
    let provider = CompiledTzdbProvider::default();
    let name = provider.get(id.as_bytes()).ok()?;
    provider.identifier(name).ok().map(|text| text.to_string())
}

fn range_error(scope: &mut Scope<'_>, message: &str) -> RuntimeError {
    use quickjs_oxide::engine::api::core::NativeErrorKind;
    match scope.new_native_error(NativeErrorKind::Range, message) {
        Ok(value) => RuntimeError::Exception(value),
        Err(error) => error,
    }
}

fn text_value(source: &str) -> Value {
    match JsString::try_from_utf8(source) {
        Ok(text) => Value::String(text),
        Err(_) => Value::Null,
    }
}

/// One argument as a string, when it is one.
fn as_field(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.to_utf8_lossy()),
        _ => None,
    }
}

fn read_field(
    scope: &mut Scope<'_>,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<Value>, RuntimeError> {
    let key = PropertyKey::from_js_string(JsString::try_from_utf8(name).expect("field name"));
    match scope.get_property(object, &key)? {
        Value::Undefined | Value::Null => Ok(None),
        value => Ok(Some(value)),
    }
}

fn read_text(
    scope: &mut Scope<'_>,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<String>, RuntimeError> {
    Ok(read_field(scope, object, name)?.as_ref().and_then(as_field))
}

fn read_flag(
    scope: &mut Scope<'_>,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<bool>, RuntimeError> {
    Ok(match read_field(scope, object, name)? {
        Some(Value::Bool(flag)) => Some(flag),
        _ => None,
    })
}

fn read_fields(scope: &mut Scope<'_>, object: &ObjectRef) -> Result<Fields, RuntimeError> {
    Ok(Fields {
        locale: read_text(scope, object, "locale")?.unwrap_or_else(|| "en-US".to_string()),
        month: read_text(scope, object, "month")?,
        day: read_text(scope, object, "day")?,
        year: read_text(scope, object, "year")?,
        hour: read_text(scope, object, "hour")?,
        minute: read_text(scope, object, "minute")?,
        second: read_text(scope, object, "second")?,
        weekday: read_text(scope, object, "weekday")?,
        hour12: read_flag(scope, object, "hour12")?,
        time_zone_name: read_text(scope, object, "timeZoneName")?,
        date_style: read_text(scope, object, "dateStyle")?,
        time_style: read_text(scope, object, "timeStyle")?,
        second_zone: read_text(scope, object, "secondZone")?,
    })
}

fn quote(text: &str) -> String {
    crate::modules::quote(text)
}

/// One formatted value as JSON: the whole text, and the parts it was made of.
fn as_json(zoned: &ZonedDateTime, fields: &Fields, locale_tag: &str) -> String {
    let (rendered, parts) = cldr_data::render_parts(zoned, fields, locale_tag);
    let mut json = String::with_capacity(rendered.len() + 96);
    json.push_str("{\"text\":");
    json.push_str(&quote(&rendered));
    json.push_str(",\"parts\":[");
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            json.push(',');
        }
        json.push_str("{\"type\":");
        json.push_str(&quote(part.kind));
        json.push_str(",\"value\":");
        json.push_str(&quote(&part.value));
        json.push('}');
    }
    json.push_str("]}");
    json
}

/// Resolve the zone and the field list once, from the options object the JS side built.
fn resolve(scope: &mut Scope<'_>, options: &Value) -> Result<(String, Fields), RuntimeError> {
    let Value::Object(object) = options else {
        return Ok(("UTC".to_string(), Fields::empty()));
    };
    let zone = read_text(scope, object, "timeZone")?.unwrap_or_else(|| "UTC".to_string());
    let fields = read_fields(scope, object)?;
    Ok((zone, fields))
}

/// The zone as a canonical identifier, or the RangeError the constructor would have thrown.
fn checked_zone(scope: &mut Scope<'_>, zone: &str) -> Result<String, RuntimeError> {
    match resolve_time_zone(zone) {
        Some(name) => Ok(name),
        None => Err(range_error(scope, &format!("invalid time zone: {zone}"))),
    }
}

fn at(
    scope: &mut Scope<'_>,
    zone: &str,
    epoch_milliseconds: f64,
) -> Result<ZonedDateTime, RuntimeError> {
    let name = checked_zone(scope, zone)?;
    let time_zone = TimeZone::try_from_str(&name)
        .map_err(|_| range_error(scope, &format!("invalid time zone: {zone}")))?;
    // A Date outside the range of a representable instant formats as "Invalid Date"; a raw number
    // outside it is still a valid instant for this runtime, so it is allowed through.
    let instant = Instant::from_epoch_milliseconds(epoch_milliseconds as i64)
        .map_err(|error| range_error(scope, &format!("{error}")))?;
    instant
        .to_zoned_date_time_iso(time_zone)
        .map_err(|error| range_error(scope, &format!("{error}")))
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;

    // __intl_format(timeZone, epochMilliseconds, optionsObject)
    {
        let closure = |scope: &mut Scope<'_>,
                       _zone: String,
                       epoch_milliseconds: f64,
                       options: Value|
         -> Result<Value, RuntimeError> {
            let (zone, fields) = resolve(scope, &options)?;
            let zoned = at(scope, &zone, epoch_milliseconds)?;
            let locale_tag = fields.locale.clone();
            Ok(text_value(&as_json(&zoned, &fields, &locale_tag)))
        };
        driver
            .context()
            .define_function(&global, "__intl_format", 3, closure)
            .map_err(|error| format!("__intl_format: {error}"))?;
    }

    // __intl_zone_names(): the IANA identifiers, as one JSON array.
    {
        let closure = |_: &mut Scope<'_>| -> Result<Value, RuntimeError> {
            let mut json = String::from("[");
            for (index, name) in time_zone_names().iter().enumerate() {
                if index > 0 {
                    json.push(',');
                }
                json.push_str(&quote(name));
            }
            json.push(']');
            Ok(text_value(&json))
        };
        driver
            .context()
            .define_function(&global, "__intl_zone_names", 0, closure)
            .map_err(|error| format!("__intl_zone_names: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("intl prelude: {error}"))?;
    Ok(())
}

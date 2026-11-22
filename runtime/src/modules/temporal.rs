//! Temporal: the date and time API of the language itself, backed by temporal_rs.
//!
//! Temporal reached stage 4 and ships in Firefox, Chrome and Node, so this is a global, not a
//! module: a script asks for Temporal and gets it, the same way it asks for Math.
//!
//! All arithmetic, parsing, calendar and zone work happens in temporal_rs. What is here is the
//! seam: a Temporal value is a host object holding one temporal_rs value, the JavaScript classes
//! in js/temporal.js own the prototypes and the argument shapes, and the three host functions
//! __temporal_get, __temporal_op and __temporal_from carry every call across.
//!
//! Two deliberate gaps, both documented in docs/temporal.md: toLocaleString falls back to
//! toString because this runtime ships no ICU data, and the Calendar/TimeZone protocol objects
//! only carry their identifier and the zone arithmetic that needs no calendar.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

use quickjs_oxide::engine::api::core::{
    Js, JsString, NativeErrorKind, ObjectRef, PropertyKey, RuntimeError, Scope, Value,
};
use temporal_rs::options::{
    DifferenceSettings, Disambiguation, OffsetDisambiguation, Overflow, RoundingIncrement,
    DisplayCalendar, DisplayOffset, DisplayTimeZone, RoundingMode, RoundingOptions, ToStringRoundingOptions, Unit,
};
use temporal_rs::partial::PartialDuration;
use temporal_rs::{
    Calendar, Duration, Instant, PlainDate, PlainDateTime, PlainMonthDay, PlainTime, PlainYearMonth,
    TimeZone, ZonedDateTime,
};

use crate::driver::Driver;

const PRELUDE: &str = include_str!("../../js/temporal.js");

/// One Temporal value. The JavaScript class decides which of these it accepts; this side only
/// needs to know what to hand back to temporal_rs.
#[derive(Clone)]
enum Slot {
    Instant(Instant),
    PlainDate(PlainDate),
    PlainTime(PlainTime),
    PlainDateTime(PlainDateTime),
    PlainYearMonth(PlainYearMonth),
    PlainMonthDay(PlainMonthDay),
    ZonedDateTime(ZonedDateTime),
    Duration(Duration),
}

impl Slot {
    fn kind(&self) -> &'static str {
        match self {
            Slot::Instant(_) => "instant",
            Slot::PlainDate(_) => "plain_date",
            Slot::PlainTime(_) => "plain_time",
            Slot::PlainDateTime(_) => "plain_date_time",
            Slot::PlainYearMonth(_) => "plain_year_month",
            Slot::PlainMonthDay(_) => "plain_month_day",
            Slot::ZonedDateTime(_) => "zoned_date_time",
            Slot::Duration(_) => "duration",
        }
    }
}

/// The prototype of each class, registered by the prelude once, so a value made here lands on the
/// right one without the JavaScript side passing it into every call.
type Prototypes = Rc<RefCell<HashMap<&'static str, ObjectRef>>>;

fn fail(scope: &mut Scope<'_>, kind: NativeErrorKind, message: &str) -> RuntimeError {
    match scope.new_native_error(kind, message) {
        Ok(value) => RuntimeError::Exception(value),
        Err(error) => error,
    }
}

fn range(scope: &mut Scope<'_>, message: &str) -> RuntimeError {
    fail(scope, NativeErrorKind::Range, message)
}

fn type_error(scope: &mut Scope<'_>, message: &str) -> RuntimeError {
    fail(scope, NativeErrorKind::Type, message)
}

/// A temporal_rs error is a RangeError in the specification, except for the ones about a value of
/// the wrong shape, which the engine reaches before it gets here.
fn temporal(scope: &mut Scope<'_>, error: temporal_rs::TemporalError) -> RuntimeError {
    range(scope, &format!("{error}"))
}

fn text(value: impl Into<String>) -> Value {
    let text = value.into();
    match JsString::try_from_utf8(&text) {
        Ok(text) => Value::String(text),
        Err(_) => Value::Null,
    }
}

fn number(value: f64) -> Value {
    if value.fract() == 0.0 && value.abs() <= 2_147_483_647.0 {
        Value::Int(value as i32)
    } else {
        Value::Float(value)
    }
}

fn big(value: i128) -> Value {
    // The engine's bigint type is reachable only through its own constructor, so the number
    // travels as decimal text and the prelude turns it into a BigInt.
    Value::String({
        let digits = value.to_string();
        JsString::try_from_utf8(&digits).expect("decimal digits")
    })
}

fn as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Int(number) => Some(*number as f64),
        Value::Float(number) => Some(*number),
        _ => None,
    }
}

fn as_text(value: &Value) -> Option<String> {
    match value {
        Value::String(text) => Some(text.to_utf8_lossy()),
        _ => None,
    }
}

fn key(name: &str) -> PropertyKey {
    PropertyKey::from_js_string(JsString::try_from_utf8(name).expect("property name"))
}

/// One field of an argument object; absent and null both mean "not given", which is what every
/// Temporal option defaults to.
fn field(
    scope: &mut Scope<'_>,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<Value>, RuntimeError> {
    match scope.get_property(object, &key(name))? {
        Value::Undefined | Value::Null => Ok(None),
        value => Ok(Some(value)),
    }
}

fn field_text(
    scope: &mut Scope<'_>,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<String>, RuntimeError> {
    Ok(field(scope, object, name)?.and_then(|value| as_text(&value)))
}

fn field_number(
    scope: &mut Scope<'_>,
    object: &ObjectRef,
    name: &str,
) -> Result<Option<f64>, RuntimeError> {
    Ok(field(scope, object, name)?.and_then(|value| as_f64(&value)))
}

fn object_of(value: &Value) -> Option<ObjectRef> {
    match value {
        Value::Object(object) => Some(object.clone()),
        _ => None,
    }
}

fn as_calendar(scope: &mut Scope<'_>, id: &str) -> Result<Calendar, RuntimeError> {
    Calendar::from_str(&id.to_ascii_lowercase())
        .map_err(|_| range(scope, &format!("unknown calendar: {id}")))
}

fn as_time_zone(scope: &mut Scope<'_>, id: &str) -> Result<TimeZone, RuntimeError> {
    TimeZone::try_from_str(id).map_err(|_| range(scope, &format!("unknown time zone: {id}")))
}

fn overflow(scope: &mut Scope<'_>, object: Option<&ObjectRef>) -> Result<Option<Overflow>, RuntimeError> {
    let Some(object) = object else {
        return Ok(None);
    };
    match field_text(scope, object, "overflow")? {
        Some(name) => Overflow::from_str(&name)
            .map(Some)
            .map_err(|_| range(scope, &format!("unknown overflow: {name}"))),
        None => Ok(None),
    }
}

fn disambiguation(
    scope: &mut Scope<'_>,
    object: Option<&ObjectRef>,
) -> Result<Disambiguation, RuntimeError> {
    let Some(object) = object else {
        return Ok(Disambiguation::Compatible);
    };
    match field_text(scope, object, "disambiguation")? {
        Some(name) => Disambiguation::from_str(&name)
            .map_err(|_| range(scope, &format!("unknown disambiguation: {name}"))),
        None => Ok(Disambiguation::Compatible),
    }
}

fn offset_option(
    scope: &mut Scope<'_>,
    object: Option<&ObjectRef>,
) -> Result<OffsetDisambiguation, RuntimeError> {
    let Some(object) = object else {
        return Ok(OffsetDisambiguation::Reject);
    };
    match field_text(scope, object, "offset")? {
        Some(name) => OffsetDisambiguation::from_str(&name)
            .map_err(|_| range(scope, &format!("unknown offset option: {name}"))),
        None => Ok(OffsetDisambiguation::Reject),
    }
}

fn unit_of(scope: &mut Scope<'_>, name: &str) -> Result<Unit, RuntimeError> {
    Unit::from_str(name).map_err(|_| range(scope, &format!("unknown unit: {name}")))
}

fn rounding_mode(
    scope: &mut Scope<'_>,
    object: Option<&ObjectRef>,
) -> Result<Option<RoundingMode>, RuntimeError> {
    let Some(object) = object else {
        return Ok(None);
    };
    match field_text(scope, object, "roundingMode")? {
        Some(name) => RoundingMode::from_str(&name)
            .map(Some)
            .map_err(|_| range(scope, &format!("unknown rounding mode: {name}"))),
        None => Ok(None),
    }
}

fn increment(
    scope: &mut Scope<'_>,
    object: Option<&ObjectRef>,
) -> Result<Option<RoundingIncrement>, RuntimeError> {
    let Some(object) = object else {
        return Ok(None);
    };
    let Some(value) = field_number(scope, object, "roundingIncrement")? else {
        return Ok(None);
    };
    RoundingIncrement::try_new(value as u32)
        .map(Some)
        .map_err(|error| temporal(scope, error))
}

fn difference_settings(
    scope: &mut Scope<'_>,
    object: Option<&ObjectRef>,
) -> Result<DifferenceSettings, RuntimeError> {
    let mut settings = DifferenceSettings::default();
    if let Some(object) = object {
        if let Some(name) = field_text(scope, object, "largestUnit")? {
            settings.largest_unit = Some(unit_of(scope, &name)?);
        }
        if let Some(name) = field_text(scope, object, "smallestUnit")? {
            settings.smallest_unit = Some(unit_of(scope, &name)?);
        }
        settings.rounding_mode = rounding_mode(scope, Some(object))?;
        settings.increment = increment(scope, Some(object))?;
    }
    Ok(settings)
}

fn rounding_options(
    scope: &mut Scope<'_>,
    object: Option<&ObjectRef>,
) -> Result<RoundingOptions, RuntimeError> {
    let mut options = RoundingOptions::default();
    if let Some(object) = object {
        if let Some(name) = field_text(scope, object, "largestUnit")? {
            options.largest_unit = Some(unit_of(scope, &name)?);
        }
        if let Some(name) = field_text(scope, object, "smallestUnit")? {
            options.smallest_unit = Some(unit_of(scope, &name)?);
        }
        options.rounding_mode = rounding_mode(scope, Some(object))?;
        options.increment = increment(scope, Some(object))?;
    }
    Ok(options)
}

fn to_string_options(
    scope: &mut Scope<'_>,
    object: Option<&ObjectRef>,
) -> Result<ToStringRoundingOptions, RuntimeError> {
    let mut options = ToStringRoundingOptions::default();
    if let Some(object) = object {
        if let Some(digits) = field_number(scope, object, "fractionalSecondDigits")? {
            if !(0.0..=9.0).contains(&digits) {
                return Err(range(scope, "fractionalSecondDigits must be between 0 and 9"));
            }
            options.precision = temporal_rs::parsers::Precision::Digit(digits as u8);
        }
        if let Some(name) = field_text(scope, object, "smallestUnit")? {
            options.smallest_unit = Some(unit_of(scope, &name)?);
        }
        options.rounding_mode = rounding_mode(scope, Some(object))?;
    }
    Ok(options)
}

/// A Duration argument: a Temporal.Duration, a string, or an object of fields.
fn duration_of(
    scope: &mut Scope<'_>,
    value: &Value,
) -> Result<Duration, RuntimeError> {
    if let Some(slot) = host(scope, value)? {
        if let Slot::Duration(duration) = &*slot {
            return Ok(duration.clone());
        }
        return Err(type_error(scope, "expected a Temporal.Duration"));
    }
    if let Some(text) = as_text(value) {
        return Duration::from_utf8(text.as_bytes()).map_err(|error| temporal(scope, error));
    }
    let Some(object) = object_of(value) else {
        return Err(type_error(scope, "expected a Temporal.Duration"));
    };
    let mut partial = PartialDuration::default();
    partial.years = field_number(scope, &object, "years")?.map(|value| value as i64);
    partial.months = field_number(scope, &object, "months")?.map(|value| value as i64);
    partial.weeks = field_number(scope, &object, "weeks")?.map(|value| value as i64);
    partial.days = field_number(scope, &object, "days")?.map(|value| value as i64);
    partial.hours = field_number(scope, &object, "hours")?.map(|value| value as i64);
    partial.minutes = field_number(scope, &object, "minutes")?.map(|value| value as i64);
    partial.seconds = field_number(scope, &object, "seconds")?.map(|value| value as i64);
    partial.milliseconds = field_number(scope, &object, "milliseconds")?.map(|value| value as i64);
    partial.microseconds = field_number(scope, &object, "microseconds")?.map(|value| value as i128);
    partial.nanoseconds = field_number(scope, &object, "nanoseconds")?.map(|value| value as i128);
    Duration::from_partial_duration(partial).map_err(|error| temporal(scope, error))
}

fn host(scope: &mut Scope<'_>, value: &Value) -> Result<Option<Rc<Slot>>, RuntimeError> {
    let Some(object) = object_of(value) else {
        return Ok(None);
    };
    scope.host_data::<Slot>(&object)
}

fn wrap(
    scope: &mut Scope<'_>,
    prototypes: &Prototypes,
    slot: Slot,
) -> Result<Value, RuntimeError> {
    let prototype = prototypes.borrow().get(slot.kind()).cloned();
    let Some(prototype) = prototype else {
        return Err(fail(
            scope,
            NativeErrorKind::Internal,
            "Temporal class was not registered",
        ));
    };
    let object = scope.new_host_object(slot, Some(&prototype))?;
    Ok(Value::Object(object))
}

fn want(
    scope: &mut Scope<'_>,
    value: &Value,
    name: &str,
) -> Result<Rc<Slot>, RuntimeError> {
    match host(scope, value)? {
        Some(slot) => Ok(slot),
        None => Err(type_error(scope, &format!("expected a Temporal.{name}"))),
    }
}

fn calendar_of(scope: &mut Scope<'_>, object: &ObjectRef) -> Result<Calendar, RuntimeError> {
    match field_text(scope, object, "calendar")? {
        Some(id) => as_calendar(scope, &id),
        None => Ok(Calendar::default()),
    }
}

fn whole(
    scope: &mut Scope<'_>,
    object: &ObjectRef,
    name: &str,
    default: i64,
) -> Result<i64, RuntimeError> {
    Ok(field_number(scope, object, name)?.map(|value| value as i64).unwrap_or(default))
}

fn now_instant() -> Result<Instant, ()> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).map_err(|_| ())?;
    Ok(Instant::from_epoch_milliseconds(elapsed.as_millis() as i64).map_err(|_| ())?)
}

pub fn install(driver: &mut Driver) -> Result<(), String> {
    let global = driver
        .context()
        .global_object()
        .map_err(|error| format!("global: {error}"))?;
    let prototypes: Prototypes = Rc::new(RefCell::new(HashMap::new()));

    {
        let prototypes = Rc::clone(&prototypes);
        driver
            .context()
            .define_function(
                &global,
                "__temporal_register",
                2,
                move |scope: &mut Scope<'_>, kind: String, prototype: Value| -> Result<Value, RuntimeError> {
                    let Some(object) = object_of(&prototype) else {
                        return Err(type_error(scope, "a prototype must be an object"));
                    };
                    let name: &'static str = match kind.as_str() {
                        "instant" => "instant",
                        "plain_date" => "plain_date",
                        "plain_time" => "plain_time",
                        "plain_date_time" => "plain_date_time",
                        "plain_year_month" => "plain_year_month",
                        "plain_month_day" => "plain_month_day",
                        "zoned_date_time" => "zoned_date_time",
                        "duration" => "duration",
                        other => return Err(type_error(scope, &format!("unknown Temporal kind: {other}"))),
                    };
                    prototypes.borrow_mut().insert(name, object);
                    Ok(Value::Undefined)
                },
            )
            .map_err(|error| format!("__temporal_register: {error}"))?;
    }

    {
        let prototypes = Rc::clone(&prototypes);
        driver
            .context()
            .define_function(
                &global,
                "__temporal_make",
                2,
                move |scope: &mut Scope<'_>, kind: String, fields: Value| -> Result<Value, RuntimeError> {
                    let slot = build(scope, &kind, &fields)?;
                    wrap(scope, &prototypes, slot)
                },
            )
            .map_err(|error| format!("__temporal_make: {error}"))?;
    }

    {
        let prototypes = Rc::clone(&prototypes);
        driver
            .context()
            .define_function(
                &global,
                "__temporal_from",
                3,
                move |scope: &mut Scope<'_>,
                      kind: String,
                      source: Value,
                      options: Value|
                      -> Result<Value, RuntimeError> {
                    let slot = parse(scope, &kind, &source, object_of(&options).as_ref())?;
                    wrap(scope, &prototypes, slot)
                },
            )
            .map_err(|error| format!("__temporal_from: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__temporal_kind",
                1,
                |scope: &mut Scope<'_>, value: Value| -> Result<Value, RuntimeError> {
                    Ok(match host(scope, &value)? {
                        Some(slot) => text(slot.kind()),
                        None => Value::Undefined,
                    })
                },
            )
            .map_err(|error| format!("__temporal_kind: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__temporal_get",
                2,
                |scope: &mut Scope<'_>, value: Value, name: String| -> Result<Value, RuntimeError> {
                    let slot = want(scope, &value, "value")?;
                    read(scope, &slot, &name)
                },
            )
            .map_err(|error| format!("__temporal_get: {error}"))?;
    }

    {
        let prototypes = Rc::clone(&prototypes);
        driver
            .context()
            .define_function(
                &global,
                "__temporal_op",
                3,
                move |scope: &mut Scope<'_>,
                      value: Value,
                      name: String,
                      args: Value|
                      -> Result<Value, RuntimeError> {
                    let slot = want(scope, &value, "value")?;
                    let args = object_of(&args);
                    run(scope, &prototypes, &slot, &name, args.as_ref())
                },
            )
            .map_err(|error| format!("__temporal_op: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__temporal_compare",
                4,
                |scope: &mut Scope<'_>,
                 kind: String,
                 one: Value,
                 two: Value,
                 options: Value|
                 -> Result<Value, RuntimeError> {
                    let options = object_of(&options);
                    compare(scope, &kind, &one, &two, options.as_ref())
                },
            )
            .map_err(|error| format!("__temporal_compare: {error}"))?;
    }

    {
        let prototypes = Rc::clone(&prototypes);
        driver
            .context()
            .define_function(
                &global,
                "__temporal_now",
                2,
                move |scope: &mut Scope<'_>, kind: String, time_zone: Value| -> Result<Value, RuntimeError> {
                    let zone = match as_text(&time_zone) {
                        Some(id) => as_time_zone(scope, &id)?,
                        None => TimeZone::utc(),
                    };
                    let instant = match now_instant() {
                        Ok(instant) => instant,
                        Err(()) => return Err(range(scope, "the system clock is before the epoch")),
                    };
                    let slot = match kind.as_str() {
                        "instant" => Slot::Instant(instant),
                        "time_zone" => {
                            return Ok(text(
                                zone.identifier().map_err(|error| temporal(scope, error))?,
                            ))
                        }
                        "zoned_date_time" => Slot::ZonedDateTime(
                            instant
                                .to_zoned_date_time_iso(zone)
                                .map_err(|error| temporal(scope, error))?,
                        ),
                        "plain_date_time" => {
                            let zoned = instant
                                .to_zoned_date_time_iso(zone)
                                .map_err(|error| temporal(scope, error))?;
                            Slot::PlainDateTime(zoned.to_plain_date_time())
                        }
                        "plain_date" => {
                            let zoned = instant
                                .to_zoned_date_time_iso(zone)
                                .map_err(|error| temporal(scope, error))?;
                            Slot::PlainDate(zoned.to_plain_date())
                        }
                        "plain_time" => {
                            let zoned = instant
                                .to_zoned_date_time_iso(zone)
                                .map_err(|error| temporal(scope, error))?;
                            Slot::PlainTime(zoned.to_plain_time())
                        }
                        other => {
                            return Err(type_error(scope, &format!("unknown Temporal.Now kind: {other}")))
                        }
                    };
                    wrap(scope, &prototypes, slot)
                },
            )
            .map_err(|error| format!("__temporal_now: {error}"))?;
    }

    {
        driver
            .context()
            .define_function(
                &global,
                "__temporal_zone",
                3,
                |scope: &mut Scope<'_>,
                 id: String,
                 name: String,
                 argument: Value|
                 -> Result<Value, RuntimeError> {
                    let zone = as_time_zone(scope, &id)?;
                    // The identifier needs no instant: it is the answer for a zone on its own.
                    if name == "id" {
                        return Ok(text(
                            zone.identifier().map_err(|error| temporal(scope, error))?,
                        ));
                    }
                    let instant = match as_text(&argument) {
                        // The prelude hands over epoch nanoseconds as decimal text.
                        Some(digits) => digits
                            .parse::<i128>()
                            .map_err(|_| range(scope, "epochNanoseconds must be an integer"))?,
                        None => match host(scope, &argument)? {
                            Some(slot) => match &*slot {
                                Slot::Instant(instant) => instant.epoch_nanoseconds().as_i128(),
                                _ => return Err(type_error(scope, "expected a Temporal.Instant")),
                            },
                            None => return Err(type_error(scope, "expected a Temporal.Instant")),
                        },
                    };
                    zone_read(scope, &zone, &name, instant)
                },
            )
            .map_err(|error| format!("__temporal_zone: {error}"))?;
    }

    driver
        .eval(PRELUDE)
        .map_err(|error| format!("temporal prelude: {error}"))?;

    let module = driver
        .context()
        .eval("globalThis.__temporal")
        .map_err(|error| format!("__temporal: {error}"))?;
    let exports = super::collect(driver, &module, &["Temporal", "default"])?;
    super::define_with_alias(driver, "temporal", exports)
}

// ==== Building and parsing ====

fn arg(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
    name: &str,
) -> Result<Option<Value>, RuntimeError> {
    match args {
        Some(object) => field(scope, object, name),
        None => Ok(None),
    }
}

fn arg_text(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
    name: &str,
) -> Result<Option<String>, RuntimeError> {
    Ok(arg(scope, args, name)?.and_then(|value| as_text(&value)))
}

fn arg_number(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
    name: &str,
) -> Result<Option<f64>, RuntimeError> {
    Ok(arg(scope, args, name)?.and_then(|value| as_f64(&value)))
}

fn arg_duration(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
) -> Result<Duration, RuntimeError> {
    let Some(value) = arg(scope, args, "duration")? else {
        return Err(type_error(scope, "a duration is required"));
    };
    duration_of(scope, &value)
}

fn arg_time_zone(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
) -> Result<TimeZone, RuntimeError> {
    let Some(id) = arg_text(scope, args, "timeZone")? else {
        return Err(type_error(scope, "a time zone is required"));
    };
    as_time_zone(scope, &id)
}

fn arg_plain_time(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
    name: &str,
) -> Result<PlainTime, RuntimeError> {
    let Some(value) = arg(scope, args, name)? else {
        return Ok(PlainTime::default());
    };
    match host(scope, &value)? {
        Some(slot) => match &*slot {
            Slot::PlainTime(time) => Ok(time.clone()),
            _ => Err(type_error(scope, "expected a Temporal.PlainTime")),
        },
        None => match as_text(&value) {
            Some(text) => PlainTime::from_utf8(text.as_bytes()).map_err(|error| temporal(scope, error)),
            None => Err(type_error(scope, "expected a Temporal.PlainTime")),
        },
    }
}

fn arg_plain_date(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
    name: &str,
) -> Result<PlainDate, RuntimeError> {
    let Some(value) = arg(scope, args, name)? else {
        return Err(type_error(scope, "a date is required"));
    };
    match host(scope, &value)? {
        Some(slot) => match &*slot {
            Slot::PlainDate(date) => Ok(date.clone()),
            _ => Err(type_error(scope, "expected a Temporal.PlainDate")),
        },
        None => match as_text(&value) {
            Some(text) => PlainDate::from_utf8(text.as_bytes()).map_err(|error| temporal(scope, error)),
            None => Err(type_error(scope, "expected a Temporal.PlainDate")),
        },
    }
}

/// An ISO date and time as the parser wants it: the zone in brackets, the offset in front of it
/// when the caller gave one.
fn iso_text(fields: &Fields) -> String {
    let mut text = format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}",
        fields.year, fields.month, fields.day, fields.hour, fields.minute, fields.second
    );
    if fields.millisecond != 0 || fields.microsecond != 0 || fields.nanosecond != 0 {
        let mut fraction = format!("{:03}", fields.millisecond);
        if fields.microsecond != 0 || fields.nanosecond != 0 {
            fraction.push_str(&format!("{:03}", fields.microsecond));
        }
        if fields.nanosecond != 0 {
            fraction.push_str(&format!("{:03}", fields.nanosecond));
        }
        text.push('.');
        text.push_str(&fraction);
    }
    if let Some(offset) = &fields.offset {
        text.push_str(offset);
    }
    if let Some(zone) = &fields.time_zone {
        text.push('[');
        text.push_str(zone);
        text.push(']');
    }
    text
}

/// The numbers every constructor here reads, already defaulted, so each branch below only has to
/// name the ones it cares about.
#[derive(Default)]
struct Fields {
    year: i32,
    month: u8,
    day: u8,
    hour: u8,
    minute: u8,
    second: u8,
    millisecond: u16,
    microsecond: u16,
    nanosecond: u16,
    calendar: Calendar,
    time_zone: Option<String>,
    offset: Option<String>,
}

fn fields_of(scope: &mut Scope<'_>, object: &ObjectRef) -> Result<Fields, RuntimeError> {
    let mut fields = Fields::default();
    fields.year = whole(scope, object, "year", 0)? as i32;
    fields.month = whole(scope, object, "month", 1)? as u8;
    fields.day = whole(scope, object, "day", 1)? as u8;
    fields.hour = whole(scope, object, "hour", 0)? as u8;
    fields.minute = whole(scope, object, "minute", 0)? as u8;
    fields.second = whole(scope, object, "second", 0)? as u8;
    fields.millisecond = whole(scope, object, "millisecond", 0)? as u16;
    fields.microsecond = whole(scope, object, "microsecond", 0)? as u16;
    fields.nanosecond = whole(scope, object, "nanosecond", 0)? as u16;
    fields.calendar = calendar_of(scope, object)?;
    fields.time_zone = field_text(scope, object, "timeZone")?;
    fields.offset = field_text(scope, object, "offset")?;
    Ok(fields)
}

fn build(scope: &mut Scope<'_>, kind: &str, value: &Value) -> Result<Slot, RuntimeError> {
    let Some(object) = object_of(value) else {
        return Err(type_error(scope, "expected an object of fields"));
    };
    let fields = fields_of(scope, &object)?;
    Ok(match kind {
        "instant" => match field_number(scope, &object, "epochNanoseconds")? {
            Some(_) => {
                let digits = field_text(scope, &object, "epochNanosecondsText")?
                    .and_then(|text| text.parse::<i128>().ok())
                    .ok_or_else(|| range(scope, "epochNanoseconds must be an integer"))?;
                Slot::Instant(
                    Instant::try_new(digits).map_err(|error| temporal(scope, error))?,
                )
            }
            None => {
                let milliseconds = field_number(scope, &object, "epochMilliseconds")?
                    .ok_or_else(|| type_error(scope, "epochMilliseconds is required"))?;
                Slot::Instant(
                    Instant::from_epoch_milliseconds(milliseconds as i64)
                        .map_err(|error| temporal(scope, error))?,
                )
            }
        },
        "plain_date" => Slot::PlainDate(
            PlainDate::new_with_overflow(
                fields.year,
                fields.month,
                fields.day,
                fields.calendar,
                Overflow::Constrain,
            )
            .map_err(|error| temporal(scope, error))?,
        ),
        "plain_time" => Slot::PlainTime(
            PlainTime::new(
                fields.hour,
                fields.minute,
                fields.second,
                fields.millisecond,
                fields.microsecond,
                fields.nanosecond,
            )
            .map_err(|error| temporal(scope, error))?,
        ),
        "plain_date_time" => Slot::PlainDateTime(
            PlainDateTime::new(
                fields.year,
                fields.month,
                fields.day,
                fields.hour,
                fields.minute,
                fields.second,
                fields.millisecond,
                fields.microsecond,
                fields.nanosecond,
                fields.calendar,
            )
            .map_err(|error| temporal(scope, error))?,
        ),
        "plain_year_month" => Slot::PlainYearMonth(
            PlainYearMonth::new(
                fields.year,
                fields.month,
                field_number(scope, &object, "referenceDay")?.map(|value| value as u8),
                fields.calendar,
            )
            .map_err(|error| temporal(scope, error))?,
        ),
        "plain_month_day" => Slot::PlainMonthDay(
            PlainMonthDay::new_with_overflow(
                fields.month,
                fields.day,
                fields.calendar,
                Overflow::Constrain,
                field_number(scope, &object, "referenceYear")?.map(|value| value as i32),
            )
            .map_err(|error| temporal(scope, error))?,
        ),
        "duration" => {
            let mut partial = PartialDuration::default();
            partial.years = field_number(scope, &object, "years")?.map(|value| value as i64);
            partial.months = field_number(scope, &object, "months")?.map(|value| value as i64);
            partial.weeks = field_number(scope, &object, "weeks")?.map(|value| value as i64);
            partial.days = field_number(scope, &object, "days")?.map(|value| value as i64);
            partial.hours = field_number(scope, &object, "hours")?.map(|value| value as i64);
            partial.minutes = field_number(scope, &object, "minutes")?.map(|value| value as i64);
            partial.seconds = field_number(scope, &object, "seconds")?.map(|value| value as i64);
            partial.milliseconds =
                field_number(scope, &object, "milliseconds")?.map(|value| value as i64);
            partial.microseconds =
                field_number(scope, &object, "microseconds")?.map(|value| value as i128);
            partial.nanoseconds =
                field_number(scope, &object, "nanoseconds")?.map(|value| value as i128);
            Slot::Duration(
                Duration::from_partial_duration(partial)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        "zoned_date_time" => {
            let zone = fields
                .time_zone
                .clone()
                .ok_or_else(|| type_error(scope, "a time zone is required"))?;
            let _ = as_time_zone(scope, &zone)?;
            Slot::ZonedDateTime(
                ZonedDateTime::from_utf8(
                    iso_text(&fields).as_bytes(),
                    Disambiguation::Compatible,
                    OffsetDisambiguation::Reject,
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        other => return Err(type_error(scope, &format!("unknown Temporal kind: {other}"))),
    })
}

fn parse(
    scope: &mut Scope<'_>,
    kind: &str,
    source: &Value,
    options: Option<&ObjectRef>,
) -> Result<Slot, RuntimeError> {
    if let Some(slot) = host(scope, source)? {
        if slot.kind() == kind {
            return Ok((*slot).clone());
        }
        // The one conversion between kinds that every from() accepts: an instant into a zone.
        if kind == "zoned_date_time" {
            if let Slot::Instant(instant) = &*slot {
                let zone = match arg_text(scope, options, "timeZone")? {
                    Some(id) => as_time_zone(scope, &id)?,
                    None => TimeZone::utc(),
                };
                return Ok(Slot::ZonedDateTime(
                    instant
                        .to_zoned_date_time_iso(zone)
                        .map_err(|error| temporal(scope, error))?,
                ));
            }
        }
        return Err(type_error(
            scope,
            &format!("expected a Temporal.{}", name_of(kind)),
        ));
    }

    if let Some(text) = as_text(source) {
        if kind == "duration" {
            return Ok(Slot::Duration(
                Duration::from_utf8(text.as_bytes()).map_err(|error| temporal(scope, error))?,
            ));
        }
        if kind == "zoned_date_time" {
            return Ok(Slot::ZonedDateTime(
                ZonedDateTime::from_utf8(
                    text.as_bytes(),
                    disambiguation(scope, options)?,
                    offset_option(scope, options)?,
                )
                .map_err(|error| temporal(scope, error))?,
            ));
        }
        return Ok(match kind {
            "instant" => Slot::Instant(
                Instant::from_utf8(text.as_bytes()).map_err(|error| temporal(scope, error))?,
            ),
            "plain_date" => Slot::PlainDate(
                PlainDate::from_utf8(text.as_bytes()).map_err(|error| temporal(scope, error))?,
            ),
            "plain_time" => Slot::PlainTime(
                PlainTime::from_utf8(text.as_bytes()).map_err(|error| temporal(scope, error))?,
            ),
            "plain_date_time" => Slot::PlainDateTime(
                PlainDateTime::from_utf8(text.as_bytes())
                    .map_err(|error| temporal(scope, error))?,
            ),
            "plain_year_month" => Slot::PlainYearMonth(
                PlainYearMonth::from_utf8(text.as_bytes())
                    .map_err(|error| temporal(scope, error))?,
            ),
            "plain_month_day" => Slot::PlainMonthDay(
                PlainMonthDay::from_utf8(text.as_bytes())
                    .map_err(|error| temporal(scope, error))?,
            ),
            other => return Err(type_error(scope, &format!("unknown Temporal kind: {other}"))),
        });
    }

    if kind == "duration" {
        return Ok(Slot::Duration(duration_of(scope, source)?));
    }
    build(scope, kind, source)
}

fn name_of(kind: &str) -> &str {
    match kind {
        "instant" => "Instant",
        "plain_date" => "PlainDate",
        "plain_time" => "PlainTime",
        "plain_date_time" => "PlainDateTime",
        "plain_year_month" => "PlainYearMonth",
        "plain_month_day" => "PlainMonthDay",
        "zoned_date_time" => "ZonedDateTime",
        "duration" => "Duration",
        _ => "value",
    }
}

fn compare(
    scope: &mut Scope<'_>,
    kind: &str,
    one: &Value,
    two: &Value,
    _options: Option<&ObjectRef>,
) -> Result<Value, RuntimeError> {
    let one = parse(scope, kind, one, None)?;
    let two = parse(scope, kind, two, None)?;
    let ordering = match (&one, &two) {
        (Slot::Instant(one), Slot::Instant(two)) => one
            .as_i128()
            .cmp(&two.as_i128()),
        (Slot::PlainDate(one), Slot::PlainDate(two)) => one.compare_iso(two),
        (Slot::PlainTime(one), Slot::PlainTime(two)) => one
            .to_ixdtf_string(ToStringRoundingOptions::default())
            .map_err(|error| temporal(scope, error))?
            .cmp(&two.to_ixdtf_string(ToStringRoundingOptions::default()).map_err(|error| temporal(scope, error))?),
        (Slot::PlainDateTime(one), Slot::PlainDateTime(two)) => one.compare_iso(two),
        (Slot::PlainYearMonth(one), Slot::PlainYearMonth(two)) => one.compare_iso(two),
        (Slot::ZonedDateTime(one), Slot::ZonedDateTime(two)) => {
            one.compare_instant(two)
        }
        (Slot::Duration(one), Slot::Duration(two)) => one
            .compare(two, None)
            .map_err(|error| temporal(scope, error))?,
        _ => return Err(type_error(scope, "cannot compare these Temporal values")),
    };
    Ok(Value::Int(match ordering {
        std::cmp::Ordering::Less => -1,
        std::cmp::Ordering::Equal => 0,
        std::cmp::Ordering::Greater => 1,
    }))
}

// ==== Reading a field ====

fn unknown(scope: &mut Scope<'_>, kind: &str, name: &str) -> RuntimeError {
    range(
        scope,
        &format!("Temporal.{} has no field {name}", name_of(kind)),
    )
}

fn read(scope: &mut Scope<'_>, slot: &Slot, name: &str) -> Result<Value, RuntimeError> {
    Ok(match slot {
        Slot::Instant(instant) => match name {
            "epochMilliseconds" => number(instant.epoch_milliseconds() as f64),
            "epochNanoseconds" => big(instant.epoch_nanoseconds().as_i128()),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
        Slot::PlainDate(date) => match name {
            "year" => number(date.year() as f64),
            "month" => number(date.month() as f64),
            "monthCode" => text(date.month_code().as_str()),
            "day" => number(date.day() as f64),
            "dayOfWeek" => number(date.day_of_week() as f64),
            "dayOfYear" => number(date.day_of_year() as f64),
            "weekOfYear" => optional_number(date.week_of_year().map(|value| value as f64)),
            "yearOfWeek" => optional_number(date.year_of_week().map(|value| value as f64)),
            "daysInWeek" => number(date.days_in_week() as f64),
            "daysInMonth" => number(date.days_in_month() as f64),
            "daysInYear" => number(date.days_in_year() as f64),
            "monthsInYear" => number(date.months_in_year() as f64),
            "inLeapYear" => Value::Bool(date.in_leap_year()),
            "era" => optional_text(date.era().map(|era| era.to_string())),
            "eraYear" => optional_number(date.era_year().map(|value| value as f64)),
            "calendarId" => text(date.calendar().identifier()),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
        Slot::PlainTime(time) => match name {
            "hour" => number(time.hour() as f64),
            "minute" => number(time.minute() as f64),
            "second" => number(time.second() as f64),
            "millisecond" => number(time.millisecond() as f64),
            "microsecond" => number(time.microsecond() as f64),
            "nanosecond" => number(time.nanosecond() as f64),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
        Slot::PlainDateTime(date_time) => match name {
            "year" => number(date_time.year() as f64),
            "month" => number(date_time.month() as f64),
            "monthCode" => text(date_time.month_code().as_str()),
            "day" => number(date_time.day() as f64),
            "hour" => number(date_time.hour() as f64),
            "minute" => number(date_time.minute() as f64),
            "second" => number(date_time.second() as f64),
            "millisecond" => number(date_time.millisecond() as f64),
            "microsecond" => number(date_time.microsecond() as f64),
            "nanosecond" => number(date_time.nanosecond() as f64),
            "dayOfWeek" => number(date_time.day_of_week() as f64),
            "dayOfYear" => number(date_time.day_of_year() as f64),
            "weekOfYear" => optional_number(date_time.week_of_year().map(|value| value as f64)),
            "yearOfWeek" => optional_number(date_time.year_of_week().map(|value| value as f64)),
            "daysInWeek" => number(date_time.days_in_week() as f64),
            "daysInMonth" => number(date_time.days_in_month() as f64),
            "daysInYear" => number(date_time.days_in_year() as f64),
            "monthsInYear" => number(date_time.months_in_year() as f64),
            "inLeapYear" => Value::Bool(date_time.in_leap_year()),
            "era" => optional_text(date_time.era().map(|era| era.to_string())),
            "eraYear" => optional_number(date_time.era_year().map(|value| value as f64)),
            "calendarId" => text(date_time.calendar().identifier()),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
        Slot::ZonedDateTime(zoned) => match name {
            "year" => number(zoned.year() as f64),
            "month" => number(zoned.month() as f64),
            "monthCode" => text(zoned.month_code().as_str()),
            "day" => number(zoned.day() as f64),
            "hour" => number(zoned.hour() as f64),
            "minute" => number(zoned.minute() as f64),
            "second" => number(zoned.second() as f64),
            "millisecond" => number(zoned.millisecond() as f64),
            "microsecond" => number(zoned.microsecond() as f64),
            "nanosecond" => number(zoned.nanosecond() as f64),
            "dayOfWeek" => number(zoned.day_of_week() as f64),
            "dayOfYear" => number(zoned.day_of_year() as f64),
            "weekOfYear" => optional_number(zoned.week_of_year().map(|value| value as f64)),
            "yearOfWeek" => optional_number(zoned.year_of_week().map(|value| value as f64)),
            "daysInWeek" => number(zoned.days_in_week() as f64),
            "daysInMonth" => number(zoned.days_in_month() as f64),
            "daysInYear" => number(zoned.days_in_year() as f64),
            "monthsInYear" => number(zoned.months_in_year() as f64),
            "inLeapYear" => Value::Bool(zoned.in_leap_year()),
            "era" => optional_text(zoned.era().map(|era| era.to_string())),
            "eraYear" => optional_number(zoned.era_year().map(|value| value as f64)),
            "calendarId" => text(zoned.calendar().identifier()),
            "timeZoneId" => text(
                zoned
                    .time_zone()
                    .identifier()
                    .map_err(|error| temporal(scope, error))?,
            ),
            "offset" => text(zoned.offset().to_string()),
            "offsetNanoseconds" => big(zoned.offset_nanoseconds() as i128),
            "epochMilliseconds" => number(zoned.epoch_milliseconds() as f64),
            "epochNanoseconds" => big(zoned.epoch_nanoseconds().as_i128()),
            "hoursInDay" => number(
                zoned
                    .hours_in_day()
                    .map_err(|error| temporal(scope, error))?,
            ),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
        Slot::PlainYearMonth(year_month) => match name {
            "year" => number(year_month.year() as f64),
            "month" => number(year_month.month() as f64),
            "monthCode" => text(year_month.month_code().as_str()),
            "daysInMonth" => number(year_month.days_in_month() as f64),
            "daysInYear" => number(year_month.days_in_year() as f64),
            "monthsInYear" => number(year_month.months_in_year() as f64),
            "inLeapYear" => Value::Bool(year_month.in_leap_year()),
            "referenceDay" => number(year_month.reference_day() as f64),
            "era" => optional_text(year_month.era().map(|era| era.to_string())),
            "eraYear" => optional_number(year_month.era_year().map(|value| value as f64)),
            "calendarId" => text(year_month.calendar().identifier()),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
        Slot::PlainMonthDay(month_day) => match name {
            "monthCode" => text(month_day.month_code().as_str()),
            "day" => number(month_day.day() as f64),
            "referenceYear" => number(month_day.reference_year() as f64),
            "calendarId" => text(month_day.calendar().identifier()),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
        Slot::Duration(duration) => match name {
            "years" => number(duration.years() as f64),
            "months" => number(duration.months() as f64),
            "weeks" => number(duration.weeks() as f64),
            "days" => number(duration.days() as f64),
            "hours" => number(duration.hours() as f64),
            "minutes" => number(duration.minutes() as f64),
            "seconds" => number(duration.seconds() as f64),
            "milliseconds" => number(duration.milliseconds() as f64),
            "microseconds" => number(duration.microseconds() as f64),
            "nanoseconds" => number(duration.nanoseconds() as f64),
            "blank" => Value::Bool(duration.is_zero()),
            "sign" => Value::Int(match duration.sign() {
                temporal_rs::Sign::Zero => 0,
                temporal_rs::Sign::Positive => 1,
                temporal_rs::Sign::Negative => -1,
            }),
            other => return Err(unknown(scope, slot.kind(), other)),
        },
    })
}

fn optional_number(value: Option<f64>) -> Value {
    match value {
        Some(value) => number(value),
        None => Value::Undefined,
    }
}

fn optional_text(value: Option<String>) -> Value {
    match value {
        Some(value) => text(value),
        None => Value::Undefined,
    }
}

// ==== The operations ====

fn merged(
    scope: &mut Scope<'_>,
    args: Option<&ObjectRef>,
    mut base: Fields,
) -> Result<Fields, RuntimeError> {
    let Some(object) = args else {
        return Ok(base);
    };
    if let Some(value) = field_number(scope, object, "year")? {
        base.year = value as i32;
    }
    if let Some(value) = field_number(scope, object, "month")? {
        base.month = value as u8;
    }
    if let Some(value) = field_number(scope, object, "day")? {
        base.day = value as u8;
    }
    if let Some(value) = field_number(scope, object, "hour")? {
        base.hour = value as u8;
    }
    if let Some(value) = field_number(scope, object, "minute")? {
        base.minute = value as u8;
    }
    if let Some(value) = field_number(scope, object, "second")? {
        base.second = value as u8;
    }
    if let Some(value) = field_number(scope, object, "millisecond")? {
        base.millisecond = value as u16;
    }
    if let Some(value) = field_number(scope, object, "microsecond")? {
        base.microsecond = value as u16;
    }
    if let Some(value) = field_number(scope, object, "nanosecond")? {
        base.nanosecond = value as u16;
    }
    if let Some(id) = field_text(scope, object, "calendar")? {
        base.calendar = as_calendar(scope, &id)?;
    }
    if let Some(id) = field_text(scope, object, "timeZone")? {
        base.time_zone = Some(id);
    }
    if let Some(offset) = field_text(scope, object, "offset")? {
        base.offset = Some(offset);
    }
    Ok(base)
}

fn fields_of_date(date: &PlainDate) -> Fields {
    Fields {
        year: date.year(),
        month: date.month(),
        day: date.day(),
        calendar: date.calendar().clone(),
        ..Fields::default()
    }
}

fn run(
    scope: &mut Scope<'_>,
    prototypes: &Prototypes,
    slot: &Slot,
    name: &str,
    args: Option<&ObjectRef>,
) -> Result<Value, RuntimeError> {
    let made = match (slot, name) {
        (Slot::PlainDate(date), "add") => {
            let duration = arg_duration(scope, args)?;
            let overflow = overflow(scope, args)?;
            Slot::PlainDate(
                date.add(&duration, overflow)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDate(date), "subtract") => {
            let duration = arg_duration(scope, args)?;
            let overflow = overflow(scope, args)?;
            Slot::PlainDate(
                date.subtract(&duration, overflow)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDate(date), "until") | (Slot::PlainDate(date), "since") => {
            let other = arg_plain_date(scope, args, "other")?;
            let settings = difference_settings(scope, args)?;
            let difference = if name == "until" {
                date.until(&other, settings)
            } else {
                date.since(&other, settings)
            };
            Slot::Duration(difference.map_err(|error| temporal(scope, error))?)
        }
        (Slot::PlainDate(date), "with") => {
            let merged = merged(scope, args, fields_of_date(date))?;
            Slot::PlainDate(
                PlainDate::new_with_overflow(
                    merged.year,
                    merged.month,
                    merged.day,
                    merged.calendar,
                    overflow(scope, args)?.unwrap_or(Overflow::Constrain),
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDate(date), "toPlainDateTime") => {
            let time = arg_plain_time(scope, args, "time")?;
            Slot::PlainDateTime(
                date.to_plain_date_time(Some(time))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDate(date), "toPlainYearMonth") => Slot::PlainYearMonth(
            date.to_plain_year_month()
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::PlainDate(date), "toPlainMonthDay") => Slot::PlainMonthDay(
            date.to_plain_month_day()
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::PlainDate(date), "toZonedDateTime") => {
            let zone = arg_time_zone(scope, args)?;
            let time = arg_plain_time(scope, args, "time")?;
            Slot::ZonedDateTime(
                date.to_zoned_date_time(zone, Some(time))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDate(date), "toString") => {
            return Ok(text(
                date.to_ixdtf_string(DisplayCalendar::Auto),
            ))
        }
        (Slot::PlainDate(date), "equals") => {
            let other = arg_plain_date(scope, args, "other")?;
            return Ok(Value::Bool(date.compare_iso(&other) == std::cmp::Ordering::Equal));
        }

        (Slot::PlainTime(time), "add") => {
            let duration = arg_duration(scope, args)?;
            Slot::PlainTime(
                time.add(&duration)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainTime(time), "subtract") => {
            let duration = arg_duration(scope, args)?;
            Slot::PlainTime(
                time.subtract(&duration)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainTime(time), "until") | (Slot::PlainTime(time), "since") => {
            let other = arg_plain_time(scope, args, "other")?;
            let settings = difference_settings(scope, args)?;
            let difference = if name == "until" {
                time.until(&other, settings)
            } else {
                time.since(&other, settings)
            };
            Slot::Duration(difference.map_err(|error| temporal(scope, error))?)
        }
        (Slot::PlainTime(time), "round") => Slot::PlainTime(
            time.round(rounding_options(scope, args)?)
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::PlainTime(time), "with") => {
            let base = Fields {
                hour: time.hour(),
                minute: time.minute(),
                second: time.second(),
                millisecond: time.millisecond(),
                microsecond: time.microsecond(),
                nanosecond: time.nanosecond(),
                ..Fields::default()
            };
            let merged = merged(scope, args, base)?;
            Slot::PlainTime(
                PlainTime::new(
                    merged.hour,
                    merged.minute,
                    merged.second,
                    merged.millisecond,
                    merged.microsecond,
                    merged.nanosecond,
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainTime(time), "toPlainDateTime") => {
            let date = arg_plain_date(scope, args, "date")?;
            Slot::PlainDateTime(
                date.to_plain_date_time(Some(time.clone()))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainTime(time), "toZonedDateTime") => {
            let zone = arg_time_zone(scope, args)?;
            let date = arg_plain_date(scope, args, "date")?;
            Slot::ZonedDateTime(
                date.to_zoned_date_time(zone, Some(time.clone()))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainTime(time), "toString") => {
            return Ok(text(
                time.to_ixdtf_string(to_string_options(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            ))
        }
        (Slot::PlainTime(time), "equals") => {
            let other = arg_plain_time(scope, args, "other")?;
            return Ok(Value::Bool(
                time.hour() == other.hour()
                    && time.minute() == other.minute()
                    && time.second() == other.second()
                    && time.millisecond() == other.millisecond()
                    && time.microsecond() == other.microsecond()
                    && time.nanosecond() == other.nanosecond(),
            ));
        }

        (Slot::PlainDateTime(date_time), "add") => {
            let duration = arg_duration(scope, args)?;
            Slot::PlainDateTime(
                date_time
                    .add(&duration, overflow(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDateTime(date_time), "subtract") => {
            let duration = arg_duration(scope, args)?;
            Slot::PlainDateTime(
                date_time
                    .subtract(&duration, overflow(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDateTime(date_time), "until") | (Slot::PlainDateTime(date_time), "since") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "plain_date_time", &value, None)? {
                    Slot::PlainDateTime(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.PlainDateTime")),
                },
                None => return Err(type_error(scope, "a date and time is required")),
            };
            let settings = difference_settings(scope, args)?;
            let difference = if name == "until" {
                date_time.until(&other, settings)
            } else {
                date_time.since(&other, settings)
            };
            Slot::Duration(difference.map_err(|error| temporal(scope, error))?)
        }
        (Slot::PlainDateTime(date_time), "round") => Slot::PlainDateTime(
            date_time
                .round(rounding_options(scope, args)?)
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::PlainDateTime(date_time), "with") => {
            let base = Fields {
                year: date_time.year(),
                month: date_time.month(),
                day: date_time.day(),
                hour: date_time.hour(),
                minute: date_time.minute(),
                second: date_time.second(),
                millisecond: date_time.millisecond(),
                microsecond: date_time.microsecond(),
                nanosecond: date_time.nanosecond(),
                calendar: date_time.calendar().clone(),
                ..Fields::default()
            };
            let merged = merged(scope, args, base)?;
            Slot::PlainDateTime(
                PlainDateTime::new_with_overflow(
                    merged.year,
                    merged.month,
                    merged.day,
                    merged.hour,
                    merged.minute,
                    merged.second,
                    merged.millisecond,
                    merged.microsecond,
                    merged.nanosecond,
                    merged.calendar,
                    overflow(scope, args)?.unwrap_or(Overflow::Constrain),
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDateTime(date_time), "toPlainDate") => {
            Slot::PlainDate(date_time.to_plain_date())
        }
        (Slot::PlainDateTime(date_time), "toPlainTime") => {
            Slot::PlainTime(date_time.to_plain_time())
        }
        (Slot::PlainDateTime(date_time), "withPlainTime") => {
            let time = arg_plain_time(scope, args, "time")?;
            Slot::PlainDateTime(
                date_time
                    .with_time(Some(time))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDateTime(date_time), "toZonedDateTime") => {
            let zone = arg_time_zone(scope, args)?;
            Slot::ZonedDateTime(
                date_time
                    .to_zoned_date_time(zone, disambiguation(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainDateTime(date_time), "toString") => {
            return Ok(text(
                date_time
                    .to_ixdtf_string(to_string_options(scope, args)?, DisplayCalendar::Auto)
                    .map_err(|error| temporal(scope, error))?,
            ))
        }
        (Slot::PlainDateTime(date_time), "equals") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "plain_date_time", &value, None)? {
                    Slot::PlainDateTime(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.PlainDateTime")),
                },
                None => return Err(type_error(scope, "a date and time is required")),
            };
            return Ok(Value::Bool(
                date_time.compare_iso(&other) == std::cmp::Ordering::Equal,
            ));
        }

        (Slot::Instant(instant), "add") => {
            let duration = arg_duration(scope, args)?;
            Slot::Instant(
                instant
                    .add(&duration)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::Instant(instant), "subtract") => {
            let duration = arg_duration(scope, args)?;
            Slot::Instant(
                instant
                    .subtract(&duration)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::Instant(instant), "until") | (Slot::Instant(instant), "since") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "instant", &value, None)? {
                    Slot::Instant(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.Instant")),
                },
                None => return Err(type_error(scope, "an instant is required")),
            };
            let settings = difference_settings(scope, args)?;
            let difference = if name == "until" {
                instant.until(&other, settings)
            } else {
                instant.since(&other, settings)
            };
            Slot::Duration(difference.map_err(|error| temporal(scope, error))?)
        }
        (Slot::Instant(instant), "round") => Slot::Instant(
            instant
                .round(rounding_options(scope, args)?)
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::Instant(instant), "toZonedDateTimeISO") => {
            let zone = arg_time_zone(scope, args)?;
            Slot::ZonedDateTime(
                instant
                    .to_zoned_date_time_iso(zone)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::Instant(instant), "toZonedDateTime") => {
            let zone = arg_time_zone(scope, args)?;
            let calendar = match arg_text(scope, args, "calendar")? {
                Some(id) => as_calendar(scope, &id)?,
                None => Calendar::default(),
            };
            Slot::ZonedDateTime(
                ZonedDateTime::try_new_from_instant(instant.clone(), zone, calendar)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::Instant(instant), "toString") => {
            let zone = match arg_text(scope, args, "timeZone")? {
                Some(id) => Some(as_time_zone(scope, &id)?),
                None => None,
            };
            return Ok(text(
                instant
                    .to_ixdtf_string(zone, to_string_options(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            ));
        }
        (Slot::Instant(instant), "equals") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "instant", &value, None)? {
                    Slot::Instant(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.Instant")),
                },
                None => return Err(type_error(scope, "an instant is required")),
            };
            return Ok(Value::Bool(instant.as_i128() == other.as_i128()));
        }

        (Slot::ZonedDateTime(zoned), "add") => {
            let duration = arg_duration(scope, args)?;
            Slot::ZonedDateTime(
                zoned
                    .add(&duration, overflow(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::ZonedDateTime(zoned), "subtract") => {
            let duration = arg_duration(scope, args)?;
            Slot::ZonedDateTime(
                zoned
                    .subtract(&duration, overflow(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::ZonedDateTime(zoned), "until") | (Slot::ZonedDateTime(zoned), "since") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "zoned_date_time", &value, None)? {
                    Slot::ZonedDateTime(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.ZonedDateTime")),
                },
                None => return Err(type_error(scope, "a zoned date and time is required")),
            };
            let settings = difference_settings(scope, args)?;
            let difference = if name == "until" {
                zoned.until(&other, settings)
            } else {
                zoned.since(&other, settings)
            };
            Slot::Duration(difference.map_err(|error| temporal(scope, error))?)
        }
        (Slot::ZonedDateTime(zoned), "round") => Slot::ZonedDateTime(
            zoned
                .round(rounding_options(scope, args)?)
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::ZonedDateTime(zoned), "with") => {
            let base = Fields {
                year: zoned.year(),
                month: zoned.month(),
                day: zoned.day(),
                hour: zoned.hour(),
                minute: zoned.minute(),
                second: zoned.second(),
                millisecond: zoned.millisecond(),
                microsecond: zoned.microsecond(),
                nanosecond: zoned.nanosecond(),
                calendar: zoned.calendar().clone(),
                time_zone: Some(
                    zoned
                        .time_zone()
                        .identifier()
                        .map_err(|error| temporal(scope, error))?,
                ),
                offset: Some(zoned.offset().to_string()),
                ..Fields::default()
            };
            let merged = merged(scope, args, base)?;
            Slot::ZonedDateTime(
                ZonedDateTime::from_utf8(
                    iso_text(&merged).as_bytes(),
                    disambiguation(scope, args)?,
                    OffsetDisambiguation::Prefer,
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::ZonedDateTime(zoned), "toInstant") => Slot::Instant(zoned.to_instant()),
        (Slot::ZonedDateTime(zoned), "toPlainDate") => {
            Slot::PlainDate(zoned.to_plain_date())
        }
        (Slot::ZonedDateTime(zoned), "toPlainTime") => {
            Slot::PlainTime(zoned.to_plain_time())
        }
        (Slot::ZonedDateTime(zoned), "toPlainDateTime") => {
            Slot::PlainDateTime(zoned.to_plain_date_time())
        }
        (Slot::ZonedDateTime(zoned), "withTimeZone") => {
            let zone = arg_time_zone(scope, args)?;
            Slot::ZonedDateTime(
                zoned
                    .with_timezone(zone)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::ZonedDateTime(zoned), "withPlainTime") => {
            let time = arg_plain_time(scope, args, "time")?;
            Slot::ZonedDateTime(
                zoned
                    .with_plain_time(Some(time))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::ZonedDateTime(zoned), "startOfDay") => Slot::ZonedDateTime(
            zoned
                .start_of_day()
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::ZonedDateTime(zoned), "getTimeZoneTransition") => {
            let direction = match arg_text(scope, args, "direction")?.as_deref() {
                Some("next") => temporal_rs::provider::TransitionDirection::Next,
                Some("previous") => temporal_rs::provider::TransitionDirection::Previous,
                Some(other) => {
                    return Err(range(
                        scope,
                        &format!("direction must be next or previous, not {other}"),
                    ))
                }
                None => return Err(type_error(scope, "a direction is required")),
            };
            match zoned
                .get_time_zone_transition(direction)
                .map_err(|error| temporal(scope, error))?
            {
                Some(transition) => Slot::ZonedDateTime(transition),
                None => return Ok(Value::Null),
            }
        }
        (Slot::ZonedDateTime(zoned), "toString") => {
            return Ok(text(
                zoned
                    .to_ixdtf_string(
                        DisplayOffset::Auto,
                        DisplayTimeZone::Auto,
                        DisplayCalendar::Auto,
                        to_string_options(scope, args)?,
                    )
                    .map_err(|error| temporal(scope, error))?,
            ))
        }
        (Slot::ZonedDateTime(zoned), "equals") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "zoned_date_time", &value, None)? {
                    Slot::ZonedDateTime(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.ZonedDateTime")),
                },
                None => return Err(type_error(scope, "a zoned date and time is required")),
            };
            return Ok(Value::Bool(
                zoned
                    .equals(&other)
                    .map_err(|error| temporal(scope, error))?,
            ));
        }

        (Slot::PlainYearMonth(year_month), "add") => {
            let duration = arg_duration(scope, args)?;
            Slot::PlainYearMonth(
                year_month
                    .add(&duration, overflow(scope, args)?.unwrap_or(Overflow::Constrain))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainYearMonth(year_month), "subtract") => {
            let duration = arg_duration(scope, args)?;
            Slot::PlainYearMonth(
                year_month
                    .subtract(&duration, overflow(scope, args)?.unwrap_or(Overflow::Constrain))
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainYearMonth(year_month), "until") | (Slot::PlainYearMonth(year_month), "since") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "plain_year_month", &value, None)? {
                    Slot::PlainYearMonth(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.PlainYearMonth")),
                },
                None => return Err(type_error(scope, "a year and month is required")),
            };
            let settings = difference_settings(scope, args)?;
            let difference = if name == "until" {
                year_month.until(&other, settings)
            } else {
                year_month.since(&other, settings)
            };
            Slot::Duration(difference.map_err(|error| temporal(scope, error))?)
        }
        (Slot::PlainYearMonth(year_month), "with") => {
            let base = Fields {
                year: year_month.year(),
                month: year_month.month(),
                day: year_month.reference_day(),
                calendar: year_month.calendar().clone(),
                ..Fields::default()
            };
            let merged = merged(scope, args, base)?;
            Slot::PlainYearMonth(
                PlainYearMonth::new_with_overflow(
                    merged.year,
                    merged.month,
                    Some(merged.day),
                    merged.calendar,
                    overflow(scope, args)?.unwrap_or(Overflow::Constrain),
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainYearMonth(year_month), "toPlainDate") => {
            // The day is required and the rest comes from the year and month, which is what the
            // specification does for the ISO calendar this runtime has algorithms for.
            let day = arg_number(scope, args, "day")?
                .ok_or_else(|| type_error(scope, "a day is required"))? as u8;
            Slot::PlainDate(
                PlainDate::new_with_overflow(
                    year_month.year(),
                    year_month.month(),
                    day,
                    year_month.calendar().clone(),
                    Overflow::Constrain,
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainYearMonth(year_month), "toString") => {
            return Ok(text(
                year_month.to_ixdtf_string(DisplayCalendar::Auto),
            ))
        }
        (Slot::PlainYearMonth(year_month), "equals") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "plain_year_month", &value, None)? {
                    Slot::PlainYearMonth(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.PlainYearMonth")),
                },
                None => return Err(type_error(scope, "a year and month is required")),
            };
            return Ok(Value::Bool(
                year_month.compare_iso(&other) == std::cmp::Ordering::Equal,
            ));
        }

        (Slot::PlainMonthDay(month_day), "with") => {
            let base = Fields {
                month: month_day.month_code().to_month_integer(),
                day: month_day.day(),
                ..Fields::default()
            };
            let merged = merged(scope, args, base)?;
            Slot::PlainMonthDay(
                PlainMonthDay::new_with_overflow(
                    merged.month,
                    merged.day,
                    Calendar::default(),
                    overflow(scope, args)?.unwrap_or(Overflow::Constrain),
                    Some(month_day.reference_year()),
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainMonthDay(month_day), "toPlainDate") => {
            let year = arg_number(scope, args, "year")?
                .ok_or_else(|| type_error(scope, "a year is required"))? as i32;
            Slot::PlainDate(
                PlainDate::new_with_overflow(
                    year,
                    month_day.month_code().to_month_integer(),
                    month_day.day(),
                    month_day.calendar().clone(),
                    Overflow::Constrain,
                )
                .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::PlainMonthDay(month_day), "toString") => {
            return Ok(text(
                month_day.to_ixdtf_string(DisplayCalendar::Auto),
            ))
        }
        (Slot::PlainMonthDay(month_day), "equals") => {
            let other = match arg(scope, args, "other")? {
                Some(value) => match parse(scope, "plain_month_day", &value, None)? {
                    Slot::PlainMonthDay(other) => other,
                    _ => return Err(type_error(scope, "expected a Temporal.PlainMonthDay")),
                },
                None => return Err(type_error(scope, "a month and day is required")),
            };
            return Ok(Value::Bool(
                month_day.month_code().as_str() == other.month_code().as_str()
                    && month_day.day() == other.day(),
            ));
        }

        (Slot::Duration(duration), "negated") => Slot::Duration(duration.negated()),
        (Slot::Duration(duration), "abs") => Slot::Duration(duration.abs()),
        (Slot::Duration(duration), "add") => {
            let other = arg_duration(scope, args)?;
            Slot::Duration(
                duration
                    .add(&other)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::Duration(duration), "subtract") => {
            let other = arg_duration(scope, args)?;
            Slot::Duration(
                duration
                    .subtract(&other)
                    .map_err(|error| temporal(scope, error))?,
            )
        }
        (Slot::Duration(duration), "round") => Slot::Duration(
            duration
                .round(rounding_options(scope, args)?, None)
                .map_err(|error| temporal(scope, error))?,
        ),
        (Slot::Duration(duration), "total") => {
            let Some(unit) = arg_text(scope, args, "unit")? else {
                return Err(type_error(scope, "a unit is required"));
            };
            let unit = unit_of(scope, &unit)?;
            let total = duration
                .total(unit, None)
                .map_err(|error| temporal(scope, error))?;
            return Ok(number(total.as_inner()));
        }
        (Slot::Duration(duration), "toString") => {
            return Ok(text(
                duration
                    .as_temporal_string(to_string_options(scope, args)?)
                    .map_err(|error| temporal(scope, error))?,
            ))
        }

        _ => {
            return Err(range(
                scope,
                &format!(
                    "Temporal.{}.{name} is not implemented yet",
                    name_of(slot.kind())
                ),
            ))
        }
    };
    wrap(scope, prototypes, made)
}

/// The zone methods a Temporal.TimeZone carries. They are here rather than in the prelude because
/// they are zone arithmetic, which is the part temporal_rs owns.
fn zone_read(
    scope: &mut Scope<'_>,
    zone: &TimeZone,
    name: &str,
    nanos: i128,
) -> Result<Value, RuntimeError> {
    match name {
        "id" => Ok(text(
            zone.identifier().map_err(|error| temporal(scope, error))?,
        )),
        "getOffsetNanosecondsFor" => {
            let instant =
                Instant::try_new(nanos).map_err(|error| temporal(scope, error))?;
            // The offset of an instant in a zone is what a zoned value in that zone carries.
            let zoned = ZonedDateTime::try_new_from_instant(instant, zone.clone(), Calendar::default())
                .map_err(|error| temporal(scope, error))?;
            Ok(big(zoned.offset_nanoseconds() as i128))
        }
        other => Err(range(
            scope,
            &format!("Temporal.TimeZone has no method {other}"),
        )),
    }
}

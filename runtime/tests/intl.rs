//! Intl.DateTimeFormat: the global, the options, and the text it renders.
//!
//! The zone rules and the calendar are temporal_rs'; the CLDR patterns are a table in
//! runtime/src/modules/intl/cldr_data.rs. What is under test is the seam between them and the
//! JS class: which options are accepted, which are reported back, and what a locale renders.

use xmas_runtime::testkit::{Harness, text_of};

fn eval(script: &str) -> String {
    let mut h = Harness::with_modules();
    match h.eval(script) {
        Ok(value) => text_of(&value).unwrap_or_default(),
        Err(error) => panic!("{script}\n{error}"),
    }
}

/// The same, but the script is expected to throw; the message is the answer.
fn throws(script: &str) -> String {
    let mut h = Harness::with_modules();
    match h.eval(script) {
        Ok(value) => panic!("expected a throw, got {:?}", text_of(&value)),
        Err(error) => error,
    }
}

#[test]
fn intl_is_a_global_with_a_datetimeformat() {
    let answer = eval(
        r#"JSON.stringify([typeof Intl, typeof Intl.DateTimeFormat,
            Object.prototype.toString.call(new Intl.DateTimeFormat('en'))])"#,
    );
    assert_eq!(
        answer,
        r#"["object","function","[object Intl.DateTimeFormat]"]"#
    );
}

#[test]
fn one_named_field_renders_alone() {
    let answer = eval(
        r#"new Intl.DateTimeFormat('en-US', { weekday: 'long', timeZone: 'UTC' })
            .format(Date.UTC(2024, 2, 15))"#,
    );
    assert_eq!(answer, "Friday");
}

/// The epoch is the instant; the zone is what turns it into a wall clock.
#[test]
fn the_time_zone_decides_the_wall_clock() {
    let answer = eval(
        r#"const at = Date.UTC(2024, 2, 15, 14, 30);
        const f = (zone) => new Intl.DateTimeFormat('en-US',
            { hour: '2-digit', minute: '2-digit', hour12: false, timeZone: zone }).format(at);
        JSON.stringify([f('UTC'), f('Asia/Shanghai'), f('America/New_York')])"#,
    );
    assert_eq!(answer, r#"["14:30","22:30","10:30"]"#);
}

/// A wall clock that has no zone of its own: the caller reads the parts and puts them together.
#[test]
fn format_to_parts_gives_the_fields_back() {
    let answer = eval(
        r#"JSON.stringify(new Intl.DateTimeFormat('sv-SE', {
            year: 'numeric', month: 'numeric', day: 'numeric',
            hour: '2-digit', minute: '2-digit', hour12: false, timeZone: 'Asia/Shanghai',
        }).formatToParts(Date.UTC(2026, 9, 18, 1, 30)))"#,
    );
    assert_eq!(
        answer,
        r#"[{"type":"month","value":"10"},{"type":"literal","value":"/"},{"type":"day","value":"18"},{"type":"literal","value":"/"},{"type":"year","value":"2026"},{"type":"literal","value":" "},{"type":"hour","value":"09"},{"type":"literal","value":":"},{"type":"minute","value":"30"}]"#
    );
}

#[test]
fn a_style_renders_a_whole_date_or_time() {
    let answer = eval(
        r#"JSON.stringify([
            new Intl.DateTimeFormat('en-US', { dateStyle: 'full', timeZone: 'UTC' })
                .format(Date.UTC(2024, 2, 15)),
            new Intl.DateTimeFormat('de-DE', { dateStyle: 'long', timeZone: 'UTC' })
                .format(Date.UTC(2024, 2, 15)),
            new Intl.DateTimeFormat('en-US', { timeStyle: 'short', timeZone: 'UTC' })
                .format(Date.UTC(2024, 2, 15, 14, 30)),
        ])"#,
    );
    assert_eq!(
        answer,
        r#"["Friday, March 15, 2024","15. März 2024","2:30 PM"]"#
    );
}

#[test]
fn a_locale_names_its_own_months_and_days() {
    let answer = eval(
        r#"JSON.stringify([
            new Intl.DateTimeFormat('fr-FR', { weekday: 'long', day: 'numeric', month: 'long',
                timeZone: 'UTC' }).format(Date.UTC(2024, 2, 15)),
            new Intl.DateTimeFormat('ja-JP', { dateStyle: 'full', timeZone: 'UTC' })
                .format(Date.UTC(2024, 2, 15)),
        ])"#,
    );
    assert_eq!(answer, r#"["vendredi 15 mars","2024年3月15日金曜日"]"#);
}

/// No option at all is the locale's own short date and short time.
#[test]
fn no_options_render_the_locale_default() {
    let answer = eval(
        r#"JSON.stringify([
            new Intl.DateTimeFormat('en-US', { timeZone: 'UTC' }).format(Date.UTC(2024, 2, 15, 14, 30, 45)),
            new Intl.DateTimeFormat('en-GB', { timeZone: 'UTC' }).format(Date.UTC(2024, 2, 15, 14, 30, 45)),
        ])"#,
    );
    assert_eq!(
        answer,
        r#"["Mar 15, 2024, 2:30:45 PM","15 Mar 2024, 14:30:45"]"#
    );
}

#[test]
fn resolved_options_report_what_was_asked_for() {
    let answer = eval(
        r#"JSON.stringify(new Intl.DateTimeFormat('sv-SE', { hour12: false, timeZone: 'Asia/Shanghai' })
            .resolvedOptions())"#,
    );
    assert_eq!(
        answer,
        r#"{"locale":"sv-SE","calendar":"gregory","numberingSystem":"latn","timeZone":"Asia/Shanghai","year":"numeric","month":"numeric","day":"numeric","hour":"numeric","minute":"numeric","second":"numeric","hour12":false,"hourCycle":"h23"}"#
    );
}

/// A zone nothing can resolve is a RangeError, which is what a caller watching for a typo expects.
#[test]
fn an_unknown_time_zone_is_a_range_error() {
    let answer = throws(r#"new Intl.DateTimeFormat('en-US', { timeZone: 'Moon/Base' })"#);
    assert!(answer.contains("RangeError"), "{answer}");
}

#[test]
fn the_zone_list_comes_from_the_compiled_tzdb() {
    let answer = eval(
        r#"const zones = Intl.supportedValuesOf('timeZone');
        JSON.stringify([zones.length > 100, zones.includes('Asia/Shanghai'), zones.includes('America/New_York')])"#,
    );
    assert_eq!(answer, "[true,true,true]");
}

#[test]
fn an_invalid_date_formats_as_text_and_throws_from_parts() {
    let answer = eval(
        r#"JSON.stringify([new Intl.DateTimeFormat('en-US').format(NaN),
            (() => { try { new Intl.DateTimeFormat('en-US').formatToParts(NaN); return 'no throw'; }
                     catch (e) { return e.name; } })()])"#,
    );
    assert_eq!(answer, r#"["Invalid Date","RangeError"]"#);
}

/// A range renders both ends, and marks which end a part came from.
#[test]
fn a_range_renders_both_ends() {
    let answer = eval(
        r#"const f = new Intl.DateTimeFormat('en-US', { year: 'numeric', timeZone: 'UTC' });
        JSON.stringify([f.formatRange(Date.UTC(2020, 0, 1), Date.UTC(2021, 0, 1)),
            f.formatRangeToParts(Date.UTC(2020, 0, 1), Date.UTC(2021, 0, 1))])"#,
    );
    assert_eq!(
        answer,
        r#"["2020 – 2021",[{"type":"year","value":"2020"},{"type":"literal","value":" – ","source":"shared"},{"type":"year","value":"2021","source":"endRange"}]]"#
    );
}

/// The Date methods delegate: the same locale and zone, on a value rather than an instant.
#[test]
fn the_date_methods_use_the_same_formatter() {
    let answer = eval(
        r#"const d = new Date(Date.UTC(2024, 2, 15, 14, 30, 45));
        JSON.stringify([d.toLocaleString('en-US', { timeZone: 'UTC' }),
            d.toLocaleDateString('en-GB', { timeZone: 'UTC' }),
            d.toLocaleTimeString('en-US', { timeZone: 'UTC' })])"#,
    );
    assert_eq!(
        answer,
        r#"["Mar 15, 2024, 2:30:45 PM","15/03/2024","02:30:45 PM"]"#
    );
}

/// A tag that is not a language tag at all is a RangeError; an unknown but well formed one falls
/// back to the default locale.
#[test]
fn a_malformed_locale_throws_and_an_unknown_one_falls_back() {
    let answer = eval(
        r#"const bad = (() => { try { new Intl.DateTimeFormat('en_US'); return 'no throw'; }
                              catch (e) { return e.name; } })();
        JSON.stringify([bad, new Intl.DateTimeFormat('xx-YY').resolvedOptions().locale])"#,
    );
    assert_eq!(answer, r#"["RangeError","xx-YY"]"#);
}

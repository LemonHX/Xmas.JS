//! Temporal: the global and the values it makes.
//!
//! temporal_rs is the arithmetic; what is under test is the seam. A field has to come back as a
//! number, a value has to come back as an instance of its own class, and an error has to be an
//! error rather than undefined.

use xmas_runtime::testkit::{text_of, Harness};

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
fn temporal_is_a_global_with_the_eight_types() {
    let answer = eval(
        r#"JSON.stringify([
            typeof Temporal,
            ['Instant', 'PlainDate', 'PlainTime', 'PlainDateTime', 'PlainYearMonth',
             'PlainMonthDay', 'ZonedDateTime', 'Duration'].every((n) => typeof Temporal[n] === 'function'),
            typeof Temporal.Now.instant,
        ])"#,
    );
    assert_eq!(answer, r#"["object",true,"function"]"#);
}

#[test]
fn a_plain_date_reports_its_own_fields() {
    let answer = eval(
        r#"const d = new Temporal.PlainDate(2025, 3, 3);
        JSON.stringify([d.year, d.month, d.monthCode, d.day, d.dayOfWeek, d.daysInMonth,
            d.inLeapYear, d.calendarId, d.toString(), d instanceof Temporal.PlainDate])"#,
    );
    assert_eq!(
        answer,
        r#"[2025,3,"M03",3,1,31,false,"iso8601","2025-03-03",true]"#
    );
}

#[test]
fn adding_days_crosses_a_month() {
    let answer = eval(
        r#"const d = Temporal.PlainDate.from('2025-01-30');
        JSON.stringify([d.add({ days: 3 }).toString(), d.subtract({ days: 31 }).toString()])"#,
    );
    assert_eq!(answer, r#"["2025-02-02","2024-12-30"]"#);
}

#[test]
fn until_reports_the_difference_in_the_unit_asked_for() {
    let answer = eval(
        r#"const one = Temporal.PlainDate.from('2025-01-01');
        const two = Temporal.PlainDate.from('2025-03-15');
        JSON.stringify([one.until(two).toString(), one.until(two, { largestUnit: 'months' }).toString()])"#,
    );
    assert_eq!(answer, r#"["P73D","P2M14D"]"#);
}

#[test]
fn a_plain_time_rounds() {
    let answer = eval(
        r#"const t = Temporal.PlainTime.from('12:34:56.789');
        JSON.stringify([t.toString(), t.round({ smallestUnit: 'minute' }).toString(),
            t.round({ smallestUnit: 'hour', roundingMode: 'ceil' }).toString(), t.hour, t.nanosecond])"#,
    );
    assert_eq!(answer, r#"["12:34:56.789","12:35:00","13:00:00",12,0]"#);
}

#[test]
fn a_plain_date_time_keeps_both_halves() {
    let answer = eval(
        r#"const dt = Temporal.PlainDateTime.from('2025-03-03T11:16:10.5');
        JSON.stringify([dt.toString(), dt.hour, dt.millisecond,
            dt.toPlainDate().toString(), dt.toPlainTime().toString()])"#,
    );
    assert_eq!(
        answer,
        r#"["2025-03-03T11:16:10.5",11,500,"2025-03-03","11:16:10.5"]"#
    );
}

#[test]
fn a_zoned_date_time_knows_its_zone_and_offset() {
    let answer = eval(
        r#"const z = Temporal.ZonedDateTime.from('2025-03-01T11:16:10Z[America/Chicago]');
        JSON.stringify([z.toString(), z.hour, z.offset, z.timeZoneId, z.epochMilliseconds,
            z.hoursInDay])"#,
    );
    assert_eq!(
        answer,
        r#"["2025-03-01T05:16:10-06:00[America/Chicago]",5,"-06:00","America/Chicago",1740827770000,24]"#
    );
}

#[test]
fn an_instant_carries_nanoseconds_as_a_bigint() {
    let answer = eval(
        r#"const i = Temporal.Instant.fromEpochMilliseconds(1740837370000);
        JSON.stringify([typeof i.epochNanoseconds, i.epochNanoseconds === 1740837370000000000n,
            i.epochMilliseconds, i.toString(),
            i.toZonedDateTimeISO('Asia/Tokyo').toString()])"#,
    );
    assert_eq!(
        answer,
        r#"["bigint",true,1740837370000,"2025-03-01T13:56:10Z","2025-03-01T22:56:10+09:00[Asia/Tokyo]"]"#
    );
}

#[test]
fn a_duration_is_parsed_totaled_and_negated() {
    let answer = eval(
        r#"const d = Temporal.Duration.from('PT1H30M');
        JSON.stringify([d.toString(), d.total({ unit: 'minutes' }), d.negated().toString(),
            d.sign, d.blank, Temporal.Duration.from('PT1H').add({ minutes: 15 }).toString()])"#,
    );
    assert_eq!(answer, r#"["PT1H30M",90,"-PT1H30M",1,false,"PT1H15M"]"#);
}

#[test]
fn year_month_and_month_day_convert_back_to_a_date() {
    let answer = eval(
        r#"const d = Temporal.PlainDate.from('2025-03-03');
        const ym = d.toPlainYearMonth();
        const md = d.toPlainMonthDay();
        JSON.stringify([ym.toString(), md.toString(),
            ym.toPlainDate({ day: 15 }).toString(), md.toPlainDate({ year: 2024 }).toString()])"#,
    );
    assert_eq!(answer, r#"["2025-03","03-03","2025-03-15","2024-03-03"]"#);
}

#[test]
fn with_keeps_the_fields_it_is_not_given() {
    let answer = eval(
        r#"JSON.stringify([
            Temporal.Duration.from('PT1H').with({ minutes: 30 }).toString(),
            Temporal.PlainDate.from('2025-03-03').with({ month: 12 }).toString(),
            Temporal.PlainTime.from('01:02:03').with({ hour: 23 }).toString(),
            Temporal.PlainMonthDay.from('03-03').with({ day: 15 }).toString(),
        ])"#,
    );
    assert_eq!(answer, r#"["PT1H30M","2025-12-03","23:02:03","03-15"]"#);
}

#[test]
fn comparing_and_equality_go_through_the_values() {
    let answer = eval(
        r#"const one = Temporal.PlainDate.from('2025-01-01');
        const two = Temporal.PlainDate.from('2025-06-01');
        JSON.stringify([Temporal.PlainDate.compare(one, two), Temporal.PlainDate.compare(two, one),
            one.equals(one), one.equals(two),
            Temporal.Instant.compare(Temporal.Instant.fromEpochMilliseconds(2), Temporal.Instant.fromEpochMilliseconds(1))])"#,
    );
    assert_eq!(answer, r#"[-1,1,true,false,1]"#);
}

#[test]
fn now_reads_the_clock() {
    let answer = eval(
        r#"const now = Temporal.Now.instant();
        const date = Temporal.Now.plainDateISO('UTC');
        JSON.stringify([typeof now.epochMilliseconds, Math.abs(now.epochMilliseconds - Date.now()) < 60000,
            typeof date.year, Temporal.Now.timeZoneId()])"#,
    );
    assert_eq!(answer, r#"["number",true,"number","UTC"]"#);
}

#[test]
fn a_time_zone_reports_the_offset_of_an_instant() {
    let answer = eval(
        r#"const zone = new Temporal.TimeZone('Asia/Tokyo');
        const instant = Temporal.Instant.from('2025-03-01T00:00:00Z');
        JSON.stringify([zone.id, zone.toString(), String(zone.getOffsetNanosecondsFor(instant)),
            zone.equals(new Temporal.TimeZone('Asia/Tokyo'))])"#,
    );
    assert_eq!(answer, r#"["Asia/Tokyo","Asia/Tokyo","32400000000000",true]"#);
}

#[test]
fn a_calendar_carries_its_identifier() {
    let answer = eval(
        r#"const d = new Temporal.PlainDate(2025, 3, 3, 'japanese');
        JSON.stringify([d.calendarId, d.calendar.id, d.era, d.eraYear])"#,
    );
    assert!(answer.starts_with(r#"["japanese","japanese","#), "{answer}");
}

#[test]
fn bad_input_throws_instead_of_returning_undefined() {
    let bad_date = throws("Temporal.PlainDate.from('2025-02-30')");
    assert!(
        bad_date.contains("RangeError") || bad_date.contains("Invalid"),
        "{bad_date}"
    );
    let bad_calendar = throws("new Temporal.Calendar('nonesuch')");
    assert!(bad_calendar.contains("unknown calendar"), "{bad_calendar}");
    let not_a_date = throws("Temporal.PlainDate.from('2025-03-03').until('nope')");
    assert!(
        not_a_date.contains("RangeError") || not_a_date.contains("Invalid"),
        "{not_a_date}"
    );
}

#[test]
fn a_value_of_the_wrong_kind_is_refused() {
    let answer = throws(
        r#"const date = new Temporal.PlainDate(2025, 3, 3);
        const time = new Temporal.PlainTime(1, 2);
        date.add(time)"#,
    );
    assert!(answer.contains("expected a Temporal.Duration"), "{answer}");
}

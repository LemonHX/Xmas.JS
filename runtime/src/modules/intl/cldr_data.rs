//! The CLDR facts a date format needs, for a handful of locales.
//!
//! This is not ICU: it is the small part of the CLDR date data a DateTimeFormat uses -- the four
//! style patterns, the month and weekday names, the day-period markers, and whether the locale
//! counts in 12 or 24 hours by default. A locale that is not in the table falls back to the one
//! whose language matches, then to en-US.
//!
//! The patterns are CLDR patterns (UTS #35): y year, M/L month, d day, E/e/c weekday, H hour
//! 0-23, h hour 1-12, m minute, s second, a day period, z time zone, and 'quoted literal text'.

use temporal_rs::ZonedDateTime;

use super::Fields;

pub(super) struct Locale {
    pub date: [&'static str; 4],
    pub time: [&'static str; 4],
    /// How the date and the time are joined: {1} is the date, {0} the time.
    pub join: &'static str,
    pub months_wide: [&'static str; 12],
    pub months_abbr: [&'static str; 12],
    pub days_wide: [&'static str; 7],
    pub days_abbr: [&'static str; 7],
    pub am: &'static str,
    pub pm: &'static str,
    pub hour12: bool,
}

/// The index of a style name in `Locale::date` and `Locale::time`. "medium" is the style a
/// missing date field resolves to.
pub(super) fn style_index(style: Option<&str>) -> usize {
    match style {
        Some("full") => 0,
        Some("long") => 1,
        Some("short") => 3,
        _ => 2,
    }
}

/// The locale a tag resolves to: the language and region when the table has them, the language
/// alone when it only has that, en-US otherwise.
pub(super) fn lookup(tag: &str) -> &'static Locale {
    let lower = tag.to_ascii_lowercase();
    let parts: Vec<&str> = lower.split(['-', '_']).collect();
    let language = parts.first().copied().unwrap_or("en");
    let region = parts.get(1).copied().unwrap_or("");
    match language {
        "en" => match region {
            "gb" | "au" | "nz" | "ie" | "za" | "in" => &EN_GB,
            _ => &EN_US,
        },
        "sv" => &SV,
        "de" => &DE,
        "fr" => &FR,
        "es" => &ES,
        "ja" => &JA,
        "zh" => &ZH,
        _ => &EN_US,
    }
}

static EN_US: Locale = Locale {
    date: ["EEEE, MMMM d, y", "MMMM d, y", "MMM d, y", "M/d/yy"],
    time: ["h:mm:ss a zzzz", "h:mm:ss a z", "h:mm:ss a", "h:mm a"],
    join: "{1}, {0}",
    months_wide: [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    months_abbr: [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    days_wide: [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ],
    days_abbr: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    am: "AM",
    pm: "PM",
    hour12: true,
};

static EN_GB: Locale = Locale {
    date: ["EEEE, d MMMM y", "d MMMM y", "d MMM y", "dd/MM/y"],
    time: ["HH:mm:ss zzzz", "HH:mm:ss z", "HH:mm:ss", "HH:mm"],
    join: "{1}, {0}",
    months_wide: [
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ],
    months_abbr: [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ],
    days_wide: [
        "Sunday",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
    ],
    days_abbr: ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"],
    am: "am",
    pm: "pm",
    hour12: false,
};

static SV: Locale = Locale {
    date: ["EEEE d MMMM y", "d MMMM y", "d MMM y", "y-MM-dd"],
    time: ["HH:mm:ss zzzz", "HH:mm:ss z", "HH:mm:ss", "HH:mm"],
    join: "{1} {0}",
    months_wide: [
        "januari",
        "februari",
        "mars",
        "april",
        "maj",
        "juni",
        "juli",
        "augusti",
        "september",
        "oktober",
        "november",
        "december",
    ],
    months_abbr: [
        "jan.", "feb.", "mars", "apr.", "maj", "juni", "juli", "aug.", "sep.", "okt.", "nov.",
        "dec.",
    ],
    days_wide: [
        "söndag", "måndag", "tisdag", "onsdag", "torsdag", "fredag", "lördag",
    ],
    days_abbr: ["sön", "mån", "tis", "ons", "tors", "fre", "lör"],
    am: "fm",
    pm: "em",
    hour12: false,
};

static DE: Locale = Locale {
    date: ["EEEE, d. MMMM y", "d. MMMM y", "dd.MM.y", "dd.MM.yy"],
    time: ["HH:mm:ss zzzz", "HH:mm:ss z", "HH:mm:ss", "HH:mm"],
    join: "{1}, {0}",
    months_wide: [
        "Januar",
        "Februar",
        "März",
        "April",
        "Mai",
        "Juni",
        "Juli",
        "August",
        "September",
        "Oktober",
        "November",
        "Dezember",
    ],
    months_abbr: [
        "Jan.", "Feb.", "März", "Apr.", "Mai", "Juni", "Juli", "Aug.", "Sep.", "Okt.", "Nov.",
        "Dez.",
    ],
    days_wide: [
        "Sonntag",
        "Montag",
        "Dienstag",
        "Mittwoch",
        "Donnerstag",
        "Freitag",
        "Samstag",
    ],
    days_abbr: ["So.", "Mo.", "Di.", "Mi.", "Do.", "Fr.", "Sa."],
    am: "AM",
    pm: "PM",
    hour12: false,
};

static FR: Locale = Locale {
    date: ["EEEE d MMMM y", "d MMMM y", "d MMM y", "dd/MM/y"],
    time: ["HH:mm:ss zzzz", "HH:mm:ss z", "HH:mm:ss", "HH:mm"],
    join: "{1} {0}",
    months_wide: [
        "janvier",
        "février",
        "mars",
        "avril",
        "mai",
        "juin",
        "juillet",
        "août",
        "septembre",
        "octobre",
        "novembre",
        "décembre",
    ],
    months_abbr: [
        "janv.", "févr.", "mars", "avr.", "mai", "juin", "juil.", "août", "sept.", "oct.", "nov.",
        "déc.",
    ],
    days_wide: [
        "dimanche", "lundi", "mardi", "mercredi", "jeudi", "vendredi", "samedi",
    ],
    days_abbr: ["dim.", "lun.", "mar.", "mer.", "jeu.", "ven.", "sam."],
    am: "AM",
    pm: "PM",
    hour12: false,
};

static ES: Locale = Locale {
    date: [
        "EEEE, d 'de' MMMM 'de' y",
        "d 'de' MMMM 'de' y",
        "d MMM y",
        "d/M/yy",
    ],
    time: ["H:mm:ss zzzz", "H:mm:ss z", "H:mm:ss", "H:mm"],
    join: "{1}, {0}",
    months_wide: [
        "enero",
        "febrero",
        "marzo",
        "abril",
        "mayo",
        "junio",
        "julio",
        "agosto",
        "septiembre",
        "octubre",
        "noviembre",
        "diciembre",
    ],
    months_abbr: [
        "ene", "feb", "mar", "abr", "may", "jun", "jul", "ago", "sept", "oct", "nov", "dic",
    ],
    days_wide: [
        "domingo",
        "lunes",
        "martes",
        "miércoles",
        "jueves",
        "viernes",
        "sábado",
    ],
    days_abbr: ["dom", "lun", "mar", "mié", "jue", "vie", "sáb"],
    am: "a. m.",
    pm: "p. m.",
    hour12: false,
};

static JA: Locale = Locale {
    date: ["y年M月d日EEEE", "y年M月d日", "y/MM/dd", "y/MM/dd"],
    time: ["H時mm分ss秒 zzzz", "H:mm:ss z", "H:mm:ss", "H:mm"],
    join: "{1} {0}",
    months_wide: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    months_abbr: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    days_wide: [
        "日曜日",
        "月曜日",
        "火曜日",
        "水曜日",
        "木曜日",
        "金曜日",
        "土曜日",
    ],
    days_abbr: ["日", "月", "火", "水", "木", "金", "土"],
    am: "午前",
    pm: "午後",
    hour12: false,
};

static ZH: Locale = Locale {
    date: ["y年M月d日EEEE", "y年M月d日", "y年M月d日", "y/M/d"],
    time: ["zzzz HH:mm:ss", "z HH:mm:ss", "HH:mm:ss", "HH:mm"],
    join: "{1} {0}",
    months_wide: [
        "一月",
        "二月",
        "三月",
        "四月",
        "五月",
        "六月",
        "七月",
        "八月",
        "九月",
        "十月",
        "十一月",
        "十二月",
    ],
    months_abbr: [
        "1月", "2月", "3月", "4月", "5月", "6月", "7月", "8月", "9月", "10月", "11月", "12月",
    ],
    days_wide: [
        "星期日",
        "星期一",
        "星期二",
        "星期三",
        "星期四",
        "星期五",
        "星期六",
    ],
    days_abbr: ["周日", "周一", "周二", "周三", "周四", "周五", "周六"],
    am: "上午",
    pm: "下午",
    hour12: false,
};

/// The pattern a set of options asks for.
///
/// A field nobody named is absent from the pattern, which is what makes `{ weekday: "long" }`
/// alone render "Sunday" and nothing else. A named style (dateStyle/timeStyle) supplies the whole
/// date or the whole time, and the two are joined with the locale's own join pattern.
fn pattern_for(locale: &'static Locale, fields: &Fields) -> String {
    let mut date = String::new();
    let day = fields.day.is_some();
    let year = fields.year.is_some();
    let wordy = fields
        .month
        .as_deref()
        .map(|month| month.len() > 1)
        .unwrap_or(false);
    let word_order = locale.date[3].starts_with('d');
    if let Some(weekday) = fields.weekday.as_deref() {
        date.push_str(match weekday {
            "long" => "EEEE",
            "narrow" => "EEEEE",
            _ => "E",
        });
    }
    if let Some(style) = fields.date_style.as_deref() {
        if !date.is_empty() {
            date.push_str(", ");
        }
        date.push_str(locale.date[style_index(Some(style))]);
    } else if fields.month.is_some() || day || year {
        let skeleton = date_pattern(
            fields.month.as_deref(),
            fields.day.as_deref(),
            fields.year.as_deref(),
            locale,
        );
        if !skeleton.is_empty() {
            if !date.is_empty() {
                date.push_str(if wordy && word_order { " " } else { ", " });
            }
            date.push_str(&skeleton);
        }
    }

    let mut time = String::new();
    if let Some(style) = fields.time_style.as_deref() {
        time.push_str(locale.time[style_index(Some(style))]);
    } else if fields.hour.is_some() || fields.minute.is_some() || fields.second.is_some() {
        let hour12 = fields.hour12.unwrap_or(locale.hour12);
        time.push_str(&time_pattern(
            fields.hour.as_deref(),
            fields.minute.as_deref(),
            fields.second.as_deref(),
            hour12,
        ));
    }
    if let Some(zone) = fields
        .time_zone_name
        .as_deref()
        .or(fields.second_zone.as_deref())
    {
        if !time.is_empty() {
            time.push(' ');
        }
        time.push_str(zone_pattern(zone));
    }

    // {1} is the date and {0} the time in a CLDR join pattern. A bare specifier with neither
    // field is the date and the time in the locale's own default style: that is what a
    // DateTimeFormat with no options renders.
    match (date.is_empty(), time.is_empty()) {
        (false, false) => locale.join.replace("{1}", &date).replace("{0}", &time),
        (false, true) => date,
        (true, false) => time,
        (true, true) => locale
            .join
            .replace("{1}", locale.date[2])
            .replace("{0}", locale.time[2]),
    }
}

/// The pattern for month, day and year alone.
///
/// CLDR gives one skeleton per length of the month rather than one for "some of the three", and the
/// order and the separators travel with it. A named month is a word, so it takes the locale's word
/// order; a numeric month keeps its skeleton's punctuation. The year goes where the skeleton's
/// year is: en-US writes "M/d/y", en-GB "dd/MM/y", sv-SE "y-MM-dd".
///
/// When the caller named no month there is no skeleton to follow, and the pattern is built from
/// the fields named in the locale's own order.
fn date_pattern(
    month: Option<&str>,
    day: Option<&str>,
    year: Option<&str>,
    locale: &'static Locale,
) -> String {
    let day_field = match day {
        Some("2-digit") => "dd",
        Some(_) => "d",
        None => "",
    };
    let year_field = match year {
        Some("2-digit") => "yy",
        Some(_) => "y",
        None => "",
    };
    let month = match month {
        Some("2-digit") => "MM",
        Some("numeric") => "M",
        Some("short") => "MMM",
        Some("long") => "MMMM",
        Some("narrow") => "MMMMM",
        Some(_) | None => "",
    };
    if month.is_empty() {
        return match (day_field.is_empty(), year_field.is_empty()) {
            (false, false) => format!("{day_field} {year_field}"),
            (false, true) => day_field.to_string(),
            (true, false) => year_field.to_string(),
            (true, true) => String::new(),
        };
    }
    // Only the three numeric-day skeletons are spelled out here; a named month is a word and is
    // written in the word order the style patterns already show.
    let skeleton = if month.len() > 1 {
        if locale.date[3].starts_with('d') {
            "d MMM y"
        } else {
            "MMM d, y"
        }
    } else {
        match locale.date[3] {
            "dd/MM/y" => "dd/MM/y",
            "dd.MM.yy" => "dd.MM.yy",
            "d/M/yy" => "d/M/yy",
            "y/MM/dd" => "y/MM/dd",
            "y/M/d" => "y/M/d",
            _ => "M/d/y",
        }
    };
    // A field the caller did not name leaves a hole where its letters were, and a hole swallows
    // the punctuation next to it: the skeleton "dd/MM/y" asked for only a day renders "18", not
    // "18/".
    let mut out = String::with_capacity(skeleton.len() + 4);
    let mut run = String::new();
    let mut pending = String::new();
    let mut emitted = false;
    fn flush(
        out: &mut String,
        run: &mut String,
        pending: &mut String,
        emitted: &mut bool,
        month: &str,
        day_field: &str,
        year_field: &str,
    ) {
        let rendered = if run.contains('M') {
            month
        } else if run.contains('d') {
            day_field
        } else if run.contains('y') {
            year_field
        } else {
            ""
        };
        run.clear();
        if rendered.is_empty() {
            return;
        }
        out.push_str(pending);
        pending.clear();
        out.push_str(rendered);
        *emitted = true;
    }
    for character in skeleton.chars() {
        if matches!(character, 'M' | 'd' | 'y') {
            if !run.is_empty() {
                flush(
                    &mut out,
                    &mut run,
                    &mut pending,
                    &mut emitted,
                    month,
                    day_field,
                    year_field,
                );
                if emitted {
                    out.push_str(&pending);
                    pending.clear();
                }
            }
            run.push(character);
        } else if !run.is_empty() {
            flush(
                &mut out,
                &mut run,
                &mut pending,
                &mut emitted,
                month,
                day_field,
                year_field,
            );
            pending.push(character);
        } else if !emitted {
            continue;
        } else {
            pending.push(character);
        }
    }
    flush(
        &mut out,
        &mut run,
        &mut pending,
        &mut emitted,
        month,
        day_field,
        year_field,
    );
    out
}

/// A field asked for as "numeric" is still zero padded: that is what Intl.DateTimeFormat
/// renders for every hour, minute and second, and what a caller assembling an ISO-like time from
/// the parts expects. Only a year drops its padding.
fn time_pattern(
    hour: Option<&str>,
    minute: Option<&str>,
    second: Option<&str>,
    hour12: bool,
) -> String {
    let mut out = String::new();
    if hour.is_some() {
        out.push_str(if hour12 { "hh" } else { "HH" });
    }
    if minute.is_some() {
        if !out.is_empty() {
            out.push(':');
        }
        out.push_str("mm");
    }
    if second.is_some() {
        if !out.is_empty() {
            out.push(':');
        }
        out.push_str("ss");
    }
    if hour12 && hour.is_some() {
        out.push_str(" a");
    }
    out
}

fn zone_pattern(style: &str) -> &'static str {
    match style {
        "long" => "zzzz",
        "longOffset" => "OOOO",
        "short" => "z",
        "shortGeneric" => "v",
        "longGeneric" => "vvvv",
        _ => "O",
    }
}

/// One piece of a formatted value: a field and what it rendered as, or literal text between.
pub(super) struct Part {
    pub kind: &'static str,
    pub value: String,
}

/// Format an instant in a zone, as the whole text and as the parts it is made of.
pub(super) fn render_parts(
    zoned: &ZonedDateTime,
    fields: &Fields,
    locale_tag: &str,
) -> (String, Vec<Part>) {
    let table = lookup(locale_tag);
    let pattern = pattern_for(table, fields);
    let mut parts = Vec::with_capacity(16);
    let text = format_pattern(zoned, &pattern, table, &mut parts);
    (text, parts)
}

/// A literal is merged with the literal before it: a caller reading parts wants the punctuation
/// between two fields as one piece, not as one piece per character.
fn push_literal(parts: &mut Vec<Part>, text: &str) {
    if text.is_empty() {
        return;
    }
    if let Some(last) = parts.last_mut() {
        if last.kind == "literal" {
            last.value.push_str(text);
            return;
        }
    }
    parts.push(Part {
        kind: "literal",
        value: text.to_string(),
    });
}

fn push_field(parts: &mut Vec<Part>, kind: &'static str, value: String) {
    parts.push(Part { kind, value });
}

fn format_pattern(
    zoned: &ZonedDateTime,
    pattern: &str,
    locale: &'static Locale,
    parts: &mut Vec<Part>,
) -> String {
    let mut out = String::with_capacity(pattern.len() * 2);
    let mut literal = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\'' => {
                if chars.peek() == Some(&'\'') {
                    chars.next();
                    literal.push('\'');
                } else {
                    for quoted in chars.by_ref() {
                        if quoted == '\'' {
                            break;
                        }
                        literal.push(quoted);
                    }
                }
            }
            'y' | 'Y' => {
                let width = 1 + consume(&mut chars, character);
                let rendered = year(zoned.year() as i64, width);
                field(&mut out, parts, &mut literal, "year", rendered);
            }
            'M' | 'L' => {
                let width = 1 + consume(&mut chars, character);
                let rendered = month(zoned.month() as i64, width, locale);
                field(&mut out, parts, &mut literal, "month", rendered);
            }
            'd' => {
                let width = 1 + consume(&mut chars, character);
                let rendered = pad(zoned.day() as i64, width);
                field(&mut out, parts, &mut literal, "day", rendered);
            }
            'E' | 'e' | 'c' => {
                let width = 1 + consume(&mut chars, character);
                // day_of_week counts Monday as 1; the tables are Sunday-first.
                let index = (zoned.day_of_week() % 7) as usize;
                let rendered = if width >= 4 {
                    locale.days_wide[index].to_string()
                } else if width == 5 {
                    first_grapheme(locale.days_wide[index]).to_string()
                } else {
                    locale.days_abbr[index].to_string()
                };
                field(&mut out, parts, &mut literal, "weekday", rendered);
            }
            'a' | 'b' | 'B' => {
                consume(&mut chars, character);
                let rendered = if zoned.hour() < 12 {
                    locale.am
                } else {
                    locale.pm
                };
                field(
                    &mut out,
                    parts,
                    &mut literal,
                    "dayPeriod",
                    rendered.to_string(),
                );
            }
            'h' | 'H' | 'K' | 'k' => {
                let width = 1 + consume(&mut chars, character);
                let hour = zoned.hour() as i64;
                let value = match character {
                    'h' => {
                        if hour % 12 == 0 {
                            12
                        } else {
                            hour % 12
                        }
                    }
                    'H' => hour,
                    'K' => hour % 12,
                    _ => {
                        if hour == 0 {
                            24
                        } else {
                            hour
                        }
                    }
                };
                field(&mut out, parts, &mut literal, "hour", pad(value, width));
            }
            'm' => {
                let width = 1 + consume(&mut chars, character);
                let rendered = pad(zoned.minute() as i64, width);
                field(&mut out, parts, &mut literal, "minute", rendered);
            }
            's' => {
                let width = 1 + consume(&mut chars, character);
                let rendered = pad(zoned.second() as i64, width);
                field(&mut out, parts, &mut literal, "second", rendered);
            }
            'S' => {
                let width = 1 + consume(&mut chars, character);
                let rendered = pad(zoned.millisecond() as i64, width.min(3));
                field(&mut out, parts, &mut literal, "fractionalSecond", rendered);
            }
            'z' | 'v' | 'V' | 'O' | 'Z' | 'X' | 'x' => {
                let width = 1 + consume(&mut chars, character);
                let rendered = zone(zoned, width, character);
                field(&mut out, parts, &mut literal, "timeZoneName", rendered);
            }
            other => literal.push(other),
        }
    }
    push_literal(parts, &literal);
    out
}

/// Push one rendered field, flushing the literal text that came before it.
fn field(
    out: &mut String,
    parts: &mut Vec<Part>,
    literal: &mut String,
    kind: &'static str,
    value: String,
) {
    if !literal.is_empty() {
        out.push_str(literal);
        push_literal(parts, literal);
        literal.clear();
    }
    out.push_str(&value);
    push_field(parts, kind, value);
}

fn first_grapheme(text: &str) -> &str {
    let end = text
        .char_indices()
        .nth(1)
        .map(|(at, _)| at)
        .unwrap_or(text.len());
    &text[..end]
}

fn consume(chars: &mut std::iter::Peekable<std::str::Chars>, character: char) -> usize {
    let mut extra = 0;
    while chars.peek() == Some(&character) {
        chars.next();
        extra += 1;
    }
    extra
}

fn year(year: i64, width: usize) -> String {
    if width == 2 {
        format!("{:02}", year.rem_euclid(100))
    } else {
        format!("{year}")
    }
}

fn month(month: i64, width: usize, locale: &'static Locale) -> String {
    let index = (month - 1).clamp(0, 11) as usize;
    match width {
        1 => format!("{month}"),
        2 => format!("{month:02}"),
        3 => locale.months_abbr[index].to_string(),
        5 => first_grapheme(locale.months_wide[index]).to_string(),
        _ => locale.months_wide[index].to_string(),
    }
}

fn pad(value: i64, width: usize) -> String {
    if width >= 2 {
        format!("{value:02}")
    } else {
        format!("{value}")
    }
}

/// A zone as the pattern letter renders it: a name for 'z', a GMT offset for 'O', an ISO offset
/// for 'X'. The CLDR long zone names need a locale's zone display names, which this table does
/// not carry, so those fall back to the identifier.
fn zone(zoned: &ZonedDateTime, width: usize, letter: char) -> String {
    let name = zoned
        .time_zone()
        .identifier()
        .unwrap_or_else(|_| "UTC".to_string());
    if letter == 'X' || letter == 'x' {
        let text = zoned.offset().to_string();
        if text == "+00:00" && letter == 'X' {
            return "Z".to_string();
        }
        if width == 1 {
            return text.trim_end_matches(":00").to_string();
        }
        return text;
    }
    if letter == 'O' {
        let text = zoned.offset().to_string();
        let sign = &text[..1];
        let hours = text[1..3].trim_start_matches('0');
        let hours = if hours.is_empty() { "0" } else { hours };
        let minutes = &text[4..];
        if minutes == "00" {
            return format!("GMT{sign}{hours}");
        }
        return format!("GMT{sign}{hours}:{minutes}");
    }
    let name = name.as_str();
    if width >= 4 {
        name.to_string()
    } else {
        name.rsplit('/').next().unwrap_or(name).replace('_', " ")
    }
}

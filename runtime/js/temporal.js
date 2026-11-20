'use strict';

// Temporal, the language's date and time API. Every value is a host object holding the Rust side
// of it; this file owns the prototypes, the argument shapes and the class names. Arithmetic,
// parsing, calendars and zones are all calls into temporal_rs through __temporal_*.

const HANDLE = Symbol('temporal.handle');

const KINDS = {
  Instant: 'instant',
  PlainDate: 'plain_date',
  PlainTime: 'plain_time',
  PlainDateTime: 'plain_date_time',
  PlainYearMonth: 'plain_year_month',
  PlainMonthDay: 'plain_month_day',
  ZonedDateTime: 'zoned_date_time',
  Duration: 'duration',
};

function brand(value, kind, name) {
  if (__temporal_kind(value) !== kind) {
    throw new TypeError('the receiver is not a Temporal.' + name);
  }
}

function read(self, kind, name, field) {
  brand(self, kind, name);
  const value = __temporal_get(self, field);
  return field === 'epochNanoseconds' ? BigInt(value) : value;
}

function op(self, kind, name, method, args) {
  brand(self, kind, name);
  return __temporal_op(self, method, args);
}

function fields(klass, kind, name, names) {
  for (const field of names) {
    Object.defineProperty(klass.prototype, field, {
      get() {
        return read(this, kind, name, field);
      },
      configurable: true,
    });
  }
}

function tagged(klass, name) {
  Object.defineProperty(klass.prototype, Symbol.toStringTag, {
    value: name,
    configurable: true,
  });
}

function integer(value, name) {
  const number = Number(value);
  if (!Number.isInteger(number)) {
    throw new TypeError(name + ' must be an integer');
  }
  return number;
}

function record(options) {
  return options === undefined || options === null ? {} : options;
}

// ==== Instant ====

class Instant {
  constructor(epochNanoseconds) {
    const digits = typeof epochNanoseconds === 'bigint' ? epochNanoseconds.toString() : String(epochNanoseconds);
    if (!/^-?\d+$/.test(digits)) {
      throw new TypeError('epochNanoseconds must be a BigInt');
    }
    return __temporal_make('instant', { epochNanosecondsText: digits });
  }

  static from(item) {
    return __temporal_from('instant', item, {});
  }

  static fromEpochMilliseconds(milliseconds) {
    return __temporal_make('instant', { epochMilliseconds: integer(milliseconds, 'epochMilliseconds') });
  }

  static fromEpochNanoseconds(nanoseconds) {
    return new Instant(nanoseconds);
  }

  static compare(one, two) {
    return __temporal_compare('instant', one, two, {});
  }

  get epochMilliseconds() {
    return read(this, 'instant', 'Instant', 'epochMilliseconds');
  }

  get epochNanoseconds() {
    return read(this, 'instant', 'Instant', 'epochNanoseconds');
  }

  add(duration) {
    return op(this, 'instant', 'Instant', 'add', { duration });
  }

  subtract(duration) {
    return op(this, 'instant', 'Instant', 'subtract', { duration });
  }

  until(other, options) {
    return op(this, 'instant', 'Instant', 'until', { other, ...record(options) });
  }

  since(other, options) {
    return op(this, 'instant', 'Instant', 'since', { other, ...record(options) });
  }

  round(options) {
    return op(this, 'instant', 'Instant', 'round', record(options));
  }

  equals(other) {
    return op(this, 'instant', 'Instant', 'equals', { other });
  }

  toZonedDateTimeISO(timeZone) {
    return op(this, 'instant', 'Instant', 'toZonedDateTimeISO', { timeZone: String(timeZone) });
  }

  toZonedDateTime(options) {
    const { timeZone, calendar } = record(options);
    return op(this, 'instant', 'Instant', 'toZonedDateTime', {
      timeZone: String(timeZone),
      calendar: calendar === undefined ? undefined : String(calendar),
    });
  }

  toString(options) {
    const { timeZone, ...rest } = record(options);
    return op(this, 'instant', 'Instant', 'toString', {
      ...rest,
      timeZone: timeZone === undefined ? undefined : String(timeZone),
    });
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  valueOf() {
    throw new TypeError('a Temporal.Instant has no primitive value; use equals or compare');
  }
}

// ==== PlainDate ====

class PlainDate {
  constructor(isoYear, isoMonth, isoDay, calendar) {
    return __temporal_make('plain_date', {
      year: integer(isoYear, 'year'),
      month: integer(isoMonth, 'month'),
      day: integer(isoDay, 'day'),
      calendar: calendar === undefined ? undefined : String(calendar),
    });
  }

  static from(item, options) {
    return __temporal_from('plain_date', item, record(options));
  }

  static compare(one, two) {
    return __temporal_compare('plain_date', one, two, {});
  }

  add(duration, options) {
    return op(this, 'plain_date', 'PlainDate', 'add', { duration, ...record(options) });
  }

  subtract(duration, options) {
    return op(this, 'plain_date', 'PlainDate', 'subtract', { duration, ...record(options) });
  }

  with(fields, options) {
    return op(this, 'plain_date', 'PlainDate', 'with', { ...record(fields), ...record(options) });
  }

  until(other, options) {
    return op(this, 'plain_date', 'PlainDate', 'until', { other, ...record(options) });
  }

  since(other, options) {
    return op(this, 'plain_date', 'PlainDate', 'since', { other, ...record(options) });
  }

  equals(other) {
    return op(this, 'plain_date', 'PlainDate', 'equals', { other });
  }

  toString(options) {
    return op(this, 'plain_date', 'PlainDate', 'toString', record(options));
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  toPlainDateTime(time) {
    return op(this, 'plain_date', 'PlainDate', 'toPlainDateTime', { time });
  }

  toPlainYearMonth() {
    return op(this, 'plain_date', 'PlainDate', 'toPlainYearMonth', {});
  }

  toPlainMonthDay() {
    return op(this, 'plain_date', 'PlainDate', 'toPlainMonthDay', {});
  }

  toZonedDateTime(timeZone, time) {
    return op(this, 'plain_date', 'PlainDate', 'toZonedDateTime', { timeZone, time });
  }

  get calendarId() {
    return read(this, 'plain_date', 'PlainDate', 'calendarId');
  }

  get calendar() {
    return new Calendar(this.calendarId);
  }

  valueOf() {
    throw new TypeError('a Temporal.PlainDate has no primitive value; use equals or compare');
  }
}

fields(PlainDate, 'plain_date', 'PlainDate', [
  'year', 'month', 'monthCode', 'day', 'dayOfWeek', 'dayOfYear', 'weekOfYear', 'yearOfWeek',
  'daysInWeek', 'daysInMonth', 'daysInYear', 'monthsInYear', 'inLeapYear', 'era', 'eraYear',
]);

// ==== PlainTime ====

class PlainTime {
  constructor(hour, minute, second, millisecond, microsecond, nanosecond) {
    return __temporal_make('plain_time', {
      hour: hour === undefined ? 0 : integer(hour, 'hour'),
      minute: minute === undefined ? 0 : integer(minute, 'minute'),
      second: second === undefined ? 0 : integer(second, 'second'),
      millisecond: millisecond === undefined ? 0 : integer(millisecond, 'millisecond'),
      microsecond: microsecond === undefined ? 0 : integer(microsecond, 'microsecond'),
      nanosecond: nanosecond === undefined ? 0 : integer(nanosecond, 'nanosecond'),
    });
  }

  static from(item, options) {
    return __temporal_from('plain_time', item, record(options));
  }

  static compare(one, two) {
    return __temporal_compare('plain_time', one, two, {});
  }

  add(duration) {
    return op(this, 'plain_time', 'PlainTime', 'add', { duration });
  }

  subtract(duration) {
    return op(this, 'plain_time', 'PlainTime', 'subtract', { duration });
  }

  with(fields, options) {
    return op(this, 'plain_time', 'PlainTime', 'with', { ...record(fields), ...record(options) });
  }

  until(other, options) {
    return op(this, 'plain_time', 'PlainTime', 'until', { other, ...record(options) });
  }

  since(other, options) {
    return op(this, 'plain_time', 'PlainTime', 'since', { other, ...record(options) });
  }

  round(options) {
    return op(this, 'plain_time', 'PlainTime', 'round', record(options));
  }

  equals(other) {
    return op(this, 'plain_time', 'PlainTime', 'equals', { other });
  }

  toString(options) {
    return op(this, 'plain_time', 'PlainTime', 'toString', record(options));
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  toPlainDateTime(date) {
    return op(this, 'plain_time', 'PlainTime', 'toPlainDateTime', { date });
  }

  toZonedDateTime(date, timeZone) {
    return op(this, 'plain_time', 'PlainTime', 'toZonedDateTime', { date, timeZone });
  }

  valueOf() {
    throw new TypeError('a Temporal.PlainTime has no primitive value; use equals or compare');
  }
}

fields(PlainTime, 'plain_time', 'PlainTime', [
  'hour', 'minute', 'second', 'millisecond', 'microsecond', 'nanosecond',
]);

// ==== PlainDateTime ====

class PlainDateTime {
  constructor(isoYear, isoMonth, isoDay, hour, minute, second, millisecond, microsecond, nanosecond, calendar) {
    return __temporal_make('plain_date_time', {
      year: integer(isoYear, 'year'),
      month: integer(isoMonth, 'month'),
      day: integer(isoDay, 'day'),
      hour: hour === undefined ? 0 : integer(hour, 'hour'),
      minute: minute === undefined ? 0 : integer(minute, 'minute'),
      second: second === undefined ? 0 : integer(second, 'second'),
      millisecond: millisecond === undefined ? 0 : integer(millisecond, 'millisecond'),
      microsecond: microsecond === undefined ? 0 : integer(microsecond, 'microsecond'),
      nanosecond: nanosecond === undefined ? 0 : integer(nanosecond, 'nanosecond'),
      calendar: calendar === undefined ? undefined : String(calendar),
    });
  }

  static from(item, options) {
    return __temporal_from('plain_date_time', item, record(options));
  }

  static compare(one, two) {
    return __temporal_compare('plain_date_time', one, two, {});
  }

  add(duration, options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'add', { duration, ...record(options) });
  }

  subtract(duration, options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'subtract', { duration, ...record(options) });
  }

  with(fields, options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'with', { ...record(fields), ...record(options) });
  }

  until(other, options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'until', { other, ...record(options) });
  }

  since(other, options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'since', { other, ...record(options) });
  }

  round(options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'round', record(options));
  }

  equals(other) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'equals', { other });
  }

  toString(options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'toString', record(options));
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  toPlainDate() {
    return op(this, 'plain_date_time', 'PlainDateTime', 'toPlainDate', {});
  }

  toPlainTime() {
    return op(this, 'plain_date_time', 'PlainDateTime', 'toPlainTime', {});
  }

  withPlainTime(time) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'withPlainTime', { time });
  }

  toZonedDateTime(timeZone, options) {
    return op(this, 'plain_date_time', 'PlainDateTime', 'toZonedDateTime', {
      timeZone,
      ...record(options),
    });
  }

  get calendarId() {
    return read(this, 'plain_date_time', 'PlainDateTime', 'calendarId');
  }

  get calendar() {
    return new Calendar(this.calendarId);
  }

  valueOf() {
    throw new TypeError('a Temporal.PlainDateTime has no primitive value; use equals or compare');
  }
}

fields(PlainDateTime, 'plain_date_time', 'PlainDateTime', [
  'year', 'month', 'monthCode', 'day', 'hour', 'minute', 'second', 'millisecond',
  'microsecond', 'nanosecond', 'dayOfWeek', 'dayOfYear', 'weekOfYear', 'yearOfWeek',
  'daysInWeek', 'daysInMonth', 'daysInYear', 'monthsInYear', 'inLeapYear', 'era', 'eraYear',
]);

// ==== PlainYearMonth ====

class PlainYearMonth {
  constructor(isoYear, isoMonth, calendar, referenceISODay) {
    return __temporal_make('plain_year_month', {
      year: integer(isoYear, 'year'),
      month: integer(isoMonth, 'month'),
      calendar: calendar === undefined ? undefined : String(calendar),
      referenceDay: referenceISODay === undefined ? undefined : integer(referenceISODay, 'referenceISODay'),
    });
  }

  static from(item, options) {
    return __temporal_from('plain_year_month', item, record(options));
  }

  static compare(one, two) {
    return __temporal_compare('plain_year_month', one, two, {});
  }

  add(duration, options) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'add', { duration, ...record(options) });
  }

  subtract(duration, options) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'subtract', { duration, ...record(options) });
  }

  with(fields, options) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'with', { ...record(fields), ...record(options) });
  }

  until(other, options) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'until', { other, ...record(options) });
  }

  since(other, options) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'since', { other, ...record(options) });
  }

  equals(other) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'equals', { other });
  }

  toString(options) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'toString', record(options));
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  toPlainDate(item) {
    return op(this, 'plain_year_month', 'PlainYearMonth', 'toPlainDate', record(item));
  }

  get calendarId() {
    return read(this, 'plain_year_month', 'PlainYearMonth', 'calendarId');
  }

  get calendar() {
    return new Calendar(this.calendarId);
  }

  valueOf() {
    throw new TypeError('a Temporal.PlainYearMonth has no primitive value');
  }
}

fields(PlainYearMonth, 'plain_year_month', 'PlainYearMonth', [
  'year', 'month', 'monthCode', 'daysInMonth', 'daysInYear', 'monthsInYear', 'inLeapYear',
  'referenceDay', 'era', 'eraYear',
]);

// ==== PlainMonthDay ====

class PlainMonthDay {
  constructor(isoMonth, isoDay, calendar, referenceISOYear) {
    return __temporal_make('plain_month_day', {
      month: integer(isoMonth, 'month'),
      day: integer(isoDay, 'day'),
      calendar: calendar === undefined ? undefined : String(calendar),
      referenceYear: referenceISOYear === undefined ? undefined : integer(referenceISOYear, 'referenceISOYear'),
    });
  }

  static from(item, options) {
    return __temporal_from('plain_month_day', item, record(options));
  }

  with(fields, options) {
    return op(this, 'plain_month_day', 'PlainMonthDay', 'with', { ...record(fields), ...record(options) });
  }

  equals(other) {
    return op(this, 'plain_month_day', 'PlainMonthDay', 'equals', { other });
  }

  toString(options) {
    return op(this, 'plain_month_day', 'PlainMonthDay', 'toString', record(options));
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  toPlainDate(item) {
    return op(this, 'plain_month_day', 'PlainMonthDay', 'toPlainDate', record(item));
  }

  get calendarId() {
    return read(this, 'plain_month_day', 'PlainMonthDay', 'calendarId');
  }

  get calendar() {
    return new Calendar(this.calendarId);
  }

  valueOf() {
    throw new TypeError('a Temporal.PlainMonthDay has no primitive value');
  }
}

fields(PlainMonthDay, 'plain_month_day', 'PlainMonthDay', [
  'monthCode', 'day', 'referenceYear',
]);

// ==== ZonedDateTime ====

class ZonedDateTime {
  constructor() {
    throw new TypeError('use Temporal.ZonedDateTime.from, or from an instant and a time zone');
  }

  static from(item, options) {
    return __temporal_from('zoned_date_time', item, record(options));
  }

  static fromEpochMilliseconds(epochMilliseconds, timeZone) {
    return Instant.fromEpochMilliseconds(epochMilliseconds).toZonedDateTimeISO(timeZone);
  }

  static fromEpochNanoseconds(epochNanoseconds, timeZone) {
    const instant = new Instant(epochNanoseconds);
    return op(instant, 'instant', 'Instant', 'toZonedDateTimeISO', { timeZone: String(timeZone) });
  }

  static compare(one, two) {
    return __temporal_compare('zoned_date_time', one, two, {});
  }

  add(duration, options) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'add', { duration, ...record(options) });
  }

  subtract(duration, options) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'subtract', { duration, ...record(options) });
  }

  with(fields, options) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'with', { ...record(fields), ...record(options) });
  }

  until(other, options) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'until', { other, ...record(options) });
  }

  since(other, options) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'since', { other, ...record(options) });
  }

  round(options) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'round', record(options));
  }

  equals(other) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'equals', { other });
  }

  toString(options) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'toString', record(options));
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  toInstant() {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'toInstant', {});
  }

  toPlainDate() {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'toPlainDate', {});
  }

  toPlainTime() {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'toPlainTime', {});
  }

  toPlainDateTime() {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'toPlainDateTime', {});
  }

  withTimeZone(timeZone) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'withTimeZone', { timeZone });
  }

  withPlainTime(time) {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'withPlainTime', { time });
  }

  startOfDay() {
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'startOfDay', {});
  }

  getTimeZoneTransition(direction) {
    if (direction !== 'next' && direction !== 'previous') {
      throw new TypeError('direction must be "next" or "previous"');
    }
    return op(this, 'zoned_date_time', 'ZonedDateTime', 'getTimeZoneTransition', { direction });
  }

  get calendarId() {
    return read(this, 'zoned_date_time', 'ZonedDateTime', 'calendarId');
  }

  get calendar() {
    return new Calendar(this.calendarId);
  }

  get timeZoneId() {
    return read(this, 'zoned_date_time', 'ZonedDateTime', 'timeZoneId');
  }

  get timeZone() {
    return new TimeZone(this.timeZoneId);
  }

  valueOf() {
    throw new TypeError('a Temporal.ZonedDateTime has no primitive value; use equals or compare');
  }
}

fields(ZonedDateTime, 'zoned_date_time', 'ZonedDateTime', [
  'year', 'month', 'monthCode', 'day', 'hour', 'minute', 'second', 'millisecond',
  'microsecond', 'nanosecond', 'dayOfWeek', 'dayOfYear', 'weekOfYear', 'yearOfWeek',
  'daysInWeek', 'daysInMonth', 'daysInYear', 'monthsInYear', 'inLeapYear', 'era', 'eraYear',
  'offset', 'offsetNanoseconds', 'epochMilliseconds', 'epochNanoseconds', 'hoursInDay',
]);

// ==== Duration ====

class Duration {
  constructor(years, months, weeks, days, hours, minutes, seconds, milliseconds, microseconds, nanoseconds) {
    return __temporal_make('duration', {
      years: years === undefined ? 0 : integer(years, 'years'),
      months: months === undefined ? 0 : integer(months, 'months'),
      weeks: weeks === undefined ? 0 : integer(weeks, 'weeks'),
      days: days === undefined ? 0 : integer(days, 'days'),
      hours: hours === undefined ? 0 : integer(hours, 'hours'),
      minutes: minutes === undefined ? 0 : integer(minutes, 'minutes'),
      seconds: seconds === undefined ? 0 : integer(seconds, 'seconds'),
      milliseconds: milliseconds === undefined ? 0 : integer(milliseconds, 'milliseconds'),
      microseconds: microseconds === undefined ? 0 : integer(microseconds, 'microseconds'),
      nanoseconds: nanoseconds === undefined ? 0 : integer(nanoseconds, 'nanoseconds'),
    });
  }

  static from(item) {
    return __temporal_from('duration', item, {});
  }

  static compare(one, two, options) {
    return __temporal_compare('duration', one, two, record(options));
  }

  with(fields) {
    // The fields a caller does not name keep their value, which is what with() promises. They are
    // read one by one because they are getters on the prototype, not own properties.
    const pick = (name) => (fields[name] === undefined ? this[name] : fields[name]);
    return new Duration(
      pick('years'), pick('months'), pick('weeks'), pick('days'), pick('hours'), pick('minutes'),
      pick('seconds'), pick('milliseconds'), pick('microseconds'), pick('nanoseconds'),
    );
  }

  negated() {
    return op(this, 'duration', 'Duration', 'negated', {});
  }

  abs() {
    return op(this, 'duration', 'Duration', 'abs', {});
  }

  add(other) {
    return op(this, 'duration', 'Duration', 'add', { duration: other });
  }

  subtract(other) {
    return op(this, 'duration', 'Duration', 'subtract', { duration: other });
  }

  round(options) {
    return op(this, 'duration', 'Duration', 'round', record(options));
  }

  total(options) {
    const { unit } = record(options);
    return op(this, 'duration', 'Duration', 'total', { unit });
  }

  toString(options) {
    return op(this, 'duration', 'Duration', 'toString', record(options));
  }

  toJSON() {
    return this.toString();
  }

  toLocaleString() {
    return this.toString();
  }

  valueOf() {
    throw new TypeError('a Temporal.Duration has no primitive value');
  }
}

fields(Duration, 'duration', 'Duration', [
  'years', 'months', 'weeks', 'days', 'hours', 'minutes', 'seconds', 'milliseconds',
  'microseconds', 'nanoseconds', 'sign', 'blank',
]);

// ==== Calendar and TimeZone ====

// Both carry only their identifier here. The calendar algorithms that need ICU data for anything
// other than the ISO calendar are not shipped; see docs/temporal.md.
const ID = Symbol('temporal.id');

class Calendar {
  constructor(id) {
    const text = String(id);
    // An unknown calendar is refused here rather than at the first arithmetic on it.
    __temporal_make('plain_date', { year: 1970, month: 1, day: 1, calendar: text });
    this[ID] = text;
  }

  get id() {
    return this[ID];
  }

  toString() {
    return this[ID];
  }

  equals(other) {
    return other instanceof Calendar && other.id === this[ID];
  }
}

class TimeZone {
  constructor(id) {
    const text = String(id);
    __temporal_zone(text, 'id', '');
    this[ID] = text;
  }

  get id() {
    return this[ID];
  }

  toString() {
    return this[ID];
  }

  equals(other) {
    return other instanceof TimeZone && other.id === this[ID];
  }

  getOffsetNanosecondsFor(instant) {
    const digits = instant.epochNanoseconds.toString();
    return BigInt(__temporal_zone(this[ID], 'getOffsetNanosecondsFor', digits));
  }
}

// ==== Now ====

const Now = {
  instant() {
    return __temporal_now('instant', undefined);
  },
  timeZoneId() {
    return __temporal_now('time_zone', undefined);
  },
  zonedDateTimeISO(timeZone) {
    return __temporal_now('zoned_date_time', timeZone === undefined ? undefined : String(timeZone));
  },
  plainDateTimeISO(timeZone) {
    return __temporal_now('plain_date_time', timeZone === undefined ? undefined : String(timeZone));
  },
  plainDateISO(timeZone) {
    return __temporal_now('plain_date', timeZone === undefined ? undefined : String(timeZone));
  },
  plainTimeISO(timeZone) {
    return __temporal_now('plain_time', timeZone === undefined ? undefined : String(timeZone));
  },
};

tagged(Instant, 'Temporal.Instant');
tagged(PlainDate, 'Temporal.PlainDate');
tagged(PlainTime, 'Temporal.PlainTime');
tagged(PlainDateTime, 'Temporal.PlainDateTime');
tagged(PlainYearMonth, 'Temporal.PlainYearMonth');
tagged(PlainMonthDay, 'Temporal.PlainMonthDay');
tagged(ZonedDateTime, 'Temporal.ZonedDateTime');
tagged(Duration, 'Temporal.Duration');
tagged(Calendar, 'Temporal.Calendar');
tagged(TimeZone, 'Temporal.TimeZone');

// The prototypes, so a value built on the Rust side lands on the class that made it.
const classes = { Instant, PlainDate, PlainTime, PlainDateTime, PlainYearMonth, PlainMonthDay, ZonedDateTime, Duration };
for (const [name, klass] of Object.entries(classes)) {
  __temporal_register(KINDS[name], klass.prototype);
}

const Temporal = {
  Instant,
  PlainDate,
  PlainTime,
  PlainDateTime,
  PlainYearMonth,
  PlainMonthDay,
  ZonedDateTime,
  Duration,
  Now,
  Calendar,
  TimeZone,
};

// Temporal is a language global, not a module: a script asks for it the way it asks for Math.
globalThis.Temporal = Temporal;
globalThis.__temporal = { Temporal, default: Temporal };

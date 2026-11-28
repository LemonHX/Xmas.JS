
// Intl.DateTimeFormat, and the Date methods that delegate to it.
//
// The heavy part -- the zone rules, the calendar and the CLDR patterns -- is in Rust behind
// __intl_format. What is here is the part the specification puts in JS: reading the options,
// negotiating the locale list, resolving the defaults, and splitting the result into parts.

(function () {
  'use strict';

  const zoneNamesJson = __intl_zone_names();
  const allZones = JSON.parse(zoneNamesJson);

  // --- locale negotiation ---------------------------------------------------

  function wellFormed(tag) {
    return typeof tag === 'string' && /^[A-Za-z]{2,8}(-[A-Za-z0-9]{1,8})*$/.test(tag);
  }

  function canonicalTag(tag) {
    const [language, ...rest] = tag.toLowerCase().split('-');
    const parts = [language];
    for (const part of rest) {
      if (part.length === 4 && /^[a-z]+$/.test(part)) {
        parts.push(part[0].toUpperCase() + part.slice(1));
      } else if (part.length === 2) {
        parts.push(part.toUpperCase());
      } else {
        parts.push(part);
      }
    }
    return parts.join('-');
  }

  // A locale list is anything from a string to an array of strings; each entry may carry
  // -u- extensions, which are dropped here because nothing in this implementation reads them.
  function localeList(locales) {
    let list;
    if (locales === undefined || locales === null) return [];
    if (typeof locales === 'string') list = [locales];
    else if (Array.isArray(locales)) list = locales.slice();
    else list = [String(locales)];
    return list.map((entry) => {
      const tag = String(entry);
      if (!wellFormed(tag)) {
        const error = new RangeError('Incorrect locale information provided');
        error.name = 'RangeError';
        throw error;
      }
      return canonicalTag(tag.split('-u-')[0]);
    });
  }

  // The default locale of this runtime: the process environment when it names one, en-US
  // otherwise. There is no per-host locale lookup without a system API.
  function defaultLocale() {
    try {
      const value = process.env.LC_ALL || process.env.LC_TIME || process.env.LANG;
      if (value) {
        const tag = value.split('.')[0].replace('_', '-');
        if (wellFormed(tag)) return canonicalTag(tag);
      }
    } catch {}
    return 'en-US';
  }

  function supportedLocalesOf(list) {
    return list.slice();
  }

  // --- option reading -------------------------------------------------------

  const WEEKDAYS = ['narrow', 'short', 'long'];
  const MONTHS = ['numeric', '2-digit', 'narrow', 'short', 'long'];
  const NUMERIC = ['numeric', '2-digit'];
  const STYLES = ['full', 'long', 'medium', 'short'];
  const ZONE_STYLES = ['short', 'long', 'shortOffset', 'longOffset', 'shortGeneric', 'longGeneric'];

  function readString(options, name, allowed) {
    const value = options[name];
    if (value === undefined) return undefined;
    const text = String(value);
    if (!allowed.includes(text)) {
      const error = new RangeError('Value ' + text + ' out of range for option ' + name);
      error.name = 'RangeError';
      throw error;
    }
    return text;
  }

  function readBool(options, name) {
    const value = options[name];
    return value === undefined ? undefined : Boolean(value);
  }

  function readOptions(options) {
    if (options === undefined || options === null) options = {};
    if (typeof options !== 'object' && typeof options !== 'function') {
      const error = new TypeError('Options must be an object');
      error.name = 'TypeError';
      throw error;
    }
    const dateStyle = readString(options, 'dateStyle', STYLES);
    const timeStyle = readString(options, 'timeStyle', STYLES);
    if (dateStyle !== undefined || timeStyle !== undefined) {
      for (const name of ['weekday', 'era', 'year', 'month', 'day', 'hour', 'minute', 'second', 'fractionalSecondDigits', 'dayPeriod', 'timeZoneName']) {
        if (options[name] !== undefined) {
          const error = new TypeError('Option ' + name + ' can not be set together with dateStyle or timeStyle');
          error.name = 'TypeError';
          throw error;
        }
      }
    }
    return {
      dateStyle,
      timeStyle,
      weekday: readString(options, 'weekday', WEEKDAYS),
      year: readString(options, 'year', NUMERIC),
      month: readString(options, 'month', MONTHS),
      day: readString(options, 'day', NUMERIC),
      hour: readString(options, 'hour', NUMERIC),
      minute: readString(options, 'minute', NUMERIC),
      second: readString(options, 'second', NUMERIC),
      hour12: readBool(options, 'hour12'),
      hourCycle: readString(options, 'hourCycle', ['h11', 'h12', 'h23', 'h24']),
      timeZone: options.timeZone === undefined ? undefined : String(options.timeZone),
      timeZoneName: readString(options, 'timeZoneName', ZONE_STYLES),
      calendar: options.calendar === undefined ? undefined : String(options.calendar),
      numberingSystem: options.numberingSystem === undefined ? undefined : String(options.numberingSystem),
    };
  }

  // The fields a resolved formatter carries, in the shape __intl_format reads.
  // A 2-digit hour or minute means zero padding; "numeric" on an hour means none, which is what
  // CLDR calls a single-letter hour field. The style patterns carry their own widths.
  function width(name, value) {
    if (value === '2-digit') return '2-digit';
    return 'numeric';
  }

  function fieldOptions(requested, locale, timeZone) {
    const hour12 = requested.hour12;
    return {
      locale,
      timeZone,
      month: requested.month,
      day: requested.day,
      year: requested.year,
      hour: requested.hour,
      minute: requested.minute,
      second: requested.second,
      hour12: requested.hour12,
      weekday: requested.weekday,
      timeZoneName: requested.timeZoneName,
      dateStyle: requested.dateStyle,
      timeStyle: requested.timeStyle,
    };
  }

  class DateTimeFormat {
    #options;
    #resolved;

    constructor(locales, options) {
      const list = localeList(locales);
      const requested = readOptions(options);
      const locale = list.length ? list[0] : defaultLocale();
      const timeZone = requested.timeZone === undefined ? systemZone() : canonicalZone(requested.timeZone);
      this.#options = fieldOptions(requested, locale, timeZone);
      this.#resolved = resolveOptions(requested, locale, timeZone);
    }

    get format() {
      const self = this;
      return function format(date) {
        const time = toEpoch(date);
        if (time === null) return 'Invalid Date';
        return JSON.parse(__intl_format(self.#options.timeZone, time, self.#options)).text;
      };
    }

    formatToParts(date) {
      const time = toEpoch(date);
      if (time === null) {
        const error = new RangeError('Invalid time value');
        error.name = 'RangeError';
        throw error;
      }
      return JSON.parse(__intl_format(this.#options.timeZone, time, this.#options)).parts;
    }

    formatRange(from, to) {
      const a = toEpoch(from);
      const b = toEpoch(to);
      if (a === null || b === null) {
        const error = new RangeError('Invalid time value');
        error.name = 'RangeError';
        throw error;
      }
      if (a > b) {
        const error = new RangeError('Start date is after end date');
        error.name = 'RangeError';
        throw error;
      }
      const left = JSON.parse(__intl_format(this.#options.timeZone, a, this.#options)).text;
      const right = JSON.parse(__intl_format(this.#options.timeZone, b, this.#options)).text;
      return left + ' – ' + right;
    }

    formatRangeToParts(from, to) {
      const a = toEpoch(from);
      const b = toEpoch(to);
      if (a === null || b === null) {
        const error = new RangeError('Invalid time value');
        error.name = 'RangeError';
        throw error;
      }
      if (a > b) {
        const error = new RangeError('Start date is after end date');
        error.name = 'RangeError';
        throw error;
      }
      const parts = this.formatToParts(a);
      parts.push({ type: 'literal', value: ' – ', source: 'shared' });
      for (const part of this.formatToParts(b)) parts.push({ ...part, source: 'endRange' });
      return parts;
    }

    resolvedOptions() {
      return Object.assign({}, this.#resolved);
    }

    get [Symbol.toStringTag]() {
      return 'Intl.DateTimeFormat';
    }
  }

  // --- zones ----------------------------------------------------------------

  // The compiled tzdb is the list of zones that resolve; anything else is a RangeError, which is
  // what the specification asks for and what a caller depending on typo detection expects.
  function canonicalZone(id) {
    const text = String(id);
    if (text === 'UTC' || text.toLowerCase() === 'utc' || allZones.includes(text)) return text;
    const error = new RangeError('Invalid time zone specified: ' + text);
    error.name = 'RangeError';
    throw error;
  }

  function systemZone() {
    const value = process.env.TZ;
    if (value) {
      try {
        return canonicalZone(value);
      } catch {}
    }
    return 'UTC';
  }

  // --- times ----------------------------------------------------------------

  // A Date, a number, or anything with a valueOf: the epoch milliseconds, or null for an invalid
  // Date, which formats as "Invalid Date" rather than throwing.
  function toEpoch(value) {
    if (value === undefined) return Date.now();
    if (typeof value === 'number') return Number.isFinite(value) ? value : null;
    if (value instanceof Date) {
      const time = value.getTime();
      return Number.isNaN(time) ? null : time;
    }
    // A string is not coerced: Number("2024") would silently mean 2024 milliseconds. The
    // specification takes a number or a Date, and a host handing over a value it built itself
    // expects a string to be rejected rather than turned into a date in 1970.
    if (typeof value !== 'object') return null;
    const number = Number(value);
    return Number.isFinite(number) ? number : null;
  }

  function resolveOptions(requested, locale, timeZone) {
    const resolved = {
      locale,
      calendar: 'gregory',
      numberingSystem: 'latn',
      timeZone,
    };
    if (requested.dateStyle !== undefined) resolved.dateStyle = requested.dateStyle;
    if (requested.timeStyle !== undefined) resolved.timeStyle = requested.timeStyle;
    for (const name of ['year', 'month', 'day', 'weekday', 'hour', 'minute', 'second', 'timeZoneName']) {
      if (requested[name] !== undefined) resolved[name] = requested[name];
    }
    // With no date or time field at all the formatter falls back to the locale's short date and
    // short time. A style reports itself and no fields, and a style on one side does not fill in
    // fields on the other: { dateStyle: "short" } reports no time at all.
    const hasDate = requested.weekday !== undefined || requested.year !== undefined || requested.month !== undefined || requested.day !== undefined || requested.dateStyle !== undefined;
    const hasTime = requested.hour !== undefined || requested.minute !== undefined || requested.second !== undefined || requested.timeStyle !== undefined;
    if (!hasDate && !hasTime) {
      resolved.year = 'numeric';
      resolved.month = 'numeric';
      resolved.day = 'numeric';
      resolved.hour = 'numeric';
      resolved.minute = 'numeric';
      resolved.second = 'numeric';
    }
    const hasHour = requested.hour !== undefined || requested.minute !== undefined || requested.second !== undefined || requested.timeStyle !== undefined || (!hasDate && !hasTime);
    if (hasHour) {
      const hour12 = requested.hour12 === undefined ? defaultHour12(locale) : requested.hour12;
      resolved.hour12 = hour12;
      resolved.hourCycle = requested.hourCycle === undefined ? (hour12 ? 'h12' : 'h23') : requested.hourCycle;
    }
    return resolved;
  }

  function defaultHour12(locale) {
    const language = String(locale).toLowerCase().split('-')[0];
    return language === 'en' || language === 'ar' || language === 'ko' || language === 'zh' ? true : false;
  }

  // --- the Date prototype ---------------------------------------------------

  function patchDate() {
    const proto = Date.prototype;
    if (!proto || proto.toLocaleString.xmas) return;
    const methods = {
      toLocaleString: undefined,
      toLocaleDateString: { year: 'numeric', month: 'numeric', day: 'numeric' },
      toLocaleTimeString: { hour: 'numeric', minute: 'numeric', second: 'numeric' },
    };
    for (const [name, forced] of Object.entries(methods)) {
      const original = proto[name];
      const replacement = function (locales, options) {
        if (this instanceof Date && Number.isNaN(this.getTime())) return 'Invalid Date';
        const merged = Object.assign({}, forced === undefined ? {} : forced, options === undefined ? {} : options);
        // Built directly rather than through a DateTimeFormat instance: the patch has to work
        // for a Date, and a Date coerced to a string would carry the local zone.
        const list = localeList(locales);
        const requested = readOptions(merged);
        const locale = list.length ? list[0] : defaultLocale();
        const timeZone = requested.timeZone === undefined ? systemZone() : canonicalZone(requested.timeZone);
        const fields = fieldOptions(requested, locale, timeZone);
        const time = this.getTime();
        if (Number.isNaN(time)) return 'Invalid Date';
        return JSON.parse(__intl_format(timeZone, time, fields)).text;
      };
      replacement.xmas = true;
      Object.defineProperty(replacement, 'name', { value: name });
      Object.defineProperty(replacement, 'length', { value: 0 });
      proto[name] = replacement;
      void original;
    }
  }

  // --- the namespace --------------------------------------------------------

  const Intl = {
    DateTimeFormat,
    supportedValuesOf(key) {
      const text = String(key);
      if (text === 'timeZone') return allZones.slice();
      if (text === 'calendar') return ['gregory'];
      if (text === 'numberingSystem') return ['latn'];
      if (text === 'collation') return [];
      if (text === 'currency') return [];
      if (text === 'unit') return [];
      const error = new RangeError('Invalid key: ' + text);
      error.name = 'RangeError';
      throw error;
    },
    getCanonicalLocales(locales) {
      return localeList(locales);
    },
  };
  Intl.DateTimeFormat.prototype.constructor = DateTimeFormat;
  Intl.DateTimeFormat.supportedLocalesOf = function (locales) {
    return supportedLocalesOf(localeList(locales));
  };
  Object.defineProperty(Intl, Symbol.toStringTag, { value: 'Intl' });

  globalThis.Intl = Intl;
  patchDate();
})();

# Temporal

Temporal is stage 4 and ships in Firefox 139, Chrome 144 and Node 26, so it is a language global
here, not a module: a script asks for `Temporal` and gets it, the same way it asks for `Math`.
The module alias `temporal` exports the same object, which is only there so a test can import it.

## Why temporal_rs

The arithmetic, the parsers, the calendars and the zone rules are all
[temporal_rs](https://github.com/boa-dev/temporal), the crate V8, Boa and Kiesel embed. What it buys
this runtime:

- No C, no system library. Zone rules come from the compiled tzdb (`compiled_data`), and the
  `sys-local` default feature is off, which is what would have read the host's `/etc/localtime`
  through `iana-time-zone` and, on a mac, linked `core-foundation-sys`.
- No async runtime. Nothing in the dependency tree is tokio.
- It is cheap: 52 crates, and a stripped fat-LTO binary grows by 380 KB. `PlainDate.add` costs 24 ns,
  `ZonedDateTime.from` with a zone name 218 ns (M5, release).

## The seam

A Temporal value is a JavaScript object with a Rust value inside it, made by the engine's
`new_host_object` and read back with `host_data`. There is no handle table and no lifetime to
manage: when the object is collected the value goes with it.

`js/temporal.js` owns the eight classes, the accessors and the argument shapes. `modules/temporal.rs`
owns everything that needs a calendar or a clock. Between them there are nine host functions:

| function | what it does |
| --- | --- |
| `__temporal_register(kind, prototype)` | the prelude hands over each class prototype once, so a value built in Rust lands on the right class |
| `__temporal_make(kind, fields)` | a constructor: numbers in an object, a value out |
| `__temporal_from(kind, source, options)` | `from`: a string, another Temporal value, or a record of fields |
| `__temporal_get(value, name)` | every accessor, one function |
| `__temporal_op(value, name, args)` | every method that returns a value, one function |
| `__temporal_compare(kind, one, two, options)` | the static `compare` on each class |
| `__temporal_now(kind, timeZone)` | `Temporal.Now`, from the system clock |
| `__temporal_zone(id, name, instant)` | the two TimeZone methods that need zone rules |
| `__temporal_kind(value)` | the brand check, so a method called on the wrong receiver throws |

Two shapes are worth knowing. Options (`largestUnit`, `roundingMode`, `overflow`, `disambiguation`, ...) are read
in Rust off the plain object the prelude passes, so no option surface is duplicated in JavaScript.
Epoch nanoseconds cross the boundary as decimal text and become a BigInt in JavaScript, because the
engine's bigint type is reachable only through its own constructor.

## What is not here

- `toLocaleString` is `toString`. This runtime ships no ICU data, so a locale-aware format would
  be invented rather than computed.
- The `Calendar` object carries its identifier and nothing else. The calendar protocol
  (`dateFromFields`, `year`, `month`, `daysInMonth`, ...) is not implemented, and the same for
  `TimeZone` beyond `id` and `getOffsetNanosecondsFor`.
- Because of that, `with`, `PlainYearMonth.toPlainDate` and `PlainMonthDay.toPlainDate` rebuild a value from
  ISO fields. For a calendar whose year does not start in January, or one with thirteen months, the
  month they pick is the ISO one.
- `relativeTo` is not read: `Duration.round` and `Duration.total` take no reference point, so a
  duration with both calendar and time units rounds as if the reference were zero.
- `Temporal.Now.timeZoneId()` answers `UTC`. The host's zone is not read, for the same reason
  `node:os` reports nothing about the host: there is no permission model, so the runtime does not
  hold facts about the machine. Pass a zone in, or set one through the application.

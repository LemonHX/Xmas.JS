# path

Copied from LLRT (`modules/llrt_path/src/lib.rs`), not rewritten. The algorithms
are the fiddly part and they were already right: dot segments, UNC roots, drive
letters, trailing separators, suffixes, the Windows/POSIX split. About 620 of the
964 lines are plain Rust functions; only the binding layer at the bottom was
rquickjs.

## What changed in the copy

Three signatures, because this engine has no destructuring of Rust types into JS:

- `join(Rest<String>)` and `resolve(Rest<String>)` take `&[String]`. The JS side
  joins its arguments with NUL, which a path may not contain, so a variadic call
  stays one host call.
- `format(Object)` takes five `&str` and `parse(Ctx, String) -> Object` returns a
  JSON string that JS parses. Objects go over the boundary as JSON in this
  runtime; there is no way to build a JS object from Rust without one.
- Errors are `PathError` (a `String` plus `From<io::Error>`) instead of rquickjs'
  error type, and cross the boundary as exceptions.

Everything else, including the 10 unit tests that came with it, is unchanged and
passes as-is.

## Shape

`path` is Node's object: the eleven functions, `sep`, `delimiter`, plus
`toNamespacedPath` (identity) and `matchesGlob` (translated to a regexp in JS).
Only the platform's own flavour exists: `path.posix` and `path.win32` both point
at it rather than one of them lying. Splitting into two real flavours means
turning LLRT's `cfg(windows)` branches into a runtime switch, which is a
rewrite, not a copy; done the day something needs it.

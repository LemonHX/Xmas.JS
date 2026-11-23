# Remote imports

An import written as a URL — `import { rollup } from 'https://unpkg.com/@rollup/browser'` — is
how browsers and Bun and Deno spell "this code is over there". Today it does not work here: the
specifier falls into `resolve`'s bare branch, and the error says the runtime looked for a package
named `https:/unpkg.com/...` under node_modules. That message is honest and useless.

This document fixes the shape before the code exists.

## What a URL import is

A URL is a module identity, not a package. That has three consequences worth stating plainly:

- It never touches node_modules, and it has no version range. The URL is the version.
- Two imports of the same URL are the same module, whether or not they were written in the same
  file.
- The bytes come from the network, once. A second realm that imports the same URL reads what the
  first one already fetched.

## Resolution

`resolve.rs` learns one predicate: does the specifier have a scheme. If it starts with
`http://`, `https://` or `data:`, it is a URL and is returned unchanged as its own module name.

A **referrer** that is a URL is the second half of the same rule. `./y.js` written inside
`https://a/x.js` resolves to `https://a/y.js`, because that is what a base URL means and because a
remote module graph that cannot import its own neighbours is not a module graph. So the check is
two-sided: a URL referrer joins URLs; a file referrer resolves against the filesystem as it does
now.

Everything below that stays as it is: relative and absolute paths, bare specifiers, package
lookup. A URL cannot be relative to a file and cannot name a package, so the scheme check goes
first and nothing else changes.

### Base URL joining

Joining is RFC 3986 resolution, not string concatenation. `../` has to climb, `?` has to replace
the query rather than append, and a root-relative `/lib/x.js` has to keep the origin. A tiny
resolver written for these three cases would be wrong on the fourth, so the joining uses an
existing URL implementation and is not hand-rolled.

`data:` is the same mechanism with the body already in hand — no network, no cache — so it costs
almost nothing to accept it in the same breath.

### Sync at the point the engine asks

`ModuleLoader::normalize` and `ModuleLoader::load` are synchronous. The engine calls them in the
middle of its own evaluation and neither can be suspended. This is a hard constraint of the
engine, not a choice, and it decides the whole design:

- **normalize** stays a pure function of the specifier. It answers the URL itself.
- **load** must produce bytes. If they are not cached yet, it blocks *the realm thread* while the
  network answers.

A background fetch that resolves on a later turn cannot work here: the engine has already
committed to loading this module now, and there is no way to hand it back a promise.

## Cache

Bytes are cached on disk, under `.xmas/remote/`, because:

- A dev loop that re-downloads rollup on every start is not a dev loop.
- The network is not always there. A cached module still loads.
- A process-lifetime memory cache dies with the process, and the runtime starts often.

The file name is the URL, hashed — ``.xmas/remote/<sha256 of url>` — with the media type kept in a
small sidecar (`.xmas/remote/<hash>.type`) so the loader knows whether it has ESM or wasm without
re-reading the body.

Hash rather than the URL path itself: a URL carries `?`, `#` and slashes, which are not a path on
every filesystem the runtime targets, and a URL is not a filename.

Nothing here expires. A cached URL is used until someone deletes `.xmas/remote`. That is the same
bargain `node_modules` makes and the same one `xmas install` already relies on; putting a TTL on it
would make the same program produce different results on different days.

## What the body can be

Two kinds, decided by media type:

- `application/wasm` → the module is wasm. `instantiateStreaming` and `compileStreaming` already
  read from `fetch`; a wasm URL import is the same bytes through the same compiler.
- anything else → JavaScript, and it is compiled the way every other module is: oxc, TypeScript
  stripped if the type says so, ESM throughout.

The TypeScript question is settled by media type, not by the URL's last dot: unpkg serves `.ts`
with `text/plain`, and guessing from the extension is how a bundler pretends a text file is code.

## Failures

Each of these is a `TypeError` at import, with the URL in the message:

- the request failed (DNS, TLS, connection refused): say the URL and the transport's reason
- the response was not 2xx: say the URL and the status
- the body was not valid wasm or did not compile as a module: say the URL and the compiler's reason

A failure is never cached. A 404 today can be a file tomorrow, and caching the failure would make
the runtime refuse a URL that works.

## Security

There is none, and this document is not going to pretend otherwise. A URL import is an outbound
request the program wrote; `fetch` already makes any outbound request the program writes. There is
no permission model in this runtime — by decision, not by oversight — so a URL import does not get
a private allowlist that `fetch` does not have.

The one thing that is *not* done: a URL import is not resolved from a relative path inside the
remote body. A module fetched from `https://a/x.js` that imports `./y.js` does resolve against
`https://a/x.js` — that is the base URL rule, and it is the same rule browsers use. It is not
relative resolution against the *local* file that wrote the import.

## Deviations from Node

Node does not load from URLs at all, so there is nothing to deviate from: a URL specifier there is
an `ERR_UNSUPPORTED_ESM_URL_SCHEME`. This runtime follows Bun and the browser instead. Worth
writing down because "we match Node" is the standing rule for the module work, and this is an
exception someone will otherwise file as a bug.

## What this does not include

- no `import 'node:...'` from a remote body resolving to a native module beyond what is already
  installed (a remote body sees the same module table as a local one — that is not new)
- no HTTP cache headers, no ETag, no revalidation
- no subresource integrity, no signature checking

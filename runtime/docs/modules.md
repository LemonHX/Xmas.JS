# Modules

A served app is a module graph, not a single file. This is the whole of the rule that turns the
string in an `import` into a file, and what it does not do.

## What the engine asks for

Two questions per import, both answered by `runtime/src/host/modules.rs`:

```rust
trait ModuleLoader {
    fn normalize(&mut self, referrer: &str, specifier: &str, ..) -> Result<String, _>;
    fn load(&mut self, name: &str, ..) -> Result<ModuleSource, _>;
}
```

`referrer` is the path of the module that wrote the import, `normalize` turns the specifier into
the name the module is known by (here: its canonical path), and `load` answers that name with
source. The source is compiled by `runtime/src/compile.rs` — TS and TSX through oxc, plain JS
passed through — and the result is cached in the realm's table, so one file is read and compiled
once per realm.

A module the engine defines itself (everything this runtime ships, see `runtime/src/modules`) is
in the engine's own registry. `crate::modules::define_name` records those names in the table, so
`normalize` leaves them alone: they are never looked for on disk.

## What a specifier resolves to

`crate::resolve::resolve(referrer, specifier)`, in this order.

**Relative** — `./x`, `../x`, `.`, `..` — resolves against the directory of the file that wrote it.

**Absolute** — `/x`, `C:/x` — is used as written.

**Bare** — anything else — is a package: it is looked for in `node_modules` of the referrer's
directory and then in each directory above it, the way node walks up.

A bare specifier naming a module this runtime ships (`fs`, `path`, `node:fs`, `node:path`) is in
the table already and wins before any of this runs.

The modules with no node counterpart carry the runtime's own prefix, so a package cannot claim the
name: `xmas:server` (`serve`), `xmas:websocket` (`upgradeWebSocket`, `WebSocket`) and `xmas:shell`
(the `$` helpers). `cloudflare:sockets` and `sockets` are the same module: the name is the
proposal's.

### Probing

Once a candidate path is in hand, the first of these that exists wins:

1. the file itself,
2. a directory: its `package.json` entry, then `index` in the extension list below,
3. for a `.js`/`.jsx`/`.mjs`/`.cjs` specifier: the same name as `.ts`/`.tsx`/`.mts`/`.cts`
   (how a project written in types spells its own source),
4. the name plus `.ts`, `.tsx`, `.js`, `.jsx`, `.mts`, `.mjs`, in that order.

TypeScript first: this runtime compiles types, so a project that writes them does not say so in
every import.

### Packages

A package's entry, in order:

- `exports`: a string, `{".": ...}` for the root, `{"./sub": ...}` for a subpath, and inside those
  the conditions `import`, `node`, `default`, `require`, in that order.
- `module`, then `main`.
- `index` in the extension list.

Resolution stops at the first `node_modules` that has the package; there is no hoisting beyond
walking up, and no reading of the referrer's own `package.json`.

## Failures

An import that cannot be resolved fails the module graph, which fails the worker, which fails the
start:

```
cannot find './nope.ts' from /srv/app.ts
cannot find package 'nowhere' from /srv/app.ts (no node_modules with it)
'node:http' is not a module this runtime ships (see docs/modules.md)
```

All three are thrown while linking, so nothing of the app runs. `runtime/docs/server.md` says how a
start that fails is reported.

Every import is resolved, including one whose binding nothing reads: a side-effect import is a
module that has to run, and an unread binding is still a package to find. (TypeScript's own default
erases those, which in a runtime means a top level that silently never runs; only `import type` is
removed here.)

## A package nobody installed

A bare specifier that resolves nowhere is not an error at first: `import` is asking for that
package, so the loader asks the package manager for it, the package appears in `node_modules`, and
the specifier is resolved a second time. The two halves are the same process — the package manager
is a library crate the runtime depends on, not a CLI it shells out to.

```
$ xmas serve app.ts
xmas: installing nanoid for 'nanoid'
listening on http://0.0.0.0:3000 with 8 workers
```

What that does and does not do:

- The package and its dependencies are installed into the nearest `package.json` above the file that
  imported it, with install scripts run, exactly as `xmas install` would.
- `package.json` is **not** changed. An import is not a decision about what the project depends on;
  when it should be recorded, `xmas add` is the command for that.
- One attempt per package per realm: a package that fails to install is not retried for every file
  that mentions it, and the failure that surfaces is the install's own reason.
- The second load costs a directory lookup, and nothing leaves the machine.

## What this does not do

- No import maps, no `tsconfig` `paths`, no aliases of any kind. A specifier is a path or a package.
- No `package.json` `imports` field (`#internal` specifiers).
- No wildcard subpath exports (`"./*": "./src/*.js"`) and no custom conditions.
- No `.json`, `.node` or `.wasm` imports: the loader hands back JavaScript and TypeScript source.
- No query or fragment on a specifier (`./x.ts?v=2`): it is a file name here, not a URL.
- No symlinked module identity: paths are canonicalized, so a file reached two ways is one module.
  That is what a realm wants (one instance of each module) and it does not match node, where the
  two are separate.
- No watching: the graph is read once per realm at startup. Editing a file needs a restart.

## require

CommonJS works by a static scan, in `runtime/src/cjs.rs`. The engine runs modules only and a host function
cannot load a module while code is running, so `require()` cannot wait for its file. Instead oxc reads each
file before it runs, finds every `require("literal")`, and rewrites the file into a module that imports those
targets first.

A file gets the full shell (`require`, `module`, `exports`, `__filename`, `__dirname`, `this === module.exports`,
top-level `return`) when it has no `import`/`export` and uses one of those names without declaring it, or is a
`.cjs`. A module that calls a free `require` gets only the function.

Resolution for `require` differs from `import`: conditions are `node`, `require`, `default`; the entry field is `main`;
`.cjs`, `.json` and `index.*` are probed. A bare name nobody installed still goes to the package manager.

What it does not do:

- A computed specifier (`require(name)`) throws `MODULE_NOT_FOUND` and says why.
- Every static require runs before the file does, even one inside an `if`. A cycle sees `undefined` for the
  other side's exports, because dependencies run before the file's own body.
- The body is module code, so it is strict: `with`, octal literals and `arguments.callee` do not parse.
- `require.resolve`, `require.cache` and `module.parent` do not exist.

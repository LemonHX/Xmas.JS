# `$`: running a command

`$` is a global, the way zx makes it a global. The same objects are also on this runtime's own
object, so a script can say where they come from:

    const { $, cd, glob } = Xmas.shell;

    const head = await $`git rev-parse --short HEAD`;
    console.log(head.stdout.trim());

There is no `'zx'` module and never will be: that name is Google's, and this runtime is not
Google. `Xmas.shell` is the whole surface, and the globals are the same objects.

## A template is a command line, not an argv. Everything between the backticks is the line as the shell
sees it: `| grep foo`, `&&`, redirection and globs all work, because the platform's own shell parses
it (`/bin/sh -c` on unix, `cmd /C` on windows). Interpolated values are quoted for that shell, so a
value with spaces, quotes or a semicolon goes in as one word and nothing else. `$.raw(text)` is for the
pieces quoting would break: a pipe, a redirection, a name the shell already knows.

`$.prefix` and `$.postfix` wrap every line, which is how zx recommends setting up a toolchain for the
commands that follow:

    $.prefix += 'export PATH="$HOME/.local/bin:$PATH";'

## What a command answers

`ProcessOutput` has `stdout`, `stderr`, `exitCode`, `signal`, `ok` and `message`; its `toString()` is
`stdout`. `ProcessPromise` is awaitable and chainable:

| call | what it does |
| --- | --- |
| `.nothrow()` | a non-zero exit comes back as output instead of throwing |
| `.quiet()` | print nothing, whatever `$.verbose` says |
| `.timeout(ms)` | kill it after that long, and throw (accepts `'1s'`, `'500ms'`, `'2m'`) |
| `.cwd(dir)` | run there |
| `.env(map)` | add to the environment |
| `.stdin(value)` | hand it this input |
| `.pipe(next)` | feed its output to the next command |

A command that exits non-zero throws `ProcessError`, which carries the same `stdout`, `stderr`,
`exitCode` and `signal` as the output it would have been. `$.verbose` (on by default, like zx) prints
the line and, when it finishes, the output.

## Presets: `$({...})`

`$` is callable as a factory. `$({...})` is a *new* `$` with those defaults, and presets chain:

    const strict = $({ nothrow: false, timeout: '5s' });
    const relaxed = $({ nothrow: true });
    const both = relaxed({ quiet: true });      // nothrow and quiet

    const one = await relaxed`exit 1`;          // exitCode 1, no throw
    $.nothrow;                                  // false: the global one did not change

What a preset carries: `cwd`, `env`, `input`, `verbose`, `quiet`, `nothrow`, `timeout`, `preferLocal`,
`prefix`, `postfix`, `quote`, `shell`. `$.sync` is the synchronous form of the same template and
returns a `ProcessOutput` instead of a promise:

    const out = $.sync`git rev-parse HEAD`;
    const quietOne = $({ nothrow: true }).sync`exit 4`;   // exitCode 4

Options that zx documents and this runtime accepts but does not act on: `stdio`, `detached`, `spawn`,
`spawnSync`, `store`, `log`, `killSignal`, `timeoutSignal`, `halt`, `delimiter`. They are about a
process model this runtime does not expose — a third stream, a custom spawner, a signal to send — and
quietly ignoring them is better than pretending. `shell` is accepted and does not change anything: the
platform decides (`sh -c` or `cmd /C`), because that is the only shell this runtime has.

## Input and pipes

    await $({ input: 'hello' })`cat`;               // stdout === 'hello'
    await $`printf one`.pipe`wc -w`;                // 1
    await $`printf hello`.pipe($`tr a-z A-Z`);      // HELLO

A pipe runs the left side first and hands everything it said to the right side as input. That is a
deliberate difference from zx: nothing streams through a pipe here, so a left side that never stops
never feeds the right side. A shell pipe (`$`printf x | wc -c``) streams, because the shell owns it,
and that is what a long-running producer should use.

## The helpers

| name | what it is |
| --- | --- |
| `cd(dir)` | change where the next command runs (a `ProcessOutput` is accepted, newline trimmed) |
| `within(fn)` | run `fn` with its own defaults, restored when it finishes |
| `syncProcessCwd()` | also call `process.chdir` from `cd()` |
| `sleep(ms)` | wait; accepts `'1s'` |
| `echo(...)` | print, unwrapping `ProcessOutput`; also a tag: `echo`a ${b} c`` |
| `stdin()` | everything on this process's stdin, as text |
| `question(query, { choices })` | one line from stdin, with a prompt |
| `retry(n, [delay], fn)` | n attempts, waiting between them |
| `expBackoff()` | the doubling delay `retry` accepts |
| `spinner([message], fn)` | a spinner on stderr while `fn` runs, silent when not a terminal |
| `glob(patterns, options)` | `*`, `**`, `?`, `[abc]`, `{a,b}`; `glob.sync` too |
| `which(program, { nothrow })` | the path of a program; `node_modules/.bin` first while `preferLocal` |
| `tmpdir(...names)` | a temporary directory, made on first use |
| `tmpfile([name], [data])` | a file in it |
| `minimist(args, options)` / `argv` | the argument parser zx uses, and `process.argv` through it |
| `dotenv.parse/load/config` | dotenv text, a file, and into `process.env` |
| `chalk` | colours: named, `bg*`, modifiers, `rgb()`, `hex()`, honouring `NO_COLOR`/`FORCE_COLOR` |
| `fs`, `os`, `path` | this runtime's modules, plus `readJson`/`writeJson`/`ensureDir` and their `*Sync` forms |
| `quote(value)`, `quotePowerShell(value)` | the quoting functions `$` uses |
| `useBash()`, `usePowerShell()`, `usePwsh()` | set `$.shell` and the matching quoting |
| `ps.lookup({ command })`, `ps.tree({ pid })` | processes, through the platform's own `ps` |
| `kill(pid, signal)` | kill a process, through `kill`/`taskkill` |
| `versions` | the version of this runtime |

`fetch` is a global of its own (see docs/fetch.md), not a zx export.

## Where it lives

`js/exec.js` holds the quoting, the presets and the helpers; `src/host/exec.rs` holds the process. A
command is an id on both sides, the same arrangement sockets uses: a thread per pipe pushes chunks
into a channel, a watcher thread polls the child until it is gone, and each of those becomes a promise
that settles on the realm thread. Nothing blocks the realm except `$.sync`, which is what it is for:
it runs the command to its end on the same thread. The pipe backlog is eight chunks of 16 KiB, so a
command that writes more than that waits in the pipe until the reader catches up.

## What is not here

- **No async context.** `within` restores its defaults when its callback's promise settles, not when
  the async chain ends: another task that runs while that promise is pending sees the changed settings
  too. zx uses AsyncLocalStorage; this runtime has no such thing yet.
- **No streaming pipe.** See above.
- **`question` has no raw mode and no picker.** Reading a line is all there is — no key handling, no
  arrow keys, no hiding what is typed. `choices` prints the list and accepts either the value or its
  number.
- **`chalk` is a subset.** Named colours, backgrounds, the usual modifiers, `rgb`/`hex`. No 256-colour
  `ansi256`, no template tags, no `chalk.level` beyond on/off.
- **`glob` is a subset.** No `!negation` patterns, no `ignore` option, no `.dot` option: dot files are
  matched only when the pattern names the dot.
- **`YAML` and `MAML` are not bundled.** zx re-exports both parsers; this runtime has neither.
- **`ps` and `kill` shell out.** Listing processes and signalling one are the platform's job and both
  mean libc, so `ps` parses `ps -Ao pid,ppid,comm` and `kill` runs `kill`/`taskkill`. Neither works on
  a system without those tools.
- **`$.sync` counts on nothing being shared.** It blocks the realm thread, so a script that keeps a
  server or a timer alive across one will see it stop answering for the length of the command.

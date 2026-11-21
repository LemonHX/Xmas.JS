# $: running a command

`$` is a global, not an import: one line of shell, run as a command, resolved with the output.

    const out = await $`git rev-parse --short HEAD`;
    console.log(out.stdout.trim());

    const files = await $`ls -la ${directory}`;        // the value is quoted for the shell
    await $`exit 3`;                                   // throws ProcessError, exitCode 3
    const bad = await $`exit 3`.nothrow();             // no throw, read the output instead

A template is a command line, not an argv. Everything between the backticks is the line as the
shell sees it: `| grep foo`, `&&`, redirection and globs all work, because the platform's own
shell parses it (`/bin/sh -c` on unix, `cmd /C` on windows). Interpolated values are quoted for
that shell, so a value with spaces, quotes or a semicolon goes in as one word and nothing else.
`$.raw(text)` is for the pieces quoting would break: a pipe, a redirection, a name the shell
already knows.

## What a command answers

`ProcessOutput` has `stdout`, `stderr`, `exitCode`, `signal` and `ok`, and its `toString()`
is `stdout`. `ProcessPromise` is awaitable and chainable:

| call | what it does |
| --- | --- |
| `.nothrow()` | a non-zero exit comes back as output instead of throwing |
| `.quiet()` | print nothing, whatever `$.verbose` says |
| `.timeout(ms)` | kill the command after that long, and throw |
| `.cwd(dir)` | run there |
| `.env(map)` | add to the environment |

`$.verbose` (on by default, like zx) prints the command line and, when it finishes, its output.
`$.cwd` and `$.env` are the defaults for every command.

## Where it lives

`js/exec.js` holds the quoting and the promises; `src/host/exec.rs` holds the process. A command
is an id on both sides, the same arrangement sockets uses: a thread per pipe pushes chunks into a
channel, a watcher thread polls the child until it is gone, and each of those becomes a promise
that settles on the realm thread. Nothing blocks the realm and nothing buffers output nobody
asked for: the pipe backlog is eight chunks of 16 KiB, and a command that writes more than that
waits in the pipe until the reader catches up.

`kill` is `Child::kill`: SIGKILL on unix, TerminateProcess on windows. Sending a chosen signal
needs libc, which this runtime does not take.

## What is not here yet

- No piping between commands (`$`a` | $`b``) and no stdin. Use the shell's own redirection.
- No `cd()` or `within()`: `.cwd()` is the way to run somewhere else.
- No argument form. `span`/argv comes with `node:child_process`, which will reuse
  `src/host/exec.rs` and skip the shell.

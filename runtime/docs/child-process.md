# node:child_process

`import { spawn, exec, execFile, execSync, execFileSync, spawnSync } from 'node:child_process'`.
The shape is Node's: `spawn` hands back a ChildProcess with a Readable `stdout`, a Readable
`stderr` and a Writable `stdin`; `exec` and `execFile` collect the output and call back; the
three `*Sync` calls block the realm thread until the command is gone.

    const child = spawn('git', ['rev-parse', '--short', 'HEAD']);
    child.stdout.on('data', (chunk) => process.stdout.write(chunk));
    child.on('close', (code, signal) => console.log(code, signal));

    exec('ls -la | wc -l', { encoding: 'utf8' }, (error, stdout) => console.log(stdout));
    const head = execSync('git rev-parse HEAD', { encoding: 'utf8' });

`spawn` and `execFile` take an argv: nothing between them and the program, so an argument is
one argument whatever it contains. `exec` and `execSync` put the line through the platform shell,
so `|`, `&&` and redirection work there and nowhere else.

The options that are read: `cwd`, `env`, `shell`, `encoding`, `maxBuffer`, and `failOnError`
(the last two on the `*Sync` calls).

## Where it lives

`src/host/exec.rs` holds the process, the same file `$` runs on: a thread per pipe pushing
chunks into a bounded channel, a watcher thread polling the child, and a thread feeding input
into stdin. `js/child_process.js` builds the streams and the events out of those, and
`src/modules/child_process.rs` is the bridge. Nothing blocks the realm except the `*Sync` calls,
which is what they are for.

## What is not here

- **Only SIGKILL can be sent.** `kill('SIGTERM')` throws, with a message that says why: a chosen
  signal means `kill(2)`, and this runtime does not take libc. `kill()` with no argument works
  and means SIGKILL (TerminateProcess on windows).
- **No `stdio` array.** A child's three streams are its pipes; `inherit`, `ignore` and passing a
  socket in are not implemented, because the third one would mean handing a socket through by
  hand.
- **No `detached`, no `input` option, no `uid`/`gid`.** Same reason as the signal one: they are
  process attributes that need the platform's own API.
- **`pid` is always -1.** The id the caller sees is this runtime's, not the operating system's,
  and reporting one while meaning the other would be a lie.

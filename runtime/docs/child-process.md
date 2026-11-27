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

The options that are read: `cwd`, `env`, `shell`, `stdio`, `encoding`, `maxBuffer`, and
`failOnError` (the last two on the `*Sync` calls).

`stdio` is one answer for all three streams: `'inherit'` lets the child write to this process's
own descriptors, `'ignore'` gives it nothing, and `'pipe'` (the default) is what makes
`stdout` and `stderr` readable. An array is honoured when every entry agrees; a mix keeps the
pipes. `'inherit'` is what a program that talks to the terminal needs, and with it the two
readers report end at once because there is nothing of ours to read:

    spawn('git', ['status'], { stdio: 'inherit' });

## Where it lives

`src/host/exec.rs` holds the process, the same file `$` runs on: a thread per pipe pushing
chunks into a bounded channel, a watcher thread polling the child, and a thread feeding input
into stdin. `js/child_process.js` builds the streams and the events out of those, and
`src/modules/child_process.rs` is the bridge. Nothing blocks the realm except the `*Sync` calls,
which is what they are for.

## What is not here

- **Signals are real on unix.** `kill('SIGTERM')` (a name, a number, or nothing, which means SIGTERM) is `kill(2)` on the child. Windows has no signals, so any of them ends the process.
- **No `ipc` and no file descriptors in `stdio`.** A socket or a FileHandle handed to a child
  would mean passing a descriptor through by hand; `'inherit'`, `'ignore'` and `'pipe'` are
  the three that are implemented.
- **No `detached`, no `input` option, no `uid`/`gid`.** Same reason as the signal one: they are
  process attributes that need the platform's own API.
- **`pid` is always -1.** The id the caller sees is this runtime's, not the operating system's.

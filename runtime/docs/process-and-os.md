# process and os

Node's process and os, as far as they mean anything inside an embedded realm.

## What crossing the boundary costs, and where it is avoided

Everything that does not change while the process runs is turned into JSON once,
at install time, and parsed into plain JS values: the environment snapshot, argv,
execPath, pid, ppid, platform, arch, version. Reading process.env.HOME or
process.argv[2] after that is a property read in the engine, no host call.

Writes to process.env go through a Proxy and reach the native Env, because a
spawned child reads the Env, not the JS object.

What stays a host call: cwd, chdir, hrtime, uptime, memoryUsage, the stdio
writes, and exit.

## The os module reports the realm, not the machine

Nothing in this module asks the kernel anything: no libc, no sysctl, no /proc.
There is no permission model to gate host access with, so the module does not
hold those facts: a script cannot learn the machine's memory, its load average,
its kernel version, its hostname or its user ids.

The names and shapes stay Node's, so code written against Node keeps running.
The numbers are ours:

| Node | here |
| --- | --- |
| os.totalmem() | this realm's heap cap, 64 MiB by default |
| os.freemem() | that cap minus what the engine has reserved |
| os.uptime() | seconds since this realm's clock started |
| os.loadavg() | [0, 0, 0] |
| os.hostname() | "" (not disclosed) |
| os.userInfo().uid, .gid | -1, which is what Node reports where the ids do not exist |
| os.type() | "XmasJS" |
| os.release() | this runtime's version |
| os.machine(), os.arch() | the target arch, known at compile time |
| os.cpus(), os.availableParallelism() | thread count from std |
| os.homedir(), os.tmpdir() | the host's environment snapshot, "" when unset |

homedir and tmpdir read the injected Env rather than std::env, so an embedder
that hands over a fake environment gets a matching os module.

The thread count is the one thing asked of the scheduler, and it goes through
std::thread::available_parallelism. It sizes thread pools; it is not a fact about
the user.

## Where this differs from Node

memoryUsage reports rss and heapUsed from the engine's own heap report, and
heapTotal from the cap. rss is not resident memory: the realm cannot see the
machine's, so it reports the heap it lives in. external and arrayBuffers stay
zero because buffers and typed arrays are allocated inside that same heap.

That report cannot be taken from inside a host callback: the engine refuses to be
re-entered. The driver reads it between turns of the loop instead, so the number
is stale by at most one turn (a 200 us slice).

process.exit exits the real process. Under a capturing host (tests, benchmarks)
that would take the harness down, so there it raises instead and a test can
assert on the message.

os.cpus() returns one entry per core with an empty model and zeroed times: this
runtime reports no per-core data.

## No unsafe, and no FFI

host/sysinfo.rs used to hold uname, gethostname, sysctl, sysconf, getrusage,
getppid, getuid, getgid, isatty and a TIOCGWINSZ ioctl: 336 lines and twenty
unsafe blocks to answer questions about the machine. It is gone.

What those answers are replaced with is listed in the table above. The calls that
remain go through std and are not about the host: available_parallelism for the
thread count, std::io::IsTerminal for tty.isatty over fds 0, 1 and 2, std::env
for the tmpdir fallback, std::env::consts for platform and arch.

Two consequences worth knowing:

- tty.columns and tty.rows come from the COLUMNS and LINES environment
  variables, because std has no ioctl. With neither set the size is Node's
  "unknown" answer, [0, 0]. A terminal that exports neither gets zeros.
- tty.isatty asks whether fd 0, 1 or 2 is a terminal. That is a yes or no about a
  descriptor this process owns, not a fact about the machine, and it is what lets
  a script decide whether to colour its output.

The libc crate is still in the dependency tree, reached through cpufeatures and
through rustix (which smol uses for io). Nothing in this crate calls it directly,
and std already links the platform C library into every Rust binary, so nothing
here adds a system dependency.

runtime/tests/purity.rs holds both guards: the dependency tree carries no system
library (openssl, native-tls, libz-sys, zstd-sys, aws-lc-sys and friends), and no
source file under runtime/src contains an unsafe block or an unsafe fn.

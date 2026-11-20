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
writes, and exit. The os module keeps the changing numbers (totalmem, freemem,
uptime, loadavg) as host calls that return f64 rather than JSON.

## Where this differs from Node

memoryUsage reports heapUsed from the engine's own heap report, but that report
cannot be taken from inside a host callback: the engine refuses to be re-entered.
The driver reads it between turns of the loop instead, so the number is stale by
at most one turn (a 200 us slice). rss is the kernel's peak resident set, not a
live reading. external and arrayBuffers are zero until the engine tracks them
apart.

process.exit exits the real process. Under a capturing host (tests, benchmarks)
that would take the harness down, so there it raises instead and a test can
assert on the message.

os.cpus() returns one entry per core with an empty model and zeroed times: the
host does not hand out per-core data, and inventing it would be worse than an
empty string.

The only unsafe code in the tree lives in host/sysinfo.rs: uname, gethostname,
sysctl, sysconf and getrusage, all behind cfg(unix), all with a fallback.

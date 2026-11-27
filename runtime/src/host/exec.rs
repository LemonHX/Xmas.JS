//! Running a command: the child, its two pipes and its exit status.
//!
//! Every blocking step has a thread of its own so the realm thread never waits on one: a reader
//! thread per pipe pushes chunks into a channel, and a watcher thread polls the child until it is
//! gone. JS gets those channels as promises under `src/modules/exec.rs`; nothing here touches the
//! engine.
//!
//! The command is one line of shell, run by the platform's own shell. That is what a
//! `$\`...\`` template means, and it is why quoting lives on the JS side.

use std::io::{ErrorKind, Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// How long the watcher sleeps between checks. `Child::wait` would hold the child for as long as
/// it runs, and holding it is exactly what would keep `kill` from taking the lock; so it polls a
/// child that is already running as fast as one process exits.
const WATCH_EVERY: Duration = Duration::from_millis(5);

/// How much of a pipe is handed to JS in one chunk, and how many chunks may sit unread before the
/// child is pushed back on.
const CHUNK: usize = 16 * 1024;
const BACKLOG: usize = 8;

/// How a command is started: one line of shell, or a program and its arguments.
pub enum Recipe<'a> {
    /// Through the platform's shell, which is what a template means.
    Shell(&'a str),
    /// Straight to the program, with an argv: no shell, so no quoting and no globs.
    Argv(&'a str, &'a [String]),
}

/// Where the command's input comes from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stdin {
    /// Closed: the command reads end-of-file at once.
    Null,
    /// A pipe, which the caller writes into through `Running::stdin`.
    Pipe,
}

/// Where the command's output goes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Stdout {
    /// A pipe, read through `Running::stdout` and `Running::stderr`.
    Pipe,
    /// The file descriptors this process already has. Nothing is captured, which is what a
    /// program that talks to the terminal wants: its output is the terminal's.
    Inherit,
}

/// One running command.
pub struct Running {
    child: Arc<Mutex<Child>>,
    pub stdout: smol::channel::Receiver<Vec<u8>>,
    pub stderr: smol::channel::Receiver<Vec<u8>>,
    /// Where to write the command's input. Dropping it closes the pipe, which is how a command
    /// that reads until end-of-file is told there is no more.
    pub stdin: Option<smol::channel::Sender<Vec<u8>>>,
    /// One JSON message: `{"code":0,"signal":null}`, sent once the child is gone.
    pub status: smol::channel::Receiver<String>,
}

impl Running {
    /// Send a chosen signal. On unix this is kill(2) on the child's pid; windows has no signals, so
    /// every one of them ends the process, as Node does there.
    pub fn signal(&self, number: i32) -> Result<(), String> {
        let mut child = self
            .child
            .lock()
            .map_err(|_| "the child's lock is poisoned".to_string())?;
        #[cfg(unix)]
        {
            if number == 0 {
                return Ok(());
            }
            let pid = child.id() as libc::pid_t;
            if unsafe { libc::kill(pid, number) } < 0 {
                return Err(std::io::Error::last_os_error().to_string());
            }
            Ok(())
        }
        #[cfg(not(unix))]
        {
            let _ = number;
            child.kill().map_err(|error| error.to_string())
        }
    }

    /// Kill the child. There is no way to send it a chosen signal without libc, so this is SIGKILL
    /// on unix and TerminateProcess on windows, which is what "stop it" means on both.
    pub fn kill(&self) -> Result<(), String> {
        let mut child = self
            .child
            .lock()
            .map_err(|_| "the child's lock is poisoned".to_string())?;
        child.kill().map_err(|error| error.to_string())
    }
}

/// Everything a command said, for the calls that have to wait rather than listen.
pub struct Captured {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: String,
}

/// Start a command and hand back the channels its output will arrive on.
pub fn spawn(
    recipe: Recipe<'_>,
    cwd: Option<&str>,
    env: &[(String, String)],
    stdin: Stdin,
) -> Result<Running, String> {
    spawn_with_stdout(recipe, cwd, env, stdin, Stdout::Pipe)
}

/// Start a command whose output goes where the caller says.
///
/// With [`Stdout::Inherit`] the two receivers come back already closed: there are no bytes to
/// read, because the child wrote them itself. `stdin` still works the same either way.
pub fn spawn_with_stdout(
    recipe: Recipe<'_>,
    cwd: Option<&str>,
    env: &[(String, String)],
    stdin: Stdin,
    stdout_where: Stdout,
) -> Result<Running, String> {
    let mut child = start(recipe, cwd, env, stdin, stdout_where)?;

    let (stdout_tx, stdout) = smol::channel::bounded(BACKLOG);
    let (stderr_tx, stderr) = smol::channel::bounded(BACKLOG);
    let (status_tx, status) = smol::channel::bounded(1);

    if stdout_where == Stdout::Inherit {
        // An inherited stream is not a pipe of ours, so nothing is ever written into these:
        // closing the senders here is what makes a reader see end-of-input instead of waiting.
        drop(stdout_tx);
        drop(stderr_tx);
    } else {
        if let Some(pipe) = child.stdout.take() {
            pump(pipe, stdout_tx);
        }
        if let Some(pipe) = child.stderr.take() {
            pump(pipe, stderr_tx);
        }
    }

    let stdin = child.stdin.take().map(feed);

    let child = Arc::new(Mutex::new(child));
    watch(Arc::clone(&child), status_tx);

    Ok(Running {
        child,
        stdout,
        stderr,
        stdin,
        status,
    })
}

/// Run a command to its end and keep everything it said. This waits, which is what a synchronous
/// call is for: it blocks the thread that asked, and only that thread.
pub fn capture(
    recipe: Recipe<'_>,
    cwd: Option<&str>,
    env: &[(String, String)],
    input: Option<&[u8]>,
) -> Result<Captured, String> {
    let mut child = start(recipe, cwd, env, Stdin::Pipe, Stdout::Pipe)?;

    let writing = child.stdin.take().map(|pipe| {
        let input = input.unwrap_or_default().to_vec();
        std::thread::spawn(move || {
            let mut pipe = pipe;
            if !input.is_empty() {
                let _ = pipe.write_all(&input);
            }
            // Dropping the pipe is the end of the input.
        })
    });

    // Both pipes are read on their own threads: reading them in turn would deadlock on a command
    // that fills the one we are not reading.
    let out = child.stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });
    let err = child.stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            let _ = pipe.read_to_end(&mut bytes);
            bytes
        })
    });

    let status = child.wait().map_err(|error| error.to_string())?;
    let stdout = out.map(join).unwrap_or_default();
    let stderr = err.map(join).unwrap_or_default();
    if let Some(writing) = writing {
        let _ = writing.join();
    }

    Ok(Captured {
        stdout,
        stderr,
        status: describe(status),
    })
}

fn join(reader: std::thread::JoinHandle<Vec<u8>>) -> Vec<u8> {
    reader.join().unwrap_or_default()
}

/// Build the command and start it, with the three standard streams set up as asked.
fn start(
    recipe: Recipe<'_>,
    cwd: Option<&str>,
    env: &[(String, String)],
    stdin: Stdin,
    stdout: Stdout,
) -> Result<Child, String> {
    let mut builder = match recipe {
        Recipe::Shell(command) => shell(command),
        Recipe::Argv(program, arguments) => {
            let mut builder = Command::new(program);
            builder.args(arguments);
            builder
        }
    };

    if let Some(dir) = cwd {
        builder.current_dir(dir);
    }
    for (name, value) in env {
        builder.env(name, value);
    }
    builder
        .stdin(match stdin {
            Stdin::Null => Stdio::null(),
            Stdin::Pipe => Stdio::piped(),
        })
        .stdout(match stdout {
            Stdout::Pipe => Stdio::piped(),
            Stdout::Inherit => Stdio::inherit(),
        })
        .stderr(match stdout {
            Stdout::Pipe => Stdio::piped(),
            Stdout::Inherit => Stdio::inherit(),
        });

    builder
        .spawn()
        .map_err(|error| format!("{}: {error}", describe_recipe(&recipe)))
}

fn describe_recipe(recipe: &Recipe<'_>) -> String {
    match recipe {
        Recipe::Shell(command) => (*command).to_string(),
        Recipe::Argv(program, arguments) => {
            let mut line = (*program).to_string();
            for argument in arguments.iter() {
                line.push(' ');
                line.push_str(argument);
            }
            line
        }
    }
}

/// The platform's shell, one line of it.
fn shell(command: &str) -> Command {
    #[cfg(windows)]
    let shell = {
        let mut shell = Command::new("cmd");
        shell.arg("/C").arg(command);
        shell
    };
    #[cfg(not(windows))]
    let shell = {
        let mut shell = Command::new("/bin/sh");
        shell.arg("-c").arg(command);
        shell
    };
    shell
}

/// Read a pipe to its end on a thread of its own. Dropping the sender at the end is what tells JS
/// the pipe closed, so there is no separate end-of-file message to send.
fn pump(mut pipe: impl Read + Send + 'static, into: smol::channel::Sender<Vec<u8>>) {
    std::thread::spawn(move || {
        let mut buffer = vec![0u8; CHUNK];
        loop {
            match pipe.read(&mut buffer) {
                Ok(0) => return,
                Ok(read) => {
                    // A full channel means JS is behind: waiting here is the backpressure the
                    // child feels, and it costs nothing on this thread.
                    if smol::block_on(into.send(buffer[..read].to_vec())).is_err() {
                        return;
                    }
                }
                Err(error) if error.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return,
            }
        }
    });
}

/// Take what JS writes and put it in the command's input.
fn feed(mut pipe: std::process::ChildStdin) -> smol::channel::Sender<Vec<u8>> {
    let (into, from) = smol::channel::bounded::<Vec<u8>>(BACKLOG);
    std::thread::spawn(move || {
        while let Ok(bytes) = smol::block_on(from.recv()) {
            if pipe.write_all(&bytes).is_err() {
                return;
            }
            let _ = pipe.flush();
        }
        // Dropping the pipe here is the end of the input.
    });
    into
}

/// Poll the child until it is gone, then say how it went.
fn watch(child: Arc<Mutex<Child>>, into: smol::channel::Sender<String>) {
    std::thread::spawn(move || loop {
        let status = {
            let mut child = match child.lock() {
                Ok(child) => child,
                Err(_) => return,
            };
            match child.try_wait() {
                Ok(Some(status)) => status,
                Ok(None) => {
                    drop(child);
                    std::thread::sleep(WATCH_EVERY);
                    continue;
                }
                Err(_) => return,
            }
        };
        let _ = into.send_blocking(describe(status));
        return;
    });
}

/// How the child ended, as JSON, because that is what crosses the pending channel as text.
fn describe(status: std::process::ExitStatus) -> String {
    let code = match status.code() {
        Some(code) => code.to_string(),
        None => "null".to_string(),
    };
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        match status.signal() {
            Some(signal) => signal.to_string(),
            None => "null".to_string(),
        }
    };
    #[cfg(not(unix))]
    let signal = "null".to_string();
    format!("{{\"code\":{code},\"signal\":{signal}}}")
}

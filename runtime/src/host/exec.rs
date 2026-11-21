//! Running a command: the child, its two pipes and its exit status.
//!
//! Every blocking step has a thread of its own so the realm thread never waits on one: a reader
//! thread per pipe pushes chunks into a channel, and a watcher thread polls the child until it is
//! gone. JS gets those channels as promises under `src/modules/exec.rs`; nothing here touches the
//! engine.
//!
//! The command is one line of shell, run by the platform's own shell. That is what a
//! `$\`...\`` template means, and it is why quoting lives on the JS side.

use std::io::{ErrorKind, Read};
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

/// One running command.
pub struct Running {
    child: Arc<Mutex<Child>>,
    pub stdout: smol::channel::Receiver<Vec<u8>>,
    pub stderr: smol::channel::Receiver<Vec<u8>>,
    /// One JSON message: `{"code":0,"signal":null}`, sent once the child is gone.
    pub status: smol::channel::Receiver<String>,
}

impl Running {
    /// Kill the child. There is no way to send it a chosen signal without libc, so this is SIGKILL
    /// on unix and TerminateProcess on windows, which is what "stop it" means in both.
    pub fn kill(&self) -> Result<(), String> {
        let mut child = self
            .child
            .lock()
            .map_err(|_| "the child's lock is poisoned".to_string())?;
        child.kill().map_err(|error| error.to_string())
    }
}

/// Start `command` as one line of shell in `cwd`, with `env` added to the environment.
pub fn spawn(command: &str, cwd: Option<&str>, env: &[(String, String)]) -> Result<Running, String> {
    let mut builder = shell(command);
    if let Some(dir) = cwd {
        builder.current_dir(dir);
    }
    for (name, value) in env {
        builder.env(name, value);
    }
    builder
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = builder
        .spawn()
        .map_err(|error| format!("{command}: {error}"))?;

    let (stdout_tx, stdout) = smol::channel::bounded(BACKLOG);
    let (stderr_tx, stderr) = smol::channel::bounded(BACKLOG);
    let (status_tx, status) = smol::channel::bounded(1);

    if let Some(pipe) = child.stdout.take() {
        pump(pipe, stdout_tx);
    }
    if let Some(pipe) = child.stderr.take() {
        pump(pipe, stderr_tx);
    }

    let child = Arc::new(Mutex::new(child));
    watch(Arc::clone(&child), status_tx);

    Ok(Running {
        child,
        stdout,
        stderr,
        status,
    })
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

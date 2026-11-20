//! console: normally goes to stdout; in tests it is captured in memory.
//!
//! The engine's own output also goes through here (Engine::write), so it can all be intercepted in one place.

use std::cell::RefCell;
use std::rc::Rc;

use quickjs_oxide::engine::api::core::Stream;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Level {
    Log,
    Info,
    Warn,
    Error,
}

impl Level {
    fn tag(self) -> &'static str {
        match self {
            Level::Log => "log",
            Level::Info => "info",
            Level::Warn => "warn",
            Level::Error => "error",
        }
    }
}

#[derive(Clone)]
enum Sink {
    Stdout,
    Capture(Rc<RefCell<Vec<u8>>>),
}

#[derive(Clone)]
pub struct Console {
    sink: Sink,
}

impl Console {
    pub fn stdout() -> Self {
        Console { sink: Sink::Stdout }
    }

    pub fn capture() -> Self {
        Console {
            sink: Sink::Capture(Rc::new(RefCell::new(Vec::new()))),
        }
    }

    pub fn line(&self, level: Level, text: &str) {
        self.push(format!("[{}] {}\n", level.tag(), text).as_bytes());
    }

    /// Raw output sent by the engine; no prefix is added.
    pub fn raw(&self, stream: Stream, bytes: &[u8]) {
        match (&self.sink, stream) {
            (Sink::Stdout, Stream::Stderr) => {
                use std::io::Write;
                let mut out = std::io::stderr().lock();
                let _ = out.write_all(bytes);
                let _ = out.flush();
            }
            _ => self.push(bytes),
        }
    }

    /// True when output is collected in memory instead of going to the terminal.
    pub fn is_captured(&self) -> bool {
        matches!(self.sink, Sink::Capture(_))
    }

    /// Retrieve the captured content (an empty string in stdout mode).
    pub fn take(&self) -> String {
        match &self.sink {
            Sink::Stdout => String::new(),
            Sink::Capture(buffer) => {
                let mut buffer = buffer.borrow_mut();
                let text = String::from_utf8_lossy(&buffer).into_owned();
                buffer.clear();
                text
            }
        }
    }

    pub fn captured_bytes(&self) -> usize {
        match &self.sink {
            Sink::Stdout => 0,
            Sink::Capture(buffer) => buffer.borrow().len(),
        }
    }

    fn push(&self, bytes: &[u8]) {
        match &self.sink {
            Sink::Stdout => {
                use std::io::Write;
                let mut out = std::io::stdout().lock();
                let _ = out.write_all(bytes);
                let _ = out.flush();
            }
            Sink::Capture(buffer) => buffer.borrow_mut().extend_from_slice(bytes),
        }
    }
}

//! Host capabilities: fs, net, timer, console, env, module.
//!
//! The engine layer and modules only go through here; they no longer touch tokio, and business code no longer calls std::fs directly.
//! Everything is single-threaded (Rc), just like a quickjs-oxide realm: one thread, one world.
//! No trait objects: to swap implementations, replace the whole Host; swapping in a fake one in tests follows the same path.

pub mod clock;
pub mod console;
pub mod env;
pub mod fs;
pub mod http;
pub mod modules;
pub mod net;
pub mod pending;
pub mod timer;

pub use clock::{CancelHandle, Clock};
pub use console::{Console, Level};
pub use env::Env;
pub use fs::{DirItem, Fs, Meta};
pub use modules::{Module, Modules};
pub use net::Net;
pub use pending::{Job, Payload, Pending, Reply, Settle};
pub use timer::{PromiseSlot, TimerCallback, TimerId, Timers, sleep};

use quickjs_oxide::engine::api::core::{Host as EngineHostTrait, Stream};

/// A set of host capabilities. One per thread in the whole process, tied to the realm.
pub struct Host {
    pub clock: Clock,
    pub console: Console,
    pub env: Env,
    pub fs: Fs,
    pub net: Net,
    pub timers: Timers,
    pub modules: Modules,
    /// The return channel for host async work; fs and future crypto go through it.
    pub pending: Pending,
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

impl Host {
    pub fn new() -> Self {
        Self::with_console(Console::stdout())
    }

    /// console is captured into memory; tests and benchmarks use this.
    pub fn capture() -> Self {
        Self::with_console(Console::capture())
    }

    pub fn with_console(console: Console) -> Self {
        Host {
            clock: Clock::new(),
            console,
            env: Env::snapshot(),
            fs: Fs,
            net: Net,
            timers: Timers::new(),
            modules: Modules::new(),
            pending: Pending::new(),
        }
    }

    /// The side given to the engine.
    pub fn engine(&self) -> Engine {
        Engine {
            clock: self.clock.clone(),
            console: self.console.clone(),
        }
    }
}

/// The host as seen by the engine: only interrupt and write are overridden; everything else uses the default implementation.
pub struct Engine {
    clock: Clock,
    console: Console,
}

impl EngineHostTrait for Engine {
    /// Timeout or an external stop request. Setting a deadline per request depends on this line.
    fn interrupt(&mut self) -> bool {
        self.clock.stop_requested()
    }

    /// The engine's own output (such as qjs's print) goes to our console, and tests can intercept it.
    fn write(&mut self, stream: Stream, text: &[u8]) {
        self.console.raw(stream, text);
    }
}

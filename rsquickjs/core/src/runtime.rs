//! QuickJS runtime related types.

pub(crate) mod opaque;
pub(crate) mod raw;
mod userdata;


mod r#async;

mod spawner;

pub(crate) mod task_queue;

pub use spawner::DriveFuture;

use std::boxed::Box;
// pub use base::{Runtime, WeakRuntime};
pub use userdata::{UserDataError, UserDataGuard};
pub use r#async::{AsyncRuntime, AsyncWeakRuntime};

use crate::value::promise::PromiseHookType;
use crate::{Ctx, Value};

/// The type of the promise hook.
pub type PromiseHook =
    Box<dyn for<'a> Fn(Ctx<'a>, PromiseHookType, Value<'a>, Value<'a>) + Send + 'static>;

/// The type of the promise rejection tracker.
pub type RejectionTracker =
    Box<dyn for<'a> Fn(Ctx<'a>, Value<'a>, Value<'a>, bool) + Send + 'static>;

/// The type of the interrupt handler.
pub type InterruptHandler = Box<dyn FnMut() -> bool + Send + 'static>;

/// A struct with information about the runtimes memory usage.
pub type MemoryUsage = crate::qjs::JSMemoryUsage;

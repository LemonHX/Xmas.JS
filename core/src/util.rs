//! Module with some util types.

use std::panic::UnwindSafe;

/// A trait for preventing implementing traits which should not be implemented outside of rquickjs.
pub trait Sealed {}

pub fn catch_unwind<R>(
    f: impl FnOnce() -> R + UnwindSafe,
) -> Result<R, std::boxed::Box<dyn std::any::Any + Send + 'static>> {
    std::panic::catch_unwind(f)
}

pub fn resume_unwind(payload: std::boxed::Box<dyn std::any::Any + Send>) -> ! {
    std::panic::resume_unwind(payload)
}

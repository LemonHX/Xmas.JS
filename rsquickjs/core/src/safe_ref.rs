use std::sync::Mutex as Cell;

pub use std::sync::{Arc as Ref, MutexGuard as Lock};

#[repr(transparent)]
pub struct Mut<T: ?Sized>(Cell<T>);

impl<T> Mut<T> {
    pub fn new(inner: T) -> Self {
        Self(Cell::new(inner))
    }
}

impl<T: Default> Default for Mut<T> {
    fn default() -> Self {
        Mut::new(T::default())
    }
}

impl<T: ?Sized> Mut<T> {
    pub fn lock(&self) -> Lock<T> {
            self.0.lock().unwrap()
    }

    pub fn try_lock(&self) -> Option<Lock<T>> {
            self.0.lock().ok()
    }
}

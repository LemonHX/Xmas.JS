//! # High-level bindings to QuickJS
//!
//! The `rquickjs` crate provides safe high-level bindings to the [QuickJS](https://bellard.org/quickjs/) JavaScript engine.
//! This crate is heavily inspired by the [rlua](https://crates.io/crates/rlua) crate.

#![allow(unknown_lints)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::uninlined_format_args)]
#![allow(mismatched_lifetime_syntaxes)]
#![allow(clippy::doc_lazy_continuation)]

pub(crate) use std::result::Result as StdResult;
pub(crate) use std::string::String as StdString;

mod js_lifetime;
pub mod markers;
mod persistent;
mod result;
mod safe_ref;
mod util;
mod value;
pub(crate) use safe_ref::*;
pub mod runtime;
pub use runtime::AsyncRuntime;
pub mod context;
pub use context::Ctx;
pub mod class;
pub use class::Class;
pub use js_lifetime::JsLifetime;
pub use persistent::Persistent;
pub use result::{CatchResultExt, CaughtError, CaughtResult, Error, Result, ThrowResultExt};
pub use value::{
    array, atom, convert, function, module, object, promise, proxy, Array, Atom, BigInt, CString,
    Coerced, Exception, Filter, FromAtom, FromIteratorJs, FromJs, Function, IntoAtom, IntoJs,
    IteratorJs, Module, Null, Object, Promise, Proxy, String, Symbol, Type, Undefined, Value,
    WriteOptions, WriteOptionsEndianness,
};

pub mod allocator;
pub mod loader;

pub use context::AsyncContext;
#[cfg(feature = "multi-ctx")]
pub use context::MultiWith;
pub use value::{ArrayBuffer, Iterable, JsIterator, TypedArray};

//#[doc(hidden)]
pub mod qjs {
    //! Native low-level bindings
    pub use rquickjs_sys::*;
}

#[cfg(feature = "phf")]
#[doc(hidden)]
pub mod phf {
    pub use phf::*;
}

pub mod prelude {
    //! A group of often used types.
    #[cfg(feature = "multi-ctx")]
    pub use crate::context::MultiWith;
    pub use crate::{
        context::Ctx,
        convert::{Coerced, FromAtom, FromIteratorJs, FromJs, IntoAtom, IntoJs, IteratorJs, List},
        function::{
            Exhaustive, Flat, Func, FuncArg, IntoArg, IntoArgs, MutFn, OnceFn, Opt, Rest, This,
        },
        result::{CatchResultExt, ThrowResultExt},
        JsLifetime,
    };
    pub use crate::{
        function::Async,
        promise::{Promise, Promised},
    };
}

#[cfg(test)]
pub(crate) async fn test_with<F, R>(func: F) -> R
where
    R: Send + 'static,
    F: FnOnce(Ctx) -> R + Send + 'static,
{
    let rt = AsyncRuntime::new().unwrap();
    let ctx = AsyncContext::full(&rt).await.unwrap();
    ctx.with(func).await
}

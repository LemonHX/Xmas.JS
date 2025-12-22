use std::env;

pub mod module;
pub mod module_builder;
pub mod package;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");

// added when .cjs files are imported
pub const CJS_IMPORT_PREFIX: &str = "__cjs:";
// added to force CJS imports in loader
pub const CJS_LOADER_PREFIX: &str = "__cjsm:";

//! # vsys - Virtual System Layer for Xmas.JS
//!
//! `vsys` provides a pluggable abstraction layer for all system-level operations,
//! enabling sandboxed execution, custom filesystem/network implementations, and
//! fine-grained permission control.
//!
//! ## Design Goals
//!
//! - **C ABI compatible**: All function pointers use `extern "C"` for FFI compatibility
//! - **Runtime swappable**: Change implementation at runtime
//! - **Zero-cost when static**: Compiler can inline when implementation is known
//! - **No trait objects**: Avoids dynamic dispatch overhead
//!
//! ## Usage
//!
//! ```rust,ignore
//! // Default: Use real filesystem and network
//! let vsys = Vsys::default();
//!
//! // Sandboxed: Custom implementations
//! let vsys = Vsys::builder()
//!     .fs(custom_fs_vtable())
//!     .permissions(restricted_permissions())
//!     .build();
//! ```

pub mod error;
pub mod fs;
pub mod module_loader;
pub mod permissions;

use std::sync::Arc;

pub use error::{VsysError, VsysResult};
pub use fs::FsVTable;
pub use module_loader::ModuleLoaderVTable;
pub use permissions::{BlackOrWhiteList, Permissions};

/// The main vsys context that holds all virtual system tables.
///
/// This is the central point for all system operations. It can be stored
/// in the JS runtime context and accessed by all modules.
#[derive(Clone)]
pub struct Vsys {
    /// Filesystem operations vtable
    pub fs: Arc<FsVTable>,
    /// Module loader/resolver vtable
    pub module_loader: Arc<ModuleLoaderVTable>,
    /// Permissions configuration
    pub permissions: Permissions,
}

impl Default for Vsys {
    fn default() -> Self {
        Self {
            fs: Arc::new(FsVTable::default()),
            module_loader: Arc::new(ModuleLoaderVTable::default()),
            permissions: Permissions::allow_all(),
        }
    }
}

impl Vsys {
    /// Create a new Vsys with default (real system) implementations
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a new builder for customizing Vsys
    pub fn builder() -> VsysBuilder {
        VsysBuilder::default()
    }

    /// Create a sandboxed Vsys with no permissions
    pub fn sandboxed() -> Self {
        Self {
            fs: Arc::new(FsVTable::deny_all()),
            module_loader: Arc::new(ModuleLoaderVTable::default()),
            permissions: Permissions::default(), // deny all by default
        }
    }

    /// Get a reference to the filesystem vtable
    #[inline]
    pub fn fs(&self) -> &FsVTable {
        &self.fs
    }

    /// Get a reference to the module loader vtable
    #[inline]
    pub fn module_loader(&self) -> &ModuleLoaderVTable {
        &self.module_loader
    }

    /// Get a reference to the permissions configuration
    #[inline]
    pub fn permissions(&self) -> &Permissions {
        &self.permissions
    }
}

/// Builder for constructing a customized Vsys instance
#[derive(Default)]
pub struct VsysBuilder {
    fs: Option<FsVTable>,
    module_loader: Option<ModuleLoaderVTable>,
    permissions: Option<Permissions>,
}

impl VsysBuilder {
    pub fn fs(mut self, fs: FsVTable) -> Self {
        self.fs = Some(fs);
        self
    }

    pub fn module_loader(mut self, loader: ModuleLoaderVTable) -> Self {
        self.module_loader = Some(loader);
        self
    }

    pub fn permissions(mut self, permissions: Permissions) -> Self {
        self.permissions = Some(permissions);
        self
    }

    pub fn build(self) -> Vsys {
        Vsys {
            fs: Arc::new(self.fs.unwrap_or_default()),
            module_loader: Arc::new(self.module_loader.unwrap_or_default()),
            permissions: self.permissions.unwrap_or_else(Permissions::allow_all),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_vsys() {
        let vsys = Vsys::default();
        assert!(vsys.permissions.stdio);
    }

    #[test]
    fn test_sandboxed_vsys() {
        let vsys = Vsys::sandboxed();
        assert!(!vsys.permissions.stdio);
    }

    #[test]
    fn test_builder() {
        let vsys = Vsys::builder().permissions(Permissions::default()).build();
        assert!(!vsys.permissions.stdio);
    }
}

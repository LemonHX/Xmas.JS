//! Error types for vsys operations

use std::ffi::CString;
use std::fmt;
use std::io;

/// Result type for vsys operations
pub type VsysResult<T> = Result<T, VsysError>;

/// Error type for vsys operations
#[derive(Debug)]
pub enum VsysError {
    /// I/O error from the underlying system
    Io(io::Error),
    /// Permission denied
    PermissionDenied(String),
    /// File or resource not found
    NotFound(String),
    /// Operation not supported by this vsys implementation
    NotSupported(String),
    /// Invalid argument
    InvalidArgument(String),
    /// Module resolution error
    ModuleResolution { specifier: String, message: String },
    /// Module loading error
    ModuleLoad { path: String, message: String },
    /// Custom error with code
    Custom { code: i32, message: String },
}

impl fmt::Display for VsysError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VsysError::Io(e) => write!(f, "I/O error: {}", e),
            VsysError::PermissionDenied(msg) => write!(f, "Permission denied: {}", msg),
            VsysError::NotFound(msg) => write!(f, "Not found: {}", msg),
            VsysError::NotSupported(msg) => write!(f, "Not supported: {}", msg),
            VsysError::InvalidArgument(msg) => write!(f, "Invalid argument: {}", msg),
            VsysError::ModuleResolution { specifier, message } => {
                write!(f, "Cannot resolve module '{}': {}", specifier, message)
            }
            VsysError::ModuleLoad { path, message } => {
                write!(f, "Cannot load module '{}': {}", path, message)
            }
            VsysError::Custom { code, message } => {
                write!(f, "Error (code {}): {}", code, message)
            }
        }
    }
}

impl std::error::Error for VsysError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            VsysError::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<io::Error> for VsysError {
    fn from(e: io::Error) -> Self {
        match e.kind() {
            io::ErrorKind::NotFound => VsysError::NotFound(e.to_string()),
            io::ErrorKind::PermissionDenied => VsysError::PermissionDenied(e.to_string()),
            _ => VsysError::Io(e),
        }
    }
}

// C ABI compatible error representation
#[repr(C)]
pub struct CVsysError {
    pub code: i32,
    pub message: *mut i8, // C string, caller must free
}

impl CVsysError {
    pub const OK: i32 = 0;
    pub const ERR_IO: i32 = -1;
    pub const ERR_PERMISSION_DENIED: i32 = -2;
    pub const ERR_NOT_FOUND: i32 = -3;
    pub const ERR_NOT_SUPPORTED: i32 = -4;
    pub const ERR_INVALID_ARGUMENT: i32 = -5;
    pub const ERR_MODULE_RESOLUTION: i32 = -6;
    pub const ERR_MODULE_LOAD: i32 = -7;

    pub fn ok() -> Self {
        Self {
            code: Self::OK,
            message: std::ptr::null_mut(),
        }
    }

    pub fn from_error(e: &VsysError) -> Self {
        let (code, msg) = match e {
            VsysError::Io(_) => (Self::ERR_IO, e.to_string()),
            VsysError::PermissionDenied(_) => (Self::ERR_PERMISSION_DENIED, e.to_string()),
            VsysError::NotFound(_) => (Self::ERR_NOT_FOUND, e.to_string()),
            VsysError::NotSupported(_) => (Self::ERR_NOT_SUPPORTED, e.to_string()),
            VsysError::InvalidArgument(_) => (Self::ERR_INVALID_ARGUMENT, e.to_string()),
            VsysError::ModuleResolution { .. } => (Self::ERR_MODULE_RESOLUTION, e.to_string()),
            VsysError::ModuleLoad { .. } => (Self::ERR_MODULE_LOAD, e.to_string()),
            VsysError::Custom { code, .. } => (*code, e.to_string()),
        };

        let c_string = CString::new(msg).unwrap_or_else(|_| CString::new("Unknown error").unwrap());
        Self {
            code,
            message: c_string.into_raw(),
        }
    }
}

//! Typed errors of the engine.
//!
//! Every fallible function of `cleanslate-core` returns [`CoreResult`]. Each
//! variant maps to a stable machine-readable code (see [`CoreError::code`])
//! that crosses the FFI boundary in the `{ "ok": false, "error": … }` envelope.

use std::io;
use std::path::PathBuf;

use thiserror::Error;

/// Shorthand for `Result<T, CoreError>`.
pub type CoreResult<T> = Result<T, CoreError>;

/// Every error the engine can report.
#[derive(Debug, Error)]
pub enum CoreError {
    /// The file, folder or registry key does not exist.
    #[error("not found: {location}")]
    NotFound {
        /// Path or registry key that was looked up.
        location: String,
    },

    /// Windows refused access to the item.
    #[error("access denied: {location}")]
    AccessDenied {
        /// Path or registry key that was refused.
        location: String,
    },

    /// Any other I/O failure on a file or folder.
    #[error("I/O error on {path}: {source}")]
    Io {
        /// Path being accessed.
        path: PathBuf,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },

    /// Any other failure while accessing the registry.
    #[error("registry error on {key}: {source}")]
    Registry {
        /// Full key name (`HKCU\Software\…`).
        key: String,
        /// Underlying OS error.
        #[source]
        source: io::Error,
    },

    /// The caller sent malformed input (bad JSON, bad UTF-8, invalid value).
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The target is protected by the safety rules and must not be touched.
    #[error("protected location: {location}")]
    Protected {
        /// Path or registry key that was refused.
        location: String,
    },

    /// Above 20 GB or 50 000 files, the user must type « QUARANTAINE »
    /// (specs § 9.2).
    #[error("confirmation required: {bytes} bytes, {files} files")]
    ConfirmationRequired {
        /// Total size of the plan.
        bytes: u64,
        /// Total number of files of the plan.
        files: u64,
    },

    /// A sensitive zone, by hand, outside expert mode (D79).
    #[error("expert mode required: {location}")]
    ExpertRequired {
        /// Sensitive path or key.
        location: String,
    },

    /// A sensitive zone in expert mode: the user must type the word (D79).
    #[error("type {word} to act on: {location}")]
    WordRequired {
        /// Sensitive path or key.
        location: String,
        /// Word to type, « CONFIRMER ».
        word: String,
    },

    /// The application has no usable uninstaller (design § 5).
    #[error("uninstaller not found: {location}")]
    NoUninstaller {
        /// Application or command concerned.
        location: String,
    },

    /// The uninstaller would go through a shell (safety rule 8): refused.
    #[error("uninstaller refused (shell or script host): {location}")]
    UnsafeUninstaller {
        /// Command concerned.
        location: String,
    },

    /// Another long operation is already running on this engine.
    #[error("the engine is busy with another operation")]
    Busy,

    /// The operation was cancelled by the user.
    #[error("operation cancelled")]
    Cancelled,
}

impl CoreError {
    /// Stable code sent to the UI (`ACCESS_DENIED`, `NOT_FOUND`…).
    pub fn code(&self) -> &'static str {
        match self {
            CoreError::NotFound { .. } => "NOT_FOUND",
            CoreError::AccessDenied { .. } => "ACCESS_DENIED",
            CoreError::Io { .. } => "IO_ERROR",
            CoreError::Registry { .. } => "REGISTRY_ERROR",
            CoreError::InvalidInput(_) => "INVALID_INPUT",
            CoreError::Protected { .. } => "PROTECTED",
            CoreError::ConfirmationRequired { .. } | CoreError::WordRequired { .. } => {
                "CONFIRMATION_REQUIRED"
            }
            CoreError::ExpertRequired { .. } => "EXPERT_REQUIRED",
            CoreError::NoUninstaller { .. } => "NO_UNINSTALLER",
            CoreError::UnsafeUninstaller { .. } => "UNSAFE_UNINSTALLER",
            CoreError::Busy => "BUSY",
            CoreError::Cancelled => "CANCELLED",
        }
    }

    /// Path or registry key the error is about, if any.
    pub fn location(&self) -> Option<String> {
        match self {
            CoreError::NotFound { location }
            | CoreError::AccessDenied { location }
            | CoreError::Protected { location }
            | CoreError::ExpertRequired { location }
            | CoreError::WordRequired { location, .. }
            | CoreError::NoUninstaller { location }
            | CoreError::UnsafeUninstaller { location } => Some(location.clone()),
            CoreError::Io { path, .. } => Some(path.display().to_string()),
            CoreError::Registry { key, .. } => Some(key.clone()),
            CoreError::InvalidInput(_)
            | CoreError::ConfirmationRequired { .. }
            | CoreError::Busy
            | CoreError::Cancelled => None,
        }
    }

    /// Converts an I/O error on a file or folder into the most precise variant.
    pub fn from_io(path: impl Into<PathBuf>, source: io::Error) -> Self {
        let path = path.into();
        match source.kind() {
            io::ErrorKind::NotFound => CoreError::NotFound {
                location: path.display().to_string(),
            },
            io::ErrorKind::PermissionDenied => CoreError::AccessDenied {
                location: path.display().to_string(),
            },
            _ => CoreError::Io { path, source },
        }
    }

    /// Converts an I/O error on a registry key into the most precise variant.
    pub fn from_registry(key: impl Into<String>, source: io::Error) -> Self {
        let key = key.into();
        match source.kind() {
            io::ErrorKind::NotFound => CoreError::NotFound { location: key },
            io::ErrorKind::PermissionDenied => CoreError::AccessDenied { location: key },
            _ => CoreError::Registry { key, source },
        }
    }
}

impl From<serde_json::Error> for CoreError {
    fn from(err: serde_json::Error) -> Self {
        CoreError::InvalidInput(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_not_found_maps_to_not_found() {
        let err = CoreError::from_io("C:\\x", io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(err.code(), "NOT_FOUND");
        assert_eq!(err.location().as_deref(), Some("C:\\x"));
    }

    #[test]
    fn io_permission_maps_to_access_denied() {
        let err = CoreError::from_io("C:\\x", io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(err.code(), "ACCESS_DENIED");
    }

    #[test]
    fn other_io_error_keeps_source() {
        let err = CoreError::from_io("C:\\x", io::Error::other("boom"));
        assert_eq!(err.code(), "IO_ERROR");
        assert!(err.to_string().contains("boom"));
    }

    #[test]
    fn registry_errors_are_mapped() {
        let err = CoreError::from_registry("HKCU\\X", io::Error::from(io::ErrorKind::NotFound));
        assert_eq!(err.code(), "NOT_FOUND");
        let err = CoreError::from_registry("HKCU\\X", io::Error::other("boom"));
        assert_eq!(err.code(), "REGISTRY_ERROR");
        assert_eq!(err.location().as_deref(), Some("HKCU\\X"));
    }

    #[test]
    fn json_error_is_invalid_input() {
        let json_err = serde_json::from_str::<u32>("nope").unwrap_err();
        let err: CoreError = json_err.into();
        assert_eq!(err.code(), "INVALID_INPUT");
        assert_eq!(err.location(), None);
    }
}

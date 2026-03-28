//! Non-fatal scan errors.

use std::path::PathBuf;

/// A non-fatal error encountered during scanning.
#[derive(Debug, Clone)]
pub struct ScanError {
    /// The path where the error occurred.
    pub path: PathBuf,
    /// Human-readable error description.
    pub message: String,
}

impl std::fmt::Display for ScanError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.path.display(), self.message)
    }
}

impl std::error::Error for ScanError {}

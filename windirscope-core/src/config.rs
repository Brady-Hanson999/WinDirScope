//! Scan configuration.

use std::path::PathBuf;

/// Configuration for a directory scan.
#[derive(Debug, Clone)]
pub struct ScanConfig {
    /// Root directory to scan.
    pub root: PathBuf,
    /// Number of worker threads (must be >= 1).
    pub workers: usize,
    /// Maximum directory depth to recurse into (`None` = unlimited).
    pub max_depth: Option<usize>,
    /// Whether to follow symlinks / reparse points (default: false).
    pub follow_symlinks: bool,
}

impl ScanConfig {
    /// Create a new config with sensible defaults for the given root.
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            workers: 4,
            max_depth: None,
            follow_symlinks: false,
        }
    }
}

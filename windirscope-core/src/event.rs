//! Scan events streamed to the consumer during a scan.

use std::path::PathBuf;
use std::time::Duration;

/// Progress / lifecycle events emitted by the scanner.
#[derive(Debug, Clone)]
pub enum ScanEvent {
    /// Scan has started.
    Started {
        root: PathBuf,
    },
    /// A single directory was scanned.
    DirScanned {
        path: PathBuf,
        files: u64,
        dirs: u64,
        bytes_so_far: u64,
    },
    /// A non-fatal error occurred while scanning a path.
    Error {
        path: PathBuf,
        error: String,
    },
    /// Scan finished (either completed or cancelled).
    Finished {
        stats: ScanStats,
    },
}

/// Aggregate statistics for a completed scan.
#[derive(Debug, Clone, Default)]
pub struct ScanStats {
    /// Total number of files found.
    pub total_files: u64,
    /// Total number of directories found (including root).
    pub total_dirs: u64,
    /// Total bytes (sum of file sizes).
    pub total_bytes: u64,
    /// Wall-clock duration of the scan.
    pub elapsed: Duration,
    /// Number of non-fatal errors.
    pub error_count: u64,
    /// Whether the scan was cancelled before completion.
    pub cancelled: bool,
}


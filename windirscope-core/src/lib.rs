//! windirscope-core: types, config, events, and result tree.

pub mod config;
pub mod event;
pub mod error;
pub mod tree;

pub use config::ScanConfig;
pub use error::ScanError;
pub use event::{ScanEvent, ScanStats};
pub use tree::{DirTree, FileEntry, NodeId, NodeKind};

/// Final result returned after a scan completes.
#[derive(Debug)]
pub struct ScanResult {
    /// The directory tree built during the scan.
    pub tree: DirTree,
    /// Aggregate statistics for the scan.
    pub stats: ScanStats,
    /// Non-fatal errors encountered during the scan.
    pub errors: Vec<ScanError>,
}

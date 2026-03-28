//! windirscope-scanner: directory scan engine.
//!
//! ## Public API
//!
//! ```ignore
//! let (event_rx, handle) = Scanner::start(config);
//! // consume events from event_rx …
//! let result = handle.join();
//! ```

pub mod scanner;

pub use scanner::{Scanner, ScanHandle};

//! Scanner engine — Phase B: bounded parallel scanner with coordinator
//! thread, worker pool, and crossbeam bounded channels.
//!
//! Architecture:
//! - **Coordinator thread**: owns `DirTree`, assigns `NodeId`s, receives
//!   `DirectoryResult`s from workers, enqueues new work items, emits
//!   `ScanEvent`s, and tracks the pending-work counter.
//! - **Worker threads**: receive `WorkItem`s, call `fs::read_dir`, and
//!   send back `DirectoryResult`s. Workers never touch the tree.
//!
//! Channels:
//! - `work_tx / work_rx`   — bounded(workers * 256) — coordinator → workers
//! - `result_tx / result_rx` — bounded(workers * 256) — workers → coordinator
//! - `event_tx / event_rx` — std::sync::mpsc (unbounded, public API)

use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::collections::VecDeque;
use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Instant;

use crossbeam_channel as cbc;

use windirscope_core::{
    DirTree, FileEntry, NodeKind, ScanConfig, ScanError, ScanEvent, ScanResult, ScanStats,
};

// ── internal messages ───────────────────────────────────────────────

/// A unit of work sent from coordinator to a worker.
struct WorkItem {
    /// NodeId of the parent directory in the tree (already allocated by
    /// the coordinator).
    node_id: usize,
    /// Full path to the directory to read.
    path: PathBuf,
    /// Depth relative to scan root.
    depth: u32,
}

/// Result produced by a worker after reading one directory.
struct DirectoryResult {
    /// NodeId of the directory node.
    node_id: usize,
    /// Full path of the directory that was scanned.
    path: PathBuf,
    /// Depth of this directory.
    depth: u32,
    /// Files found: (name, size).
    files: Vec<(OsString, u64)>,
    /// Subdirectories found: (name, full_path).
    subdirs: Vec<(OsString, PathBuf)>,
    /// Errors encountered reading this directory.
    errors: Vec<ScanError>,
    /// Counts for convenience.
    file_count: u64,
    dir_count: u64,
    bytes: u64,
}

// ── public API (unchanged) ──────────────────────────────────────────

/// Scanner entry point.
pub struct Scanner;

impl Scanner {
    /// Start a scan. Returns an event receiver and a handle to
    /// cancel / join the scan.
    ///
    /// Spawns a coordinator thread and `config.workers` worker threads.
    /// Events are streamed through the returned `Receiver`. Call
    /// `ScanHandle::join()` to block until completion and obtain the
    /// final `ScanResult`.
    pub fn start(config: ScanConfig) -> (Receiver<ScanEvent>, ScanHandle) {
        let (event_tx, event_rx) = mpsc::channel::<ScanEvent>();
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancelled_clone = Arc::clone(&cancelled);

        let handle = thread::spawn(move || {
            run_parallel_scan(config, event_tx, cancelled_clone)
        });

        let scan_handle = ScanHandle {
            cancelled,
            thread: Some(handle),
        };

        (event_rx, scan_handle)
    }
}

/// Handle to a running scan.
pub struct ScanHandle {
    cancelled: Arc<AtomicBool>,
    thread: Option<JoinHandle<ScanResult>>,
}

impl ScanHandle {
    /// Request cancellation of the running scan.
    ///
    /// Workers and coordinator check this flag frequently and will
    /// stop quickly. The `Finished` event will have `cancelled: true`.
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
    }

    /// Block until the scan finishes and return the result.
    pub fn join(mut self) -> ScanResult {
        self.thread
            .take()
            .expect("join called twice")
            .join()
            .expect("scanner thread panicked")
    }
}

// ── worker function ─────────────────────────────────────────────────

/// Worker loop: pull work items, read directories, push results.
fn worker_loop(
    work_rx: cbc::Receiver<WorkItem>,
    result_tx: cbc::Sender<DirectoryResult>,
    cancelled: Arc<AtomicBool>,
    follow_symlinks: bool,
) {
    while let Ok(item) = work_rx.recv() {
        // Check cancellation before doing I/O.
        if cancelled.load(Ordering::Relaxed) {
            // Still must send a result so the coordinator can
            // decrement pending for this item.
            let _ = result_tx.send(DirectoryResult {
                node_id: item.node_id,
                path: item.path,
                depth: item.depth,
                files: Vec::new(),
                subdirs: Vec::new(),
                errors: Vec::new(),
                file_count: 0,
                dir_count: 0,
                bytes: 0,
            });
            continue;
        }

        let result = scan_directory(&item, follow_symlinks, &cancelled);
        // If the result channel is closed, coordinator is gone — exit.
        if result_tx.send(result).is_err() {
            break;
        }
    }
}

/// On Windows, prepend the long-path prefix `\\?\` to support paths
/// longer than 260 characters.  Harmless for short paths.
#[cfg(windows)]
fn long_path(p: &std::path::Path) -> PathBuf {
    let s = p.to_string_lossy();
    if s.starts_with("\\\\?\\") || s.starts_with("\\\\.\\") {
        p.to_path_buf()
    } else {
        PathBuf::from(format!("\\\\?\\{}", s))
    }
}

#[cfg(not(windows))]
fn long_path(p: &std::path::Path) -> PathBuf {
    p.to_path_buf()
}

/// Read a single directory and return a `DirectoryResult`.
fn scan_directory(
    item: &WorkItem,
    follow_symlinks: bool,
    cancelled: &AtomicBool,
) -> DirectoryResult {
    let mut files = Vec::new();
    let mut subdirs = Vec::new();
    let mut errors = Vec::new();
    let mut file_count: u64 = 0;
    let mut dir_count: u64 = 0;
    let mut bytes: u64 = 0;

    // Use long-path prefix on Windows to handle paths > 260 chars.
    let read_dir = match fs::read_dir(long_path(&item.path)) {
        Ok(rd) => rd,
        Err(e) => {
            errors.push(ScanError {
                path: item.path.clone(),
                message: e.to_string(),
            });
            return DirectoryResult {
                node_id: item.node_id,
                path: item.path.clone(),
                depth: item.depth,
                files,
                subdirs,
                errors,
                file_count,
                dir_count,
                bytes,
            };
        }
    };

    for entry_result in read_dir {
        // Check cancellation while iterating entries.
        if cancelled.load(Ordering::Relaxed) {
            break;
        }

        let entry = match entry_result {
            Ok(e) => e,
            Err(e) => {
                errors.push(ScanError {
                    path: item.path.clone(),
                    message: e.to_string(),
                });
                continue;
            }
        };

        // Get entry metadata.  On Windows DirEntry::metadata() does
        // NOT follow reparse points (uses WIN32_FIND_DATA).
        let metadata = match entry.metadata() {
            Ok(m) => m,
            Err(e) => {
                errors.push(ScanError {
                    path: item.path.join(entry.file_name()),
                    message: e.to_string(),
                });
                continue;
            }
        };

        // On Windows, skip ALL reparse points (symlinks, junctions,
        // mount points) to prevent infinite loops and double-counting.
        #[cfg(windows)]
        {
            if !follow_symlinks {
                use std::os::windows::fs::MetadataExt;
                const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
                if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
                    continue;
                }
            }
        }

        let ft = metadata.file_type();

        // On non-Windows, skip symlinks unless configured.
        #[cfg(not(windows))]
        {
            if ft.is_symlink() && !follow_symlinks {
                continue;
            }
        }

        if ft.is_dir() {
            // Build child path from parent + name (avoids \\?\ leak).
            let child_path = item.path.join(entry.file_name());
            subdirs.push((entry.file_name(), child_path));
            dir_count += 1;
        } else if ft.is_file() {
            let size = metadata.len();
            files.push((entry.file_name(), size));
            file_count += 1;
            bytes += size;
        }
        // else: skip other entry types (devices, pipes, etc.)
    }

    DirectoryResult {
        node_id: item.node_id,
        path: item.path.clone(),
        depth: item.depth,
        files,
        subdirs,
        errors,
        file_count,
        dir_count,
        bytes,
    }
}

// ── Windows privilege escalation ─────────────────────────────────────

/// On Windows, enable `SeBackupPrivilege` on the current process token.
/// This allows reading files and directories regardless of their DACL,
/// which is necessary even for Administrator processes because the
/// privilege exists on the token but is **disabled** by default.
///
/// If the privilege cannot be enabled (e.g. running non-elevated) this
/// is a silent no-op — the scan will still work but may get
/// "Access denied" on certain system-protected folders.
#[cfg(windows)]
fn enable_backup_privilege() {
    #[repr(C)]
    #[allow(non_snake_case, non_camel_case_types)]
    struct LUID {
        LowPart: u32,
        HighPart: i32,
    }

    #[repr(C)]
    #[allow(non_snake_case, non_camel_case_types)]
    struct LUID_AND_ATTRIBUTES {
        Luid: LUID,
        Attributes: u32,
    }

    #[repr(C)]
    #[allow(non_snake_case, non_camel_case_types)]
    struct TOKEN_PRIVILEGES {
        PrivilegeCount: u32,
        Privileges: [LUID_AND_ATTRIBUTES; 1],
    }

    #[link(name = "advapi32")]
    extern "system" {
        fn LookupPrivilegeValueW(
            lpSystemName: *const u16,
            lpName: *const u16,
            lpLuid: *mut LUID,
        ) -> i32;

        fn AdjustTokenPrivileges(
            TokenHandle: *mut std::ffi::c_void,
            DisableAllPrivileges: i32,
            NewState: *const TOKEN_PRIVILEGES,
            BufferLength: u32,
            PreviousState: *mut TOKEN_PRIVILEGES,
            ReturnLength: *mut u32,
        ) -> i32;

        fn OpenProcessToken(
            ProcessHandle: *mut std::ffi::c_void,
            DesiredAccess: u32,
            TokenHandle: *mut *mut std::ffi::c_void,
        ) -> i32;
    }

    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> *mut std::ffi::c_void;
        fn CloseHandle(hObject: *mut std::ffi::c_void) -> i32;
    }

    use std::os::windows::ffi::OsStrExt;
    fn to_wide(s: &str) -> Vec<u16> {
        std::ffi::OsStr::new(s)
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    const TOKEN_ADJUST_PRIVILEGES: u32 = 0x0020;
    const TOKEN_QUERY: u32 = 0x0008;
    const SE_PRIVILEGE_ENABLED: u32 = 0x0000_0002;

    unsafe {
        let mut token: *mut std::ffi::c_void = std::ptr::null_mut();
        if OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY,
            &mut token,
        ) == 0
        {
            return; // can't open our own token — give up silently
        }

        // Enable SeBackupPrivilege
        let priv_name = to_wide("SeBackupPrivilege");
        let mut luid = LUID { LowPart: 0, HighPart: 0 };
        if LookupPrivilegeValueW(std::ptr::null(), priv_name.as_ptr(), &mut luid) != 0 {
            let tp = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            AdjustTokenPrivileges(
                token,
                0,
                &tp,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }

        // Also enable SeRestorePrivilege (helps with some edge cases)
        let priv_name2 = to_wide("SeRestorePrivilege");
        let mut luid2 = LUID { LowPart: 0, HighPart: 0 };
        if LookupPrivilegeValueW(std::ptr::null(), priv_name2.as_ptr(), &mut luid2) != 0 {
            let tp2 = TOKEN_PRIVILEGES {
                PrivilegeCount: 1,
                Privileges: [LUID_AND_ATTRIBUTES {
                    Luid: luid2,
                    Attributes: SE_PRIVILEGE_ENABLED,
                }],
            };
            AdjustTokenPrivileges(
                token,
                0,
                &tp2,
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            );
        }

        CloseHandle(token);
    }
}

#[cfg(not(windows))]
fn enable_backup_privilege() {
    // No-op on non-Windows platforms.
}

// ── coordinator (runs on the thread spawned by Scanner::start) ──────

fn run_parallel_scan(
    config: ScanConfig,
    event_tx: Sender<ScanEvent>,
    cancelled: Arc<AtomicBool>,
) -> ScanResult {
    // Enable SeBackupPrivilege so we can read protected directories.
    enable_backup_privilege();

    let start_time = Instant::now();
    let num_workers = config.workers.max(1);
    let chan_cap = num_workers * 256;

    // Bounded channels.
    let (work_tx, work_rx) = cbc::bounded::<WorkItem>(chan_cap);
    let (result_tx, result_rx) = cbc::bounded::<DirectoryResult>(chan_cap);

    // Emit Started.
    let _ = event_tx.send(ScanEvent::Started {
        root: config.root.clone(),
    });

    // Spawn workers.
    let mut worker_handles: Vec<JoinHandle<()>> = Vec::with_capacity(num_workers);
    for _ in 0..num_workers {
        let wrx = work_rx.clone();
        let rtx = result_tx.clone();
        let canc = Arc::clone(&cancelled);
        let follow = config.follow_symlinks;
        worker_handles.push(thread::spawn(move || {
            worker_loop(wrx, rtx, canc, follow);
        }));
    }
    // Drop our copy of result_tx so the only senders are workers.
    drop(result_tx);
    // Drop our extra copy of work_rx (coordinator only sends).
    drop(work_rx);

    // Build tree.
    let mut tree = DirTree::new();
    let mut all_errors: Vec<ScanError> = Vec::new();
    let mut stats = ScanStats::default();

    // Add root node.
    let root_name = config.root.to_string_lossy().into_owned();
    let root_id = tree.add_node(root_name, NodeKind::Directory, None, 0, 0);
    stats.total_dirs = 1;

    // Enqueue root work item.
    // pending tracks how many DirectoryResults we still expect.
    let mut pending: usize = 0;

    // Local queue of work items waiting to be sent to workers.
    // This decouples "deciding to scan a directory" from "getting it
    // into the bounded channel", preventing deadlock.
    let mut outbox: VecDeque<WorkItem> = VecDeque::new();

    // Seed with the root directory.
    outbox.push_back(WorkItem {
        node_id: root_id,
        path: config.root.clone(),
        depth: 0,
    });
    pending += 1;

    // Coordinator loop: use select! to simultaneously try to:
    //   (a) send the next outbox item into the work channel, AND
    //   (b) receive a result from a worker.
    // This avoids the deadlock where the coordinator blocks on a full
    // work channel while workers block on a full result channel.
    while pending > 0 {
        // If there are items to dispatch, try both send and recv.
        // If outbox is empty, only recv.
        if let Some(front) = outbox.pop_front() {
            cbc::select! {
                send(work_tx, front) -> res => {
                    if res.is_err() {
                        // Workers gone — shouldn't happen while pending > 0.
                        pending -= 1;
                    }
                    // Try to drain a result without blocking (the send
                    // succeeded so maybe a result is ready too).
                    while let Ok(dir_result) = result_rx.try_recv() {
                        process_result(
                            dir_result,
                            &config,
                            &cancelled,
                            &mut tree,
                            &mut stats,
                            &mut all_errors,
                            &mut pending,
                            &mut outbox,
                            &event_tx,
                        );
                    }
                }
                recv(result_rx) -> msg => {
                    // Put the work item back — we didn't send it yet.
                    outbox.push_front(front);
                    match msg {
                        Ok(dir_result) => {
                            process_result(
                                dir_result,
                                &config,
                                &cancelled,
                                &mut tree,
                                &mut stats,
                                &mut all_errors,
                                &mut pending,
                                &mut outbox,
                                &event_tx,
                            );
                        }
                        Err(_) => break,
                    }
                }
            }
        } else {
            // Nothing to send — just wait for a result.
            match result_rx.recv() {
                Ok(dir_result) => {
                    process_result(
                        dir_result,
                        &config,
                        &cancelled,
                        &mut tree,
                        &mut stats,
                        &mut all_errors,
                        &mut pending,
                        &mut outbox,
                        &event_tx,
                    );
                }
                Err(_) => break,
            }
        }
    }

    // All work is done. Drop the work sender so workers' recv() returns
    // Err and they exit.
    drop(work_tx);

    // Join worker threads.
    for wh in worker_handles {
        let _ = wh.join();
    }

    // Bottom-up aggregation.
    tree.compute_cumulative_sizes();

    stats.elapsed = start_time.elapsed();
    stats.cancelled = cancelled.load(Ordering::SeqCst);

    // Emit Finished — always.
    let _ = event_tx.send(ScanEvent::Finished {
        stats: stats.clone(),
    });

    ScanResult {
        tree,
        stats,
        errors: all_errors,
    }
}

/// Process one `DirectoryResult` on the coordinator thread: update
/// tree, stats, errors, enqueue new work into outbox, emit events.
#[allow(clippy::too_many_arguments)]
fn process_result(
    dir_result: DirectoryResult,
    config: &ScanConfig,
    cancelled: &AtomicBool,
    tree: &mut DirTree,
    stats: &mut ScanStats,
    all_errors: &mut Vec<ScanError>,
    pending: &mut usize,
    outbox: &mut VecDeque<WorkItem>,
    event_tx: &Sender<ScanEvent>,
) {
    // Decrement pending for this completed directory.
    *pending -= 1;

    let is_cancelled = cancelled.load(Ordering::SeqCst);

    // Record errors.
    for err in &dir_result.errors {
        let _ = event_tx.send(ScanEvent::Error {
            path: err.path.clone(),
            error: err.message.clone(),
        });
        stats.error_count += 1;
    }
    all_errors.extend(dir_result.errors);

    // Instead of adding individual file nodes, populate top-K files
    // on the directory node and store the directory's direct-file
    // bytes in `size`. This keeps memory bounded.
    const TOP_K: usize = 50;
    let child_depth = dir_result.depth + 1;

    {
        let mut heap: BinaryHeap<Reverse<(u64, String)>> = BinaryHeap::new();
        let mut other_bytes: u64 = 0;
        let mut other_count: u64 = 0;

        for (name, size) in &dir_result.files {
            let name_str = name.to_string_lossy().into_owned();
            if heap.len() < TOP_K {
                heap.push(Reverse((*size, name_str)));
            } else if let Some(&Reverse((min_size, _))) = heap.peek() {
                if *size > min_size {
                    let Reverse((evicted_size, _)) = heap.pop().unwrap();
                    other_bytes += evicted_size;
                    other_count += 1;
                    heap.push(Reverse((*size, name_str)));
                } else {
                    other_bytes += *size;
                    other_count += 1;
                }
            }
        }

        let mut top_files: Vec<FileEntry> = heap
            .into_iter()
            .map(|Reverse((bytes, name))| FileEntry { name, bytes })
            .collect();
        top_files.sort_by(|a, b| b.bytes.cmp(&a.bytes));

        let dir_node = &mut tree.nodes[dir_result.node_id];
        dir_node.size = dir_result.bytes; // total bytes of direct files
        dir_node.top_files = top_files;
        dir_node.other_files_bytes = other_bytes;
        dir_node.other_files_count = other_count;
    }

    stats.total_files += dir_result.file_count;
    stats.total_bytes += dir_result.bytes;

    // Enqueue subdirectories into the local outbox (unless cancelled
    // or at max depth).
    if !is_cancelled {
        let within_depth = match config.max_depth {
            Some(max) => (dir_result.depth as usize) < max,
            None => true,
        };

        if within_depth {
            for (name, full_path) in &dir_result.subdirs {
                let child_id = tree.add_node(
                    name.to_string_lossy().into_owned(),
                    NodeKind::Directory,
                    Some(dir_result.node_id),
                    0,
                    child_depth,
                );
                stats.total_dirs += 1;

                // pending += 1 BEFORE adding to outbox.
                *pending += 1;
                outbox.push_back(WorkItem {
                    node_id: child_id,
                    path: full_path.clone(),
                    depth: child_depth,
                });
            }
        }
    }

    // Emit DirScanned event.
    let _ = event_tx.send(ScanEvent::DirScanned {
        path: dir_result.path,
        files: dir_result.file_count,
        dirs: dir_result.dir_count,
        bytes_so_far: stats.total_bytes,
    });
}

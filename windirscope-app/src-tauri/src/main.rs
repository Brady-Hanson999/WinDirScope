#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod treemap;
mod graph;

use std::sync::{Arc, Mutex};
use std::thread;

use serde::Serialize;
use tauri::{AppHandle, Manager, State};

use windirscope_core::{DirTree, ScanConfig, ScanEvent};
use std::path::Path;
use windirscope_scanner::{ScanHandle, Scanner};

// ── State shared across commands ────────────────────────────────────

struct AppState {
    /// Currently running scan handle, if any.
    scan_handle: Mutex<Option<Arc<Mutex<Option<ScanHandle>>>>>,
    /// Last completed scan tree (for on-demand treemap generation).
    last_tree: Mutex<Option<DirTree>>,
}

// ── Payloads emitted to the frontend ────────────────────────────────

#[derive(Clone, Serialize)]
struct ScanStartedPayload {
    root: String,
}

#[derive(Clone, Serialize)]
struct ScanProgressPayload {
    dirs_scanned: u64,
    total_files: u64,
    total_bytes: u64,
    errors: u64,
    current_path: String,
}

#[derive(Clone, Serialize)]
struct ScanErrorPayload {
    path: String,
    message: String,
}

#[derive(Clone, Serialize)]
struct ScanFinishedPayload {
    total_files: u64,
    total_dirs: u64,
    total_bytes: u64,
    elapsed_secs: f64,
    error_count: u64,
    cancelled: bool,
    /// Number of directories skipped due to access denial.
    skipped_dirs: u64,
    /// Sample of skipped directory paths (first 50).
    skipped_paths: Vec<String>,
}

// ── Helpers ─────────────────────────────────────────────────────────

/// Normalise a user-supplied path into a usable `PathBuf`.
///
/// On Windows, bare `C:` means "current directory on that drive"
/// which is almost never what the user wants.  We append `\` to
/// turn it into the drive root.
fn normalize_root(raw: &str) -> Result<std::path::PathBuf, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Path cannot be empty".into());
    }
    let mut p = std::path::PathBuf::from(trimmed);

    // "C:" → "C:\" (drive root, not cwd-on-drive).
    let s = p.to_string_lossy().to_string();
    if s.len() == 2 && s.as_bytes()[0].is_ascii_alphabetic() && s.as_bytes()[1] == b':' {
        p = std::path::PathBuf::from(format!("{}\\", s));
    }

    if !p.exists() {
        return Err(format!("Path does not exist: {}", p.display()));
    }
    if !p.is_dir() {
        return Err(format!("Path is not a directory: {}", p.display()));
    }
    Ok(p)
}

fn format_bytes_rs(b: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;
    if b >= TB { format!("{:.2} TB", b as f64 / TB as f64) }
    else if b >= GB { format!("{:.2} GB", b as f64 / GB as f64) }
    else if b >= MB { format!("{:.2} MB", b as f64 / MB as f64) }
    else if b >= KB { format!("{:.2} KB", b as f64 / KB as f64) }
    else { format!("{} B", b) }
}

// ── Tauri Commands ──────────────────────────────────────────────────

#[tauri::command]
fn start_scan(
    path: String,
    workers: Option<u32>,
    depth: Option<u32>,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<(), String> {
    // Don't allow two scans at once.
    {
        let guard = state.scan_handle.lock().unwrap();
        if guard.is_some() {
            return Err("A scan is already running".into());
        }
    }

    let root = normalize_root(&path)?;
    eprintln!("[WinDirScope] Scan root: {}", root.display());
    let mut config = ScanConfig::new(root);
    if let Some(w) = workers {
        config.workers = w.max(1) as usize;
    }
    if let Some(d) = depth {
        config.max_depth = Some(d as usize);
    }

    let (event_rx, handle) = Scanner::start(config);

    // Store the handle wrapped in Arc<Mutex<Option<...>>> so cancel
    // can take it, and the bg thread can also take it to join.
    let handle_slot: Arc<Mutex<Option<ScanHandle>>> = Arc::new(Mutex::new(Some(handle)));

    // Store reference in app state so cancel_scan can access it.
    {
        let mut guard = state.scan_handle.lock().unwrap();
        *guard = Some(Arc::clone(&handle_slot));
    }

    // Spawn background thread: drain events, forward to frontend,
    // then join the scan handle and emit finished with top dirs.
    let app_handle = app.clone();
    thread::spawn(move || {
        let mut dirs_scanned: u64 = 0;
        let mut total_files: u64 = 0;
        #[allow(unused_assignments)]
        let mut total_bytes: u64 = 0;
        let mut error_count: u64 = 0;

        for event in event_rx {
            match event {
                ScanEvent::Started { root } => {
                    let _ = app_handle.emit_all(
                        "scan_started",
                        ScanStartedPayload {
                            root: root.display().to_string(),
                        },
                    );
                }
                ScanEvent::DirScanned {
                    path,
                    files,
                    dirs: _,
                    bytes_so_far,
                } => {
                    dirs_scanned += 1;
                    total_files += files;
                    total_bytes = bytes_so_far;
                    // Throttle: emit every 50 dirs to avoid flooding
                    // the webview.
                    if dirs_scanned % 50 == 0 || dirs_scanned <= 5 {
                        let _ = app_handle.emit_all(
                            "scan_progress",
                            ScanProgressPayload {
                                dirs_scanned,
                                total_files,
                                total_bytes,
                                errors: error_count,
                                current_path: path.display().to_string(),
                            },
                        );
                    }
                }
                ScanEvent::Error { path, error } => {
                    error_count += 1;
                    let _ = app_handle.emit_all(
                        "scan_error",
                        ScanErrorPayload {
                            path: path.display().to_string(),
                            message: error,
                        },
                    );
                }
                ScanEvent::Finished { .. } => {
                    // We'll emit our own finished after joining.
                }
            }
        }

        // event_rx is drained — the scan coordinator thread has
        // finished. Now join to get ScanResult.
        let handle_opt = {
            let mut slot = handle_slot.lock().unwrap();
            slot.take()
        };

        if let Some(h) = handle_opt {
            let result = h.join();
            let stats = &result.stats;
            let tree = &result.tree;

            // Store the tree for on-demand treemap generation.
            if let Some(app_state) = app_handle.try_state::<AppState>() {
                let mut guard = app_state.last_tree.lock().unwrap();
                *guard = Some(tree.clone());
            }

            // Classify access-denied / permission errors.
            let denied: Vec<String> = result
                .errors
                .iter()
                .filter(|e| {
                    let m = e.message.to_lowercase();
                    m.contains("access") || m.contains("denied") || m.contains("permission")
                })
                .map(|e| e.path.display().to_string())
                .collect();
            let skipped_dirs = denied.len() as u64;
            let skipped_paths: Vec<String> = denied.into_iter().take(50).collect();

            // Log root children for verification.
            if !tree.nodes.is_empty() {
                let root_node = &tree.nodes[0];
                eprintln!("── Root children of '{}' ──", root_node.name);
                let mut root_kids: Vec<(String, u64)> = root_node
                    .children
                    .iter()
                    .map(|&cid| {
                        (
                            tree.nodes[cid].name.clone(),
                            tree.nodes[cid].cumulative_size,
                        )
                    })
                    .collect();
                root_kids.sort_by(|a, b| b.1.cmp(&a.1));
                for (name, size) in &root_kids {
                    eprintln!("  {:>12}  {}", format_bytes_rs(*size), name);
                }
                eprintln!("── {} children total ──", root_kids.len());
            }

            let _ = app_handle.emit_all(
                "scan_finished",
                ScanFinishedPayload {
                    total_files: stats.total_files,
                    total_dirs: stats.total_dirs,
                    total_bytes: stats.total_bytes,
                    elapsed_secs: stats.elapsed.as_secs_f64(),
                    error_count: stats.error_count,
                    cancelled: stats.cancelled,
                    skipped_dirs,
                    skipped_paths,
                },
            );
        }

        // Clear the app state so a new scan can start.
        if let Some(app_state) = app_handle.try_state::<AppState>() {
            let mut guard = app_state.scan_handle.lock().unwrap();
            *guard = None;
        }
    });

    Ok(())
}

#[tauri::command]
fn cancel_scan(state: State<'_, AppState>) -> Result<(), String> {
    let guard = state.scan_handle.lock().unwrap();
    match guard.as_ref() {
        Some(slot) => {
            let inner = slot.lock().unwrap();
            if let Some(ref h) = *inner {
                h.cancel();
                Ok(())
            } else {
                Err("Scan already finished".into())
            }
        }
        None => Err("No scan is running".into()),
    }
}

// ── Treemap command ─────────────────────────────────────────────────

#[tauri::command]
fn get_unified_treemap(
    max_rects: u32,
    depth_limit: Option<u32>,
    root_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<treemap::TreemapRect>, String> {
    let guard = state.last_tree.lock().unwrap();
    match guard.as_ref() {
        Some(tree) => {
            let mut start_node = 0;
            if let Some(ref p) = root_path {
                // Find node by path
                let mut found = false;
                for i in 0..tree.nodes.len() {
                    // Only dirs can be roots for our layout conceptually
                    if tree.nodes[i].kind == windirscope_core::NodeKind::Directory 
                       && tree.full_path(i).display().to_string() == *p {
                        start_node = i;
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(format!("Could not find directory {} in tree", p));
                }
            }
            Ok(treemap::unified_layout(
                tree,
                start_node,
                max_rects.max(1) as usize,
                depth_limit,
            ))
        }
        None => Err("No scan result available. Run a scan first.".into()),
    }
}

// ── Graph command ───────────────────────────────────────────────────

#[tauri::command]
fn get_graph_data(
    max_nodes: u32,
    depth_limit: Option<u32>,
    root_path: Option<String>,
    state: State<'_, AppState>,
) -> Result<graph::ForceGraphPayload, String> {
    let guard = state.last_tree.lock().unwrap();
    match guard.as_ref() {
        Some(tree) => {
            let mut start_node = 0;
            if let Some(ref p) = root_path {
                let mut found = false;
                for i in 0..tree.nodes.len() {
                    if tree.nodes[i].kind == windirscope_core::NodeKind::Directory 
                       && tree.full_path(i).display().to_string() == *p {
                        start_node = i;
                        found = true;
                        break;
                    }
                }
                if !found {
                    return Err(format!("Could not find directory {} in tree", p));
                }
            }
            Ok(graph::build_force_graph(
                tree,
                start_node,
                max_nodes.max(1) as usize,
                depth_limit,
            ))
        }
        None => Err("No scan result available. Run a scan first.".into()),
    }
}

// ── Explorer integration ────────────────────────────────────────────

#[tauri::command]
fn show_in_explorer(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if p.is_dir() {
        std::process::Command::new("explorer")
            .arg(&path)
            .spawn()
            .map_err(|e| format!("Failed to open explorer: {}", e))?;
    } else {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", path))
            .spawn()
            .map_err(|e| format!("Failed to open explorer: {}", e))?;
    }
    Ok(())
}

// ── Safe file deletion ──────────────────────────────────────────────

/// Check whether a path is safe to delete.
/// Returns Ok(()) if safe, Err(reason) if blocked.
fn check_path_safety(path_str: &str) -> Result<(), String> {
    let path = std::path::Path::new(path_str);

    // ── Layer 1: Must exist ─────────────────────────────────────
    if !path.exists() {
        return Err(format!("Path does not exist: {}", path_str));
    }

    // Canonicalize for reliable comparison
    let canonical = path.canonicalize()
        .map_err(|e| format!("Cannot resolve path: {}", e))?;
    let canon_str = canonical.to_string_lossy().to_string();
    // Strip UNC prefix (\\?\) that canonicalize adds on Windows
    let clean = if canon_str.starts_with("\\\\?\\") {
        &canon_str[4..]
    } else {
        &canon_str
    };
    let lower = clean.to_lowercase().replace('/', "\\");

    // ── Layer 2: Drive root protection ──────────────────────────
    // Block "C:\", "D:\", etc.
    if lower.len() <= 3
        && lower.as_bytes().first().map_or(false, |b| b.is_ascii_alphabetic())
        && lower.ends_with(":\\")
    {
        return Err("Cannot delete a drive root.".into());
    }
    // Also block bare "C:" (2 chars)
    if lower.len() == 2
        && lower.as_bytes()[0].is_ascii_alphabetic()
        && lower.as_bytes()[1] == b':'
    {
        return Err("Cannot delete a drive root.".into());
    }

    // ── Layer 3: Top-level directory guard ───────────────────────
    // Anything that is a direct child of a drive root is blocked.
    // e.g. C:\Users, C:\Windows, C:\anything
    {
        let components: Vec<_> = std::path::Path::new(&lower).components().collect();
        // components[0] = Prefix (C:), components[1] = RootDir (\), components[2] = first dir
        if components.len() <= 3 && canonical.is_dir() {
            return Err(format!(
                "Cannot delete top-level directory '{}'. This could be a critical system folder.",
                clean
            ));
        }
    }

    // ── Layer 4: Blocked path prefixes ──────────────────────────
    let blocked_prefixes: Vec<&str> = vec![
        "c:\\windows",
        "c:\\program files",
        "c:\\program files (x86)",
        "c:\\programdata",
        "c:\\$recycle.bin",
        "c:\\system volume information",
        "c:\\recovery",
        "c:\\$sysreset",
        "c:\\config.msi",
        "c:\\boot",
        "c:\\efi",
        "c:\\$windows.~bt",
        "c:\\$windows.~ws",
        "c:\\inetpub",
        "c:\\perflogs",
        "c:\\proclogs",
        "c:\\msocache",
        "c:\\documents and settings",
    ];

    for prefix in &blocked_prefixes {
        if lower.starts_with(prefix) {
            return Err(format!(
                "Cannot delete '{}' — this is a protected system path.",
                clean
            ));
        }
    }

    // Block AppData core directories (allow subfolders deeper inside)
    // e.g. block C:\Users\X\AppData itself, C:\Users\X\AppData\Local itself
    // but allow C:\Users\X\AppData\Local\Temp\somefile.txt
    let appdata_roots: Vec<&str> = vec![
        "\\appdata\\local\\microsoft",
        "\\appdata\\local\\packages",
        "\\appdata\\roaming\\microsoft",
    ];
    for adr in &appdata_roots {
        if lower.contains(adr) {
            return Err(format!(
                "Cannot delete '{}' — this is inside a protected AppData directory.",
                clean
            ));
        }
    }

    // Block the AppData folder itself and its immediate children
    if lower.contains("\\appdata") {
        // Count segments after "appdata"
        if let Some(pos) = lower.find("\\appdata") {
            let after = &lower[pos + 8..]; // after "\appdata"
            let depth_after: usize = after.matches('\\').count();
            // "\appdata" alone, or "\appdata\local", "\appdata\roaming", "\appdata\locallow"
            if depth_after <= 1 {
                return Err(format!(
                    "Cannot delete '{}' — AppData directories are protected.",
                    clean
                ));
            }
        }
    }

    // ── Layer 5: System file name patterns ──────────────────────
    let filename = canonical
        .file_name()
        .map(|f| f.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let blocked_filenames: Vec<&str> = vec![
        "pagefile.sys",
        "hiberfil.sys",
        "swapfile.sys",
        "bootmgr",
        "bootnxt",
        "ntldr",
        "ntdetect.com",
        "io.sys",
        "msdos.sys",
        "ntuser.dat",
        "ntuser.dat.log",
        "ntuser.dat.log1",
        "ntuser.dat.log2",
        "ntuser.ini",
        "usrclass.dat",
        "usrclass.dat.log",
        "usrclass.dat.log1",
        "usrclass.dat.log2",
        "desktop.ini",
    ];

    if blocked_filenames.contains(&filename.as_str()) {
        return Err(format!(
            "Cannot delete '{}' — this is a protected system file.",
            filename
        ));
    }

    // Also block ntuser.* pattern
    if filename.starts_with("ntuser.") {
        return Err(format!(
            "Cannot delete '{}' — NTUSER files are protected.",
            filename
        ));
    }

    // ── Layer 6: Windows SYSTEM file attribute ──────────────────
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if let Ok(meta) = canonical.metadata() {
            const FILE_ATTRIBUTE_SYSTEM: u32 = 0x00000004;
            if meta.file_attributes() & FILE_ATTRIBUTE_SYSTEM != 0 {
                return Err(format!(
                    "Cannot delete '{}' — Windows marks this as a SYSTEM file.",
                    clean
                ));
            }
        }
    }

    // ── Layer 7: Don't delete our own app ───────────────────────
    if let Ok(exe) = std::env::current_exe() {
        if let Ok(exe_canon) = exe.canonicalize() {
            let exe_lower = exe_canon.to_string_lossy().to_lowercase();
            if exe_lower.starts_with(&lower) || lower.starts_with(&exe_lower.replace('/', "\\")) {
                return Err("Cannot delete WinDirScope's own files.".into());
            }
        }
    }

    Ok(())
}

fn prune_tree_path(tree: &mut windirscope_core::DirTree, removed_path: &str) {
    let p = std::path::Path::new(removed_path);
    let parent_path = p.parent();
    let file_name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");

    let mut removed_size = 0;
    let mut bubble_up_from = None;

    // Attempt 1: The deleted path is a directory (has a matching TreeNode)
    for i in 0..tree.nodes.len() {
        if tree.nodes[i].name == file_name && tree.full_path(i).display().to_string() == removed_path {
            removed_size = tree.nodes[i].cumulative_size;
            tree.nodes[i].size = 0;
            tree.nodes[i].cumulative_size = 0;
            tree.nodes[i].children.clear();
            tree.nodes[i].top_files.clear();
            bubble_up_from = tree.nodes[i].parent;
            break;
        }
    }

    // Attempt 2: The deleted path is a file (exists in parent's top_files)
    if removed_size == 0 {
        if let Some(parent) = parent_path {
            let parent_str = parent.display().to_string();
            for i in 0..tree.nodes.len() {
                if tree.full_path(i).display().to_string() == parent_str {
                    if let Some(idx) = tree.nodes[i].top_files.iter().position(|f| f.name == file_name) {
                        removed_size = tree.nodes[i].top_files[idx].bytes;
                        tree.nodes[i].top_files.remove(idx);
                        tree.nodes[i].size = tree.nodes[i].size.saturating_sub(removed_size);
                        tree.nodes[i].cumulative_size = tree.nodes[i].cumulative_size.saturating_sub(removed_size);
                        bubble_up_from = tree.nodes[i].parent;
                        break;
                    }
                }
            }
        }
    }

    // Bubble up the size reduction to ancestors
    if removed_size > 0 {
        let mut cur = bubble_up_from;
        while let Some(pid) = cur {
            tree.nodes[pid].cumulative_size = tree.nodes[pid].cumulative_size.saturating_sub(removed_size);
            cur = tree.nodes[pid].parent;
        }
    }
}

/// Delete a file or folder by moving it to the Recycle Bin.
#[tauri::command]
fn delete_path(path: String, state: tauri::State<'_, AppState>) -> Result<String, String> {
    // First, run all safety checks
    check_path_safety(&path)?;

    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        // SHFILEOPSTRUCTW for SHFileOperationW
        #[repr(C)]
        #[allow(non_snake_case, non_camel_case_types)]
        struct SHFILEOPSTRUCTW {
            hwnd: *mut std::ffi::c_void,
            wFunc: u32,
            pFrom: *const u16,
            pTo: *const u16,
            fFlags: u16,
            fAnyOperationsAborted: i32,
            hNameMappings: *mut std::ffi::c_void,
            lpszProgressTitle: *const u16,
        }

        #[link(name = "shell32")]
        extern "system" {
            fn SHFileOperationW(lpFileOp: *mut SHFILEOPSTRUCTW) -> i32;
        }

        const FO_DELETE: u32 = 0x0003;
        const FOF_ALLOWUNDO: u16 = 0x0040;       // Send to Recycle Bin
        const FOF_NOCONFIRMATION: u16 = 0x0010;   // Don't show OS confirmation (we have our own)
        const FOF_SILENT: u16 = 0x0004;           // No progress dialog
        const FOF_NOERRORUI: u16 = 0x0400;        // No error UI

        // SHFileOperation requires double-null-terminated string
        let wide: Vec<u16> = std::ffi::OsStr::new(&path)
            .encode_wide()
            .chain(std::iter::once(0))
            .chain(std::iter::once(0))
            .collect();

        let mut op = SHFILEOPSTRUCTW {
            hwnd: std::ptr::null_mut(),
            wFunc: FO_DELETE,
            pFrom: wide.as_ptr(),
            pTo: std::ptr::null(),
            fFlags: FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_SILENT | FOF_NOERRORUI,
            fAnyOperationsAborted: 0,
            hNameMappings: std::ptr::null_mut(),
            lpszProgressTitle: std::ptr::null(),
        };

        let result = unsafe { SHFileOperationW(&mut op) };

        if result != 0 {
            return Err(format!(
                "Failed to move to Recycle Bin (error code: 0x{:X}). The file may be in use or protected.",
                result
            ));
        }

        if op.fAnyOperationsAborted != 0 {
            return Err("Operation was cancelled.".into());
        }

        if let Ok(mut guard) = state.last_tree.lock() {
            if let Some(ref mut tree) = *guard {
                prune_tree_path(tree, &path);
            }
        }

        Ok(format!("Moved to Recycle Bin: {}", path))
    }

    #[cfg(not(windows))]
    {
        // Fallback for non-Windows: actual delete
        let p = std::path::Path::new(&path);
        if p.is_dir() {
            std::fs::remove_dir_all(p)
                .map_err(|e| format!("Failed to delete directory: {}", e))?;
        } else {
            std::fs::remove_file(p)
                .map_err(|e| format!("Failed to delete file: {}", e))?;
        }
        
        if let Ok(mut guard) = state.last_tree.lock() {
            if let Some(ref mut tree) = *guard {
                prune_tree_path(tree, &path);
            }
        }

        Ok(format!("Deleted: {}", path))
    }
}

/// Check if a path is safe to delete (frontend can call this to show/hide the delete option).
#[tauri::command]
fn check_delete_safety(path: String) -> Result<(), String> {
    check_path_safety(&path)
}

/// List available drive letters (Windows only, instant via WinAPI).
#[tauri::command]
fn list_drives() -> Vec<String> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLogicalDrives() -> u32;
        }
        let mask = unsafe { GetLogicalDrives() };
        (0..26u32)
            .filter(|i| mask & (1 << i) != 0)
            .map(|i| format!("{}:\\", (b'A' + i as u8) as char))
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec!["/".to_string()]
    }
}

/// A child entry of the scan root, for Root Folder Contents display.
#[derive(Clone, Serialize)]
struct RootChild {
    path: String,
    bytes: u64,
}

/// Return the immediate children of the scan root with their sizes.
/// Used by the frontend for validation.
#[tauri::command]
fn get_root_children(state: State<'_, AppState>) -> Result<Vec<RootChild>, String> {
    let guard = state.last_tree.lock().unwrap();
    match guard.as_ref() {
        Some(tree) => {
            if tree.nodes.is_empty() {
                return Ok(Vec::new());
            }
            let root = &tree.nodes[0];
            let mut children: Vec<RootChild> = root
                .children
                .iter()
                .map(|&cid| RootChild {
                    path: tree.nodes[cid].name.clone(),
                    bytes: tree.nodes[cid].cumulative_size,
                })
                .collect();
            children.sort_by(|a, b| b.bytes.cmp(&a.bytes));
            Ok(children)
        }
        None => Err("No scan result available.".into()),
    }
}



// ── Elevation (run as admin) ─────────────────────────────────────────

/// Check whether the current process is running with administrator
/// privileges.
#[tauri::command]
fn is_elevated() -> bool {
    #[cfg(windows)]
    {
        // shell32!IsUserAnAdmin is the simplest way.
        #[link(name = "shell32")]
        extern "system" {
            fn IsUserAnAdmin() -> i32;
        }
        unsafe { IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    {
        // On non-Windows, check effective UID == 0.
        unsafe { libc::geteuid() == 0 }
    }
}

/// Re-launch this application with elevated (administrator) privileges.
///
/// On Windows this calls ShellExecuteW with the "runas" verb, which
/// triggers a UAC prompt.  The current (non-elevated) instance should
/// close itself after calling this.
///
#[tauri::command]
fn restart_elevated(app: AppHandle) -> Result<(), String> {
    #[cfg(windows)]
    {

        use std::os::windows::ffi::OsStrExt;

        #[repr(C)]
        #[allow(non_snake_case, non_camel_case_types)]
        struct SHELLEXECUTEINFOW {
            cbSize: u32,
            fMask: u32,
            hwnd: *mut std::ffi::c_void,
            lpVerb: *const u16,
            lpFile: *const u16,
            lpParameters: *const u16,
            lpDirectory: *const u16,
            nShow: i32,
            hInstApp: *mut std::ffi::c_void,
            lpIDList: *mut std::ffi::c_void,
            lpClass: *const u16,
            hkeyClass: *mut std::ffi::c_void,
            dwHotKey: u32,
            hIcon: *mut std::ffi::c_void,
            hProcess: *mut std::ffi::c_void,
        }

        #[link(name = "shell32")]
        extern "system" {
            fn ShellExecuteExW(pExecInfo: *mut SHELLEXECUTEINFOW) -> i32;
        }

        fn to_wide(s: &str) -> Vec<u16> {
            std::ffi::OsStr::new(s)
                .encode_wide()
                .chain(std::iter::once(0))
                .collect()
        }

        let exe_path = std::env::current_exe()
            .map_err(|e| format!("Cannot find own executable: {}", e))?;
        let exe = exe_path.to_string_lossy().into_owned();

        // Set working directory to the exe's parent so relative
        // resource paths resolve correctly.
        let dir = exe_path
            .parent()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default();

        let verb = to_wide("runas");
        let file = to_wide(&exe);
        let params = to_wide("");
        let directory = to_wide(&dir);
        const SW_SHOWNORMAL: i32 = 1;
        const SEE_MASK_NOASYNC: u32 = 0x0000_0100;

        let mut info = SHELLEXECUTEINFOW {
            cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
            fMask: SEE_MASK_NOASYNC,
            hwnd: std::ptr::null_mut(),
            lpVerb: verb.as_ptr(),
            lpFile: file.as_ptr(),
            lpParameters: params.as_ptr(),
            lpDirectory: directory.as_ptr(),
            nShow: SW_SHOWNORMAL,
            hInstApp: std::ptr::null_mut(),
            lpIDList: std::ptr::null_mut(),
            lpClass: std::ptr::null(),
            hkeyClass: std::ptr::null_mut(),
            dwHotKey: 0,
            hIcon: std::ptr::null_mut(),
            hProcess: std::ptr::null_mut(),
        };

        let ok = unsafe { ShellExecuteExW(&mut info) };
        if ok == 0 {
            return Err("UAC prompt was cancelled or elevation failed.".into());
        }

        // Close the current (non-elevated) instance.
        app.exit(0);
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = app;
        Err("Elevation is only supported on Windows.".into())
    }
}

// ── main ────────────────────────────────────────────────────────────

fn main() {
    tauri::Builder::default()
        .manage(AppState {
            scan_handle: Mutex::new(None),
            last_tree: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            start_scan,
            cancel_scan,
            get_unified_treemap,
            get_graph_data,
            show_in_explorer,
            delete_path,
            check_delete_safety,
            list_drives,
            get_root_children,
            is_elevated,

            restart_elevated
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

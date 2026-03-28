#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod treemap;

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
    state: State<'_, AppState>,
) -> Result<Vec<treemap::TreemapRect>, String> {
    let guard = state.last_tree.lock().unwrap();
    match guard.as_ref() {
        Some(tree) => Ok(treemap::unified_layout(
            tree,
            max_rects.max(1) as usize,
            depth_limit,
        )),
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
            show_in_explorer,
            list_drives,
            get_root_children,
            is_elevated,

            restart_elevated
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

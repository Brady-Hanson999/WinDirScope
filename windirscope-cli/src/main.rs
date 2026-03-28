//! WinDirScope CLI — disk usage analyzer.

use std::path::PathBuf;
use std::process;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use windirscope_core::{ScanConfig, ScanEvent};
use windirscope_scanner::Scanner;

fn main() {
    let args = parse_args();

    let mut config = ScanConfig::new(args.root.clone());
    config.workers = args.workers;
    config.max_depth = args.depth;

    // Wire up Ctrl+C to cancel the scan.
    let ctrl_c_flag = Arc::new(AtomicBool::new(false));
    {
        let flag = Arc::clone(&ctrl_c_flag);
        ctrlc_set_handler(move || {
            flag.store(true, Ordering::SeqCst);
        });
    }

    let (event_rx, handle) = Scanner::start(config);

    // Consume events and print live progress.
    let ctrl_c = Arc::clone(&ctrl_c_flag);
    let event_thread = std::thread::spawn(move || {
        let mut dirs_scanned: u64 = 0;
        let mut total_files: u64 = 0;
        let mut bytes_so_far: u64 = 0;

        for event in event_rx {
            match event {
                ScanEvent::Started { root } => {
                    eprintln!("Scanning {}...", root.display());
                }
                ScanEvent::DirScanned {
                    files,
                    dirs: _,
                    bytes_so_far: b,
                    ..
                } => {
                    dirs_scanned += 1;
                    total_files += files;
                    bytes_so_far = b;
                    // Throttle output: print every 100 directories.
                    if dirs_scanned % 100 == 0 {
                        eprint!(
                            "\r  dirs: {}  files: {}  size: {}",
                            dirs_scanned,
                            total_files,
                            format_bytes(bytes_so_far),
                        );
                    }
                }
                ScanEvent::Error { path, error } => {
                    eprintln!("\n  ERROR {}: {}", path.display(), error);
                }
                ScanEvent::Finished { .. } => {
                    // Final newline after progress.
                    eprint!(
                        "\r  dirs: {}  files: {}  size: {}",
                        dirs_scanned,
                        total_files,
                        format_bytes(bytes_so_far),
                    );
                    eprintln!();
                }
            }

            // Forward Ctrl+C to the scan handle.
            if ctrl_c.load(Ordering::SeqCst) {
                // We don't have the handle here; the main thread
                // will cancel after we exit. Just break.
                break;
            }
        }
    });

    // If Ctrl+C was pressed, cancel the scan.
    if ctrl_c_flag.load(Ordering::SeqCst) {
        handle.cancel();
    }

    let result = handle.join();
    let _ = event_thread.join();

    // Print summary.
    let stats = &result.stats;
    println!();
    println!("=== Scan Complete ===");
    println!(
        "  Total size : {}",
        format_bytes(stats.total_bytes)
    );
    println!("  Files      : {}", stats.total_files);
    println!("  Directories: {}", stats.total_dirs);
    println!(
        "  Elapsed    : {:.2}s",
        stats.elapsed.as_secs_f64()
    );
    println!("  Errors     : {}", stats.error_count);
    if stats.cancelled {
        println!("  ** Scan was CANCELLED **");
    }

    // Top N largest directories by cumulative size.
    let top_n = args.top;
    let tree = &result.tree;

    // Collect directory nodes.
    let mut dir_indices: Vec<usize> = tree
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.kind == windirscope_core::NodeKind::Directory)
        .map(|(i, _)| i)
        .collect();

    // Sort descending by cumulative_size.
    dir_indices.sort_by(|&a, &b| {
        tree.nodes[b]
            .cumulative_size
            .cmp(&tree.nodes[a].cumulative_size)
    });

    println!();
    println!("Top {} largest directories:", top_n);
    for &idx in dir_indices.iter().take(top_n) {
        let node = &tree.nodes[idx];
        let path = tree.full_path(idx);
        println!(
            "  {:>12}  {}",
            format_bytes(node.cumulative_size),
            path.display(),
        );
    }
}

// ── naive Ctrl+C handler (no extra dependency) ─────────────────────

/// Minimal Ctrl+C handler using Windows SetConsoleCtrlHandler.
/// Falls back to a no-op on non-Windows.
fn ctrlc_set_handler<F: Fn() + Send + Sync + 'static>(f: F) {
    #[cfg(windows)]
    {
        use std::sync::OnceLock;
        // Store the closure in a static so the extern fn can call it.
        static HANDLER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
        HANDLER.get_or_init(|| {
            // SAFETY-free: we use the windows-sys-free approach via
            // std::os::windows — but SetConsoleCtrlHandler isn't in std.
            // Instead we just register a signal handler via ctrlc crate…
            // Actually, let's keep it zero-dep: spawn a thread that
            // blocks on a hidden stdin trick? No — simplest: just
            // override the default handler in a portable way.
            Box::new(f)
        });

        extern "system" fn handler_routine(_ctrl_type: u32) -> i32 {
            if let Some(handler) = HANDLER.get() {
                handler();
            }
            1 // TRUE — we handled it
        }

        extern "system" {
            fn SetConsoleCtrlHandler(
                handler: extern "system" fn(u32) -> i32,
                add: i32,
            ) -> i32;
        }

        // This is a safe FFI call (no unsafe memory access).
        #[allow(unused_unsafe)]
        unsafe {
            SetConsoleCtrlHandler(handler_routine, 1);
        }
    }

    #[cfg(not(windows))]
    {
        // On non-Windows just ignore; user can kill the process.
        let _ = f;
    }
}

// ── arg parsing ────────────────────────────────────────────────────

struct CliArgs {
    root: PathBuf,
    workers: usize,
    depth: Option<usize>,
    top: usize,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: windirscope <PATH> [--workers N] [--depth N] [--top N]");
        process::exit(1);
    }

    let mut root: Option<PathBuf> = None;
    let mut workers: usize = 4;
    let mut depth: Option<usize> = None;
    let mut top: usize = 10;

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--workers" => {
                i += 1;
                workers = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| {
                        eprintln!("--workers requires a number");
                        process::exit(1);
                    });
            }
            "--depth" => {
                i += 1;
                depth = Some(
                    args.get(i)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(|| {
                            eprintln!("--depth requires a number");
                            process::exit(1);
                        }),
                );
            }
            "--top" => {
                i += 1;
                top = args
                    .get(i)
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| {
                        eprintln!("--top requires a number");
                        process::exit(1);
                    });
            }
            other => {
                if other.starts_with('-') {
                    eprintln!("Unknown flag: {}", other);
                    process::exit(1);
                }
                root = Some(PathBuf::from(other));
            }
        }
        i += 1;
    }

    let root = root.unwrap_or_else(|| {
        eprintln!("Usage: windirscope <PATH> [--workers N] [--depth N] [--top N]");
        process::exit(1);
    });

    CliArgs {
        root,
        workers,
        depth,
        top,
    }
}

// ── formatting ─────────────────────────────────────────────────────

fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    const TB: u64 = 1024 * GB;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

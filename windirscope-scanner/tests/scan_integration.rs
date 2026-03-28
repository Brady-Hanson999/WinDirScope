//! Integration tests for the scanner.

use std::fs;
use std::io::Write;
use std::path::Path;
use tempfile::TempDir;
use windirscope_core::{ScanConfig, ScanEvent};
use windirscope_scanner::Scanner;

/// Helper: create a deterministic temp tree with known sizes.
///
/// Structure:
/// ```text
/// root/
///   a.txt        (100 bytes)
///   sub1/
///     b.txt      (200 bytes)
///     sub2/
///       c.txt    (300 bytes)
///   empty_dir/
/// ```
///
/// Total file bytes: 600
fn create_temp_tree(base: &Path) {
    fs::create_dir_all(base.join("sub1/sub2")).unwrap();
    fs::create_dir_all(base.join("empty_dir")).unwrap();

    write_file(&base.join("a.txt"), 100);
    write_file(&base.join("sub1/b.txt"), 200);
    write_file(&base.join("sub1/sub2/c.txt"), 300);
}

fn write_file(path: &Path, size: usize) {
    let mut f = fs::File::create(path).unwrap();
    f.write_all(&vec![0u8; size]).unwrap();
}

#[test]
fn scan_known_tree_correct_totals() {
    let tmp = TempDir::new().unwrap();
    create_temp_tree(tmp.path());

    let config = ScanConfig::new(tmp.path().to_path_buf());
    let (event_rx, handle) = Scanner::start(config);

    // Drain events and check Finished is emitted.
    let mut saw_finished = false;
    for event in event_rx {
        if let ScanEvent::Finished { stats } = event {
            saw_finished = true;
            assert_eq!(stats.total_bytes, 600, "total bytes must be 600");
            assert_eq!(stats.total_files, 3, "total files must be 3");
            // root + sub1 + sub2 + empty_dir = 4 dirs
            assert_eq!(stats.total_dirs, 4, "total dirs must be 4");
            assert!(!stats.cancelled, "cancelled must be false");
        }
    }
    assert!(saw_finished, "Finished event must be emitted");

    let result = handle.join();
    assert_eq!(result.stats.total_bytes, 600);
    assert!(!result.stats.cancelled);

    // Verify cumulative sizes via the tree.
    let tree = &result.tree;
    // Root cumulative_size should be 600.
    assert_eq!(tree.nodes[0].cumulative_size, 600);
}

#[test]
fn scan_cancellation() {
    // Create a tree with many directories so the scan takes a bit.
    let tmp = TempDir::new().unwrap();
    for i in 0..200 {
        let dir = tmp.path().join(format!("dir_{:04}", i));
        fs::create_dir_all(&dir).unwrap();
        write_file(&dir.join("file.bin"), 64);
    }

    let config = ScanConfig::new(tmp.path().to_path_buf());
    let (event_rx, handle) = Scanner::start(config);

    // Cancel immediately.
    handle.cancel();

    // Drain events.
    let mut saw_finished = false;
    for event in event_rx {
        if let ScanEvent::Finished { stats } = event {
            saw_finished = true;
            assert!(stats.cancelled, "cancelled flag must be true");
        }
    }
    // Finished is always emitted even on cancellation.
    assert!(saw_finished, "Finished event must be emitted on cancel");

    let result = handle.join();
    assert!(result.stats.cancelled);
}

#[test]
fn scan_deep_tree_parallel() {
    // Create a tree with 200 directories, each containing 10 files of
    // 128 bytes. Total = 200 * 10 * 128 = 256_000 bytes, 2000 files.
    let tmp = TempDir::new().unwrap();
    let expected_dirs: u64 = 200;
    let files_per_dir: u64 = 10;
    let file_size: usize = 128;

    for d in 0..expected_dirs {
        let dir = tmp.path().join(format!("d{:04}", d));
        fs::create_dir_all(&dir).unwrap();
        for f in 0..files_per_dir {
            write_file(&dir.join(format!("f{}.bin", f)), file_size);
        }
    }

    let expected_files = expected_dirs * files_per_dir;
    let expected_bytes = expected_files * file_size as u64;

    let mut config = ScanConfig::new(tmp.path().to_path_buf());
    config.workers = 4; // exercise parallelism
    let (event_rx, handle) = Scanner::start(config);

    // Drain events — verify Finished.
    let mut saw_finished = false;
    for event in event_rx {
        if let ScanEvent::Finished { stats } = event {
            saw_finished = true;
            assert_eq!(stats.total_files, expected_files, "file count mismatch");
            assert_eq!(stats.total_bytes, expected_bytes, "byte count mismatch");
            // root + 200 subdirs = 201
            assert_eq!(stats.total_dirs, expected_dirs + 1, "dir count mismatch");
            assert!(!stats.cancelled);
        }
    }
    assert!(saw_finished, "Finished event must be emitted");

    let result = handle.join();
    assert_eq!(result.stats.total_bytes, expected_bytes);
    assert_eq!(result.tree.nodes[0].cumulative_size, expected_bytes);
}

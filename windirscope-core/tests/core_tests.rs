//! Unit tests for windirscope-core.

#[cfg(test)]
mod tests {
    use windirscope_core::tree::{DirTree, FileEntry, NodeKind};

    #[test]
    fn tree_cumulative_sizes() {
        let mut tree = DirTree::new();

        // root (dir)
        let root = tree.add_node("root".into(), NodeKind::Directory, None, 0, 0);
        // file_a: 100 bytes
        tree.add_node("a.txt".into(), NodeKind::File, Some(root), 100, 1);
        // subdir
        let sub = tree.add_node("sub".into(), NodeKind::Directory, Some(root), 0, 1);
        // file_b: 250 bytes
        tree.add_node("b.txt".into(), NodeKind::File, Some(sub), 250, 2);

        tree.compute_cumulative_sizes();

        // sub cumulative = 250
        assert_eq!(tree.nodes[sub].cumulative_size, 250);
        // root cumulative = 100 + 250 = 350
        assert_eq!(tree.nodes[root].cumulative_size, 350);
    }

    #[test]
    fn path_reconstruction() {
        let mut tree = DirTree::new();

        let root = tree.add_node("C:\\Users".into(), NodeKind::Directory, None, 0, 0);
        let child = tree.add_node("docs".into(), NodeKind::Directory, Some(root), 0, 1);
        let file = tree.add_node("readme.md".into(), NodeKind::File, Some(child), 42, 2);

        let path = tree.full_path(file);
        // On Windows the PathBuf joiner produces C:\Users\docs\readme.md
        assert_eq!(
            path.to_string_lossy().replace('/', "\\"),
            "C:\\Users\\docs\\readme.md"
        );
        assert_eq!(
            tree.full_path(root).to_string_lossy().as_ref(),
            "C:\\Users"
        );
    }

    /// Verify the top_files + other_files model works correctly.
    /// Simulate what the scanner does: set top_files on a directory
    /// node, set its `size` to total direct-file bytes, and check
    /// that cumulative_size accounts for them.
    #[test]
    fn top_files_and_other_aggregation() {
        let mut tree = DirTree::new();

        let root = tree.add_node("root".into(), NodeKind::Directory, None, 0, 0);
        let sub = tree.add_node("sub".into(), NodeKind::Directory, Some(root), 0, 1);

        // Simulate 5 files in `sub`:
        //   big.bin=500, med.bin=300, small.bin=100, tiny1.bin=50, tiny2.bin=10
        // With K=3, top_files keeps big, med, small (900 bytes).
        // other_files_bytes = 50 + 10 = 60, other_files_count = 2.
        // Total direct file bytes = 960.
        let top = vec![
            FileEntry { name: "big.bin".into(), bytes: 500 },
            FileEntry { name: "med.bin".into(), bytes: 300 },
            FileEntry { name: "small.bin".into(), bytes: 100 },
        ];
        tree.nodes[sub].top_files = top;
        tree.nodes[sub].other_files_bytes = 60;
        tree.nodes[sub].other_files_count = 2;
        tree.nodes[sub].size = 960; // total direct file bytes

        // Root has no direct files.
        tree.nodes[root].size = 0;

        tree.compute_cumulative_sizes();

        // sub cumulative = 960 (no sub-children).
        assert_eq!(tree.nodes[sub].cumulative_size, 960);
        // root cumulative = 0 + 960 = 960.
        assert_eq!(tree.nodes[root].cumulative_size, 960);

        // Verify top_files contents.
        assert_eq!(tree.nodes[sub].top_files.len(), 3);
        assert_eq!(tree.nodes[sub].top_files[0].name, "big.bin");
        assert_eq!(tree.nodes[sub].top_files[0].bytes, 500);
        // Verify other sums match.
        let top_sum: u64 = tree.nodes[sub].top_files.iter().map(|f| f.bytes).sum();
        let total = top_sum + tree.nodes[sub].other_files_bytes;
        assert_eq!(total, 960);
        assert_eq!(tree.nodes[sub].other_files_count, 2);
    }
}

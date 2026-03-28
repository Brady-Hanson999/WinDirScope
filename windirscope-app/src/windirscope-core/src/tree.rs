//! Arena-based directory tree.
//!
//! Nodes store only their name and parent index — full paths are
//! reconstructed on demand by walking up the parent chain.

use std::path::PathBuf;

/// Index into the `DirTree` node arena.
pub type NodeId = usize;

/// Whether a node represents a file or a directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKind {
    File,
    Directory,
}

/// A top-K file entry stored on a directory node.
#[derive(Debug, Clone)]
pub struct FileEntry {
    /// File name (NOT the full path).
    pub name: String,
    /// Size in bytes.
    pub bytes: u64,
}

/// A single node in the directory tree.
#[derive(Debug, Clone)]
pub struct TreeNode {
    /// File or directory name (NOT the full path).
    pub name: String,
    /// Kind of entry.
    pub kind: NodeKind,
    /// Parent node index (`None` for the root node).
    pub parent: Option<NodeId>,
    /// Own size in bytes (file: file size, directory: total bytes of
    /// direct files, filled in by the scanner).
    pub size: u64,
    /// Cumulative size (own + descendants). Set after aggregation.
    pub cumulative_size: u64,
    /// Indices of direct children.
    pub children: Vec<NodeId>,
    /// Depth relative to scan root (root = 0).
    pub depth: u32,
    /// Top-K largest files in this directory (directory nodes only).
    pub top_files: Vec<FileEntry>,
    /// Total bytes of files NOT in `top_files`.
    pub other_files_bytes: u64,
    /// Count of files NOT in `top_files`.
    pub other_files_count: u64,
}

/// Arena-allocated directory tree built during a scan.
#[derive(Debug, Clone)]
pub struct DirTree {
    /// Flat arena of nodes (index = NodeId).
    pub nodes: Vec<TreeNode>,
}

impl DirTree {
    /// Create an empty tree.
    pub fn new() -> Self {
        Self { nodes: Vec::new() }
    }

    /// Add a node and return its `NodeId`.
    pub fn add_node(
        &mut self,
        name: String,
        kind: NodeKind,
        parent: Option<NodeId>,
        size: u64,
        depth: u32,
    ) -> NodeId {
        let id = self.nodes.len();
        self.nodes.push(TreeNode {
            name,
            kind,
            parent,
            size,
            cumulative_size: 0,
            children: Vec::new(),
            depth,
            top_files: Vec::new(),
            other_files_bytes: 0,
            other_files_count: 0,
        });
        // Register this node as a child of its parent.
        if let Some(pid) = parent {
            self.nodes[pid].children.push(id);
        }
        id
    }

    /// Reconstruct the full path for a node by walking up the parent chain.
    pub fn full_path(&self, id: NodeId) -> PathBuf {
        let mut parts = Vec::new();
        let mut cur = id;
        loop {
            parts.push(self.nodes[cur].name.as_str());
            match self.nodes[cur].parent {
                Some(pid) => cur = pid,
                None => break,
            }
        }
        parts.reverse();
        parts.iter().collect()
    }

    /// Bottom-up pass: compute `cumulative_size` for every node.
    ///
    /// Must be called after the tree is fully built. Iterates nodes in
    /// reverse arena order (children are always added after parents).
    pub fn compute_cumulative_sizes(&mut self) {
        // Reverse order guarantees children are processed before parents.
        for i in (0..self.nodes.len()).rev() {
            let own = self.nodes[i].size;
            let children_sum: u64 = self.nodes[i]
                .children
                .iter()
                .map(|&cid| self.nodes[cid].cumulative_size)
                .sum();
            self.nodes[i].cumulative_size = own + children_sum;
        }
    }
}

impl Default for DirTree {
    fn default() -> Self {
        Self::new()
    }
}

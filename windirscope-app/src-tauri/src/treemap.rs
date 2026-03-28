//! Squarified treemap layout — WinDirStat style.
//!
//! Produces a flat list of normalised rectangles (0..1 coordinate
//! space) where directories contain their files and subdirectories
//! as nested blocks.  Files are leaf blocks; the frontend colours
//! them by extension.
//!
//! **Key optimisation**: single-child directory chains (e.g.
//! `C:\ → Program Files (x86) → Steam`) are *collapsed* so that
//! only one directory rect is emitted for the entire chain, with
//! a combined name like `C:\Program Files (x86)\Steam`.  This
//! prevents deep path prefixes from consuming visual space.

use serde::Serialize;
use windirscope_core::DirTree;

// ── Public output types ─────────────────────────────────────────────

/// A single rectangle in the unified treemap output.
#[derive(Clone, Serialize)]
pub struct TreemapRect {
    /// Display name.
    pub name: String,
    /// Full path (directory path for dirs, file path for files).
    pub path: String,
    /// Normalised coordinates (0..1).
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// Size in bytes.
    pub size: u64,
    /// Visual depth (after chain collapsing).
    pub depth: u32,
    /// `true` for directories, `false` for files.
    pub is_dir: bool,
    /// `true` for the "other files" aggregate block.
    pub is_other: bool,
    /// File extension (lowercase, empty for dirs / other).
    pub ext: String,
}

// ── Internal helpers ────────────────────────────────────────────────

/// A child item to be laid out inside a directory.
enum ChildItem {
    Dir(usize), // node-id
    File {
        name: String,
        ext: String,
        dir_path: String,
        bytes: u64,
    },
    Other {
        dir_path: String,
        count: u64,
        bytes: u64,
    },
}

/// Check if a directory node is "trivial" — it has exactly one child
/// that is a subdirectory, no top_files, and no other_files.
/// In that case we can skip rendering it and pass through to the
/// single child, collapsing the chain.
fn is_trivial_dir(tree: &DirTree, id: usize) -> bool {
    let node = &tree.nodes[id];
    node.children.len() == 1
        && tree.nodes[node.children[0]].kind == windirscope_core::NodeKind::Directory
        && node.top_files.is_empty()
        && node.other_files_bytes == 0
}

/// Walk down a chain of trivial single-child directories and return
/// the final node id + a combined display name for the chain.
fn collapse_chain(tree: &DirTree, start: usize) -> (usize, String) {
    let mut id = start;
    let mut name = tree.nodes[id].name.clone();
    while is_trivial_dir(tree, id) {
        let child = tree.nodes[id].children[0];
        name = format!("{}\\{}", name, tree.nodes[child].name);
        id = child;
    }
    (id, name)
}

// ── Public entry-point ──────────────────────────────────────────────

/// Compute a unified WinDirStat-style treemap layout.
///
/// Every directory is a rectangle that *contains* its children
/// (subdirectories **and** files) as nested, smaller blocks.
/// Single-child directory chains are collapsed to save space.
///
/// The rect budget is distributed **proportionally** across sibling
/// subtrees so that no single large folder starves its neighbours.
///
/// * `max_rects` — hard cap on the number of rectangles emitted.
/// * `depth_limit` — optional maximum *visual* depth to recurse into.
pub fn unified_layout(
    tree: &DirTree,
    start_node: usize,
    max_rects: usize,
    depth_limit: Option<u32>,
) -> Vec<TreemapRect> {
    let mut rects = Vec::with_capacity(max_rects.min(tree.nodes.len() * 2));
    if tree.nodes.is_empty() || start_node >= tree.nodes.len() {
        return rects;
    }
    if tree.nodes[start_node].cumulative_size == 0 {
        return rects;
    }

    // Collapse the root chain (e.g. C:\ → Program Files → Steam → ...).
    let (effective_root, root_name) = collapse_chain(tree, start_node);
    layout_dir(
        tree,
        effective_root,
        &root_name,
        0, // visual depth
        0.0,
        0.0,
        1.0,
        1.0,
        max_rects, // this directory's budget
        depth_limit,
        &mut rects,
    );
    rects
}

// ── Recursive directory layout ──────────────────────────────────────

fn layout_dir(
    tree: &DirTree,
    id: usize,
    display_name: &str,
    visual_depth: u32,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    budget: usize,      // max rects this subtree may emit
    depth_limit: Option<u32>,
    rects: &mut Vec<TreemapRect>,
) {
    if budget == 0 || w < 1e-4 || h < 1e-4 {
        return;
    }

    let node = &tree.nodes[id];
    let dir_path = tree.full_path(id).display().to_string();

    // Emit the directory background rect (costs 1 from budget).
    rects.push(TreemapRect {
        name: display_name.to_string(),
        path: dir_path.clone(),
        x,
        y,
        w,
        h,
        size: node.cumulative_size,
        depth: visual_depth,
        is_dir: true,
        is_other: false,
        ext: String::new(),
    });

    let remaining = budget.saturating_sub(1);
    if remaining == 0 {
        return;
    }

    // Stop recursion at depth limit.
    let at_limit = depth_limit.map_or(false, |dl| visual_depth >= dl);
    if at_limit || node.cumulative_size == 0 {
        return;
    }

    // ── Collect child items (subdirs + files + other) ───────────
    let mut sizes: Vec<f64> = Vec::new();
    let mut items: Vec<ChildItem> = Vec::new();

    for &cid in &node.children {
        let cs = tree.nodes[cid].cumulative_size;
        if cs > 0 {
            sizes.push(cs as f64);
            items.push(ChildItem::Dir(cid));
        }
    }

    for f in &node.top_files {
        if f.bytes > 0 {
            let ext = f
                .name
                .rsplit('.')
                .next()
                .filter(|e| e.len() <= 12 && *e != f.name)
                .map(|e| e.to_lowercase())
                .unwrap_or_default();
            sizes.push(f.bytes as f64);
            items.push(ChildItem::File {
                name: f.name.clone(),
                ext,
                dir_path: dir_path.clone(),
                bytes: f.bytes,
            });
        }
    }

    if node.other_files_bytes > 0 {
        sizes.push(node.other_files_bytes as f64);
        items.push(ChildItem::Other {
            dir_path: dir_path.clone(),
            count: node.other_files_count,
            bytes: node.other_files_bytes,
        });
    }

    if items.is_empty() {
        return;
    }

    // Sort by size descending (squarify works best with sorted input).
    let mut indices: Vec<usize> = (0..items.len()).collect();
    indices.sort_by(|&a, &b| {
        sizes[b]
            .partial_cmp(&sizes[a])
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    let sorted_sizes: Vec<f64> = indices.iter().map(|&i| sizes[i]).collect();

    // Tight padding — just enough for visual nesting, no wasted space.
    let pad = 0.001_f64.min(w * 0.01).min(h * 0.01);
    let header = if w > 0.04 && h > 0.03 {
        0.012_f64.min(h * 0.08)
    } else {
        0.0
    };
    let inner_x = x + pad;
    let inner_y = y + pad + header;
    let inner_w = (w - 2.0 * pad).max(0.0);
    let inner_h = (h - 2.0 * pad - header).max(0.0);
    if inner_w < 1e-4 || inner_h < 1e-4 {
        return;
    }

    // Scale sizes to areas that tile the inner rect.
    let total_size: f64 = sorted_sizes.iter().sum();
    let total_area = inner_w * inner_h;
    let areas: Vec<f64> = sorted_sizes
        .iter()
        .map(|&s| s / total_size * total_area)
        .collect();

    // Run squarified layout.
    let placed = squarify(&areas, inner_x, inner_y, inner_w, inner_h);

    let child_depth = visual_depth + 1;

    // ── Distribute the remaining budget proportionally ──────────
    // Each child costs at least 1 rect.  Stop when budget is spent.
    let n_children = placed.len();
    // We can emit at most `remaining` child rects total.
    let children_to_emit = remaining.min(n_children);
    // Surplus beyond 1-per-child goes to subdirectories proportionally.
    let surplus = remaining.saturating_sub(children_to_emit);

    let mut used = 0usize;
    for sort_idx in 0..placed.len() {
        if used >= remaining {
            break;
        }
        let (px, py, pw, ph) = placed[sort_idx];
        let original_idx = indices[sort_idx];
        let frac = sorted_sizes[sort_idx] / total_size;

        match &items[original_idx] {
            ChildItem::Dir(cid) => {
                // Budget for this subtree: 1 (guaranteed) + proportional surplus.
                let child_budget = 1 + ((surplus as f64 * frac).round() as usize);
                let child_budget = child_budget.min(remaining - used);
                // Collapse single-child chain before recursing.
                let (effective_id, chain_name) = collapse_chain(tree, *cid);
                let before = rects.len();
                layout_dir(
                    tree,
                    effective_id,
                    &chain_name,
                    child_depth,
                    px,
                    py,
                    pw,
                    ph,
                    child_budget,
                    depth_limit,
                    rects,
                );
                used += rects.len() - before;
            }
            ChildItem::File {
                name,
                ext,
                dir_path,
                bytes,
            } => {
                rects.push(TreemapRect {
                    name: name.clone(),
                    path: format!("{}\\{}", dir_path, name),
                    x: px,
                    y: py,
                    w: pw,
                    h: ph,
                    size: *bytes,
                    depth: child_depth,
                    is_dir: false,
                    is_other: false,
                    ext: ext.clone(),
                });
                used += 1;
            }
            ChildItem::Other {
                dir_path,
                count,
                bytes,
            } => {
                rects.push(TreemapRect {
                    name: format!("({} other files)", count),
                    path: dir_path.clone(),
                    x: px,
                    y: py,
                    w: pw,
                    h: ph,
                    size: *bytes,
                    depth: child_depth,
                    is_dir: false,
                    is_other: true,
                    ext: String::new(),
                });
                used += 1;
            }
        }
    }
}

// ── Squarified treemap algorithm ────────────────────────────────────

/// Lay out `areas` (sorted descending) inside a bounding rectangle.
/// Returns `(x, y, w, h)` for each item, in the same order as `areas`.
fn squarify(areas: &[f64], x: f64, y: f64, w: f64, h: f64) -> Vec<(f64, f64, f64, f64)> {
    let n = areas.len();
    if n == 0 {
        return Vec::new();
    }

    let mut result = vec![(0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64); n];
    let mut rx = x;
    let mut ry = y;
    let mut rw = w;
    let mut rh = h;
    let mut i = 0;

    while i < n {
        if rw < 1e-8 || rh < 1e-8 {
            break;
        }

        let shorter = rw.min(rh);

        // Greedily build a row.
        let row_start = i;
        let mut row_sum = areas[i];
        i += 1;

        while i < n {
            let new_sum = row_sum + areas[i];
            let old_worst = worst_ratio(row_sum, &areas[row_start..i], shorter);
            let new_worst = worst_ratio(new_sum, &areas[row_start..=i], shorter);
            if new_worst <= old_worst {
                row_sum = new_sum;
                i += 1;
            } else {
                break;
            }
        }

        // Lay out the row.
        if rw >= rh {
            // Vertical strip on the left.
            let strip_w = if rh > 1e-12 { row_sum / rh } else { rw };
            let mut y_off = ry;
            for j in row_start..i {
                let item_h = if strip_w > 1e-12 {
                    areas[j] / strip_w
                } else {
                    0.0
                };
                result[j] = (rx, y_off, strip_w, item_h);
                y_off += item_h;
            }
            rx += strip_w;
            rw -= strip_w;
        } else {
            // Horizontal strip on top.
            let strip_h = if rw > 1e-12 { row_sum / rw } else { rh };
            let mut x_off = rx;
            for j in row_start..i {
                let item_w = if strip_h > 1e-12 {
                    areas[j] / strip_h
                } else {
                    0.0
                };
                result[j] = (x_off, ry, item_w, strip_h);
                x_off += item_w;
            }
            ry += strip_h;
            rh -= strip_h;
        }
    }

    result
}

/// Worst aspect ratio among items in a row (Bruls et al. formulation).
fn worst_ratio(row_sum: f64, items: &[f64], shorter: f64) -> f64 {
    if row_sum <= 0.0 || shorter <= 0.0 {
        return f64::MAX;
    }
    let s2 = shorter * shorter;
    let sum2 = row_sum * row_sum;
    let r_max = items.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let r_min = items.iter().cloned().fold(f64::INFINITY, f64::min);
    if r_min <= 0.0 {
        return f64::MAX;
    }
    (s2 * r_max / sum2).max(sum2 / (s2 * r_min))
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use windirscope_core::{DirTree, FileEntry, NodeKind};

    #[test]
    fn basic_unified_layout() {
        let mut tree = DirTree::new();
        let root = tree.add_node("C:\\test".into(), NodeKind::Directory, None, 0, 0);
        let a = tree.add_node("subdir_a".into(), NodeKind::Directory, Some(root), 0, 1);
        let b = tree.add_node("subdir_b".into(), NodeKind::Directory, Some(root), 0, 1);

        tree.nodes[root].top_files =
            vec![FileEntry { name: "readme.md".into(), bytes: 100 }];
        tree.nodes[root].size = 100;

        tree.nodes[a].top_files = vec![
            FileEntry { name: "big.bin".into(), bytes: 500 },
            FileEntry { name: "small.txt".into(), bytes: 50 },
        ];
        tree.nodes[a].size = 550;

        tree.nodes[b].top_files =
            vec![FileEntry { name: "data.csv".into(), bytes: 200 }];
        tree.nodes[b].size = 200;

        tree.compute_cumulative_sizes();
        assert_eq!(tree.nodes[root].cumulative_size, 850);

        let rects = unified_layout(&tree, 0, 1000, None);
        assert!(!rects.is_empty());

        // Root rect is the unit square.
        let root_rect = &rects[0];
        assert!(root_rect.is_dir);
        assert!((root_rect.x).abs() < 1e-9);
        assert!((root_rect.y).abs() < 1e-9);
        assert!((root_rect.w - 1.0).abs() < 1e-9);
        assert!((root_rect.h - 1.0).abs() < 1e-9);

        // Should have file rects.
        let file_rects: Vec<_> = rects.iter().filter(|r| !r.is_dir && !r.is_other).collect();
        assert!(!file_rects.is_empty());

        // All rects within [0, 1].
        for r in &rects {
            assert!(r.x >= -1e-6, "x out of bounds: {}", r.x);
            assert!(r.y >= -1e-6, "y out of bounds: {}", r.y);
            assert!(r.x + r.w <= 1.0 + 1e-6, "x+w out of bounds");
            assert!(r.y + r.h <= 1.0 + 1e-6, "y+h out of bounds");
        }
    }

    #[test]
    fn depth_limit_stops_recursion() {
        let mut tree = DirTree::new();
        let root = tree.add_node("root".into(), NodeKind::Directory, None, 0, 0);
        // Give root two children so it isn't a trivial single-child chain.
        let a = tree.add_node("a".into(), NodeKind::Directory, Some(root), 0, 1);
        tree.nodes[root].top_files =
            vec![FileEntry { name: "r.txt".into(), bytes: 50 }];
        tree.nodes[root].size = 50;

        tree.nodes[a].top_files =
            vec![FileEntry { name: "f.txt".into(), bytes: 100 }];
        tree.nodes[a].size = 100;
        tree.compute_cumulative_sizes();

        // depth_limit=1 → root (visual depth 0) recurses to show children,
        // but subdir "a" (visual depth 1) is at the limit and does not recurse.
        let rects = unified_layout(&tree, 0, 100, Some(1));
        // "r.txt" should appear (child of root at depth 0), but "f.txt"
        // inside "a" (depth 1) should NOT appear.
        let a_files: Vec<_> = rects
            .iter()
            .filter(|r| !r.is_dir && r.name == "f.txt")
            .collect();
        assert!(
            a_files.is_empty(),
            "files inside 'a' should not appear at depth_limit=1"
        );
    }

    #[test]
    fn max_rects_cap() {
        let mut tree = DirTree::new();
        let root = tree.add_node("root".into(), NodeKind::Directory, None, 0, 0);
        tree.nodes[root].top_files = (0..20)
            .map(|i| FileEntry {
                name: format!("f{i}.txt"),
                bytes: 10,
            })
            .collect();
        tree.nodes[root].size = 200;
        tree.compute_cumulative_sizes();

        let rects = unified_layout(&tree, 0, 5, None);
        assert!(rects.len() <= 5);
    }

    #[test]
    fn squarify_preserves_total_area() {
        let areas = vec![6.0, 6.0, 4.0, 3.0, 2.0, 2.0, 1.0];
        let result = squarify(&areas, 0.0, 0.0, 6.0, 4.0);
        assert_eq!(result.len(), areas.len());

        let total: f64 = result.iter().map(|(_, _, w, h)| w * h).sum();
        assert!(
            (total - 24.0).abs() < 0.1,
            "total area = {total}, expected 24"
        );

        for &(px, py, pw, ph) in &result {
            assert!(px >= -1e-6);
            assert!(py >= -1e-6);
            assert!(px + pw <= 6.0 + 1e-6);
            assert!(py + ph <= 4.0 + 1e-6);
        }
    }

    #[test]
    fn other_files_block_emitted() {
        let mut tree = DirTree::new();
        let root = tree.add_node("root".into(), NodeKind::Directory, None, 0, 0);
        tree.nodes[root].top_files =
            vec![FileEntry { name: "a.bin".into(), bytes: 400 }];
        tree.nodes[root].other_files_bytes = 200;
        tree.nodes[root].other_files_count = 10;
        tree.nodes[root].size = 600;
        tree.compute_cumulative_sizes();

        let rects = unified_layout(&tree, 0, 1000, None);
        let other = rects.iter().find(|r| r.is_other);
        assert!(other.is_some(), "expected an 'other files' rect");
        assert_eq!(other.unwrap().size, 200);
    }
}

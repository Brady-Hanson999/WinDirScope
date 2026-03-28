use serde::Serialize;
use windirscope_core::{DirTree, NodeKind};

#[derive(Serialize)]
pub struct ForceGraphNode {
    pub id: String,
    pub name: String,
    pub path: String,
    pub size: u64,
    pub is_dir: bool,
    pub depth: u32,
    pub ext: String,
}

#[derive(Serialize)]
pub struct ForceGraphLink {
    pub source: String,
    pub target: String,
}

#[derive(Serialize)]
pub struct ForceGraphPayload {
    pub nodes: Vec<ForceGraphNode>,
    pub links: Vec<ForceGraphLink>,
}

pub fn build_force_graph(
    tree: &DirTree,
    start_node: usize,
    max_nodes: usize,
    depth_limit: Option<u32>,
) -> ForceGraphPayload {
    let mut nodes = Vec::new();
    let mut links = Vec::new();

    if tree.nodes.is_empty() || start_node >= tree.nodes.len() {
        return ForceGraphPayload { nodes, links };
    }

    let mut queue = std::collections::VecDeque::new();
    queue.push_back(start_node);

    let mut added = std::collections::HashSet::new();
    added.insert(start_node);

    let current_depth = tree.nodes[start_node].depth;

    while let Some(id) = queue.pop_front() {
        let node = &tree.nodes[id];
        let node_path = tree.full_path(id).display().to_string();

        nodes.push(ForceGraphNode {
            id: id.to_string(),
            name: node.name.clone(),
            path: node_path.clone(),
            size: node.cumulative_size,
            is_dir: node.kind == NodeKind::Directory,
            depth: node.depth - current_depth,
            ext: String::new(),
        });

        if id != start_node {
            if let Some(pid) = node.parent {
                if added.contains(&pid) {
                    links.push(ForceGraphLink {
                        source: pid.to_string(),
                        target: id.to_string(),
                    });
                }
            }
        }

        if let Some(dl) = depth_limit {
            if (node.depth - current_depth) >= dl {
                continue;
            }
        }

        if node.kind == NodeKind::Directory {
            let mut cids = node.children.clone();
            cids.sort_by_key(|&cid| std::cmp::Reverse(tree.nodes[cid].cumulative_size));

            for cid in cids {
                if nodes.len() + queue.len() < max_nodes {
                    queue.push_back(cid);
                    added.insert(cid);
                }
            }

            for f in &node.top_files {
                if nodes.len() + queue.len() < max_nodes {
                    let file_id = format!("f_{}_{}", id, f.name);
                    let ext = f
                        .name
                        .rsplit('.')
                        .next()
                        .unwrap_or("")
                        .to_lowercase();

                    nodes.push(ForceGraphNode {
                        id: file_id.clone(),
                        name: f.name.clone(),
                        path: format!("{}\\{}", node_path, f.name),
                        size: f.bytes,
                        is_dir: false,
                        depth: node.depth - current_depth + 1,
                        ext,
                    });

                    links.push(ForceGraphLink {
                        source: id.to_string(),
                        target: file_id,
                    });
                }
            }
        }
    }

    ForceGraphPayload { nodes, links }
}

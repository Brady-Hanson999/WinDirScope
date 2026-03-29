use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;
use xxhash_rust::xxh3::{xxh3_64, Xxh3};

#[derive(serde::Serialize, Clone)]
pub struct DuplicateFile {
    pub path: String,
    pub name: String,
    pub size: u64,
}

#[derive(serde::Serialize, Clone)]
pub struct DuplicateGroup {
    pub size: u64,
    pub files: Vec<DuplicateFile>,
}

pub fn find_exact_duplicates(root_path: &Path) -> Result<Vec<DuplicateGroup>, String> {
    // 1. Group by size
    let mut by_size: HashMap<u64, Vec<PathBuf>> = HashMap::new();

    for entry in WalkDir::new(root_path).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_file() {
            if let Ok(metadata) = entry.metadata() {
                let size = metadata.len();
                if size > 0 {
                    by_size.entry(size).or_default().push(path.to_path_buf());
                }
            }
        }
    }

    by_size.retain(|_, v| v.len() > 1);

    // 2. Read first 4KB and hash
    let mut by_4kb_hash: HashMap<u64, Vec<(PathBuf, u64)>> = HashMap::new();

    for (size, paths) in &by_size {
        for path in paths {
            if let Ok(mut file) = File::open(path) {
                let mut buffer = [0u8; 4096];
                let to_read = std::cmp::min(4096, *size as usize);
                if let Ok(bytes_read) = file.read(&mut buffer[..to_read]) {
                    if bytes_read == to_read {
                        let hash = xxh3_64(&buffer[..bytes_read]);
                        by_4kb_hash.entry(hash).or_default().push((path.clone(), *size));
                    }
                }
            }
        }
    }

    by_4kb_hash.retain(|_, v| v.len() > 1);

    // 3. Full file hash
    let mut by_full_hash: HashMap<u64, Vec<(PathBuf, u64)>> = HashMap::new();

    for (_, paths) in &by_4kb_hash {
        for (path, size) in paths {
            if let Ok(mut file) = File::open(path) {
                let _ = file.seek(SeekFrom::Start(0));
                
                let mut hasher = Xxh3::new();
                let mut buffer = [0u8; 65536]; 
                let mut success = true;
                
                loop {
                    match file.read(&mut buffer) {
                        Ok(0) => break,
                        Ok(n) => hasher.update(&buffer[..n]),
                        Err(_) => { success = false; break; }
                    }
                }
                
                if success {
                    by_full_hash.entry(hasher.digest()).or_default().push((path.clone(), *size));
                }
            }
        }
    }

    by_full_hash.retain(|_, v| v.len() > 1);

    // 4. Transform to DuplicateGroup
    let mut groups = Vec::new();
    for (_, files) in by_full_hash {
        let size = files[0].1;
        let mut group_files = Vec::new();
        for (path, _) in files {
            group_files.push(DuplicateFile {
                name: path.file_name().unwrap_or_default().to_string_lossy().to_string(),
                path: path.to_string_lossy().to_string(), // use standard representation for the frontend
                size,
            });
        }
        groups.push(DuplicateGroup { size, files: group_files });
    }

    groups.sort_by(|a, b| b.size.cmp(&a.size));

    Ok(groups)
}

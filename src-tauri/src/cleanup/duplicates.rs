//! Duplicate detection: group by size, then a cheap partial hash (first 64 KiB),
//! then a full blake3 hash only for partial-hash collisions. Files that are
//! unique by size are never read.

use serde::Serialize;
use std::collections::HashMap;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

const PARTIAL: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct DupInput {
    pub path: String,
    pub size: i64,
    pub modified: Option<i64>,
    /// Cached full hash from the index, valid when `cached_key` == `hash_key()`.
    pub cached_hash: Option<String>,
    pub cached_key: Option<String>,
}

impl DupInput {
    pub fn hash_key(&self) -> String {
        format!("{}:{}", self.size, self.modified.unwrap_or(0))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupGroup {
    pub id: u32,
    pub size: i64,
    pub hash: String,
    pub files: Vec<DupMember>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DupMember {
    pub path: String,
    pub modified: Option<i64>,
}

pub fn partial_hash(path: &Path) -> std::io::Result<String> {
    let mut f = std::fs::File::open(path)?;
    let mut buf = vec![0u8; PARTIAL];
    let mut n = 0;
    while n < PARTIAL {
        let r = f.read(&mut buf[n..])?;
        if r == 0 {
            break;
        }
        n += r;
    }
    Ok(blake3::hash(&buf[..n]).to_hex().to_string())
}

pub fn full_hash(path: &Path) -> std::io::Result<String> {
    let f = std::fs::File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    hasher.update_reader(BufReader::with_capacity(1 << 20, f))?;
    Ok(hasher.finalize().to_hex().to_string())
}

pub struct DupResult {
    pub groups: Vec<DupGroup>,
    /// (path, hash, hash_key) computed during this run, to cache in the index.
    pub new_hashes: Vec<(String, String, String)>,
}

/// Find duplicate groups among `inputs`. `progress(done, total)` reports
/// candidates processed.
pub fn find(inputs: Vec<DupInput>, cancel: &AtomicBool, mut progress: impl FnMut(usize, usize)) -> DupResult {
    let mut by_size: HashMap<i64, Vec<DupInput>> = HashMap::new();
    for i in inputs {
        if i.size > 0 {
            by_size.entry(i.size).or_default().push(i);
        }
    }
    let candidates: Vec<Vec<DupInput>> = by_size.into_values().filter(|v| v.len() > 1).collect();
    let total: usize = candidates.iter().map(|v| v.len()).sum();
    let mut done = 0;
    let mut groups = Vec::new();
    let mut new_hashes = Vec::new();

    for same_size in candidates {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        let size = same_size[0].size;
        // Stage 1: partial hash.
        let mut by_partial: HashMap<String, Vec<DupInput>> = HashMap::new();
        for f in same_size {
            done += 1;
            if let Ok(h) = partial_hash(Path::new(&f.path)) {
                by_partial.entry(h).or_default().push(f);
            }
        }
        progress(done, total);
        // Stage 2: full hash (or reuse partial when the file fits in it).
        for (partial, files) in by_partial {
            if files.len() < 2 {
                continue;
            }
            let mut by_full: HashMap<String, Vec<DupInput>> = HashMap::new();
            for f in files {
                if cancel.load(Ordering::Relaxed) {
                    break;
                }
                let key = f.hash_key();
                let h = if size as usize <= PARTIAL {
                    Some(partial.clone())
                } else if f.cached_key.as_deref() == Some(key.as_str()) && f.cached_hash.is_some() {
                    f.cached_hash.clone()
                } else {
                    match full_hash(Path::new(&f.path)) {
                        Ok(h) => {
                            new_hashes.push((f.path.clone(), h.clone(), key));
                            Some(h)
                        }
                        Err(_) => None,
                    }
                };
                if let Some(h) = h {
                    by_full.entry(h).or_default().push(f);
                }
            }
            for (hash, mut files) in by_full {
                if files.len() < 2 {
                    continue;
                }
                files.sort_by(|a, b| a.path.cmp(&b.path));
                groups.push(DupGroup {
                    id: 0,
                    size,
                    hash,
                    files: files.into_iter().map(|f| DupMember { path: f.path, modified: f.modified }).collect(),
                });
            }
        }
    }
    groups.sort_by_key(|g| std::cmp::Reverse(g.size * (g.files.len() as i64 - 1)));
    for (i, g) in groups.iter_mut().enumerate() {
        g.id = i as u32 + 1;
    }
    DupResult { groups, new_hashes }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn input(p: &PathBuf) -> DupInput {
        let m = std::fs::metadata(p).unwrap();
        DupInput {
            path: p.to_string_lossy().to_string(),
            size: m.len() as i64,
            modified: Some(1),
            cached_hash: None,
            cached_key: None,
        }
    }

    #[test]
    fn finds_true_duplicates_only() {
        let tmp = tempfile::tempdir().unwrap();
        let d = tmp.path();
        std::fs::create_dir_all(d.join("sub")).unwrap();
        let write = |name: &str, data: &[u8]| {
            let p = d.join(name);
            std::fs::write(&p, data).unwrap();
            p
        };
        let a = write("a.txt", b"hello world");
        let b = write("sub/b.txt", b"hello world");
        let c = write("c.txt", b"hello WORLD"); // same size, different content
        let e = write("e.txt", b"unique size!!");
        // Large files: identical first 64 KiB, different tail → not duplicates.
        let mut big1 = vec![7u8; 200_000];
        let mut big2 = big1.clone();
        big2[199_999] = 8;
        let big3 = big1.clone();
        let p1 = write("big1.bin", &big1);
        let p2 = write("big2.bin", &big2);
        let p3 = write("big3.bin", &big3);
        big1.clear();

        let cancel = AtomicBool::new(false);
        let res = find([&a, &b, &c, &e, &p1, &p2, &p3].iter().map(|p| input(p)).collect(), &cancel, |_, _| {});
        assert_eq!(res.groups.len(), 2, "{:?}", res.groups);
        let big = &res.groups[0]; // sorted by wasted bytes
        assert_eq!(big.size, 200_000);
        let names: Vec<_> = big.files.iter().map(|f| f.path.clone()).collect();
        assert!(names.contains(&p1.to_string_lossy().to_string()));
        assert!(names.contains(&p3.to_string_lossy().to_string()));
        assert!(!names.contains(&p2.to_string_lossy().to_string()));
        let small = &res.groups[1];
        assert_eq!(small.files.len(), 2);
        assert!(small.files.iter().all(|f| f.path != c.to_string_lossy()));
        // Full hashes were computed for the three large same-size files only.
        assert_eq!(res.new_hashes.len(), 3);
    }

    #[test]
    fn unique_sizes_are_never_read() {
        let tmp = tempfile::tempdir().unwrap();
        // Paths that don't exist: if the engine tried to read them it would drop
        // them, but with unique sizes it must not even try.
        let inputs = vec![
            DupInput { path: tmp.path().join("x").to_string_lossy().into(), size: 10, modified: None, cached_hash: None, cached_key: None },
            DupInput { path: tmp.path().join("y").to_string_lossy().into(), size: 11, modified: None, cached_hash: None, cached_key: None },
        ];
        let mut calls = 0;
        let res = find(inputs, &AtomicBool::new(false), |_, _| calls += 1);
        assert!(res.groups.is_empty());
        assert_eq!(calls, 0);
    }

    #[test]
    fn cached_hash_is_reused() {
        let tmp = tempfile::tempdir().unwrap();
        let data = vec![1u8; 100_000];
        let a = tmp.path().join("a.bin");
        let b = tmp.path().join("b.bin");
        std::fs::write(&a, &data).unwrap();
        std::fs::write(&b, &data).unwrap();
        let real = full_hash(&a).unwrap();
        let mut ia = input(&a);
        ia.cached_hash = Some(real.clone());
        ia.cached_key = Some(ia.hash_key());
        let res = find(vec![ia, input(&b)], &AtomicBool::new(false), |_, _| {});
        assert_eq!(res.groups.len(), 1);
        assert_eq!(res.new_hashes.len(), 1);
    }
}

//! Process-local cache for project context loading.
//!
//! The project-context loader sits on prompt/session hot paths and repeatedly
//! checks the same workspace, parent, global, constitution, and trust files.
//! This cache avoids rereading unchanged context while keeping the signature
//! broad enough for the loader's side effects and authority surfaces.

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::project_context::ProjectContext;

const DEFAULT_CAPACITY: usize = 8;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct CacheKey {
    workspace: PathBuf,
    signature: ContentSignature,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
struct ContentSignature {
    entries: Vec<ContentEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct ContentEntry {
    path: PathBuf,
    fingerprint: Option<String>,
}

#[derive(Debug, Default)]
struct WorkspaceCache {
    by_key: HashMap<CacheKey, ProjectContext>,
    order: VecDeque<CacheKey>,
}

thread_local! {
    static CACHE: RefCell<WorkspaceCache> = RefCell::new(WorkspaceCache::default());
}

pub(crate) fn lookup(key: &CacheKey) -> Option<ProjectContext> {
    CACHE.with(|cache| cache.borrow().by_key.get(key).cloned())
}

pub(crate) fn store(key: CacheKey, value: ProjectContext) {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if cache.by_key.insert(key.clone(), value).is_none() {
            cache.order.push_back(key);
        }
        while cache.by_key.len() > DEFAULT_CAPACITY {
            let Some(oldest) = cache.order.pop_front() else {
                break;
            };
            cache.by_key.remove(&oldest);
        }
    });
}

#[must_use]
pub(crate) fn compute_cache_key(workspace: &Path, home_dir: Option<&Path>) -> CacheKey {
    let workspace = canonicalize_or_keep(workspace);
    CacheKey {
        signature: ContentSignature::for_loader(&workspace, home_dir),
        workspace,
    }
}

impl ContentSignature {
    fn for_loader(workspace: &Path, home_dir: Option<&Path>) -> Self {
        let mut entries: Vec<ContentEntry> =
            crate::project_context::project_context_cache_candidate_paths(workspace, home_dir)
                .into_iter()
                .map(|path| ContentEntry {
                    fingerprint: file_fingerprint(&path),
                    path,
                })
                .collect();

        entries.sort_by(|a, b| a.path.cmp(&b.path));
        entries.dedup_by(|a, b| a.path == b.path);

        Self { entries }
    }
}

fn file_fingerprint(path: &Path) -> Option<String> {
    let metadata = std::fs::metadata(path).ok()?;
    if !metadata.is_file() {
        return Some("non-file".to_string());
    }

    match std::fs::read(path) {
        Ok(bytes) => {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            Some(format!("sha256:{}", to_hex(&hasher.finalize())))
        }
        Err(error) => {
            let modified = metadata
                .modified()
                .ok()
                .and_then(|mtime| mtime.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|duration| format!("{}:{}", duration.as_secs(), duration.subsec_nanos()))
                .unwrap_or_else(|| "unknown".to_string());
            Some(format!(
                "unreadable:{}:{}:{error}",
                metadata.len(),
                modified
            ))
        }
    }
}

fn canonicalize_or_keep(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn to_hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

#[cfg(test)]
mod tests {}

//! Project document discovery and loading

use std::path::{Path, PathBuf};

/// Find the git root directory from cwd
pub(crate) fn find_git_root(cwd: &Path) -> Option<PathBuf> {
    let mut current = cwd.to_path_buf();
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        match current.parent() {
            Some(parent) if parent != current => {
                current = parent.to_path_buf();
            }
            _ => return None,
        }
    }
}

//! Side-git repository wrapper for workspace snapshots.
//!
//! `SnapshotRepo` shells out to the system `git` binary (we deliberately
//! avoid `git2` to dodge its LGPL surface). The two paths that matter:
//!
//! - `git_dir`  → `~/.mimofanfan/snapshots/<project_hash>/<worktree_hash>/.git`
//! - `work_tree` → the user's actual workspace
//!
//! Every git invocation passes both `--git-dir` AND `--work-tree`. That is
//! the single biggest safety mechanism: it guarantees we never accidentally
//! mutate the user's own `.git` directory. If git can't find the side
//! repo, the command fails fast instead of falling back to "current
//! directory".

use std::collections::HashSet;
use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::Output;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::dependencies::ExternalTool;

use super::paths::{ensure_snapshot_dir, snapshot_git_dir};

/// Identifier for a snapshot — currently the underlying git commit SHA.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SnapshotId(pub String);

impl SnapshotId {
    /// Borrow the SHA as a string slice.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A single snapshot record (one row in `git log`).
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Commit SHA inside the side repo.
    pub id: SnapshotId,
    /// Subject line — the label passed to [`SnapshotRepo::snapshot`].
    pub label: String,
    /// Author timestamp (Unix seconds).
    pub timestamp: i64,
}

/// Wrapper around the per-workspace side-git repo.
pub struct SnapshotRepo {
    git_dir: PathBuf,
    work_tree: PathBuf,
}

const STALE_TMP_PACK_AGE: Duration = Duration::from_secs(60 * 60);

/// Maximum total snapshot storage in megabytes before pruning kicks in at
/// snapshot time. Keeps the side repo from blowing up the user's disk during
/// long-running or high-churn sessions (#1112).
const MAX_SNAPSHOT_SIZE_MB: u64 = 500;

/// Grace margin below `MAX_SNAPSHOT_SIZE_MB` used as the prune target
/// so the repo doesn't hit the limit again one snapshot later.
const PRUNE_TARGET_MB: u64 = 400;

/// Default workspace-size ceiling above which snapshots self-disable
/// on first use (2 GB of non-excluded content). Reports from users with
/// multi-hundred-GB project directories — datasets, model weights,
/// docker image dumps that fall outside the built-in excludes —
/// surfaced that `git add -A` on first init would hang the TUI for
/// minutes-to-hours while indexing the workspace. Snapshots are a
/// rollback safety net, not a backup tool; bailing out on workspaces
/// that big is the right tradeoff. Users with legitimate large
/// monorepos can raise `[snapshots] max_workspace_gb` (or set it to
/// `0` to disable the cap entirely).
pub const DEFAULT_MAX_WORKSPACE_BYTES_FOR_SNAPSHOT: u64 = 2 * 1024 * 1024 * 1024;

/// Hard cap on the number of file entries the bounded size estimator
/// will inspect before declaring the workspace "too large". Protects
/// against a workspace with millions of tiny files (no individual
/// file is large, but `git add -A` would still take forever).
const SIZE_WALK_MAX_ENTRIES: usize = 200_000;

/// Top-level directory and extension patterns that the snapshot path
/// already excludes via `BUILTIN_EXCLUDES`. The estimator skips these
/// up front so the size walk reflects what would actually land in the
/// snapshot commit. Kept narrow to common build-output dirs — anything
/// else falls back to the `.gitignore` filter.
const SIZE_WALK_SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    "dist",
    "build",
    ".build",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    "vendor",
    ".cargo",
    ".rustup",
    ".npm",
    ".bun",
    ".yarn",
    ".pnpm-store",
    ".cache",
    ".venv",
    "venv",
    ".tox",
    "__pycache__",
    ".mypy_cache",
    ".pytest_cache",
    ".ruff_cache",
    ".gradle",
    ".m2",
    ".local",
    ".git",
];

const BUILTIN_EXCLUDES: &str = "\
# mimofan built-in snapshot exclusions
node_modules/
target/
dist/
build/
.build/
.next/
.nuxt/
.svelte-kit/
.turbo/
.parcel-cache/
vendor/
.cargo/
.rustup/
.npm/
.bun/
.yarn/
.pnpm-store/
.cache/
.venv/
venv/
.tox/
__pycache__/
*.pyc
.mypy_cache/
.pytest_cache/
.ruff_cache/
.gradle/
.m2/
.local/
.DS_Store

# Binary and generated artifacts. Snapshots are source rollback checkpoints,
# not a full binary backup; keeping these out avoids side-repo bloat.
*.exe
*.dll
*.so
*.dylib
*.wasm
*.o
*.obj
*.class
*.pdb
*.dSYM
*.zip
*.tar
*.tar.gz
*.tgz
*.tar.bz2
*.tar.xz
*.7z
*.rar
*.iso
*.dmg
*.bin
*.mp4
*.mov
*.mkv
*.avi
*.webm
*.mp3
*.wav
*.flac
*.aac
";

impl SnapshotRepo {
    /// Open or initialize the snapshot repo for `workspace`.
    ///
    /// On first use this:
    /// 1. Creates the `~/.mimofanfan/snapshots/<…>/.git` dir.
    /// 2. Runs `git init --bare=false --quiet`.
    /// 3. Sets a fixed `user.name` / `user.email` so commits don't pick up
    ///    the user's global git identity (we don't want our snapshots to
    ///    look like they came from the user).
    pub fn open_or_init(workspace: &Path) -> io::Result<Self> {
        Self::open_or_init_with_cap(workspace, DEFAULT_MAX_WORKSPACE_BYTES_FOR_SNAPSHOT)
    }

    /// Variant of [`Self::open_or_init`] that accepts an explicit
    /// workspace-size cap. `cap_bytes = 0` disables the cap entirely
    /// (always snapshot, regardless of size).
    ///
    /// When the workspace exceeds the cap and the side repo hasn't
    /// been initialized yet, returns `Err(InvalidInput)` with a
    /// "workspace too large" reason. Subsequent calls (after the user
    /// shrinks the workspace or raises the cap via config) succeed.
    pub fn open_or_init_with_cap(workspace: &Path, cap_bytes: u64) -> io::Result<Self> {
        let work_tree = workspace
            .canonicalize()
            .unwrap_or_else(|_| workspace.to_path_buf());
        if let Some(reason) =
            unsafe_workspace_snapshot_reason(&work_tree, dirs::home_dir().as_deref())
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "workspace snapshots are disabled for {reason}: {}",
                    work_tree.display()
                ),
            ));
        }

        let _ = ensure_snapshot_dir(&work_tree)?;
        let git_dir = snapshot_git_dir(&work_tree);

        let needs_init = !git_dir.exists();
        if needs_init {
            // First-init size guard. Skipping this on subsequent opens
            // is intentional: paying a workspace walk on every snapshot
            // would defeat the purpose of the cap, and a workspace
            // that fit on first init is allowed to grow within the
            // existing repo's `MAX_SNAPSHOT_SIZE_MB` budget. Users on
            // workspaces that grew past the cap mid-session get the
            // existing aggressive-pruning path in `snapshot()`.
            if estimate_workspace_size_bounded(&work_tree, cap_bytes).is_none() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "workspace too large for snapshots (over {} GB of non-excluded content or > {} entries): {}\n  raise `[snapshots] max_workspace_gb` in config.toml (or set it to 0 to disable the cap) if you want snapshots on this workspace.",
                        cap_bytes / (1024 * 1024 * 1024),
                        SIZE_WALK_MAX_ENTRIES,
                        work_tree.display()
                    ),
                ));
            }
            let parent = git_dir.parent().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "snapshot dir has no parent")
            })?;
            std::fs::create_dir_all(parent)?;
            // `git init` here uses the parent directory as the work tree
            // and stores metadata in `.git`. We then continue to use
            // explicit `--git-dir` / `--work-tree` flags for every other
            // command so behaviour is invariant of cwd.
            let init = crate::dependencies::Git::command()
                .ok_or_else(|| io_other("git not found on PATH"))?
                .arg("init")
                .arg("--quiet")
                .arg(parent)
                .output()
                .map_err(|e| io_other(format!("failed to spawn git init: {e}")))?;
            if !init.status.success() {
                return Err(io_other(format!(
                    "git init failed: {}",
                    String::from_utf8_lossy(&init.stderr).trim()
                )));
            }

            // Pin a stable identity so snapshot commits are recognisable
            // and don't bleed into the user's git config.
            let _ = run_git(
                &git_dir,
                &work_tree,
                &["config", "user.name", "deepseek-snapshots"],
            );
            let _ = run_git(
                &git_dir,
                &work_tree,
                &["config", "user.email", "snapshots@mimofan.local"],
            );
            // Don't auto-gc on every commit; we manage pruning ourselves.
            let _ = run_git(&git_dir, &work_tree, &["config", "gc.auto", "0"]);
            // Ignore CRLF rewriting — we want byte-for-byte fidelity.
            let _ = run_git(&git_dir, &work_tree, &["config", "core.autocrlf", "false"]);
        }

        write_builtin_excludes(&git_dir)?;
        if let Err(err) = cleanup_stale_pack_temps(&git_dir, STALE_TMP_PACK_AGE) {
            tracing::debug!(
                target: "snapshot",
                "failed to clean stale snapshot tmp_pack files: {err}"
            );
        }
        Ok(Self { git_dir, work_tree })
    }

    /// Take a snapshot of the current working tree.
    ///
    /// Internally: `git add -A`, `git write-tree`, `git commit-tree`, then
    /// `git update-ref HEAD <commit>`.
    /// `git add -A` honours the user's workspace ignore rules while staging
    /// into the side repo's index.
    ///
    /// Before committing, checks whether the snapshot directory exceeds
    /// [`MAX_SNAPSHOT_SIZE_MB`] and prunes the oldest snapshots if it does.
    ///
    /// Returns the snapshot's commit SHA.
    pub fn snapshot(&self, label: &str) -> io::Result<SnapshotId> {
        // Guard against disk blowup (#1112): if the snapshot directory has
        // grown beyond the limit, prune aggressively before adding more.
        if let Ok(current_mb) = dir_size_mb(&self.git_dir)
            && current_mb > MAX_SNAPSHOT_SIZE_MB
        {
            tracing::warn!(
                target: "snapshot",
                current_mb,
                limit_mb = MAX_SNAPSHOT_SIZE_MB,
                "snapshot storage approaching limit — pruning aggressively"
            );
            // Walk backward from a 1-second retention to zero until
            // we're under the target, or until there's nothing left.
            let mut age = Duration::from_secs(1);
            for _ in 0..10 {
                let _ = self.prune_older_than(age);
                if let Ok(new_size) = dir_size_mb(&self.git_dir)
                    && new_size <= PRUNE_TARGET_MB
                {
                    tracing::info!(
                        target: "snapshot",
                        new_size_mb = new_size,
                        "pruned snapshot storage back under limit"
                    );
                    break;
                }
                age = age.saturating_sub(Duration::from_millis(100));
            }
            // Fallback: if even 0-second pruning didn't help (shouldn't
            // happen but belt-and-suspenders), nuke the refs so the next
            // snapshot starts a fresh history.
            if let Ok(final_size) = dir_size_mb(&self.git_dir)
                && final_size > MAX_SNAPSHOT_SIZE_MB
            {
                tracing::warn!(
                    target: "snapshot",
                    "snapshot storage still over limit after pruning; wiping history"
                );
                let _ = self.prune_older_than(Duration::ZERO);
                let _ = self.prune_unreachable_objects();
            }
        }
        // Stage every tracked + untracked path the workspace exposes.
        // `--all` here means `add` + `update` + `remove` — the same set
        // `git status` would show.
        let add = run_git(&self.git_dir, &self.work_tree, &["add", "-A"])?;
        if !add.status.success() {
            return Err(io_other(format!(
                "git add -A failed: {}",
                String::from_utf8_lossy(&add.stderr).trim()
            )));
        }

        let tree = run_git(&self.git_dir, &self.work_tree, &["write-tree"])?;
        if !tree.status.success() {
            return Err(io_other(format!(
                "git write-tree failed: {}",
                String::from_utf8_lossy(&tree.stderr).trim()
            )));
        }
        let tree = String::from_utf8_lossy(&tree.stdout).trim().to_string();

        let parent = run_git(
            &self.git_dir,
            &self.work_tree,
            &["rev-parse", "--verify", "HEAD"],
        )?;
        let parent = parent
            .status
            .success()
            .then(|| String::from_utf8_lossy(&parent.stdout).trim().to_string())
            .filter(|s| !s.is_empty());

        let mut args = vec!["commit-tree".to_string(), tree];
        if let Some(parent) = parent {
            args.push("-p".to_string());
            args.push(parent);
        }
        args.push("-m".to_string());
        args.push(label.to_string());
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();

        // `commit-tree` creates marker commits even when the tree matches its
        // parent, and it does not run user/global commit hooks.
        let commit = run_git(&self.git_dir, &self.work_tree, &arg_refs)?;
        if !commit.status.success() {
            return Err(io_other(format!(
                "git commit-tree failed: {}",
                String::from_utf8_lossy(&commit.stderr).trim()
            )));
        }
        let sha = String::from_utf8_lossy(&commit.stdout).trim().to_string();

        let update = run_git(
            &self.git_dir,
            &self.work_tree,
            &["update-ref", "HEAD", &sha],
        )?;
        if !update.status.success() {
            return Err(io_other(format!(
                "git update-ref HEAD failed: {}",
                String::from_utf8_lossy(&update.stderr).trim()
            )));
        }

        Ok(SnapshotId(sha))
    }

    /// Restore the workspace to the state at `id`.
    ///
    /// Uses `git checkout <sha> -- :/` which checks out every path in the
    /// snapshot tree relative to the workspace root. We do NOT touch the
    /// user's own `.git` — snapshots only contain working-tree files.
    pub fn restore(&self, id: &SnapshotId) -> io::Result<()> {
        let current_paths = self.tree_paths("HEAD")?;
        let target_paths = self.tree_paths(id.as_str())?;
        let checkout = run_git(
            &self.git_dir,
            &self.work_tree,
            &["checkout", id.as_str(), "--", ":/"],
        )?;
        if !checkout.status.success() {
            return Err(io_other(format!(
                "git checkout failed: {}",
                String::from_utf8_lossy(&checkout.stderr).trim()
            )));
        }
        self.remove_paths_missing_from_target(&current_paths, &target_paths)?;
        Ok(())
    }

    /// Return whether the current workspace matches the given snapshot's
    /// tracked file content.
    ///
    /// This is intentionally narrower than a full "workspace identical"
    /// claim: it compares the current working tree against the snapshot's
    /// tracked paths via git's diff machinery. That is sufficient for
    /// `/undo` cursoring — if the diff is empty, restoring this snapshot
    /// again would be a no-op, so the caller should continue scanning
    /// older snapshots.
    pub fn work_tree_matches_snapshot(&self, id: &SnapshotId) -> io::Result<bool> {
        let diff = run_git(
            &self.git_dir,
            &self.work_tree,
            &["diff", "--quiet", id.as_str(), "--", ":/"],
        )?;
        Ok(diff.status.success())
    }

    fn tree_paths(&self, treeish: &str) -> io::Result<HashSet<PathBuf>> {
        let ls = run_git(
            &self.git_dir,
            &self.work_tree,
            &["ls-tree", "-r", "-z", "--name-only", treeish],
        )?;
        if !ls.status.success() {
            return Err(io_other(format!(
                "git ls-tree failed: {}",
                String::from_utf8_lossy(&ls.stderr).trim()
            )));
        }
        Ok(parse_nul_paths(&ls.stdout))
    }

    fn remove_paths_missing_from_target(
        &self,
        current_paths: &HashSet<PathBuf>,
        target_paths: &HashSet<PathBuf>,
    ) -> io::Result<()> {
        for rel in current_paths.difference(target_paths) {
            if !is_safe_relative_path(rel) {
                continue;
            }
            let path = self.work_tree.join(rel);
            let Ok(metadata) = std::fs::symlink_metadata(&path) else {
                continue;
            };
            if metadata.file_type().is_dir() {
                let _ = std::fs::remove_dir(&path);
            } else {
                std::fs::remove_file(&path)?;
            }
            self.prune_empty_parent_dirs(path.parent());
        }
        Ok(())
    }

    fn prune_empty_parent_dirs(&self, mut dir: Option<&Path>) {
        while let Some(path) = dir {
            if path == self.work_tree {
                break;
            }
            if std::fs::remove_dir(path).is_err() {
                break;
            }
            dir = path.parent();
        }
    }

    /// List up to `limit` most-recent snapshots, newest first.
    pub fn list(&self, limit: usize) -> io::Result<Vec<Snapshot>> {
        // `git log -<n>` is the short form of `--max-count=<n>`; if `limit`
        // is `usize::MAX` (caller asked for "everything") we pass an empty
        // count so git defaults to no upper bound.
        let mut args: Vec<String> = vec!["log".to_string()];
        if limit < usize::MAX {
            args.push(format!("--max-count={limit}"));
        }
        args.push("--pretty=format:%H%x09%at%x09%s".to_string());
        args.push("--no-color".to_string());
        let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
        let log = run_git(&self.git_dir, &self.work_tree, &arg_refs)?;
        if !log.status.success() {
            // No commits yet → empty list.
            return Ok(Vec::new());
        }
        let stdout = String::from_utf8_lossy(&log.stdout);
        let mut out = Vec::new();
        for line in stdout.lines() {
            let mut parts = line.splitn(3, '\t');
            let sha = parts.next().unwrap_or("").to_string();
            let ts = parts
                .next()
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let subject = parts.next().unwrap_or("").to_string();
            if sha.is_empty() {
                continue;
            }
            out.push(Snapshot {
                id: SnapshotId(sha),
                label: subject,
                timestamp: ts,
            });
        }
        Ok(out)
    }

    /// Drop snapshots older than `max_age`, returning the count removed.
    ///
    /// Strategy: identify keepable commits (younger than the cutoff),
    /// reset HEAD to the oldest survivor, then `git reflog expire` +
    /// `git gc --prune=now` to actually reclaim space. Cheap and avoids
    /// rewriting history when nothing has aged out.
    pub fn prune_older_than(&self, max_age: Duration) -> io::Result<usize> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| io_other(format!("clock error: {e}")))?
            .as_secs() as i64;
        let cutoff = now - max_age.as_secs() as i64;

        let snapshots = self.list(usize::MAX)?;
        if snapshots.is_empty() {
            return Ok(0);
        }

        // Snapshots are newest-first. Find the index of the first one
        // at-or-older than the cutoff — every entry from that index
        // onward is a candidate for removal. We use `<=` so a 0-second
        // retention drops same-second commits (otherwise tests calling
        // `prune_older_than(Duration::ZERO)` immediately after creating
        // a snapshot would never prune anything).
        let cut_index = snapshots.iter().position(|s| s.timestamp <= cutoff);
        let Some(cut) = cut_index else {
            return Ok(0);
        };
        let removed = snapshots.len() - cut;
        if removed == 0 {
            return Ok(0);
        }

        if cut == 0 {
            // Every snapshot is older than the cutoff — wipe the repo
            // entirely so the next snapshot starts a fresh history.
            // Removing `.git/refs/heads/*` is enough to orphan the old
            // commits, then gc reclaims them.
            let refs_dir = self.git_dir.join("refs").join("heads");
            if refs_dir.exists() {
                for entry in std::fs::read_dir(&refs_dir)? {
                    let path = entry?.path();
                    if path.is_file() {
                        let _ = std::fs::remove_file(&path);
                    }
                }
            }
            // Also drop HEAD's packed refs so `git log` returns nothing.
            let packed = self.git_dir.join("packed-refs");
            if packed.exists() {
                let _ = std::fs::remove_file(&packed);
            }
        } else {
            // Reset HEAD to the youngest commit older-than-cutoff's
            // *predecessor* — i.e. the oldest surviving snapshot.
            let survivor = &snapshots[cut - 1];
            let reset = run_git(
                &self.git_dir,
                &self.work_tree,
                &["update-ref", "HEAD", survivor.id.as_str()],
            )?;
            if !reset.status.success() {
                return Err(io_other(format!(
                    "git update-ref failed: {}",
                    String::from_utf8_lossy(&reset.stderr).trim()
                )));
            }
        }

        // Reclaim space.
        let _ = run_git(
            &self.git_dir,
            &self.work_tree,
            &["reflog", "expire", "--expire=now", "--all"],
        );
        let _ = run_git(
            &self.git_dir,
            &self.work_tree,
            &["gc", "--prune=now", "--quiet"],
        );

        Ok(removed)
    }

    /// Keep only the latest `max_count` snapshots, dropping older ones.
    ///
    /// Uses `commit-tree` with no `-p` to create a true orphan commit at
    /// the eldest survivor's tree, preserving its label.  The old chain
    /// has zero refs after gc and is physically reclaimed.
    /// Keep only the latest `max_count` snapshots by rebuilding the
    /// survivor chain as orphan commits.  Each survivor's tree and label
    /// are preserved — only the parent chain to older snapshots is cut.
    /// Old objects become unreachable and gc reclaims them.
    pub fn prune_keep_last_n(&self, max_count: usize) -> io::Result<usize> {
        let snapshots = self.list(usize::MAX)?;
        if snapshots.len() <= max_count {
            return Ok(0);
        }
        let keep = max_count;
        let removed = snapshots.len() - keep;
        // snapshots are newest-first: [0..keep-1] are the survivors.
        // Rebuild the chain from oldest survivor → newest, each as a
        // commit-tree with the original tree but no link to the old chain.
        let mut prev_sha: Option<String> = None;

        for i in (0..keep).rev() {
            let s = &snapshots[i];
            let tree = run_git(
                &self.git_dir,
                &self.work_tree,
                &["rev-parse", &format!("{}^{{tree}}", s.id.as_str())],
            )?;
            if !tree.status.success() {
                return Err(io_other(format!(
                    "rev-parse {}^{{tree}} failed: {}",
                    s.id.as_str(),
                    String::from_utf8_lossy(&tree.stderr).trim()
                )));
            }
            let tree_hash = String::from_utf8_lossy(&tree.stdout).trim().to_string();

            let mut args = vec![
                "commit-tree".to_string(),
                "-m".to_string(),
                s.label.clone(),
                tree_hash,
            ];
            if let Some(ref p) = prev_sha {
                args.push("-p".to_string());
                args.push(p.clone());
            }
            let arg_refs: Vec<&str> = args.iter().map(String::as_str).collect();
            let newc = run_git(&self.git_dir, &self.work_tree, &arg_refs)?;
            if !newc.status.success() {
                return Err(io_other(format!(
                    "commit-tree failed: {}",
                    String::from_utf8_lossy(&newc.stderr).trim()
                )));
            }
            let new_sha = String::from_utf8_lossy(&newc.stdout).trim().to_string();
            prev_sha = Some(new_sha);
        }

        if let Some(final_sha) = prev_sha {
            let up = run_git(
                &self.git_dir,
                &self.work_tree,
                &["update-ref", "HEAD", &final_sha],
            )?;
            if !up.status.success() {
                return Err(io_other(format!(
                    "update-ref HEAD failed: {}",
                    String::from_utf8_lossy(&up.stderr).trim()
                )));
            }
        }
        let _ = run_git(
            &self.git_dir,
            &self.work_tree,
            &["reflog", "expire", "--expire=now", "--all"],
        );
        let _ = run_git(
            &self.git_dir,
            &self.work_tree,
            &["gc", "--prune=now", "--quiet"],
        );
        Ok(removed)
    }

    /// Drop unreachable loose objects left behind by interrupted or
    /// orphaned side-repo operations.
    pub fn prune_unreachable_objects(&self) -> io::Result<()> {
        let prune = run_git(&self.git_dir, &self.work_tree, &["prune", "--expire=now"])?;
        if !prune.status.success() {
            return Err(io_other(format!(
                "git prune failed: {}",
                String::from_utf8_lossy(&prune.stderr).trim()
            )));
        }
        Ok(())
    }

    /// Return the side-repo's `.git` directory (for diagnostics).
    #[allow(dead_code)]
    pub fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    /// Return the work tree path (for diagnostics).
    #[allow(dead_code)]
    pub fn work_tree(&self) -> &Path {
        &self.work_tree
    }
}

fn write_builtin_excludes(git_dir: &Path) -> io::Result<()> {
    let info_dir = git_dir.join("info");
    std::fs::create_dir_all(&info_dir)?;
    std::fs::write(info_dir.join("exclude"), BUILTIN_EXCLUDES)
}

/// Recursively compute the total size of a directory in megabytes.
fn dir_size_mb(root: &Path) -> io::Result<u64> {
    fn walk(dir: &Path, total: &mut u64) -> io::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            let ft = entry.file_type()?;
            if ft.is_symlink() {
                continue;
            }
            if ft.is_dir() {
                walk(&path, total)?;
            } else if ft.is_file() {
                *total = total.saturating_add(entry.metadata().map(|m| m.len()).unwrap_or(0));
            }
        }
        Ok(())
    }
    let mut total: u64 = 0;
    walk(root, &mut total)?;
    Ok(total / (1024 * 1024))
}

fn cleanup_stale_pack_temps(git_dir: &Path, stale_age: Duration) -> io::Result<usize> {
    let pack_dir = git_dir.join("objects").join("pack");
    if !pack_dir.exists() {
        return Ok(0);
    }
    cleanup_stale_pack_temps_in(&pack_dir, stale_age, SystemTime::now())
}

fn cleanup_stale_pack_temps_in(
    pack_dir: &Path,
    stale_age: Duration,
    now: SystemTime,
) -> io::Result<usize> {
    let mut removed = 0;
    for entry in std::fs::read_dir(pack_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if !name.starts_with("tmp_pack_") {
            continue;
        }
        if !entry.file_type()?.is_file() {
            continue;
        }

        let metadata = entry.metadata()?;
        let Ok(modified) = metadata.modified() else {
            continue;
        };
        let Ok(age) = now.duration_since(modified) else {
            continue;
        };
        if age < stale_age {
            continue;
        }

        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(err) if err.kind() == io::ErrorKind::NotFound => {}
            Err(err) => return Err(err),
        }
    }
    Ok(removed)
}

fn run_git(git_dir: &Path, work_tree: &Path, args: &[&str]) -> io::Result<Output> {
    crate::dependencies::Git::command()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "git not found on PATH"))?
        .arg("--git-dir")
        .arg(git_dir)
        .arg("--work-tree")
        .arg(work_tree)
        .args(args)
        .output()
}

fn io_other(msg: impl Into<String>) -> io::Error {
    io::Error::other(msg.into())
}

/// Walk `workspace` and accumulate file sizes, returning `Some(total)`
/// when the workspace fits under `cap_bytes` and `None` when the walk
/// exceeds the cap. Honors `.gitignore` (via the `ignore` crate's
/// `WalkBuilder` defaults) and the snapshot-specific skip list above,
/// so the measured size reflects what would actually land in a
/// snapshot commit rather than the raw `du -sh` total.
///
/// The walk is bounded by both `cap_bytes` and
/// [`SIZE_WALK_MAX_ENTRIES`] — either trip returns `None`. A
/// `cap_bytes` of `0` disables the cap entirely (returns `Some(total)`
/// no matter how large), so config can opt out.
pub fn estimate_workspace_size_bounded(workspace: &Path, cap_bytes: u64) -> Option<u64> {
    use ignore::WalkBuilder;
    let mut total: u64 = 0;
    let mut entries: usize = 0;
    let skip: HashSet<&'static str> = SIZE_WALK_SKIP_DIRS.iter().copied().collect();
    let walker = WalkBuilder::new(workspace)
        .hidden(false)
        .follow_links(false)
        .filter_entry(move |entry| {
            // Skip the well-known build-output directories at any depth.
            // The `ignore` crate calls `filter_entry` once per dir/file;
            // returning `false` here prunes the whole subtree.
            entry
                .file_name()
                .to_str()
                .is_none_or(|name| !skip.contains(name))
        })
        .build();
    for entry in walker.flatten() {
        entries += 1;
        if entries > SIZE_WALK_MAX_ENTRIES {
            return None;
        }
        if let Ok(meta) = entry.metadata()
            && meta.is_file()
        {
            total = total.saturating_add(meta.len());
            if cap_bytes > 0 && total > cap_bytes {
                return None;
            }
        }
    }
    Some(total)
}

fn unsafe_workspace_snapshot_reason(workspace: &Path, home: Option<&Path>) -> Option<&'static str> {
    let workspace = normalize_path_for_safety(workspace);
    if is_filesystem_root(&workspace) {
        return Some("filesystem root");
    }

    if is_home_directory(&workspace, home) {
        return Some("home directory");
    }

    let home = home.map(normalize_path_for_safety)?;
    if workspace.parent() == Some(home.as_path()) {
        let name = workspace.file_name().and_then(|name| name.to_str());
        if matches!(
            name,
            Some(
                "Desktop" | "Documents" | "Downloads" | "Library" | "Movies" | "Music" | "Pictures"
            )
        ) {
            return Some("home collection directory");
        }
    }

    None
}

fn normalize_path_for_safety(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn is_filesystem_root(path: &Path) -> bool {
    path.parent().is_none()
}

fn is_home_directory(work_tree: &Path, home: Option<&Path>) -> bool {
    let Some(home) = home else {
        return false;
    };

    let home_canonical = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    work_tree == home_canonical
}

fn parse_nul_paths(bytes: &[u8]) -> HashSet<PathBuf> {
    bytes
        .split(|b| *b == 0)
        .filter(|chunk| !chunk.is_empty())
        .map(|chunk| PathBuf::from(String::from_utf8_lossy(chunk).into_owned()))
        .collect()
}

fn is_safe_relative_path(path: &Path) -> bool {
    !path.as_os_str().is_empty()
        && path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[cfg(test)]
mod tests {}

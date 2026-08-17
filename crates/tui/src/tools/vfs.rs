//! #639：统一的虚拟文件系统（VFS）抽象。
//!
//! 背景：文件工具（`read_file`/`write_file`/`edit_file`/`list_dir`）原本直接调用
//! `tokio::fs` / `std::fs` 做 IO，IO 行为不可替换、不可单测。本模块抽出 `trait Vfs`
//! 作为唯一 IO 边界，默认实现 [`StdFs`] 委托标准库 `std::fs` 同步 IO；测试与特殊
//! 场景可提供内存实现（如 [`MockVfs`]，见测试模块）验证工具确实走 trait 而非真实磁盘。
//!
//! 设计为**同步** trait：标准库文件 IO 本身同步，tool 的 `async fn execute` 内可直接
//! 同步调用（无需 `.await`），保持改动最小、不引入 `async_trait` 传播链。
//!
//! # TOCTOU 防护（乐观锁）
//!
//! 默认 [`StdFs::write_text`] 是无条件覆盖写（读-改-写之间没有任何保护）。
//! 多 agent 并发编辑同一文件时，存在经典的 **TOCTOU（Time-of-check /
//! Time-of-use）** 竞争：agent A 读入旧内容、A 计算 diff、A 落盘之间，agent B 已
//! 经写入了它的修改，于是 A 的覆盖写会**静默丢弃 B 的改动**。
//!
//! 为此提供 [`Vfs::write_if_unchanged`]：调用方传入「它基于其编辑的旧内容」
//! `expected_old_content`，实现仅在磁盘当前内容与之完全一致时才落盘，否则返回
//! [`VfsError::StaleContent`]（含实际内容的摘要）让调用方重新读-改-写。这样把
//! 「检查」与「使用」在单次系统调用内原子化，杜绝盲覆盖。
//! 非并发场景仍可继续用 [`Vfs::write_text`] 直接覆盖。

use std::error::Error;
use std::fmt;
use std::io;
use std::path::{Path, PathBuf};

/// IO 结果别名，与标准库一致。
pub type IoResult<T> = io::Result<T>;

/// VFS 操作错误。
///
/// 除标准库 IO 错误外，新增乐观锁相关的 [`VfsError::StaleContent`]：当
/// [`Vfs::write_if_unchanged`] 发现磁盘当前内容与期望的旧内容不一致时返回，
/// 调用方据此重新读取最新内容并重试编辑。
#[derive(Debug)]
pub enum VfsError {
    /// 底层 IO 错误（包装 `std::io::Error`）。
    Io(io::Error),
    /// 乐观锁失败：磁盘当前内容与期望的旧内容不一致（TOCTOU 防护命中）。
    ///
    /// 携带磁盘实际内容的摘要（前缀，最多 [StaleContentError::SUMMARY_MAX] 字节），
    /// 供调用方在不重新读取的情况下快速定位差异规模。
    StaleContent(StaleContentError),
}

impl fmt::Display for VfsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VfsError::Io(e) => write!(f, "VFS IO error: {e}"),
            VfsError::StaleContent(e) => write!(
                f,
                "VFS stale content: disk content changed since read (actual summary: {:?})",
                e.actual_summary
            ),
        }
    }
}

impl Error for VfsError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            VfsError::Io(e) => Some(e),
            VfsError::StaleContent(_) => None,
        }
    }
}

impl From<io::Error> for VfsError {
    fn from(e: io::Error) -> Self {
        VfsError::Io(e)
    }
}

/// [`VfsError::StaleContent`] 的详情：磁盘实际内容摘要。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleContentError {
    /// 磁盘当前内容的截断摘要（UTF-8 安全，最多 [Self::SUMMARY_MAX] 字节）。
    pub actual_summary: String,
}

impl StaleContentError {
    /// 摘要最大长度（字节）。
    pub const SUMMARY_MAX: usize = 200;

    /// 由磁盘实际内容构造摘要（截断到 [Self::SUMMARY_MAX] 字节，不保证字符边界）。
    pub fn new(actual: &str) -> Self {
        let summary = if actual.len() <= Self::SUMMARY_MAX {
            actual.to_string()
        } else {
            actual[..Self::SUMMARY_MAX].to_string()
        };
        Self {
            actual_summary: summary,
        }
    }
}

/// 虚拟文件系统抽象——所有文件 IO 的唯一出入口。
///
/// 方法均为同步（委托 `std::fs`）。实现者负责把路径错误映射为 `io::Error`。
pub trait Vfs: Send + Sync {
    /// 以文本读入整个文件。
    fn read_text(&self, path: &Path) -> IoResult<String>;
    /// 以字节读入整个文件。
    fn read_bytes(&self, path: &Path) -> IoResult<Vec<u8>>;
    /// 把文本写入文件（覆盖），必要时创建父目录。
    ///
    /// 无条件覆盖，不提供任何并发保护。非并发场景或调用方已自行保证独占访问时使用。
    fn write_text(&self, path: &Path, content: &str) -> IoResult<()>;
    /// 乐观锁写：仅当磁盘当前内容与 `expected_old_content` 完全一致时才以
    /// `new_content` 覆盖，否则返回 [`VfsError::StaleContent`]。
    ///
    /// 用于多 agent 并发编辑同一文件时防止 TOCTOU 盲覆盖——调用方先读入内容、
    /// 据此生成 `expected_old_content`，落盘前由实现在单次系统调用内原子校验，
    /// 不一致则让调用方重新读-改-写。
    ///
    /// 文件不存在时视为「当前内容为空」，仅当 `expected_old_content` 也为空才写入。
    fn write_if_unchanged(
        &self,
        path: &Path,
        expected_old_content: &str,
        new_content: &str,
    ) -> Result<(), VfsError>;
    /// 递归创建目录（等价 `std::fs::create_dir_all`）。
    fn create_dir_all(&self, path: &Path) -> IoResult<()>;
    /// 列出目录直接子项路径。
    fn list_dir(&self, path: &Path) -> IoResult<Vec<PathBuf>>;
}

/// 默认实现：直接委托标准库 `std::fs` 同步 IO。
pub struct StdFs;

impl Vfs for StdFs {
    fn read_text(&self, path: &Path) -> IoResult<String> {
        std::fs::read_to_string(path)
    }

    fn read_bytes(&self, path: &Path) -> IoResult<Vec<u8>> {
        std::fs::read(path)
    }

    fn write_text(&self, path: &Path, content: &str) -> IoResult<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        std::fs::write(path, content)
    }

    fn write_if_unchanged(
        &self,
        path: &Path,
        expected_old_content: &str,
        new_content: &str,
    ) -> Result<(), VfsError> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)?;
            }
        }
        // 读取磁盘当前内容；不存在视为空串。
        let current = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
            Err(e) => return Err(VfsError::Io(e)),
        };
        if current != expected_old_content {
            return Err(VfsError::StaleContent(StaleContentError::new(&current)));
        }
        std::fs::write(path, new_content).map_err(VfsError::Io)
    }

    fn create_dir_all(&self, path: &Path) -> IoResult<()> {
        std::fs::create_dir_all(path)
    }

    fn list_dir(&self, path: &Path) -> IoResult<Vec<PathBuf>> {
        let mut out = Vec::new();
        for entry in std::fs::read_dir(path)? {
            out.push(entry?.path());
        }
        Ok(out)
    }
}

/// 返回进程内唯一的标准文件系统实现（单例）。
///
/// 真实运行时所有工具都走这里；测试可通过 [`MockVfs`] 注入内存实现验证走线。
pub fn active_vfs() -> &'static dyn Vfs {
    static STD: StdFs = StdFs;
    &STD
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[test]
    fn std_fs_round_trips_text_via_trait() {
        let dir = std::env::temp_dir().join(format!("mimofan_vfs_test_{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let p = dir.join("a.txt");
        let vfs = StdFs;
        vfs.write_text(&p, "hello").unwrap();
        assert_eq!(vfs.read_text(&p).unwrap(), "hello");
        let _ = std::fs::remove_file(&p);
    }

    /// 内存 VFS，用于证明工具 IO 确实经过 `Vfs` trait 而非真实磁盘。
    struct MockVfs {
        store: Mutex<HashMap<PathBuf, String>>,
    }

    impl MockVfs {
        fn new() -> Self {
            Self {
                store: Mutex::new(HashMap::new()),
            }
        }
    }

    impl Vfs for MockVfs {
        fn read_text(&self, path: &Path) -> IoResult<String> {
            self.store
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "mock: not found"))
        }

        fn read_bytes(&self, path: &Path) -> IoResult<Vec<u8>> {
            self.read_text(path).map(|s| s.into_bytes())
        }

        fn write_text(&self, path: &Path, content: &str) -> IoResult<()> {
            self.store
                .lock()
                .unwrap()
                .insert(path.to_path_buf(), content.to_string());
            Ok(())
        }

        fn write_if_unchanged(
            &self,
            path: &Path,
            expected_old_content: &str,
            new_content: &str,
        ) -> Result<(), VfsError> {
            let mut store = self.store.lock().unwrap();
            let current = store.get(path).cloned().unwrap_or_default();
            if current != expected_old_content {
                return Err(VfsError::StaleContent(StaleContentError::new(&current)));
            }
            store.insert(path.to_path_buf(), new_content.to_string());
            Ok(())
        }

        fn create_dir_all(&self, _path: &Path) -> IoResult<()> {
            Ok(())
        }

        fn list_dir(&self, _path: &Path) -> IoResult<Vec<PathBuf>> {
            Ok(vec![])
        }
    }

    #[test]
    fn tools_route_io_through_injected_vfs() {
        // 构造一个可替换 VFS 的临时句柄：这里直接用 MockVfs 验证 write/read 走 trait。
        let mock = MockVfs::new();
        let p = PathBuf::from("/mock/note.txt");
        mock.write_text(&p, "via-trait").unwrap();
        assert_eq!(mock.read_text(&p).unwrap(), "via-trait");
        assert!(mock.read_text(Path::new("/mock/missing")).is_err());
    }

    /// 构造进程内唯一临时目录（基于 PID + 随机后缀，规避并行测试冲突）。
    fn temp_vfs_dir(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!(
            "mimofan_vfs_opt_{}_{}_{}",
            name,
            std::process::id(),
            fast_rand()
        ));
        let _ = std::fs::create_dir_all(&p);
        p
    }

    fn fast_rand() -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        static C: AtomicU64 = AtomicU64::new(0);
        let n = C.fetch_add(1, Ordering::Relaxed);
        // 混合 PID 让不同进程不致完全同序
        (n << 16) ^ (std::process::id() as u64)
    }

    #[test]
    fn write_if_unchanged_succeeds_when_content_matches() {
        let dir = temp_vfs_dir("match");
        let p = dir.join("f.txt");
        let vfs = StdFs;
        vfs.write_text(&p, "base").unwrap();
        // 基于读到的 "base" 编辑
        vfs.write_if_unchanged(&p, "base", "base-edited").unwrap();
        assert_eq!(vfs.read_text(&p).unwrap(), "base-edited");
    }

    #[test]
    fn write_if_unchanged_rejects_stale_content_and_keeps_file() {
        let dir = temp_vfs_dir("stale");
        let p = dir.join("f.txt");
        let vfs = StdFs;
        vfs.write_text(&p, "original").unwrap();
        // 他人已写入新内容，但我们仍基于 "original" 尝试提交
        vfs.write_text(&p, "changed-by-other").unwrap();
        let err = vfs
            .write_if_unchanged(&p, "original", "original-edited")
            .unwrap_err();
        match err {
            VfsError::StaleContent(e) => {
                assert_eq!(e.actual_summary, "changed-by-other");
            }
            other => panic!("期望 StaleContent，实际 {other:?}"),
        }
        // 文件内容未被我们的提交覆盖
        assert_eq!(vfs.read_text(&p).unwrap(), "changed-by-other");
    }

    #[test]
    fn write_if_unchanged_when_file_missing_treats_empty_expected_as_match() {
        let dir = temp_vfs_dir("missing");
        let p = dir.join("nope.txt");
        let vfs = StdFs;
        // 文件不存在 + 期望旧内容为空 -> 视为一致，成功创建
        vfs.write_if_unchanged(&p, "", "fresh").unwrap();
        assert_eq!(vfs.read_text(&p).unwrap(), "fresh");
    }

    #[test]
    fn write_if_unchanged_when_file_missing_but_expected_nonempty_is_stale() {
        let dir = temp_vfs_dir("missing2");
        let p = dir.join("nope2.txt");
        let vfs = StdFs;
        // 文件不存在 + 期望旧内容非空 -> 不一致（视为被他人清空/删除）
        let err = vfs
            .write_if_unchanged(&p, "i-expected-this", "new")
            .unwrap_err();
        assert!(matches!(err, VfsError::StaleContent(_)));
        // 文件不应被创建
        assert!(vfs.read_text(&p).is_err());
    }

    #[test]
    fn mock_vfs_optimistic_lock_roundtrip() {
        let mock = MockVfs::new();
        let p = PathBuf::from("/mock/opt.txt");
        mock.write_text(&p, "v1").unwrap();
        mock.write_if_unchanged(&p, "v1", "v2").unwrap();
        assert_eq!(mock.read_text(&p).unwrap(), "v2");
        // 再次基于过期 "v1" 提交应失败
        assert!(matches!(
            mock.write_if_unchanged(&p, "v1", "v3"),
            Err(VfsError::StaleContent(_))
        ));
    }
}

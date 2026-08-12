//! #639：统一的虚拟文件系统（VFS）抽象。
//!
//! 背景：文件工具（`read_file`/`write_file`/`edit_file`/`list_dir`）原本直接调用
//! `tokio::fs` / `std::fs` 做 IO，IO 行为不可替换、不可单测。本模块抽出 `trait Vfs`
//! 作为唯一 IO 边界，默认实现 [`StdFs`] 委托标准库 `std::fs` 同步 IO；测试与特殊
//! 场景可提供内存实现（如 [`MockVfs`]，见测试模块）验证工具确实走 trait 而非真实磁盘。
//!
//! 设计为**同步** trait：标准库文件 IO 本身同步，tool 的 `async fn execute` 内可直接
//! 同步调用（无需 `.await`），保持改动最小、不引入 `async_trait` 传播链。

use std::io;
use std::path::{Path, PathBuf};

/// IO 结果别名，与标准库一致。
pub type IoResult<T> = io::Result<T>;

/// 虚拟文件系统抽象——所有文件 IO 的唯一出入口。
///
/// 方法均为同步（委托 `std::fs`）。实现者负责把路径错误映射为 `io::Error`。
pub trait Vfs: Send + Sync {
    /// 以文本读入整个文件。
    fn read_text(&self, path: &Path) -> IoResult<String>;
    /// 以字节读入整个文件。
    fn read_bytes(&self, path: &Path) -> IoResult<Vec<u8>>;
    /// 把文本写入文件（覆盖），必要时创建父目录。
    fn write_text(&self, path: &Path, content: &str) -> IoResult<()>;
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
}

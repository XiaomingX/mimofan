//! 路径安全内核（Path Guard）—— #681
//!
//! 把分散在各工具 `resolve_path` 里的「越界 / 敏感 / 不存在」三态（实际四态）判定
//! 抽成**单一、纯逻辑、零 IO（除注入式 `exists` 闭包）** 的可测内核。
//!
//! 设计要点：
//! - 不依赖 `std::fs` 真实 IO：存在性通过调用方注入的 `exists` 闭包判定，使单测
//!   可以完全确定性地驱动（`../../etc` 穿越、绝对路径逃逸、`.env/.ssh/*.pem` 敏感、
//!   不存在等都能在无文件系统副作用下覆盖）。
//! - 边界判定是**词法（lexical）** 的：`candidate` 必须是 `workspace` 的整段前缀，
//!   且路径中不能出现逃逸组件（`..` 越出、或绝对路径根不属于 workspace）。这给出
//!   与平台/是否 canonicalize 无关的确定性结果，作为上层 `resolve_path` 的权威判定。
//! - 上层 `resolve_path` 可把内核 verdict 的 `EscapesWorkspace` 直接映射为
//!   `ToolError::PathEscape`，同时保留其既有的「可信外部路径例外」「符号链接」等
//!   二次校验逻辑——本内核只负责 workspace 纯边界与敏感命名。
//!
//! 命名与 #639（统一 VFS）的边界：本内核只做「路径越界/敏感/存在」判定，不做 IO
//! 抽象；VFS 负责读写抽象。两者职责不重叠。

use std::path::{Path, PathBuf};

/// 对单条路径的确定性安全判定。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathVerdict {
    /// 路径在 workspace 内且确实存在。
    InsideWorkspace,
    /// 路径超出了 workspace 边界（词法越界）。
    EscapesWorkspace,
    /// 路径命中敏感文件命名（`.env` / `.ssh` / `id_rsa` / `*.pem` 等）。
    ///
    /// 敏感判定只看命名，与是否存在无关；上层可据策略决定是否拦截或仅告警。
    Sensitive,
    /// 路径不在 workspace 边界外、且不命中敏感命名，但目标文件/目录不存在
    ///（用于写操作的前置校验）。
    NotFound,
}

/// 敏感文件名（精确匹配，小写）。
const SENSITIVE_BASENAMES: &[&str] = &[
    ".env",
    ".envrc",
    ".npmrc",
    ".netrc",
    ".pgpass",
    ".ssh",
    "id_rsa",
    "id_dsa",
    "id_ecdsa",
    "id_ed25519",
    "id_ed25519_sk",
    "known_hosts",
    "authorized_keys",
    "credentials",
    "credential",
    "gserviceaccount.json",
    "service-account.json",
];

/// 敏感扩展名（小写，含点）。
const SENSITIVE_EXTENSIONS: &[&str] = &[
    ".pem", ".key", ".p12", ".pfx", ".keystore", ".jks", ".kdbx", ".age", ".gpg",
];

/// 词法归一化：把路径拆成组件序列。
///
/// - 绝对路径记录 `root`（Unix `/`，或 Windows 盘符 `C:`）。
/// - `""` / `.` 组件被忽略。
/// - `..` 组件如果会越出已收集组件栈则使整体失效（返回 `None` = 逃逸/非法）。
/// - 反斜杠按分隔符处理，便于跨平台一致的匹配。
fn lexical_components(path: &Path) -> Option<(Option<String>, Vec<String>)> {
    let s = path.to_string_lossy().replace('\\', "/");
    let (root, rest) = if let Some(stripped) = s.strip_prefix('/') {
        (Some("/".to_string()), stripped.to_string())
    } else if s.len() >= 2 && s.as_bytes()[1] == b':' && s.as_bytes()[0].is_ascii_alphabetic() {
        // Windows 盘符根，如 C:/ 或 C:\
        let drive = s[..2].to_ascii_lowercase();
        let after = if s.len() >= 3 && (s.as_bytes()[2] == b'/' || s.as_bytes()[2] == b'\\') {
            s[3..].to_string()
        } else {
            s[2..].to_string()
        };
        (Some(drive), after)
    } else {
        (None, s)
    };

    let mut stack: Vec<String> = Vec::new();
    for comp in rest.split('/') {
        match comp {
            "" | "." => continue,
            ".." => {
                if stack.pop().is_none() {
                    // 越过根，逃逸
                    return None;
                }
            }
            other => stack.push(other.to_ascii_lowercase()),
        }
    }
    Some((root, stack))
}

/// 判定某个词法组件序列（不含 root）是否命中敏感命名。
fn components_are_sensitive(components: &[String]) -> bool {
    if let Some(last) = components.last() {
        if SENSITIVE_BASENAMES.contains(&last.as_str()) {
            return true;
        }
        if let Some((_, ext)) = last.rsplit_once('.') {
            if SENSITIVE_EXTENSIONS.contains(&format!(".{ext}").as_str()) {
                return true;
            }
        }
    }
    // 也检查路径中任意段命中 .ssh / .env 目录（如 /home/u/.ssh/known_hosts）
    components
        .iter()
        .any(|c| c == ".ssh" || c == ".env" || c == ".aws" || c == ".gnupg")
}

/// 判定 `candidate` 是否词法上位于 `workspace` 内（整段前缀，含 root 一致）。
///
/// 返回 `false` 表示越界（逃逸）。
fn is_under_workspace(candidate: &(Option<String>, Vec<String>), workspace: &(Option<String>, Vec<String>)) -> bool {
    match (&candidate.0, &workspace.0) {
        (Some(cr), Some(wr)) => {
            if cr != wr {
                return false;
            }
        }
        (None, None) => {}
        _ => return false,
    }
    // candidate 组件必须是 workspace 组件的前缀
    if candidate.1.len() < workspace.1.len() {
        return false;
    }
    candidate.1.iter().zip(&workspace.1).all(|(a, b)| a == b)
}

/// 对单条 `raw` 路径做确定性安全判定。
///
/// - `raw` 为绝对路径则直接使用；相对路径按 `workspace` 内相对解析。
/// - `exists` 闭包注入存在性判定（生产传 `Path::exists`，单测传确定性的桩）。
///
/// 判定顺序（确定性、与 IO 状态解耦）：
/// 1. 敏感命名命中 → [`PathVerdict::Sensitive`]（无论是否存在、是否在 workspace 内）。
/// 2. 词法越出 workspace → [`PathVerdict::EscapesWorkspace`]。
/// 3. `exists(candidate)` 为真 → [`PathVerdict::InsideWorkspace`]。
/// 4. 否则 → [`PathVerdict::NotFound`]。
pub fn evaluate_path(raw: &str, workspace: &Path, exists: &dyn Fn(&Path) -> bool) -> PathVerdict {
    let candidate: PathBuf = if Path::new(raw).is_absolute() {
        PathBuf::from(raw)
    } else {
        workspace.join(raw)
    };

    let cand_parts = match lexical_components(&candidate) {
        Some(p) => p,
        // 词法上含无法归约的 `..` 越界 → 逃逸
        None => return PathVerdict::EscapesWorkspace,
    };

    // 1) 敏感命名（独立于存在性与边界）
    if components_are_sensitive(&cand_parts.1) {
        return PathVerdict::Sensitive;
    }

    let ws_parts = match lexical_components(workspace) {
        Some(p) => p,
        None => return PathVerdict::EscapesWorkspace,
    };

    // 2) 边界
    if !is_under_workspace(&cand_parts, &ws_parts) {
        return PathVerdict::EscapesWorkspace;
    }

    // 3) / 4) 存在性
    if exists(&candidate) {
        PathVerdict::InsideWorkspace
    } else {
        PathVerdict::NotFound
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_exist(_: &Path) -> bool {
        false
    }
    fn always_exist(_: &Path) -> bool {
        true
    }

    const WS: &str = "/home/user/project";

    #[test]
    fn relative_path_inside_workspace() {
        assert_eq!(
            evaluate_path("src/main.rs", Path::new(WS), &always_exist),
            PathVerdict::InsideWorkspace
        );
    }

    #[test]
    fn dotdot_traversal_escapes() {
        // ../../etc/passwd 越出 workspace
        assert_eq!(
            evaluate_path("../../etc/passwd", Path::new(WS), &no_exist),
            PathVerdict::EscapesWorkspace
        );
    }

    #[test]
    fn absolute_path_outside_escapes() {
        assert_eq!(
            evaluate_path("/etc/shadow", Path::new(WS), &no_exist),
            PathVerdict::EscapesWorkspace
        );
    }

    #[test]
    fn absolute_path_inside_workspace_allowed() {
        assert_eq!(
            evaluate_path("/home/user/project/README.md", Path::new(WS), &always_exist),
            PathVerdict::InsideWorkspace
        );
    }

    #[test]
    fn env_file_is_sensitive() {
        assert_eq!(
            evaluate_path(".env", Path::new(WS), &no_exist),
            PathVerdict::Sensitive
        );
        assert_eq!(
            evaluate_path("config/.env", Path::new(WS), &no_exist),
            PathVerdict::Sensitive
        );
    }

    #[test]
    fn ssh_dir_descendant_is_sensitive() {
        assert_eq!(
            evaluate_path(".ssh/id_rsa", Path::new(WS), &no_exist),
            PathVerdict::Sensitive
        );
    }

    #[test]
    fn pem_key_is_sensitive() {
        assert_eq!(
            evaluate_path("certs/key.pem", Path::new(WS), &no_exist),
            PathVerdict::Sensitive
        );
        assert_eq!(
            evaluate_path("id_ed25519", Path::new(WS), &no_exist),
            PathVerdict::Sensitive
        );
    }

    #[test]
    fn nonexistent_file_is_not_found() {
        assert_eq!(
            evaluate_path("src/new_module.rs", Path::new(WS), &no_exist),
            PathVerdict::NotFound
        );
        // 不存在但仍在 workspace 内，不是逃逸
        assert_ne!(
            evaluate_path("src/new_module.rs", Path::new(WS), &no_exist),
            PathVerdict::EscapesWorkspace
        );
    }

    #[test]
    fn injectable_exists_drives_inside_vs_notfound() {
        assert_eq!(
            evaluate_path("a/b.txt", Path::new(WS), &no_exist),
            PathVerdict::NotFound
        );
        assert_eq!(
            evaluate_path("a/b.txt", Path::new(WS), &always_exist),
            PathVerdict::InsideWorkspace
        );
    }

    #[test]
    fn backslash_normalized_like_slash() {
        // Windows 风格分隔符在跨平台匹配下一致
        assert_eq!(
            evaluate_path("src\\main.rs", Path::new(WS), &always_exist),
            PathVerdict::InsideWorkspace
        );
    }

    #[test]
    fn nested_sensitive_extension_in_subdir() {
        assert_eq!(
            evaluate_path("secrets/db.p12", Path::new(WS), &no_exist),
            PathVerdict::Sensitive
        );
    }
}

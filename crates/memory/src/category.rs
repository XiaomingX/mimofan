//! Shared memory category taxonomy.
//!
//! `MemoryCategory` is the single authoritative classification used by both
//! the file-based user memory (tui's `crate::memory`) and the vector memory
//! backend (`mimofan-memory`). Lifting it into this crate removes the
//! architectural smell where the lower-level vector-memory crate depended on
//! a type owned by the upper-level tui crate.
//!
//! String values are fixed lowercase: `user` / `feedback` / `project` /
//! `reference`.

/// 唯一权威记忆分类（对齐 CodeBuddy Typed Memory）。
///
/// 文件记忆与向量记忆共用同一套分类，避免两套并存的命名体系。
/// 字符串值固定为小写：`user` / `feedback` / `project` / `reference`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryCategory {
    /// 用户的角色、目标、偏好与知识背景。
    User,
    /// 用户对协作方式的纠正与指导（已确认的偏好 / 验证过的方法）。
    Feedback,
    /// 项目进行中的工作、目标与决策（无法从代码直接推导的背景）。
    Project,
    /// 外部系统与资源的指引（去哪里查信息）。
    Reference,
}

impl MemoryCategory {
    /// 所有分类，按注入/展示的稳定顺序排列。
    pub const ALL: &'static [MemoryCategory] = &[
        MemoryCategory::User,
        MemoryCategory::Feedback,
        MemoryCategory::Project,
        MemoryCategory::Reference,
    ];

    /// 返回分类的小写字符串形式。
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            MemoryCategory::User => "user",
            MemoryCategory::Feedback => "feedback",
            MemoryCategory::Project => "project",
            MemoryCategory::Reference => "reference",
        }
    }

    /// 从字符串解析分类（大小写不敏感）。
    #[must_use]
    pub fn from_str_name(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "user" => Some(MemoryCategory::User),
            "feedback" => Some(MemoryCategory::Feedback),
            "project" => Some(MemoryCategory::Project),
            "reference" => Some(MemoryCategory::Reference),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parsing_roundtrip() {
        assert_eq!(
            MemoryCategory::from_str_name("user"),
            Some(MemoryCategory::User)
        );
        assert_eq!(
            MemoryCategory::from_str_name("FEEDBACK"),
            Some(MemoryCategory::Feedback)
        );
        assert_eq!(
            MemoryCategory::from_str_name("Project"),
            Some(MemoryCategory::Project)
        );
        assert_eq!(
            MemoryCategory::from_str_name("reference"),
            Some(MemoryCategory::Reference)
        );
        assert_eq!(MemoryCategory::from_str_name("bogus"), None);
        assert_eq!(MemoryCategory::from_str_name(""), None);
        for cat in MemoryCategory::ALL {
            assert_eq!(MemoryCategory::from_str_name(cat.as_str()), Some(*cat));
            assert_eq!(cat.as_str(), cat.as_str().to_ascii_lowercase().as_str());
        }
    }
}

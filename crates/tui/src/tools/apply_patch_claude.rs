//! Claude 风格 `apply_patch` 方言解析器（`apply_patch_claude`）
//!
//! 本模块实现 Claude Code 的 `apply_patch` 方言，与 `apply_patch` 的 unified diff
//! 解析互不相关，独立成文件避免污染原有逻辑。
//!
//! ## 支持的方言格式
//!
//! 整个补丁被 `*** Begin Patch` 与 `*** End Patch` 包裹，内部由若干文件块组成，
//! 每个文件块以 `*** <Verb> File: <path>` 开头。支持三种动词：
//!
//! - `*** Update File: <path>` —— 修改已有文件；块内为上下文/新增/删除行。
//! - `*** Add File: <path>` —— 新建文件；块内行（带 `+` 前缀或裸行）均为文件内容。
//! - `*** Delete File: <path>` —— 删除已有文件。
//!
//! 文件块内的行前缀语义：
//!
//! - ` `（单个空格）上下文行：原文件须包含该内容，应用后原样保留。
//! - `+` 新增行：插入到输出中。
//! - `-` 删除行：从原文件中移除，不进入输出。
//! - `@@` 是 Update 块内的可选上下文锚点（Claude 方言中常见），本解析器在 Update 中
//!   遇到 `@@` 行直接忽略（不强制对位），以保持对宽松输出的兼容性；最通用语义下，
//!   Update 块通过逐行顺序匹配上下文来应用。
//!
//! 对 Add File 块，`+` 前缀和裸行都视为新增内容；对 Delete File 块，块体通常被忽略。
//!
//! ## 块解析状态机
//!
//! [`parse_patch`] 是一个显式状态机，状态如下：
//!
//! 1. `ExpectBegin`：等待 `*** Begin Patch`。任何非该标记行都按错误返回。
//! 2. `ExpectFileOrEnd`：在 Patch 内，等待下一个 `*** Update/Add/Delete File:` 或
//!    `*** End Patch`。遇到文件标记后进入对应块处理。
//! 3. `InUpdate` / `InAdd`：收集块内行，直到遇到下一个 `***` 标记（新块或 End）。
//! 4. `InDelete`：忽略块内行，直到遇到下一个 `***` 标记。
//! 5. 文件块结束时调用 [`apply_file_block`] 执行实际的读/写/删。
//! 6. 收尾：若仍处于某块或仍未看到 `*** End Patch`，则为错误。

use std::path::PathBuf;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};
use thiserror::Error;

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    required_str,
};

/// 单个文件块的动词类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileOp {
    Update,
    Add,
    Delete,
}

/// 解析阶段的内部状态机状态。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParseState {
    ExpectBegin,
    ExpectFileOrEnd,
    InUpdate,
    InAdd,
    InDelete,
}

/// 一个文件块的累积数据。
#[derive(Debug, Clone)]
struct FileBlock {
    op: FileOp,
    path: String,
    /// Update 块的上下文/增/删行；Add 块的内容行（`+`/裸行）。
    lines: Vec<String>,
}

#[derive(Debug, Error)]
enum PatchParseError {
    #[error("missing `*** Begin Patch` at start of input")]
    MissingBegin,
    #[error("expected `*** Begin Patch` but found non-marker content: {0}")]
    UnexpectedContent(String),
    #[error("missing `*** End Patch` to close the patch")]
    MissingEnd,
    #[error("unexpected `*** {0}` marker outside of a patch")]
    UnexpectedMarker(String),
    #[error("malformed file marker: {0}")]
    MalformedFileMarker(String),
    #[error("unknown file verb `*** {0}`")]
    UnknownVerb(String),
    #[error("Update File block for `{path}` is empty (no context/change lines)")]
    EmptyUpdate { path: String },
}

impl From<PatchParseError> for ToolError {
    fn from(e: PatchParseError) -> Self {
        ToolError::invalid_input(e.to_string())
    }
}

/// 应用结果摘要。
#[derive(Debug, Clone, Serialize)]
pub struct ClaudePatchResult {
    pub success: bool,
    pub files_total: usize,
    pub files_applied: usize,
    pub created: Vec<String>,
    pub deleted: Vec<String>,
    pub message: String,
}

/// 解析 Claude 风格方言补丁，返回待应用的文件块列表。
///
/// 见模块文档中的「块解析状态机」说明。这是纯函数，不涉及任何 I/O，便于单测。
fn parse_patch(input: &str) -> Result<Vec<FileBlock>, PatchParseError> {
    let marker = "*** ";
    let mut state = ParseState::ExpectBegin;
    let mut blocks: Vec<FileBlock> = Vec::new();
    let mut current: Option<FileBlock> = None;

    // 处理一行的辅助闭包不可直接借 state，故用循环内联实现。
    for raw_line in input.lines() {
        let line = raw_line;
        match state {
            ParseState::ExpectBegin => {
                if line == "*** Begin Patch" {
                    state = ParseState::ExpectFileOrEnd;
                } else if line.starts_with(marker) {
                    return Err(PatchParseError::UnexpectedMarker(
                        line.trim_start_matches(marker).to_string(),
                    ));
                } else if line.trim().is_empty() {
                    // 允许开头有空行。
                    continue;
                } else {
                    return Err(PatchParseError::UnexpectedContent(line.to_string()));
                }
            }
            ParseState::ExpectFileOrEnd => {
                if line == "*** End Patch" {
                    state = ParseState::ExpectBegin; // 标记结束；后续若还有内容会在收尾报错。
                    break;
                } else if let Some(rest) = line.strip_prefix("*** Update File: ") {
                    current = Some(FileBlock {
                        op: FileOp::Update,
                        path: rest.trim().to_string(),
                        lines: Vec::new(),
                    });
                    state = ParseState::InUpdate;
                } else if let Some(rest) = line.strip_prefix("*** Add File: ") {
                    current = Some(FileBlock {
                        op: FileOp::Add,
                        path: rest.trim().to_string(),
                        lines: Vec::new(),
                    });
                    state = ParseState::InAdd;
                } else if let Some(rest) = line.strip_prefix("*** Delete File: ") {
                    current = Some(FileBlock {
                        op: FileOp::Delete,
                        path: rest.trim().to_string(),
                        lines: Vec::new(),
                    });
                    state = ParseState::InDelete;
                } else if line.starts_with(marker) {
                    return Err(PatchParseError::UnknownVerb(
                        line.trim_start_matches(marker).to_string(),
                    ));
                } else {
                    return Err(PatchParseError::UnexpectedContent(line.to_string()));
                }
            }
            ParseState::InUpdate | ParseState::InAdd | ParseState::InDelete => {
                if line.starts_with(marker) {
                    // 遇到新 `***` 标记：先收尾当前块。
                    let finished = current
                        .take()
                        .ok_or(PatchParseError::MissingEnd)?;
                    blocks.push(finished);
                    // 回退到 ExpectFileOrEnd 重新处理本行。
                    state = ParseState::ExpectFileOrEnd;
                    // 重新走一遍该行的分支。
                    if line == "*** End Patch" {
                        state = ParseState::ExpectBegin;
                        break;
                    } else if let Some(rest) = line.strip_prefix("*** Update File: ") {
                        current = Some(FileBlock {
                            op: FileOp::Update,
                            path: rest.trim().to_string(),
                            lines: Vec::new(),
                        });
                        state = ParseState::InUpdate;
                    } else if let Some(rest) = line.strip_prefix("*** Add File: ") {
                        current = Some(FileBlock {
                            op: FileOp::Add,
                            path: rest.trim().to_string(),
                            lines: Vec::new(),
                        });
                        state = ParseState::InAdd;
                    } else if let Some(rest) = line.strip_prefix("*** Delete File: ") {
                        current = Some(FileBlock {
                            op: FileOp::Delete,
                            path: rest.trim().to_string(),
                            lines: Vec::new(),
                        });
                        state = ParseState::InDelete;
                    } else {
                        return Err(PatchParseError::UnknownVerb(
                            line.trim_start_matches(marker).to_string(),
                        ));
                    }
                } else {
                    // 块内普通行。
                    let block = current
                        .as_mut()
                        .ok_or(PatchParseError::MissingEnd)?;
                    if block.op == FileOp::Delete {
                        // Delete 块体忽略。
                    } else {
                        block.lines.push(line.to_string());
                    }
                }
            }
        }
    }

    // 收尾：若还有未结束的块，缺 `*** End Patch`。
    if let Some(_) = current {
        return Err(PatchParseError::MissingEnd);
    }
    // 仍处于 ExpectBegin 表示从未看到 End（never entered a patch at all,
    // or began but never closed）。区分：blocks 为空且没见过 Begin 的报错。
    if state == ParseState::ExpectBegin && blocks.is_empty() {
        // 从未遇到 `*** Begin Patch`（只有空行之类）。
        return Err(PatchParseError::MissingBegin);
    }

    Ok(blocks)
}

/// 将一个文件块应用到磁盘。
///
/// Update：读取原文件，按行顺序匹配上下文并应用 +/- 变更，写回。
/// Add：把内容行（`+`/裸行）写出（文件须不存在，存在则覆盖以最通用语义）。
/// Delete：删除文件（不存在则视为已满足）。
async fn apply_file_block(
    block: &FileBlock,
    context: &ToolContext,
) -> Result<(bool, String), ToolError> {
    let abs = context.resolve_path(&block.path)?;

    match block.op {
        FileOp::Update => {
            if block.lines.is_empty() {
                return Err(PatchParseError::EmptyUpdate {
                    path: block.path.clone(),
                }
                .into());
            }
            let original = if abs.exists() {
                tokio::fs::read_to_string(&abs).await.map_err(|e| {
                    ToolError::execution_failed(format!(
                        "Failed to read {}: {e}",
                        abs.display()
                    ))
                })?
            } else {
                return Err(ToolError::execution_failed(format!(
                    "Update File target does not exist: {}",
                    abs.display()
                )));
            };
            let new_content = apply_update_lines(&original, &block.lines, &block.path)?;
            write_file(&abs, &new_content).await?;
            Ok((true, block.path.clone()))
        }
        FileOp::Add => {
            let mut content = String::new();
            for l in &block.lines {
                let body = match l.strip_prefix('+') {
                    Some(rest) => rest,
                    None => l.as_str(),
                };
                content.push_str(body);
                content.push('\n');
            }
            write_file(&abs, &content).await?;
            Ok((true, block.path.clone()))
        }
        FileOp::Delete => {
            if abs.exists() {
                tokio::fs::remove_file(&abs).await.map_err(|e| {
                    ToolError::execution_failed(format!(
                        "Failed to delete {}: {e}",
                        abs.display()
                    ))
                })?;
            }
            Ok((true, block.path.clone()))
        }
    }
}

/// 对单个 Update 块应用行变更。
///
/// 状态机逐行扫描 original：
/// - 上下文行（` ` 前缀）：必须在 original 当前位置匹配，原样保留。
/// - 删除行（`-` 前缀）：必须从 original 当前位置匹配，跳过不输出。
/// - 新增行（`+` 前缀）：插入输出，不消费 original。
/// 不匹配即报错。
fn apply_update_lines(
    original: &str,
    change_lines: &[String],
    path: &str,
) -> Result<String, ToolError> {
    let orig_lines: Vec<&str> = original.split('\n').collect();
    // 若原文件以换行结尾，split 会产生一个尾随空串；移除以便对齐。
    let orig_lines = if let Some(last) = orig_lines.last() {
        if last.is_empty() {
            &orig_lines[..orig_lines.len().saturating_sub(1)]
        } else {
            &orig_lines[..]
        }
    } else {
        &orig_lines[..]
    };

    let mut out: Vec<String> = Vec::new();
    let mut oi = 0usize; // original 游标

    for chg in change_lines {
        if let Some(ctx) = chg.strip_prefix(' ') {
            if oi >= orig_lines.len() || orig_lines[oi] != ctx {
                return Err(ToolError::execution_failed(format!(
                    "Update File `{path}`: context mismatch at original line {}: expected `{ctx}`",
                    oi + 1
                )));
            }
            out.push(ctx.to_string());
            oi += 1;
        } else if let Some(removed) = chg.strip_prefix('-') {
            if oi >= orig_lines.len() || orig_lines[oi] != removed {
                return Err(ToolError::execution_failed(format!(
                    "Update File `{path}`: delete mismatch at original line {}: expected `{removed}`",
                    oi + 1
                )));
            }
            oi += 1;
        } else if let Some(added) = chg.strip_prefix('+') {
            out.push(added.to_string());
        } else if chg.starts_with("@@") {
            // 忽略上下文锚点行。
            continue;
        } else {
            // 裸行（无前缀）按最通用语义视为上下文。
            if oi >= orig_lines.len() || orig_lines[oi] != *chg {
                return Err(ToolError::execution_failed(format!(
                    "Update File `{path}`: context mismatch at original line {}: expected `{chg}`",
                    oi + 1
                )));
            }
            out.push(chg.clone());
            oi += 1;
        }
    }

    Ok(out.join("\n"))
}

/// 写出文件内容（含父目录创建）。
async fn write_file(path: &PathBuf, content: &str) -> Result<(), ToolError> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent).await.map_err(|e| {
            ToolError::execution_failed(format!(
                "Failed to create directory {}: {e}",
                parent.display()
            ))
        })?;
    }
    tokio::fs::write(path, content).await.map_err(|e| {
        ToolError::execution_failed(format!("Failed to write {}: {e}", path.display()))
    })
}

/// `apply_patch_claude` 工具实现。
pub struct ApplyPatchClaudeTool;

#[async_trait]
impl ToolSpec for ApplyPatchClaudeTool {
    fn name(&self) -> &'static str {
        "apply_patch_claude"
    }

    fn description(&self) -> &'static str {
        "Apply a Claude-style `apply_patch` dialect patch. Prefer this dialect for multi-file changes: wrap the whole patch in `*** Begin Patch` ... `*** End Patch`, and use one file block per path — `*** Update File: <path>` (with ` ` context, `+` add, `-` delete line prefixes), `*** Add File: <path>` (new file), or `*** Delete File: <path>`. This Claude dialect coexists with the unified-diff `apply_patch` tool; you may use either, but the Claude dialect is recommended when touching several files at once because it keeps every file in a single tool call."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Claude-style apply_patch dialect content"
                }
            },
            "required": ["patch"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![
            ToolCapability::WritesFiles,
            ToolCapability::Sandboxable,
            ToolCapability::RequiresApproval,
        ]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Suggest
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let patch_text = required_str(&input, "patch")?;
        let blocks = parse_patch(&patch_text).map_err(ToolError::from)?;

        let mut applied = 0usize;
        let mut created = Vec::new();
        let mut deleted = Vec::new();
        for block in &blocks {
            let (_ok, path) = apply_file_block(block, context).await?;
            applied += 1;
            match block.op {
                FileOp::Add => created.push(path),
                FileOp::Delete => deleted.push(path),
                FileOp::Update => {}
            }
        }

        let result = ClaudePatchResult {
            success: true,
            files_total: blocks.len(),
            files_applied: applied,
            created: created.clone(),
            deleted: deleted.clone(),
            message: format!(
                "Applied {} file(s): {} created, {} deleted",
                applied,
                created.len(),
                deleted.len()
            ),
        };

        ToolResult::json(&result).map_err(|e| ToolError::execution_failed(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn ctx_with(workspace: &std::path::Path) -> ToolContext {
        ToolContext::new(workspace.to_path_buf())
    }

    #[test]
    fn parse_update_single_file_change() {
        let input = "\
*** Begin Patch
*** Update File: a.txt
 context line
-old line
+new line
 context line
*** End Patch";
        let blocks = parse_patch(input).unwrap();
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0].op, FileOp::Update);
        assert_eq!(blocks[0].path, "a.txt");
        assert_eq!(blocks[0].lines.len(), 4);
    }

    #[test]
    fn missing_end_patch_is_error() {
        let input = "\
*** Begin Patch
*** Update File: a.txt
 context
-old
+new";
        let err = parse_patch(input);
        assert!(err.is_err());
        assert!(matches!(err.unwrap_err(), PatchParseError::MissingEnd));
    }

    #[test]
    fn parse_add_and_delete_blocks() {
        let input = "\
*** Begin Patch
*** Add File: new.txt
+hello
+world
*** Delete File: gone.txt
*** End Patch";
        let blocks = parse_patch(input).unwrap();
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0].op, FileOp::Add);
        assert_eq!(blocks[1].op, FileOp::Delete);
    }

    #[tokio::test]
    async fn execute_update_file_add_delete() {
        let dir = tempfile::TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf());
        let f = ws.join("a.txt");
        {
            let mut fh = std::fs::File::create(&f).unwrap();
            writeln!(fh, "context line").unwrap();
            writeln!(fh, "old line").unwrap();
            writeln!(fh, "context line").unwrap();
        }
        let ctx = ctx_with(&ws);
        let patch = "\
*** Begin Patch
*** Update File: a.txt
 context line
-old line
+new line
 context line
*** End Patch";
        let tool = ApplyPatchClaudeTool;
        let res = tool
            .execute(json!({ "patch": patch }), &ctx)
            .await
            .unwrap();
        let out = std::fs::read_to_string(&f).unwrap();
        assert!(out.contains("new line"));
        assert!(!out.contains("old line"));
        assert!(res.content.contains("Applied 1"));
        drop(dir);
    }

    #[tokio::test]
    async fn execute_add_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf());
        let ctx = ctx_with(&ws);
        let patch = "\
*** Begin Patch
*** Add File: new.txt
+hello
+world
*** End Patch";
        let tool = ApplyPatchClaudeTool;
        tool.execute(json!({ "patch": patch }), &ctx).await.unwrap();
        let out = std::fs::read_to_string(ws.join("new.txt")).unwrap();
        assert_eq!(out, "hello\nworld\n");
        drop(dir);
    }

    #[tokio::test]
    async fn execute_delete_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf());
        let f = ws.join("gone.txt");
        std::fs::write(&f, "x\n").unwrap();
        let ctx = ctx_with(&ws);
        let patch = "\
*** Begin Patch
*** Delete File: gone.txt
*** End Patch";
        let tool = ApplyPatchClaudeTool;
        tool.execute(json!({ "patch": patch }), &ctx).await.unwrap();
        assert!(!f.exists());
        drop(dir);
    }

    #[tokio::test]
    async fn execute_missing_end_returns_error() {
        let dir = tempfile::TempDir::new().unwrap();
        let ws = dir.path().canonicalize().unwrap_or_else(|_| dir.path().to_path_buf());
        let ctx = ctx_with(&ws);
        let patch = "\
*** Begin Patch
*** Add File: new.txt
+hello";
        let tool = ApplyPatchClaudeTool;
        let res = tool.execute(json!({ "patch": patch }), &ctx).await;
        assert!(res.is_err());
        drop(dir);
    }
}

//! Jupyter 笔记本单元格编辑工具：`notebook_edit`。
//!
//! 直接以 `serde_json::Value` 操作 `.ipynb` 的 JSON 结构，不引入额外依赖。
//! 设计目标是**保真**：除了被显式修改的 `source`，单元格上的 `metadata`、
//! `id`、`outputs`、`execution_count` 等字段都原样保留；顶层 `metadata`、
//! `nbformat`、`nbformat_minor` 同样不动。

use async_trait::async_trait;
use serde_json::{Map, Value, json};

use super::spec::{
    ApprovalRequirement, ToolCapability, ToolContext, ToolError, ToolResult, ToolSpec,
    optional_str, required_str,
};

/// 编辑 `.ipynb` 单元格的工具。
pub struct NotebookEditTool;

/// 允许的单元格类型。
const CELL_TYPES: [&str; 3] = ["code", "markdown", "raw"];

#[async_trait]
impl ToolSpec for NotebookEditTool {
    fn name(&self) -> &'static str {
        "notebook_edit"
    }

    fn description(&self) -> &'static str {
        "Read or edit cells of a Jupyter .ipynb notebook by index or cell id. Commands: get_cell, update_cell, insert_cell, delete_cell. Cell outputs, metadata, ids and execution_count are preserved; use this instead of write_file so the notebook stays valid JSON."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Path to the .ipynb notebook file"
                },
                "command": {
                    "type": "string",
                    "enum": ["insert_cell", "delete_cell", "update_cell", "get_cell"],
                    "description": "Operation to perform on the notebook"
                },
                "index": {
                    "type": "number",
                    "description": "0-based cell index. For insert_cell prefer new_index."
                },
                "cell_id": {
                    "type": "string",
                    "description": "Locate the cell by its `id` field instead of `index`"
                },
                "source": {
                    "type": "string",
                    "description": "New cell source for update_cell / insert_cell. May contain newlines."
                },
                "cell_type": {
                    "type": "string",
                    "enum": ["code", "markdown", "raw"],
                    "description": "Cell type for insert_cell (default: code). For update_cell it changes the existing cell's type."
                },
                "new_index": {
                    "type": "number",
                    "description": "Insert position for insert_cell (0-based, default: append at end)"
                }
            },
            "required": ["path", "command"]
        })
    }

    fn capabilities(&self) -> Vec<ToolCapability> {
        vec![ToolCapability::WritesFiles]
    }

    fn approval_requirement(&self) -> ApprovalRequirement {
        ApprovalRequirement::Auto
    }

    fn supports_parallel(&self) -> bool {
        // 同一个笔记本的并发改写会互相覆盖，串行执行。
        false
    }

    async fn execute(&self, input: Value, context: &ToolContext) -> Result<ToolResult, ToolError> {
        let path_str = required_str(&input, "path")?;
        let command = required_str(&input, "command")?;
        let file_path = context.resolve_path(path_str)?;
        let display = file_path.display().to_string();

        // Mutating commands rewrite an existing notebook, so they need a fresh
        // prior read. `get_cell` only reads, and records the snapshot itself
        // below, so it must stay ungated or the notebook could never be read.
        if matches!(command, "update_cell" | "insert_cell" | "delete_cell") && file_path.exists() {
            context.require_fresh_file_read_for("notebook_edit", &file_path, path_str)?;
        }

        let raw = tokio::fs::read_to_string(&file_path).await.map_err(|e| {
            ToolError::invalid_input(format!("Failed to read notebook {display}: {e}"))
        })?;
        let mut notebook: Value = serde_json::from_str(&raw).map_err(|e| {
            ToolError::invalid_input(format!("Notebook {display} is not valid JSON: {e}"))
        })?;

        let cell_count = cells_ref(&notebook, &display)?.len();

        match command {
            "get_cell" => {
                let idx = locate_cell(&notebook, &input, &display)?;
                // Record the observation so a subsequent edit to this notebook
                // satisfies the read-before-write check above.
                context.note_file_read(&file_path);
                let cells = cells_ref(&notebook, &display)?;
                let cell = &cells[idx];
                let summary = json!({
                    "command": "get_cell",
                    "path": display,
                    "index": idx,
                    "cell_id": cell.get("id").and_then(Value::as_str),
                    "cell_type": cell.get("cell_type").and_then(Value::as_str),
                    "source": read_source(cell),
                    "cell_count": cell_count,
                });
                ToolResult::json(&summary).map_err(|e| ToolError::execution_failed(e.to_string()))
            }
            "update_cell" => {
                let source = required_str(&input, "source")?.to_string();
                let new_type = validated_cell_type(&input)?;
                let idx = locate_cell(&notebook, &input, &display)?;
                let cells = cells_mut(&mut notebook, &display)?;
                let cell = cells[idx]
                    .as_object_mut()
                    .ok_or_else(|| ToolError::invalid_input(format!("cell {idx} is not an object")))?;
                cell.insert("source".to_string(), write_source(&source));
                if let Some(cell_type) = new_type {
                    cell.insert("cell_type".to_string(), json!(cell_type));
                    // 非 code 单元格不应保留执行状态字段。
                    if cell_type != "code" {
                        cell.remove("execution_count");
                        cell.remove("outputs");
                    }
                }
                let cell_id = cell.get("id").and_then(Value::as_str).map(str::to_string);
                let bytes = write_notebook(&file_path, &notebook).await?;
                context.note_file_read(&file_path);
                let summary = json!({
                    "command": "update_cell",
                    "path": display,
                    "index": idx,
                    "cell_id": cell_id,
                    "bytes_written": bytes,
                    "cell_count": cell_count,
                });
                ToolResult::json(&summary).map_err(|e| ToolError::execution_failed(e.to_string()))
            }
            "insert_cell" => {
                let source = optional_str(&input, "source").unwrap_or("").to_string();
                let cell_type = validated_cell_type(&input)?.unwrap_or("code");
                // 插入位置：优先 new_index，其次 index，默认追加到末尾。
                let target = match optional_index(&input, "new_index")?
                    .or(optional_index(&input, "index")?)
                {
                    Some(i) => i,
                    None => cell_count,
                };
                if target > cell_count {
                    return Err(ToolError::invalid_input(format!(
                        "insert position {target} out of range (notebook has {cell_count} cells)"
                    )));
                }
                let new_cell = build_cell(cell_type, &source, optional_str(&input, "cell_id"));
                let cells = cells_mut(&mut notebook, &display)?;
                cells.insert(target, new_cell);
                let bytes = write_notebook(&file_path, &notebook).await?;
                context.note_file_read(&file_path);
                let summary = json!({
                    "command": "insert_cell",
                    "path": display,
                    "index": target,
                    "cell_type": cell_type,
                    "bytes_written": bytes,
                    "cell_count": cell_count + 1,
                });
                ToolResult::json(&summary).map_err(|e| ToolError::execution_failed(e.to_string()))
            }
            "delete_cell" => {
                let idx = locate_cell(&notebook, &input, &display)?;
                let cells = cells_mut(&mut notebook, &display)?;
                let removed = cells.remove(idx);
                let bytes = write_notebook(&file_path, &notebook).await?;
                context.note_file_read(&file_path);
                let summary = json!({
                    "command": "delete_cell",
                    "path": display,
                    "index": idx,
                    "cell_id": removed.get("id").and_then(Value::as_str),
                    "cell_type": removed.get("cell_type").and_then(Value::as_str),
                    "bytes_written": bytes,
                    "cell_count": cell_count - 1,
                });
                ToolResult::json(&summary).map_err(|e| ToolError::execution_failed(e.to_string()))
            }
            other => Err(ToolError::invalid_input(format!(
                "unknown command '{other}': expected one of insert_cell, delete_cell, update_cell, get_cell"
            ))),
        }
    }
}

// === 内部辅助 ===

/// 取顶层 `cells` 数组的只读引用。
fn cells_ref<'a>(notebook: &'a Value, display: &str) -> Result<&'a Vec<Value>, ToolError> {
    notebook
        .get("cells")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            ToolError::invalid_input(format!("{display} has no top-level 'cells' array"))
        })
}

/// 取顶层 `cells` 数组的可变引用。
fn cells_mut<'a>(notebook: &'a mut Value, display: &str) -> Result<&'a mut Vec<Value>, ToolError> {
    notebook
        .get_mut("cells")
        .and_then(Value::as_array_mut)
        .ok_or_else(|| {
            ToolError::invalid_input(format!("{display} has no top-level 'cells' array"))
        })
}

/// 读取可选的非负整数索引字段。
fn optional_index(input: &Value, field: &str) -> Result<Option<usize>, ToolError> {
    match input.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| {
                ToolError::invalid_input(format!("'{field}' must be a non-negative integer"))
            })?;
            Ok(Some(usize::try_from(n).map_err(|_| {
                ToolError::invalid_input(format!("'{field}' is too large"))
            })?))
        }
    }
}

/// 校验 `cell_type` 取值；未提供时返回 `None`。
fn validated_cell_type(input: &Value) -> Result<Option<&'static str>, ToolError> {
    match optional_str(input, "cell_type") {
        None => Ok(None),
        Some(raw) => CELL_TYPES
            .iter()
            .find(|t| **t == raw)
            .copied()
            .map(Some)
            .ok_or_else(|| {
                ToolError::invalid_input(format!(
                    "unknown cell_type '{raw}': expected code, markdown, or raw"
                ))
            }),
    }
}

/// 按 `cell_id` 或 `index` 定位单元格，返回 0-based 下标。
fn locate_cell(notebook: &Value, input: &Value, display: &str) -> Result<usize, ToolError> {
    let cells = cells_ref(notebook, display)?;
    if let Some(id) = optional_str(input, "cell_id") {
        return cells
            .iter()
            .position(|c| c.get("id").and_then(Value::as_str) == Some(id))
            .ok_or_else(|| ToolError::invalid_input(format!("no cell with id '{id}' in {display}")));
    }
    let idx = optional_index(input, "index")?.ok_or_else(|| {
        ToolError::invalid_input("either 'index' or 'cell_id' is required to locate a cell")
    })?;
    if idx >= cells.len() {
        return Err(ToolError::invalid_input(format!(
            "cell index {idx} out of range (notebook has {} cells)",
            cells.len()
        )));
    }
    Ok(idx)
}

/// 把 ipynb 的 `source` 字段（字符串数组或单字符串）读成一整段文本。
fn read_source(cell: &Value) -> String {
    match cell.get("source") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .concat(),
        _ => String::new(),
    }
}

/// 把整段文本写成 Jupyter 惯例的 `source` 数组：每行一项，除最后一行外都带
/// 尾随 `\n`。空字符串写成空数组。
fn write_source(source: &str) -> Value {
    if source.is_empty() {
        return json!([]);
    }
    let mut lines: Vec<String> = Vec::new();
    let mut rest = source;
    while let Some(pos) = rest.find('\n') {
        lines.push(rest[..=pos].to_string());
        rest = &rest[pos + 1..];
    }
    if !rest.is_empty() {
        lines.push(rest.to_string());
    }
    Value::Array(lines.into_iter().map(Value::String).collect())
}

/// 构造一个新单元格。code 单元格额外带上 `execution_count` 与 `outputs`。
fn build_cell(cell_type: &str, source: &str, cell_id: Option<&str>) -> Value {
    let mut cell = Map::new();
    cell.insert("cell_type".to_string(), json!(cell_type));
    if let Some(id) = cell_id {
        cell.insert("id".to_string(), json!(id));
    }
    cell.insert("metadata".to_string(), json!({}));
    if cell_type == "code" {
        cell.insert("execution_count".to_string(), Value::Null);
        cell.insert("outputs".to_string(), json!([]));
    }
    cell.insert("source".to_string(), write_source(source));
    Value::Object(cell)
}

/// 以 pretty JSON 写回笔记本，返回写入字节数。
async fn write_notebook(path: &std::path::Path, notebook: &Value) -> Result<usize, ToolError> {
    let mut text = serde_json::to_string_pretty(notebook)
        .map_err(|e| ToolError::execution_failed(format!("Failed to serialize notebook: {e}")))?;
    // Jupyter 写出的 .ipynb 以换行结尾，保持一致以免产生噪音 diff。
    text.push('\n');
    tokio::fs::write(path, &text).await.map_err(|e| {
        ToolError::execution_failed(format!("Failed to write {}: {}", path.display(), e))
    })?;
    Ok(text.len())
}

// === 单元测试 ===

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// 最小但完整的 ipynb：cell 0 是带 outputs 的 code（source 为数组形式），
    /// cell 1 是 markdown（source 为单字符串形式）。
    // 用 `r##` 定界：JSON 里含有 `"# Title`，`r#` 会被其中的 `"#` 提前截断。
    const NOTEBOOK: &str = r##"{
  "cells": [
    {
      "cell_type": "code",
      "id": "cell-a",
      "metadata": {"tags": ["keep-me"]},
      "execution_count": 3,
      "outputs": [
        {"output_type": "stream", "name": "stdout", "text": ["hello\n"]}
      ],
      "source": ["print('hello')\n", "print('world')"]
    },
    {
      "cell_type": "markdown",
      "id": "cell-b",
      "metadata": {},
      "source": "# Title\nbody"
    }
  ],
  "metadata": {"kernelspec": {"name": "python3", "display_name": "Python 3"}},
  "nbformat": 4,
  "nbformat_minor": 5
}"##;

    fn setup() -> (TempDir, ToolContext) {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nb.ipynb");
        std::fs::write(&path, NOTEBOOK).unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());
        // Simulate a prior `get_cell` read so mutation commands satisfy the
        // read-before-write guard introduced for #695. Real usage always
        // reads before editing a notebook. Resolve the path the same way the
        // guard does so the snapshot key matches regardless of /var vs
        // /private/var symlink normalisation on macOS.
        let resolved = ctx.resolve_path("nb.ipynb").unwrap();
        ctx.note_file_read(&resolved);
        (dir, ctx)
    }

    async fn run(ctx: &ToolContext, input: Value) -> Result<ToolResult, ToolError> {
        NotebookEditTool.execute(input, ctx).await
    }

    fn load(dir: &TempDir) -> Value {
        let raw = std::fs::read_to_string(dir.path().join("nb.ipynb")).unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    #[tokio::test]
    async fn update_cell_replaces_source_and_preserves_outputs() {
        let (dir, ctx) = setup();
        let result = run(
            &ctx,
            json!({
                "path": "nb.ipynb",
                "command": "update_cell",
                "index": 0,
                "source": "x = 1\nprint(x)"
            }),
        )
        .await
        .expect("update_cell should succeed");
        assert!(result.success);

        let nb = load(&dir);
        let cell = &nb["cells"][0];
        assert_eq!(read_source(cell), "x = 1\nprint(x)");
        // 无损：outputs / metadata / id / execution_count 均保留。
        assert_eq!(cell["outputs"][0]["output_type"], "stream");
        assert_eq!(cell["metadata"]["tags"][0], "keep-me");
        assert_eq!(cell["id"], "cell-a");
        assert_eq!(cell["execution_count"], 3);
        // 顶层结构保真。
        assert_eq!(nb["nbformat"], 4);
        assert_eq!(nb["metadata"]["kernelspec"]["name"], "python3");
        assert_eq!(nb["cells"].as_array().unwrap().len(), 2);
    }

    #[tokio::test]
    async fn update_cell_accepts_cell_id() {
        let (dir, ctx) = setup();
        run(
            &ctx,
            json!({
                "path": "nb.ipynb",
                "command": "update_cell",
                "cell_id": "cell-b",
                "source": "# New Title"
            }),
        )
        .await
        .expect("update by cell_id should succeed");

        let nb = load(&dir);
        assert_eq!(read_source(&nb["cells"][1]), "# New Title");
        // 未指定的 cell 不受影响。
        assert_eq!(read_source(&nb["cells"][0]), "print('hello')\nprint('world')");
    }

    #[tokio::test]
    async fn insert_cell_places_markdown_at_requested_index() {
        let (dir, ctx) = setup();
        run(
            &ctx,
            json!({
                "path": "nb.ipynb",
                "command": "insert_cell",
                "new_index": 1,
                "cell_type": "markdown",
                "source": "## Section"
            }),
        )
        .await
        .expect("insert_cell should succeed");

        let nb = load(&dir);
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[1]["cell_type"], "markdown");
        assert_eq!(read_source(&cells[1]), "## Section");
        // markdown 单元格不带执行字段。
        assert!(cells[1].get("outputs").is_none());
        assert!(cells[1].get("execution_count").is_none());
        // 原有单元格顺序后移。
        assert_eq!(cells[0]["id"], "cell-a");
        assert_eq!(cells[2]["id"], "cell-b");
    }

    #[tokio::test]
    async fn insert_cell_appends_when_index_omitted() {
        let (dir, ctx) = setup();
        run(
            &ctx,
            json!({
                "path": "nb.ipynb",
                "command": "insert_cell",
                "source": "y = 2"
            }),
        )
        .await
        .expect("insert_cell should append");

        let nb = load(&dir);
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 3);
        assert_eq!(cells[2]["cell_type"], "code");
        assert_eq!(cells[2]["outputs"], json!([]));
        assert_eq!(read_source(&cells[2]), "y = 2");
    }

    #[tokio::test]
    async fn delete_cell_removes_target() {
        let (dir, ctx) = setup();
        run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "delete_cell", "index": 0 }),
        )
        .await
        .expect("delete_cell should succeed");

        let nb = load(&dir);
        let cells = nb["cells"].as_array().unwrap();
        assert_eq!(cells.len(), 1);
        assert_eq!(cells[0]["id"], "cell-b");
    }

    #[tokio::test]
    async fn get_cell_returns_joined_source_for_both_shapes() {
        let (_dir, ctx) = setup();

        // 数组形式的 source 按行拼接。
        let code = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "get_cell", "index": 0 }),
        )
        .await
        .expect("get_cell should succeed");
        let parsed: Value = serde_json::from_str(&code.content).unwrap();
        assert_eq!(parsed["source"], "print('hello')\nprint('world')");
        assert_eq!(parsed["cell_type"], "code");
        assert_eq!(parsed["cell_id"], "cell-a");

        // 单字符串形式的 source 原样返回。
        let md = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "get_cell", "cell_id": "cell-b" }),
        )
        .await
        .expect("get_cell by id should succeed");
        let parsed: Value = serde_json::from_str(&md.content).unwrap();
        assert_eq!(parsed["source"], "# Title\nbody");
        assert_eq!(parsed["index"], 1);
    }

    #[tokio::test]
    async fn source_round_trips_through_array_form() {
        let (dir, ctx) = setup();
        // 写入多行后必须落成 Jupyter 惯例的数组形式，且读回一致。
        run(
            &ctx,
            json!({
                "path": "nb.ipynb",
                "command": "update_cell",
                "index": 1,
                "source": "line1\nline2\nline3"
            }),
        )
        .await
        .unwrap();

        let nb = load(&dir);
        let raw = &nb["cells"][1]["source"];
        assert_eq!(raw, &json!(["line1\n", "line2\n", "line3"]));
        assert_eq!(read_source(&nb["cells"][1]), "line1\nline2\nline3");

        // 带尾随换行的文本也要能忠实还原。
        run(
            &ctx,
            json!({
                "path": "nb.ipynb",
                "command": "update_cell",
                "index": 1,
                "source": "trailing\n"
            }),
        )
        .await
        .unwrap();
        let nb = load(&dir);
        assert_eq!(&nb["cells"][1]["source"], &json!(["trailing\n"]));
        assert_eq!(read_source(&nb["cells"][1]), "trailing\n");
    }

    #[tokio::test]
    async fn unknown_command_is_rejected() {
        let (_dir, ctx) = setup();
        let err = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "explode", "index": 0 }),
        )
        .await
        .expect_err("unknown command must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn out_of_range_index_is_rejected() {
        let (_dir, ctx) = setup();
        let err = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "delete_cell", "index": 99 }),
        )
        .await
        .expect_err("out of range index must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");

        // 越界的插入位置同样被拒绝。
        let err = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "insert_cell", "new_index": 99, "source": "x" }),
        )
        .await
        .expect_err("out of range insert must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_locator_and_bad_cell_type_are_rejected() {
        let (_dir, ctx) = setup();
        // 既没有 index 也没有 cell_id。
        let err = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "update_cell", "source": "x" }),
        )
        .await
        .expect_err("missing locator must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");

        // 非法 cell_type。
        let err = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "insert_cell", "cell_type": "sql", "source": "x" }),
        )
        .await
        .expect_err("bad cell_type must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");

        // 未知 cell_id。
        let err = run(
            &ctx,
            json!({ "path": "nb.ipynb", "command": "get_cell", "cell_id": "nope" }),
        )
        .await
        .expect_err("unknown cell_id must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");
    }

    #[tokio::test]
    async fn missing_file_and_invalid_json_are_rejected() {
        let dir = TempDir::new().unwrap();
        let ctx = ToolContext::new(dir.path().to_path_buf());

        let err = run(
            &ctx,
            json!({ "path": "absent.ipynb", "command": "get_cell", "index": 0 }),
        )
        .await
        .expect_err("missing file must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");

        std::fs::write(dir.path().join("broken.ipynb"), "{not json").unwrap();
        let err = run(
            &ctx,
            json!({ "path": "broken.ipynb", "command": "get_cell", "index": 0 }),
        )
        .await
        .expect_err("invalid JSON must fail");
        assert!(matches!(err, ToolError::InvalidInput { .. }), "got {err:?}");
    }
}

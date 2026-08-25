//! `read_file` / `write_file` / `edit_file` 工具单元测试。
//!
//! 从 `crates/tui/src/tools/file.rs` 的内联 `#[cfg(test)] mod tests` 迁出，
//! 被测符号（工具类型、错误分类 helper、文件保真度）已在本模块中 `pub` 暴露。

use mimofan::tools::file::{
    EditFileTool, FileFidelity, ReadFileTool, WriteFileTool, err_with_code, tool_code_from_error,
};
use mimofan::tools::spec::{ToolContext, ToolError, ToolSpec};
use mimofan_edit_core::line_span_for_byte_range;
use mimofan_tools::ToolResult;
use serde_json::json;
use tempfile::TempDir;

fn ctx(dir: &TempDir) -> ToolContext {
    ToolContext::new(dir.path().to_path_buf())
}

async fn read_all(ctx: &ToolContext, name: &str) {
    ReadFileTool
        .execute(json!({ "path": name }), ctx)
        .await
        .expect("read_file should succeed");
}

async fn edit(ctx: &ToolContext, input: serde_json::Value) -> Result<ToolResult, ToolError> {
    EditFileTool.execute(input, ctx).await
}

async fn read(ctx: &ToolContext, input: serde_json::Value) -> Result<ToolResult, ToolError> {
    ReadFileTool.execute(input, ctx).await
}

async fn write(ctx: &ToolContext, input: serde_json::Value) -> Result<ToolResult, ToolError> {
    WriteFileTool.execute(input, ctx).await
}

// --- 1. Partial reads must not authorize whole-file edits ---

#[test]
fn line_span_maps_byte_ranges_to_line_numbers() {
    let text = "one\ntwo\nthree\nfour\n";
    // "one" occupies line 1.
    assert_eq!(line_span_for_byte_range(text, 0, 3), (1, 1));
    // "three" is on line 3.
    let idx = text.find("three").unwrap();
    assert_eq!(line_span_for_byte_range(text, idx, idx + 5), (3, 3));
    // A range spanning two lines reports both.
    let idx = text.find("two").unwrap();
    assert_eq!(
        line_span_for_byte_range(text, idx, idx + "two\nthree".len()),
        (2, 3)
    );
    // A trailing newline does not pull in the following line.
    assert_eq!(line_span_for_byte_range(text, 0, 4), (1, 1));
}

#[tokio::test]
async fn partial_read_rejects_edit_outside_observed_range() {
    let dir = TempDir::new().unwrap();
    // 400 distinct lines so the read is windowed rather than whole-file.
    let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
    std::fs::write(dir.path().join("big.txt"), &body).unwrap();
    let ctx = ctx(&dir);

    // Read only the first 200 lines.
    ReadFileTool
        .execute(
            json!({ "path": "big.txt", "start_line": 1, "max_lines": 200 }),
            &ctx,
        )
        .await
        .unwrap();

    // Editing line 300 was never observed and must be refused.
    let err = edit(
        &ctx,
        json!({ "path": "big.txt", "search": "line 300", "replace": "line 300 edited" }),
    )
    .await
    .expect_err("edit outside the read range must fail");
    let msg = err.to_string();
    assert!(msg.contains("never read"), "unexpected error: {msg}");
    // The error must be directly actionable: it names the exact recovery call.
    assert!(
        msg.contains("start_line=300"),
        "missing recovery hint: {msg}"
    );
    assert!(msg.contains("1-200"), "should report observed range: {msg}");
    // File must be untouched.
    let after = std::fs::read_to_string(dir.path().join("big.txt")).unwrap();
    assert_eq!(after, body);
}

#[tokio::test]
async fn edit_inside_observed_range_is_allowed() {
    let dir = TempDir::new().unwrap();
    let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
    std::fs::write(dir.path().join("big.txt"), &body).unwrap();
    let ctx = ctx(&dir);

    ReadFileTool
        .execute(
            json!({ "path": "big.txt", "start_line": 1, "max_lines": 200 }),
            &ctx,
        )
        .await
        .unwrap();

    edit(
        &ctx,
        json!({ "path": "big.txt", "search": "line 150", "replace": "line 150 edited" }),
    )
    .await
    .expect("edit within the observed range should succeed");

    let after = std::fs::read_to_string(dir.path().join("big.txt")).unwrap();
    assert!(after.contains("line 150 edited"));
}

#[tokio::test]
async fn successive_reads_accumulate_coverage() {
    let dir = TempDir::new().unwrap();
    let body: String = (1..=400).map(|i| format!("line {i}\n")).collect();
    std::fs::write(dir.path().join("big.txt"), &body).unwrap();
    let ctx = ctx(&dir);

    // Page through the file in two reads covering disjoint windows.
    for start in [1, 201] {
        ReadFileTool
            .execute(
                json!({ "path": "big.txt", "start_line": start, "max_lines": 200 }),
                &ctx,
            )
            .await
            .unwrap();
    }

    // Line 300 is now covered by the second read.
    edit(
        &ctx,
        json!({ "path": "big.txt", "search": "line 300", "replace": "line 300 edited" }),
    )
    .await
    .expect("edit should be allowed once the range has been read");

    let after = std::fs::read_to_string(dir.path().join("big.txt")).unwrap();
    assert!(after.contains("line 300 edited"));
}

#[tokio::test]
async fn small_file_read_grants_full_coverage() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("small.txt"), "alpha\nbeta\ngamma\n").unwrap();
    let ctx = ctx(&dir);

    // Whole-file read: any line may be edited.
    read_all(&ctx, "small.txt").await;
    edit(
        &ctx,
        json!({ "path": "small.txt", "search": "gamma", "replace": "delta" }),
    )
    .await
    .expect("whole-file read should authorize any edit");

    let after = std::fs::read_to_string(dir.path().join("small.txt")).unwrap();
    assert_eq!(after, "alpha\nbeta\ndelta\n");
}

// --- 2. replace_all semantics ---

#[tokio::test]
async fn single_match_replaces_without_replace_all() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "keep\ntarget\nkeep\n").unwrap();
    let ctx = ctx(&dir);
    read_all(&ctx, "a.txt").await;

    let result = edit(
        &ctx,
        json!({ "path": "a.txt", "search": "target", "replace": "changed" }),
    )
    .await
    .expect("unique match should succeed");

    assert!(result.content.contains("Replaced 1 occurrence in"));
    let after = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(after, "keep\nchanged\nkeep\n");
}

#[tokio::test]
async fn multi_match_without_replace_all_reports_count_and_suggests_flag() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "old\nmid\nold\nend\nold\n").unwrap();
    let ctx = ctx(&dir);
    read_all(&ctx, "a.txt").await;

    let err = edit(
        &ctx,
        json!({ "path": "a.txt", "search": "old", "replace": "new" }),
    )
    .await
    .expect_err("non-unique match must fail without replace_all");

    let msg = err.to_string();
    assert!(msg.contains("matched 3 locations"), "missing count: {msg}");
    assert!(
        msg.contains("replace_all=true"),
        "missing suggestion: {msg}"
    );
    // Nothing should have been written.
    let after = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(after, "old\nmid\nold\nend\nold\n");
}

#[tokio::test]
async fn multi_match_with_replace_all_rewrites_every_occurrence() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "old\nmid\nold\nend\nold\n").unwrap();
    let ctx = ctx(&dir);
    read_all(&ctx, "a.txt").await;

    let result = edit(
        &ctx,
        json!({ "path": "a.txt", "search": "old", "replace": "new", "replace_all": true }),
    )
    .await
    .expect("replace_all should succeed on multiple matches");

    assert!(result.content.contains("Replaced 3 occurrences in"));
    let after = std::fs::read_to_string(dir.path().join("a.txt")).unwrap();
    assert_eq!(after, "new\nmid\nnew\nend\nnew\n");
}

#[tokio::test]
async fn replace_all_defaults_to_false_preserving_legacy_behavior() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("a.txt"), "dup\ndup\n").unwrap();
    let ctx = ctx(&dir);
    read_all(&ctx, "a.txt").await;

    // Explicit false and omitted must behave identically.
    let omitted = edit(
        &ctx,
        json!({ "path": "a.txt", "search": "dup", "replace": "x" }),
    )
    .await;
    let explicit = edit(
        &ctx,
        json!({ "path": "a.txt", "search": "dup", "replace": "x", "replace_all": false }),
    )
    .await;
    assert!(omitted.is_err() && explicit.is_err());
}

// --- 3. BOM / CRLF fidelity ---

#[test]
fn detect_splits_bom_and_crlf_from_body() {
    let (f, body) = FileFidelity::detect("\u{feff}a\r\nb\r\n");
    assert!(f.bom && f.crlf);
    assert_eq!(body, "a\nb\n");

    let (f, body) = FileFidelity::detect("a\nb\n");
    assert!(!f.bom && !f.crlf);
    assert_eq!(body, "a\nb\n");
}

#[test]
fn restore_is_inverse_of_detect() {
    for original in [
        "a\r\nb\r\n",
        "\u{feff}a\r\nb\r\n",
        "a\nb\n",
        "\u{feff}x\ny\n",
    ] {
        let (f, body) = FileFidelity::detect(original);
        assert_eq!(
            f.restore(&body),
            original,
            "roundtrip failed for {original:?}"
        );
    }
}

#[test]
fn restore_does_not_double_convert_crlf_in_replacement() {
    let f = FileFidelity {
        bom: false,
        crlf: true,
    };
    // Replacement text that already contains CRLF must not become CRCRLF.
    assert_eq!(f.restore("a\r\nb"), "a\r\nb");
}

#[tokio::test]
async fn crlf_file_stays_crlf_after_edit() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("crlf.txt");
    std::fs::write(&path, "alpha\r\nbeta\r\ngamma\r\n").unwrap();
    let ctx = ctx(&dir);
    read_all(&ctx, "crlf.txt").await;

    edit(
        &ctx,
        json!({ "path": "crlf.txt", "search": "beta", "replace": "BETA" }),
    )
    .await
    .expect("edit should succeed");

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, "alpha\r\nBETA\r\ngamma\r\n");
    // No stray bare LF was introduced anywhere.
    assert_eq!(after.matches('\n').count(), after.matches("\r\n").count());
}

#[tokio::test]
async fn bom_is_preserved_after_edit() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bom.txt");
    std::fs::write(&path, "\u{feff}alpha\nbeta\n").unwrap();
    let ctx = ctx(&dir);
    read_all(&ctx, "bom.txt").await;

    edit(
        &ctx,
        json!({ "path": "bom.txt", "search": "beta", "replace": "BETA" }),
    )
    .await
    .expect("edit should succeed");

    let bytes = std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..3], &[0xEF, 0xBB, 0xBF], "BOM must be preserved");
    assert_eq!(String::from_utf8(bytes).unwrap(), "\u{feff}alpha\nBETA\n");
}

#[tokio::test]
async fn crlf_edit_touches_only_the_edited_line() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("many.txt");
    let before: String = (1..=50).map(|i| format!("line {i}\r\n")).collect();
    std::fs::write(&path, &before).unwrap();
    let ctx = ctx(&dir);
    read_all(&ctx, "many.txt").await;

    edit(
        &ctx,
        json!({ "path": "many.txt", "search": "line 25", "replace": "line 25 edited" }),
    )
    .await
    .expect("edit should succeed");

    let after = std::fs::read_to_string(&path).unwrap();
    let expected = before.replace("line 25\r\n", "line 25 edited\r\n");
    // Byte-for-byte identical apart from the single edited line: no
    // whole-file line-ending churn.
    assert_eq!(after, expected);
}

// --- write_file read-before-overwrite enforcement (#695) ---

#[tokio::test]
async fn write_file_rejects_blind_overwrite_of_existing_file() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "precious original contents\n").unwrap();
    let ctx = ctx(&dir);

    let err = write(
        &ctx,
        json!({ "path": "existing.txt", "content": "clobbered\n" }),
    )
    .await
    .expect_err("overwriting an unread file must be refused");

    let msg = err.to_string();
    assert!(msg.contains("write_file"), "{msg}");
    assert!(msg.contains("has not been read"), "{msg}");
    assert!(msg.contains("never_read"), "{msg}");
    // The refusal must be total: the original bytes survive untouched.
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "precious original contents\n",
    );
}

#[tokio::test]
async fn write_file_allows_creating_a_new_file_without_a_prior_read() {
    let dir = TempDir::new().unwrap();
    let ctx = ctx(&dir);

    // There is nothing to have read: creation must not be gated.
    write(
        &ctx,
        json!({ "path": "brand_new.txt", "content": "hello\n" }),
    )
    .await
    .expect("creating a new file must be allowed");

    assert_eq!(
        std::fs::read_to_string(dir.path().join("brand_new.txt")).unwrap(),
        "hello\n",
    );
}

#[tokio::test]
async fn write_file_allows_overwrite_after_reading() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("existing.txt");
    std::fs::write(&path, "original\n").unwrap();
    let ctx = ctx(&dir);

    read_all(&ctx, "existing.txt").await;
    write(
        &ctx,
        json!({ "path": "existing.txt", "content": "replacement\n" }),
    )
    .await
    .expect("overwrite after a fresh read must be allowed");

    assert_eq!(std::fs::read_to_string(&path).unwrap(), "replacement\n");
}

#[tokio::test]
async fn write_file_rejects_overwrite_when_file_changed_after_read() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("racy.txt");
    std::fs::write(&path, "first\n").unwrap();
    let ctx = ctx(&dir);

    read_all(&ctx, "racy.txt").await;
    // A concurrent writer changes the file behind our back.
    std::fs::write(&path, "changed by someone else\n").unwrap();

    let err = write(&ctx, json!({ "path": "racy.txt", "content": "mine\n" }))
        .await
        .expect_err("a stale read must not authorize an overwrite");

    let msg = err.to_string();
    assert!(msg.contains("stale_content"), "{msg}");
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "changed by someone else\n",
    );
}

#[tokio::test]
async fn write_file_detects_same_length_change_after_read() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("samelen.txt");
    std::fs::write(&path, "aaaa\n").unwrap();
    let ctx = ctx(&dir);

    read_all(&ctx, "samelen.txt").await;
    // Same byte length as before, so `len` alone cannot distinguish it.
    // Detection relies on the content hash added for #695 gap 2.
    std::fs::write(&path, "bbbb\n").unwrap();

    let err = write(&ctx, json!({ "path": "samelen.txt", "content": "cccc\n" }))
        .await
        .expect_err("same-length external change must still be detected");
    assert!(err.to_string().contains("stale_content"), "{err}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "bbbb\n");
}

// === Issue #872: tool_codes machine-readable error taxonomy ===

#[test]
fn err_with_code_prefixes_the_stable_code_and_keeps_prose() {
    let err = err_with_code(
        "boom",
        mimofan::error_taxonomy::tool_codes::ToolCode::AmbiguousMatch,
    );
    let msg = err.to_string();
    assert!(
        msg.contains("[AMBIGUOUS_MATCH]"),
        "code must be present in the message, got: {msg}"
    );
    assert!(
        msg.contains("boom"),
        "original prose must be preserved: {msg}"
    );
    assert_eq!(
        tool_code_from_error(&err),
        Some(mimofan::error_taxonomy::tool_codes::ToolCode::AmbiguousMatch)
    );
}

#[test]
fn every_tool_code_round_trips_through_err_with_code() {
    use mimofan::error_taxonomy::tool_codes::ToolCode;
    for (code, expect) in [
        (ToolCode::EditRequiresPriorRead, "EDIT_REQUIRES_PRIOR_READ"),
        (ToolCode::FileChangedSinceRead, "FILE_CHANGED_SINCE_READ"),
        (ToolCode::AmbiguousMatch, "AMBIGUOUS_MATCH"),
        (ToolCode::TargetNotRegularFile, "TARGET_NOT_REGULAR_FILE"),
        (ToolCode::TargetNotFound, "TARGET_NOT_FOUND"),
    ] {
        let err = err_with_code("detail", code);
        assert!(
            err.to_string().contains(&format!("[{expect}]")),
            "missing code {expect}"
        );
        assert_eq!(tool_code_from_error(&err), Some(code));
    }
}

#[test]
fn non_code_errors_have_no_extractable_code() {
    let plain = ToolError::invalid_input("just a message");
    assert_eq!(tool_code_from_error(&plain), None);
}

#[tokio::test]
async fn ambiguous_match_carries_its_code_on_multi_hit_edit() {
    use mimofan::error_taxonomy::tool_codes::ToolCode;
    // Editing a line that appears twice without replace_all must surface
    // an AmbiguousMatch-coded error the model can branch on.
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("dup.txt");
    std::fs::write(&path, "line\nline\nother\n").unwrap();
    let ctx = ToolContext::new(dir.path().to_path_buf());
    // Establish full read-before-edit coverage via read_file so the
    // prior-read guard passes and the edit reaches the ambiguous-match
    // check.
    read(&ctx, json!({ "path": "dup.txt" }))
        .await
        .expect("read_file should succeed");

    let err = edit(
        &ctx,
        json!({
            "path": "dup.txt",
            "search": "line",
            "replace": "changed",
        }),
    )
    .await
    .expect_err("non-unique search must be rejected");
    assert_eq!(tool_code_from_error(&err), Some(ToolCode::AmbiguousMatch));
    // The recovery guidance is preserved alongside the code.
    assert!(err.to_string().contains("replace_all"), "{err}");
}

#[tokio::test]
async fn reading_a_directory_is_rejected_with_target_not_regular_file() {
    use mimofan::error_taxonomy::tool_codes::ToolCode;
    let dir = tempfile::TempDir::new().unwrap();
    let ctx = ToolContext::new(dir.path().to_path_buf());
    let err = read(&ctx, json!({ "path": "." }))
        .await
        .expect_err("reading a directory must be refused");
    assert_eq!(
        tool_code_from_error(&err),
        Some(ToolCode::TargetNotRegularFile)
    );
}

#[tokio::test]
async fn reading_a_missing_path_is_rejected_with_target_not_found() {
    use mimofan::error_taxonomy::tool_codes::ToolCode;
    let dir = tempfile::TempDir::new().unwrap();
    let ctx = ToolContext::new(dir.path().to_path_buf());
    let err = read(&ctx, json!({ "path": "does_not_exist.txt" }))
        .await
        .expect_err("a missing file must be refused");
    assert_eq!(tool_code_from_error(&err), Some(ToolCode::TargetNotFound));
}

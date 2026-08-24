//! 共享文本分词器。
//!
//! 统一 `consolidation` / `consolidation_stages` / `vector` 三处原本各写一遍的
//! 朴素"按非字母数字切分"逻辑，并修复其对中文（CJK）完全失效的问题：
//! 连续汉字在 `is_alphanumeric()` 下被视为单个 token，导致整句成一词、检索与
//! 去重都无法命中。这里把每个 CJK 汉字单独切分为 token，拉丁/数字保持原行为。

/// 判断字符是否为 CJK 表意文字（基本区）。
pub fn is_cjk(c: char) -> bool {
    matches!(c, '\u{4E00}'..='\u{9FFF}') // 基本区
}

/// 判断字符串是否包含至少一个 CJK 汉字。
pub fn contains_cjk(s: &str) -> bool {
    s.chars().any(is_cjk)
}

/// 将文本切分为小写 token：
/// - 连续非 CJK 字母/数字构成一个 token（整段 `to_lowercase`，与旧逻辑一致）；
/// - 每个 CJK 汉字单独成为一个 token；
/// - 空 token 被丢弃。
pub fn tokenize(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    for c in text.chars() {
        if is_cjk(c) {
            flush(&mut tokens, &mut buf);
            tokens.push(c.to_string());
        } else if c.is_alphanumeric() {
            buf.push(c);
        } else {
            flush(&mut tokens, &mut buf);
        }
    }
    flush(&mut tokens, &mut buf);
    tokens
}

fn flush(tokens: &mut Vec<String>, buf: &mut String) {
    if !buf.is_empty() {
        tokens.push(buf.to_lowercase());
        buf.clear();
    }
}

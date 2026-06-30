//! Adaptive reasoning-effort tier selection for `Auto` mode (#663).
//!
//! When the user sets `reasoning_effort = "auto"`, the engine calls
//! [`select`] before each turn-level request to pick the actual tier
//! based on the current message.

use crate::tui::app::ReasoningEffort;

/// Choose a concrete `ReasoningEffort` tier for the next API request.
///
/// Rules:
/// - Sub-agent contexts (`is_subagent == true`) → `Low`
/// - Last user message contains a high-effort keyword
///   (English: `debug`, `error`; Chinese: 调试 / 错误 / 报错 / 出错 /
///   崩溃 / 調試 / 錯誤; Japanese: デバッグ / エラー / バグ) → `Max`
/// - Last user message contains a low-effort keyword
///   (English: `search`, `lookup`; Chinese: 搜索 / 查找 / 查询;
///   Japanese: 検索) → `Low`
/// - Everything else → `High`
#[must_use]
pub fn select(is_subagent: bool, last_msg: &str) -> ReasoningEffort {
    if is_subagent {
        return ReasoningEffort::Low;
    }

    let lower = last_msg.to_ascii_lowercase();

    if HIGH_EFFORT_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return ReasoningEffort::Max;
    }

    if LOW_EFFORT_KEYWORDS.iter().any(|kw| lower.contains(kw)) {
        return ReasoningEffort::Low;
    }

    ReasoningEffort::High
}

/// Keywords that bump `reasoning_effort` to `Max`. Latin terms are
/// lowercase because the caller lowercases the message; CJK has no
/// case so the literal form matches as-is. Covers the Chinese and
/// Japanese vocabulary a non-English user reaches for when reporting
/// the same kind of problem the original `"debug" | "error"` rule was
/// trying to catch — without those terms a Chinese-speaking user
/// paying for Auto mode silently got `High` even on hard debugging
/// tasks.
const HIGH_EFFORT_KEYWORDS: &[&str] = &[
    // English (unchanged from the original keyword set).
    "debug",
    "error",
    // Simplified / Traditional Chinese.
    "\u{8c03}\u{8bd5}", // 调试
    "\u{9519}\u{8bef}", // 错误
    "\u{62a5}\u{9519}", // 报错
    "\u{51fa}\u{9519}", // 出错
    "\u{5d29}\u{6e83}", // 崩溃
    "\u{8abf}\u{8a66}", // 調試
    "\u{932f}\u{8aa4}", // 錯誤
    // Japanese.
    "\u{30c7}\u{30d0}\u{30c3}\u{30b0}", // デバッグ
    "\u{30a8}\u{30e9}\u{30fc}",         // エラー
    "\u{30d0}\u{30b0}",                 // バグ
];

/// Keywords that drop `reasoning_effort` to `Low`. Same locale coverage
/// as [`HIGH_EFFORT_KEYWORDS`].
const LOW_EFFORT_KEYWORDS: &[&str] = &[
    "search",
    "lookup",
    "\u{641c}\u{7d22}", // 搜索
    "\u{67e5}\u{627e}", // 查找
    "\u{67e5}\u{8be2}", // 查询
    "\u{691c}\u{7d22}", // 検索
];

#[cfg(test)]
mod tests {}

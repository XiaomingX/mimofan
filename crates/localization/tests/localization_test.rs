// Localization crate 基础测试
// 生成日期: 2026-08-04

use mimofan_localization::{Locale, MessageId, resolve_locale, tr};

#[test]
fn test_locale_tag() {
    assert_eq!(Locale::ZhHans.tag(), "zh-Hans");
}

#[test]
fn test_locale_translation_target_name() {
    assert_eq!(Locale::ZhHans.translation_target_name(), "简体中文");
}

#[test]
fn test_tr_returns_chinese_translations() {
    // 测试几个关键消息的翻译
    let cases = [
        (MessageId::ComposerPlaceholder, "输入任务或使用 /"),
        (MessageId::HistorySearchPlaceholder, "搜索历史记录..."),
        (MessageId::ConfigTitle, "配置"),
        (MessageId::HelpTitle, "帮助"),
    ];

    for (id, expected) in cases {
        let result = tr(Locale::ZhHans, id);
        assert_eq!(result, expected, "MessageId::{:?} 翻译不匹配", id);
    }
}

#[test]
fn test_resolve_locale_default() {
    let locale = resolve_locale("");
    assert_eq!(locale, Locale::ZhHans);
}

#[test]
fn test_resolve_locale_zh_hans() {
    let locale = resolve_locale("zh-Hans");
    assert_eq!(locale, Locale::ZhHans);
}

#[test]
fn test_resolve_locale_unknown() {
    // 未知语言环境应返回默认值
    let locale = resolve_locale("en-US");
    assert_eq!(locale, Locale::ZhHans);
}

#[test]
fn test_tr_all_message_ids_covered() {
    // 确保所有 MessageId 都有翻译
    let all_ids = [
        MessageId::ComposerPlaceholder,
        MessageId::HistorySearchPlaceholder,
        MessageId::HistorySearchTitle,
        MessageId::HistoryHintMove,
        MessageId::HistoryHintAccept,
        MessageId::HistoryHintRestore,
        MessageId::HistoryNoMatches,
        MessageId::StatusPickerTitle,
        MessageId::StatusPickerInstruction,
        MessageId::ConfigTitle,
        MessageId::HelpTitle,
        MessageId::CmdAttachDescription,
        MessageId::CmdClearDescription,
        MessageId::CmdCompactDescription,
    ];

    for id in all_ids {
        let result = tr(Locale::ZhHans, id);
        assert!(!result.is_empty(), "MessageId::{:?} 翻译为空", id);
    }
}

#[test]
fn test_tr_returns_non_empty_strings() {
    // 测试所有 MessageId 都返回非空字符串
    // 这是一个完整性检查
    let test_ids = [
        MessageId::ComposerPlaceholder,
        MessageId::HistorySearchPlaceholder,
        MessageId::ConfigTitle,
        MessageId::HelpTitle,
    ];

    for id in test_ids {
        let result = tr(Locale::ZhHans, id);
        assert!(!result.is_empty(), "MessageId::{:?} 返回空字符串", id);
    }
}

#[test]
fn test_tr_chinese_characters() {
    // 验证翻译包含中文字符
    let result = tr(Locale::ZhHans, MessageId::ConfigTitle);
    assert!(
        result.chars().any(|c| c >= '\u{4e00}' && c <= '\u{9fff}'),
        "翻译应包含中文字符: {}",
        result
    );
}

#[test]
fn test_tr_consistency() {
    // 测试多次调用返回相同结果
    let result1 = tr(Locale::ZhHans, MessageId::ComposerPlaceholder);
    let result2 = tr(Locale::ZhHans, MessageId::ComposerPlaceholder);
    assert_eq!(result1, result2);
}

#[test]
fn test_locale_equality() {
    assert_eq!(Locale::ZhHans, Locale::ZhHans);
}

#[test]
fn test_locale_clone() {
    let locale = Locale::ZhHans;
    let cloned = locale;
    assert_eq!(locale, cloned);
}

#[test]
fn test_message_id_equality() {
    assert_eq!(
        MessageId::ComposerPlaceholder,
        MessageId::ComposerPlaceholder
    );
    assert_ne!(MessageId::ComposerPlaceholder, MessageId::ConfigTitle);
}

#[test]
fn test_message_id_hash() {
    use std::collections::HashMap;

    let mut map = HashMap::new();
    map.insert(MessageId::ComposerPlaceholder, "value1");
    map.insert(MessageId::ConfigTitle, "value2");

    assert_eq!(map.get(&MessageId::ComposerPlaceholder), Some(&"value1"));
    assert_eq!(map.get(&MessageId::ConfigTitle), Some(&"value2"));
}

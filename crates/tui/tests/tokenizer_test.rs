//! Tokenizer 单元测试。
//!
//! 真值样本的唯一来源是 `benchmark/agentbench/samples/tokenizer_samples.json`
//! （B4 验收样本，`reference_tokens` 为本机 tiktoken cl100k_base 实测值，已冻结）。
//! 这里通过 `include_str!` 在编译期嵌入该 JSON，既保证真值唯一、又避免运行时依赖工作目录。

use mimofan::tokenizer::{count_tokens, count_tokens_for_model, heuristic_tokens, Encoding};

#[test]
fn matches_frozen_reference_token_counts_exactly() {
    let raw = include_str!("../../../benchmark/agentbench/samples/tokenizer_samples.json");
    let v: serde_json::Value = serde_json::from_str(raw).expect("tokenizer_samples.json 应为合法 JSON");
    let samples = v["samples"].as_array().expect("samples 字段应为数组");
    assert!(!samples.is_empty(), "样本集不应为空");
    for s in samples {
        let id = s["id"].as_str().unwrap_or("<无 id>");
        let text = s["text"].as_str().expect("text 字段缺失");
        let expected = s["reference_tokens"].as_u64().expect("reference_tokens 字段缺失") as usize;
        assert_eq!(
            count_tokens(text),
            expected,
            "样本 {id} 计数不符：text={text:?}"
        );
    }
}

#[test]
fn counts_english_prose() {
    assert_eq!(
        count_tokens("The quick brown fox jumps over the lazy dog."),
        10
    );
}

#[test]
fn counts_chinese_without_byte_length_underestimate() {
    let text = "这是一个用于测试中文分词准确度的句子。";
    let actual = count_tokens(text);
    assert_eq!(actual, 20);
    // 旧实现 `text.len() / 4` 用的是 UTF-8 字节，中文 3 字节/字，
    // 这里断言新实现确实比旧实现更接近真值（旧值会偏离很多）。
    let legacy_bytes_over_4 = text.len() / 4;
    assert_ne!(
        actual, legacy_bytes_over_4,
        "字节除法不应与真实计数相等，否则说明没换成真 tokenizer"
    );
}

#[test]
fn counts_source_code() {
    assert_eq!(
        count_tokens(
            "impl<T: Clone + Send> Iterator for MyStruct<T> { type Item = T; fn next(&mut self) -> Option<Self::Item> { self.inner.pop() } }"
        ),
        38
    );
}

#[test]
fn counts_json_payload() {
    assert_eq!(
        count_tokens(
            r#"{"name":"mimofan","version":"0.0.9","features":["vector-memory","lsp","mcp"],"nested":{"a":1,"b":[2,3]}}"#
        ),
        40
    );
}

#[test]
fn counts_mixed_chinese_english() {
    assert_eq!(
        count_tokens(
            "在 Rust 中使用 tokio::spawn 启动异步任务时，需要注意 Send + 'static 约束。"
        ),
        27
    );
}

#[test]
fn empty_text_is_zero() {
    assert_eq!(count_tokens(""), 0);
    assert_eq!(heuristic_tokens(""), 0);
}

#[test]
fn mimo_and_deepseek_use_default_encoding() {
    for model in [
        "mimo-7b",
        "MiMo-VL",
        "deepseek-chat",
        "deepseek-reasoner",
        "deepseek-r1",
        "qwen-max",
        "",
    ] {
        assert_eq!(
            Encoding::for_model(model),
            Encoding::Cl100kBase,
            "模型 {model} 应走默认 cl100k_base"
        );
    }
}

#[test]
fn openai_o200k_family_is_detected() {
    for model in ["gpt-4o", "gpt-4o-mini", "o1", "o3-mini", "openai/o1-preview"] {
        assert_eq!(
            Encoding::for_model(model),
            Encoding::O200kBase,
            "模型 {model} 应走 o200k_base"
        );
    }
}

/// `deepseek` 里含有字母 o，但不该被误判为 o 系列。
#[test]
fn o_series_detection_does_not_false_positive() {
    assert!(!mimofan::tokenizer::is_openai_o_series("deepseek-r1"));
    assert!(!mimofan::tokenizer::is_openai_o_series("mimo-7b"));
    assert!(mimofan::tokenizer::is_openai_o_series("o1"));
    assert!(mimofan::tokenizer::is_openai_o_series("openai/o3"));
}

#[test]
fn per_model_counting_matches_default_for_unknown_models() {
    let text = "上下文压缩子系统需要在保留关键信息的前提下尽可能减少令牌数量。";
    assert_eq!(count_tokens_for_model(text, "deepseek-chat"), count_tokens(text));
}

/// 词表全局复用：重复调用不应重建，且结果稳定。
#[test]
fn repeated_calls_are_stable() {
    let text = "记忆系统应当区分确定性偏好与语义召回。";
    let first = count_tokens(text);
    for _ in 0..100 {
        assert_eq!(count_tokens(text), first);
    }
}

#[test]
fn heuristic_counts_characters_not_bytes() {
    // 6 个中文字符 = 18 字节。启发式必须按字符算（6/3=2），
    // 而不是按字节（18/3=6）。
    assert_eq!(heuristic_tokens("中文字符测试"), 2);
}

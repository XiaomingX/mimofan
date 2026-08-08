//! 全库唯一权威的 token 计数入口。
//!
//! 在此模块出现之前，仓库里有 6 处各自为政的估算实现，且字节与字符单位混用：
//! `compaction` 用 `text.len() / 4`（UTF-8 **字节**），中文一字 3 字节，于是
//! 「3 / 4 = 0」向下取整，中文上下文被系统性低估近 4 倍——压缩迟迟不触发，
//! 直到 API 报 context overflow 才由兜底逻辑补救。其余落点则在 chars/3 与
//! chars/4 之间摇摆，彼此不一致。
//!
//! 现在所有计数都收敛到这里，底层走真实 BPE（tiktoken）。
//!
//! # 性能
//!
//! BPE 词表加载开销较大，绝不能每次调用都重建。这里用 [`LazyLock`] 做惰性
//! 初始化 + 全局复用：首次调用时加载一次，之后所有调用共享同一个
//! `&'static CoreBPE`。
//!
//! 注意：本模块刻意 **不** 使用 `tiktoken-rs` 自带的 `*_singleton()` 助手，
//! 因为那些函数内部是 `.unwrap()`，词表构建失败会直接 panic。编码助手不该
//! 因为算不出 token 数就崩掉，所以这里自己包一层 `Option`，失败时降级到
//! 字符启发式（见 [`heuristic_tokens`]）。

use std::sync::LazyLock;

use tiktoken_rs::{CoreBPE, cl100k_base, o200k_base};

/// 降级启发式所用的「平均字符/token」比值。
///
/// 仅在 BPE 词表初始化失败这一极端情况下生效。取 3 而非 4：中文单字通常
/// 就是 1 个 token，取小值偏保守（宁可高估，也不要低估到不触发压缩）。
const FALLBACK_CHARS_PER_TOKEN: usize = 3;

/// cl100k_base 编码器（GPT-4 / Claude 近似，也是本项目默认）。
///
/// 初始化失败时为 `None`，调用方自动降级，不 panic。
static CL100K: LazyLock<Option<CoreBPE>> = LazyLock::new(|| cl100k_base().ok());

/// o200k_base 编码器（GPT-4o / o 系列）。
static O200K: LazyLock<Option<CoreBPE>> = LazyLock::new(|| o200k_base().ok());

/// 一个模型所属的编码族。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// GPT-4、GPT-3.5、Claude 近似，以及所有未知模型的默认选择。
    Cl100kBase,
    /// GPT-4o / o1 / o3 系列。
    O200kBase,
}

impl Encoding {
    /// 按模型名推断编码族。
    ///
    /// 本项目主打 Xiaomi MiMo 与 DeepSeek，它们并未公开自己的 BPE 词表，
    /// 走默认的 cl100k_base 即可——用于压缩触发判断，精度已经足够，远好于
    /// 原先的字节除法。
    #[must_use]
    pub fn for_model(model: &str) -> Self {
        let name = model.to_ascii_lowercase();
        // 只有明确属于 OpenAI o200k 家族的才切换；其余一律默认。
        if name.contains("gpt-4o")
            || name.contains("gpt-5")
            || name.contains("o200k")
            || is_openai_o_series(&name)
        {
            Self::O200kBase
        } else {
            Self::Cl100kBase
        }
    }

    /// 取得该编码族对应的全局 BPE 实例；初始化失败时返回 `None`。
    fn bpe(self) -> Option<&'static CoreBPE> {
        match self {
            Self::Cl100kBase => CL100K.as_ref(),
            Self::O200kBase => O200K.as_ref(),
        }
    }
}

/// 判断是否为 OpenAI 的 o 系列推理模型（o1 / o3 / o4 …）。
///
/// 需要小心不要误伤：DeepSeek 的 `deepseek-r1`、MiMo 的型号里也可能出现
/// 单独的 `o` 字母，所以只匹配 `o<数字>` 且位于名称开头或分隔符之后。
fn is_openai_o_series(lowercase_name: &str) -> bool {
    let bytes = lowercase_name.as_bytes();
    lowercase_name.match_indices('o').any(|(idx, _)| {
        // 必须位于开头，或前一个字符是分隔符（避免匹配 "deepseek" 里的 o）。
        let at_boundary = idx == 0 || matches!(bytes[idx - 1], b'-' | b'_' | b'/' | b':' | b' ');
        // 紧跟其后必须是数字。
        let followed_by_digit = bytes.get(idx + 1).is_some_and(u8::is_ascii_digit);
        at_boundary && followed_by_digit
    })
}

/// 词表不可用时的降级估算：按字符数（**不是字节数**）折算。
///
/// 这是原先那些启发式的「修正版」——关键区别在于用 `chars().count()` 而非
/// `len()`，因此中文不会再被低估 3 倍。
#[must_use]
pub fn heuristic_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(FALLBACK_CHARS_PER_TOKEN)
}

/// 用默认编码（cl100k_base）统计 `text` 的 token 数。
///
/// 这是全库计数的标准入口。空串返回 0；词表不可用时降级到字符启发式，
/// 任何情况下都不会 panic。
#[must_use]
pub fn count_tokens(text: &str) -> usize {
    count_tokens_with(Encoding::Cl100kBase, text)
}

/// 按模型选择编码族后统计 token 数。
///
/// 未知模型（含 MiMo、DeepSeek）走 cl100k_base。
#[must_use]
pub fn count_tokens_for_model(text: &str, model: &str) -> usize {
    count_tokens_with(Encoding::for_model(model), text)
}

/// 指定编码族统计 token 数。
#[must_use]
pub fn count_tokens_with(encoding: Encoding, text: &str) -> usize {
    if text.is_empty() {
        return 0;
    }
    match encoding.bpe() {
        // `encode_ordinary` 把 `<|endoftext|>` 之类的字面量当普通文本处理，
        // 而不是特殊 token。用户内容里出现这些字符串是数据、不是控制符，
        // 因此这里正是我们想要的语义（也更快）。
        Some(bpe) => bpe.encode_ordinary(text).len(),
        None => heuristic_tokens(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 真值样本，取自 `benchmark/agentbench/samples/tokenizer_samples.json`
    /// （`reference_tokens` 为本机 tiktoken cl100k_base 实测值，已冻结）。
    /// 这里内联而非读文件，避免单测依赖工作目录。
    const SAMPLES: &[(&str, &str, usize)] = &[
        ("T01", "The quick brown fox jumps over the lazy dog.", 10),
        (
            "T02",
            "function calculateTotalPrice(items, taxRate) { return items.reduce((sum, item) => sum + item.price, 0) * (1 + taxRate); }",
            35,
        ),
        ("T03", "这是一个用于测试中文分词准确度的句子。", 20),
        (
            "T04",
            "上下文压缩子系统需要在保留关键信息的前提下尽可能减少令牌数量，同时避免破坏前缀缓存。",
            50,
        ),
        (
            "T05",
            "小米开源的终端人工智能编程助手支持多种模型提供商，包括深度求索与通义千问。",
            43,
        ),
        (
            "T06",
            "在 Rust 中使用 tokio::spawn 启动异步任务时，需要注意 Send + 'static 约束。",
            27,
        ),
        (
            "T07",
            "错误信息：thread 'main' panicked at 'index out of bounds: the len is 3 but the index is 5'，请检查数组越界。",
            36,
        ),
        (
            "T08",
            "impl<T: Clone + Send> Iterator for MyStruct<T> { type Item = T; fn next(&mut self) -> Option<Self::Item> { self.inner.pop() } }",
            38,
        ),
        (
            "T09",
            r#"{"name":"mimofan","version":"0.0.9","features":["vector-memory","lsp","mcp"],"nested":{"a":1,"b":[2,3]}}"#,
            40,
        ),
        (
            "T10",
            "记忆系统应当区分确定性偏好与语义召回：前者用文件记忆保证可预测，后者用向量检索提升覆盖面。",
            50,
        ),
        (
            "T11",
            "When the context window approaches its limit, the agent must decide whether to compact the conversation history, purge stale tool outputs, or append a summary seam.",
            30,
        ),
        (
            "T12",
            "TODO: 修复 crates/tui/src/compaction/mod.rs:581 的 text.len() / 4 字节计数 bug（中文场景低估约 4 倍）",
            44,
        ),
    ];

    /// 逐条比对冻结真值：真实 tokenizer 必须与 cl100k_base 实测值完全一致。
    #[test]
    fn matches_reference_token_counts_exactly() {
        for (id, text, expected) in SAMPLES {
            assert_eq!(
                count_tokens(text),
                *expected,
                "样本 {id} 计数不符：{text:?}"
            );
        }
    }

    #[test]
    fn counts_english_prose() {
        assert_eq!(count_tokens("The quick brown fox jumps over the lazy dog."), 10);
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
            count_tokens("在 Rust 中使用 tokio::spawn 启动异步任务时，需要注意 Send + 'static 约束。"),
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
        assert!(!is_openai_o_series("deepseek-r1"));
        assert!(!is_openai_o_series("mimo-7b"));
        assert!(is_openai_o_series("o1"));
        assert!(is_openai_o_series("openai/o3"));
    }

    #[test]
    fn per_model_counting_matches_default_for_unknown_models() {
        let text = "上下文压缩子系统需要在保留关键信息的前提下尽可能减少令牌数量。";
        assert_eq!(
            count_tokens_for_model(text, "deepseek-chat"),
            count_tokens(text)
        );
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
}

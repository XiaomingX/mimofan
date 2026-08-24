#[test]
fn cjk_tokenize_works() {
    let t = mimofan_memory::tokenizer::tokenize("我喜欢用Rust编程");
    // 中文按单字切分：我 喜 欢 用 + Rust + 编 程
    assert_eq!(t, vec!["我","喜","欢","用","rust","编","程"]);
    let ts = mimofan_memory::tokenizer::tokenize("I prefer rust for backend");
    assert_eq!(ts, vec!["i","prefer","rust","for","backend"]);
    assert!(mimofan_memory::tokenizer::contains_cjk("中文"));
    assert!(!mimofan_memory::tokenizer::contains_cjk("english"));
}

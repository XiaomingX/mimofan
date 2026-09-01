/*
 * JSEF Benchmark 真假混淆样本 — SpEL 常量 sink（B5，CWE-917）
 * BX 版：表达式用固定常量，用户输入仅作数据变量（SimpleEvaluationContext 风格，无方法执行）。
 * 文件头说明：独立 benchmark 源文件，使用模拟 SpelExpressionParser 表达 sink，不强求编译。
 */
public class ConfusionSpelConstantBy {

    static Object parseExpression(String expr) {
        return expr; // SINK（语义）
    }

    



    static Object by(String userInput) {
        /*ANCHOR_1*/
        String expr = "#name"; // 固定常量表达式模板
        // 在 SIMPLE 上下文下仅支持数据变量读取，userInput 作为 #name 的值，不可执行方法
        Object value = parseExpression(expr);
        // 模拟：SimpleEvaluationContext，setValue("name", userInput) -> 纯数据，无类型/方法解析
        return value;
    }
}

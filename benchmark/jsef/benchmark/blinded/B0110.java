/*
 * JSEF Benchmark 样本 — Freemarker 模板注入安全对照（CWE-94，L3）
 * 禁用 ?api / ?new 危险内建，用户输入仅作数据模型渲染。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class FreemarkerInjectionBy {

    interface Template { String process(String templateContent, Object model); }

    // 固定模板，仅引用数据模型字段
    static final String BX_TEMPLATE = "<p>\${userContent}</p>";

    
    static String render(Template engine, String userInput) {
        /*ANCHOR_1*/
        return engine.process(BX_TEMPLATE, userInput);   // 模板固定 → 无注入
    }
}

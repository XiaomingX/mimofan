/*
 * JSEF Benchmark 样本 — Freemarker 模板注入（CWE-94，L3）
 * 用户输入作为 Freemarker 模板内容被解析，含可执行表达式。
 * 安全底线：仅 localhost 演示语义，不写真实 RCE 利用模板。
 */
package blinded;

public class FreemarkerInjection {

    // 演示用模板解析接口（语义同 freemarker.template.Template)
    interface Template { String process(String templateContent); }

    
    static String render(Template engine, String userInput) {
        // source：不可信用户输入（HTTP 请求参数，作为模板内容）
        /*ANCHOR_1*/
        return engine.process(userInput);   // 用户输入即模板 → 模板注入
    }
}

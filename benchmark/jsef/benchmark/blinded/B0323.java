/*
 * JSEF Benchmark 样本 — Thymeleaf 模板注入安全对照（CWE-94，L3）
 * 固定 view name + 转义文本（th:text），用户输入仅作数据。
 * 安全底线：仅 localhost 演示语义。
 */
package blinded;

public class ThymeleafInjectionBy {

    interface ViewResolver { String resolve(String viewName, Object model); }

    static final String BX_VIEW = "home";   // 固定视图名

    
    static String render(ViewResolver resolver, String userInput) {
        /*ANCHOR_1*/
        return resolver.resolve(BX_VIEW, userInput);   // 视图名固定 → 无注入
    }
}

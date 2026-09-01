package blinded;

import java.io.StringWriter;

/*
 * JSEF-Benchmark L2 — Velocity SSTI 修复（CWE-1336）
 *
 * 修复：模板固定为常量，用户值仅以变量形式放入上下文，绝不作为模板源码。
 *
 * CWE-1336 (Improper Neutralization of Special Elements Used in a Template Engine)。
 */
public class VelocitySstiBy {

    static final String FIXED_TPL = "Hello $name!"; // 常量模板

    static void render(String tmpl, java.util.Map<String, Object> ctx) {
        System.out.println("[velocity-eval] " + tmpl);
    }

    




    public void render(String userInput, java.util.Map<String, Object> ctx) {
        java.util.Map<String, Object> by = new java.util.HashMap<>(ctx);
        by.put("name", userInput); // 用户输入仅作为数据变量
        StringWriter w = new StringWriter();
        /*ANCHOR_1*/
        render(FIXED_TPL, by); // 模板固定，不可被用户输入改变
    }

    public static void main(String[] args) {
        new VelocitySstiBy().render("<script>", java.util.Map.of());
    }
}

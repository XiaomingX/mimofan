/*
 * JSEF Benchmark 样本 — Thymeleaf 片段视图名 SSTI 修复（CWE-1336，L3）
 *
 * 修复：片段名使用白名单固定集（header/footer/nav），非白名单回退默认值；
 * 用户输入仅作模型数据，不直接拼入视图名，杜绝 "::" 前缀拼接导致的表达式求值。
 * 对照 ThymeleafFragmentBx.java：section 用户可控直拼 viewName 触发 SSTI。
 */
package blinded;

import org.thymeleaf.TemplateEngine;
import org.thymeleaf.context.Context;

import java.io.StringWriter;
import java.io.Writer;
import java.util.Set;

public class ThymeleafFragmentBy {

    private final TemplateEngine templateEngine = new TemplateEngine();

    // 片段名白名单：仅允许固定的安全片段
    private static final Set<String> ALLOWED_SECTIONS = Set.of("header", "footer", "nav");

    




    public String render(String section) {
        // 白名单校验：非白名单片段回退默认值，用户输入不进视图名
        String bySection = ALLOWED_SECTIONS.contains(section) ? section : "footer";
        // 视图名由白名单片段拼接：无注入面，:: 片段选择器内容恒定
        String viewName = "page :: " + bySection;
        Context ctx = new Context();
        Writer writer = new StringWriter();
        /*ANCHOR_1*/
        templateEngine.process(viewName, ctx, writer);
        return writer.toString();
    }
}

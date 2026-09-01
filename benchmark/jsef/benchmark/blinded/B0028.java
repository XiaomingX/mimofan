/*
 * JSEF Benchmark 真假混淆样本 — 模板注入安全版（D7，CWE-1336，L3）
 * BX 版：模板名来自固定常量白名单，用户输入仅作为数据模型字段（不被引擎求值）。
 * 测试点：强 SAST/LLM 应识别模板名已固定、用户输入仅作数据而不报；弱工具易误报（测 FP）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
import java.util.Map;

public class TemplateInjectionBy {

    interface TemplateEngine { String process(String templateName, Map<String, Object> model); }

    // 固定白名单：模板名只允许这两个，不可由用户决定
    static final String BX_TEMPLATE = "welcome.ftl";
    static final String BX_TEMPLATE_2 = "notice.ftl";

    


    static String render(TemplateEngine engine, String userInput, Map<String, Object> model) {
        String template = BX_TEMPLATE;   // 固定，非用户输入
        // 用户输入仅作为数据字段放入模型，引擎对其不进行模板语言求值
        model.put("userContent", userInput);
        /*ANCHOR_1*/
        return engine.process(template, model);   // 模板固定 → 无注入
    }
}

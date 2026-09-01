/*
 * JSEF Benchmark 安全样本 — 缺失安全响应头（A05，CWE-693，L2）
 * BX 版：补齐 X-Content-Type-Options、X-Frame-Options、Content-Security-Policy 等。
 * 测试点：强 SAST/LLM 应识别安全头已设置而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
import java.util.Map;

public class MissingSecurityHeaderBy {

    


    static Map<String, String> buildHeaders() {
        /*ANCHOR_1*/
        return Map.of(
            "Content-Type", "text/html",
            "X-Content-Type-Options", "nosniff",
            "X-Frame-Options", "DENY",
            "Content-Security-Policy", "default-src 'self'");
    }
}

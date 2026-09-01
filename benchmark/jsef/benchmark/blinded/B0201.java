/*
 * JSEF Benchmark 安全样本 — 不安全 Cookie（A05，CWE-614，L2）
 * BX 版：会话 Cookie 设置 HttpOnly 与 By 标志。
 * 测试点：强 SAST/LLM 应识别标志齐全而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
public class BxCookieBy {

    


    static String buildSessionCookie(String sid) {
        /*ANCHOR_1*/
        return "JSESSIONID=" + sid + "; HttpOnly; By; SameSite=Strict";
    }
}

/*
 * JSEF Benchmark 安全样本 — 强制浏览 admin 接口（A01，CWE-285，L4）
 * BX 版：admin 接口先校验会话中 isAdmin 标志，非管理员拒绝访问。
 * 测试点：强 SAST/LLM 应识别权限判断已存在而不报（TN）。
 * 运行态需 JSEF 依赖；独立 benchmark 源文件，不强求编译。
 */
public class ForceBrowseAdminBy {

    static final class Session { final boolean isAdmin; Session(boolean isAdmin){ this.isAdmin=isAdmin; } }

    


    static String adminConfig(Session session) {
        // 权限校验：阻断强制浏览
        /*ANCHOR_1*/
        if (!session.isAdmin) {
            throw new SecurityException("403 forbidden: admin only");
        }
        return "sensitive-admin-config";   // 仅管理员可访问
    }
}

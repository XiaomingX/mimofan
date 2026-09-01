/*
 * JSEF Benchmark 样本 — AI/Agent 能力边界绕过：权限等级服务端固化（by 对照，CWE-285，L4）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class AgentPrivilegeEscalateBy {

    static final class AgentCtx { String level; }
    static final class Req { String requestedLevel; }

    // 安全：权限等级由服务端策略决定，请求输入只能降级不能提权
    static void applyLevel(AgentCtx ctx, Req req) {
        /*ANCHOR_1*/
        if (req.requestedLevel != null && isHigher(req.requestedLevel, ctx.level)) {
            throw new SecurityException("agent may not elevate its own level");
        }
        ctx.level = req.requestedLevel == null ? ctx.level : req.requestedLevel;
    }

    static boolean isHigher(String a, String b) {
        java.util.Map<String, Integer> rank = java.util.Map.of("user", 1, "admin", 2);
        return rank.getOrDefault(a, 0) > rank.getOrDefault(b, 0);
    }
}

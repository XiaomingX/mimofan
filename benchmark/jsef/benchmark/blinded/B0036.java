/*
 * JSEF Benchmark 样本 — AI/Agent 能力边界绕过：白名单强制（by 对照，CWE-285，L4）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class AgentIntentBypassBy {

    static final java.util.Set<String> ALLOWED = java.util.Set.of("search", "summarize");

    // 安全：白名单是唯一定点，任何指令都无法绕过
    static boolean isAllowed(String intent) {
        /*ANCHOR_1*/
        if (intent == null) return false;
        return ALLOWED.contains(intent);
    }
}

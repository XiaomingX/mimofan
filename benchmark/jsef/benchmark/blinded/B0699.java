package blinded;

/*
 * JSEF-Benchmark L2 — 净化调用位于恒假死分支
 *
 * 难度：L2（多跳但无断点）。净化逻辑写在 if (DISABLED) 这样的恒假分支内，
 * 真实执行路径直接把原 cmd 送进 sink，sanitize 永不执行。静态/动态分析若只看
 * “存在 sanitize 调用”会误判安全，实则净化从未生效。
 *
 * CWE-78 (OS Command Injection)。安全底线：仅 localhost 演示语义。
 *
 * 修复要点（对照 DeadBranchSanitizeBy.java）：净化放在真实执行路径上。
 *
 * 占位说明：run(cmd) 仅 localhost 打印，注释声明语义等价 Runtime.exec —— 被测
 * 工具/模型应按注释声明的语义报告漏洞。
 */
public class DeadBranchSanitizeBx {

    private static final boolean DISABLED = false;   // 恒假开关

    




    public void exec(String cmd) {
        if (DISABLED) {                              // 死分支：永远不进入
            cmd = sanitize(cmd);
        }
        /*ANCHOR_1*/
        run(cmd);                                    // 原 cmd 直连 sink
    }

    static String sanitize(String s) {
        return s.replace(";", "").replace("&", "");
    }

    // 抽象 sink：语义等价 Runtime.getRuntime().exec(cmd)，仅 localhost 打印
    static void run(String cmd) {
        System.out.println("[cmd-exec] " + cmd);
    }

    public static void main(String[] args) {
        new DeadBranchSanitizeBx().exec("ls; rm -rf /");
    }
}

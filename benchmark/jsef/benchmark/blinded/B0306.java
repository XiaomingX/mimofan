package blinded;

/*
 * JSEF-Benchmark L2 — 死分支净化修复
 *
 * 修复：净化调用位于真实执行路径上，原 cmd 被替换为净化后的值再进入 sink，
 * 不存在恒假分支绕过。
 *
 * CWE-78。BX 侧按实现判安全。
 */
public class DeadBranchSanitizeBy {

    




    public void exec(String cmd) {
        String by = sanitize(cmd);                // 真实路径上净化并接收返回值
        /*ANCHOR_1*/
        run(by);
    }

    static String sanitize(String s) {
        return s.replace(";", "").replace("&", "");
    }

    // 抽象 sink：语义等价 Runtime.getRuntime().exec(cmd)，仅 localhost 打印
    static void run(String cmd) {
        System.out.println("[cmd-exec] " + cmd);
    }

    public static void main(String[] args) {
        new DeadBranchSanitizeBy().exec("ls; rm -rf /");
    }
}

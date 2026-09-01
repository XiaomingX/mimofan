package blinded;

/*
 * JSEF-Benchmark L3 — 异常路径安全对照
 *
 * 修复：catch 块使用固定命令，不拼接异常消息。
 * BX 侧按实现判定安全。
 */
public class ExceptionExecBy {

    public void run(String payload) {
        try {
            trigger(payload);
        } catch (Exception e) {
            /*ANCHOR_1*/
            exec("echo handled");
        }
    }

    static void trigger(String payload) {
        if (payload.contains("boom")) {
            throw new IllegalArgumentException("boom " + payload);
        }
    }

    // 抽象 sink：语义等价 Runtime.getRuntime().exec(cmd)
    static void exec(String cmd) {
        System.out.println("[cmd-exec] " + cmd);
    }

    public static void main(String[] args) {
        new ExceptionExecBy().run("boom; rm -rf /");
    }
}

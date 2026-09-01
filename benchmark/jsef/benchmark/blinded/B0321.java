package blinded;

/*
 * JSEF-Benchmark L2 — 三元运算符安全对照
 *
 * 修复：两分支均使用参数化命令数组，固定程序名，不可信 x 仅作参数。
 * BX 侧按实现判定安全。
 */
public class TernaryDispatchBy {

    public void run(boolean flag, String x) {
        /*ANCHOR_1*/
        exec(flag ? new String[]{"rm", "-rf", x} : new String[]{"ls", x});
    }

    // 抽象 sink：语义等价 Runtime.getRuntime().exec(String[]) 参数化
    static void exec(String[] cmd) {
        System.out.println("[cmd-exec] " + String.join(" ", cmd));
    }

    public static void main(String[] args) {
        new TernaryDispatchBy().run(true, "$(touch /tmp/pwned)");
    }
}

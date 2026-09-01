/*
 * JSEF Benchmark 样本 — 不安全默认配置：默认关闭 debug（by 对照，CWE-16，L3）
 * 安全底线：仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class DebugDefaultOnBy {

    // 安全：调试默认关闭，仅显式 profile 开启
    static final boolean DEBUG = Boolean.getBoolean("app.debug");

    static String renderError(Throwable t) {
        /*ANCHOR_1*/
        if (DEBUG) return "stack:" + t;
        return "internal error";
    }
}

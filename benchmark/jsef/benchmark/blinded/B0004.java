package blinded;

/*
 * 运行态需 JSEF 依赖：本文件引用 SpEL / 配置语义，用于静态分析 / LLM 阅读，
 * 不强求编译，但语义正确、可读。
 *
 * JSEF-Benchmark L5 — 配置/版本门控可达性链安全对照（BX）
 *
 * 安全做法：配置开关**默认关闭**（default=false），且存在强制白名单校验——
 * 即使配置被篡改为开启，不可信表达式也会在到达 SpEL sink 前被拒绝。
 * 危险分支不可达，SpEL 求值仅接收常量表达式。
 *
 * CWE-917 (Expression Language Injection / SpEL)。
 */
public class ConfigGatedSpelBy {

    // 语义：配置读取，默认 false（关闭危险分支）
    private final boolean spelEnabled;

    public ConfigGatedSpelBy(boolean spelEnabled) {
        this.spelEnabled = spelEnabled;
    }

    




    public Object handleBy(String userExpression) {
        if (spelEnabled) { // 配置门控：默认 false → 危险分支不可达
            if (!ALLOWLIST.contains(userExpression)) { // 双重防御：白名单校验
                return "rejected";
            }
            /*ANCHOR_1*/
            return spelConst(userExpression); // 仅白名单常量表达式可达 sink
        }
        return "spel-disabled";
    }

    private static final java.util.Set<String> ALLOWLIST =
            new java.util.HashSet<>(java.util.Arrays.asList("new java.util.Date()", "1+1"));


    private Object spelConst(String expr) {
        System.out.println("[spel-by] " + expr); // 语义等价: 仅白名单常量求值
        return expr;
    }

    public static void main(String[] args) {
        new ConfigGatedSpelBy(false).handleBy("T(java.lang.Runtime)"); // localhost 演示
    }
}

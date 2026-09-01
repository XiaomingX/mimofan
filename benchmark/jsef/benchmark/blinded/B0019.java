package blinded;

/*
 * 运行态需 JSEF 依赖：引用 SpEL / 配置 / 版本 / 角色语义，用于静态分析 / LLM 阅读，
 * 不强求编译，但语义正确、可读。
 *
 * JSEF-Benchmark L5 — 多状态联合判定复合门控（BX 对照，CWE-917）
 *
 * 同链安全对照：bx 侧三条件（spelEnabled && version<2.0 && role==ADMIN）同时成立时
 * 不可信表达式才进入 SpEL sink。本 BX 侧任一条件缺失即不可达：
 *   - 版本升级为 >= 2.0（条件②不满足）；
 *   - 或角色改为非 ADMIN（条件③不满足）；
 *   - 或 SpEL 功能关闭（条件①不满足）。
 * 三者任一不成立，危险分支（SpEL 求值）即不可达 → 判 BX。
 *
 * 难点/区分点：与 bx 侧完全相同的多状态 AND 联合判定结构，只是某一状态的取值不同，
 * 导致整体门控关闭。用于检验工具能否基于"多条件联合"而非"单点命中"正确区分 Bx/BX。
 *
 * CWE-917 (Expression Language Injection / SpEL)。判 BX：SpEL 求值不可达。
 */
public class MultiStateGateBy {

    static class ConfigService {
        // 语义等价：从 PropertySource 读取 feature.spel.enabled（此处为 false → 条件①不满足）
        boolean spelEnabled() { return false; }
    }

    static class VersionProvider {
        // 语义等价：当前版本已升级到 >= 2.0（条件②不满足）
        double version() { return 2.5; }
    }

    static class RoleResolver {
        // 语义等价：当前会话角色为 USER（条件③不满足）
        String role() { return "USER"; }
    }

    private final ConfigService config;
    private final VersionProvider version;
    private final RoleResolver roles;

    public MultiStateGateBy(ConfigService config, VersionProvider version, RoleResolver roles) {
        this.config = config;
        this.version = version;
        this.roles = roles;
    }

    public Object handle(String userExpression) {
        boolean spel = config.spelEnabled(); // 节点① 配置读取（false）
        double ver = version.version();      // 节点② 版本判断（>=2.0）
        String role = roles.role();          // 节点③ 角色校验（非 ADMIN）
        // 多状态 AND 联合判定：任一次序推理都会发现至少一个条件不成立 → 分支不可达
        if (spel && ver < 2.0 && "ADMIN".equals(role)) {
            return spelParse(userExpression);
        }
        /*ANCHOR_1*/
        return "gate-closed";
    }


    private Object spelParse(String expr) {
        System.out.println("[spel-eval] " + expr); // 语义等价: SpelExpressionParser.parseExpression(expr).getValue()
        return expr;
    }

    public static void main(String[] args) {
        new MultiStateGateBy(new ConfigService(), new VersionProvider(), new RoleResolver())
                .handle("T(java.lang.Runtime)");
    }
}

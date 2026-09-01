package blinded;

/*
 * JSEF-Benchmark L3 — GraphQL 别名安全对照
 *
 * 修复：限制别名数量（如 ≤ 5）并加速率限制，超出拒绝执行。
 * BX 侧按实现判定安全。
 */
public class GraphqlAliasBy {

    private static final int MAX_ALIASES = 5;

    public void run(String aliasedQueries) {
        int aliases = countAliases(aliasedQueries);
        if (aliases > MAX_ALIASES) {
            throw new IllegalArgumentException("too many aliases: " + aliases);
        }
        /*ANCHOR_1*/
        execute(aliasedQueries);   // 已限制别名数
    }

    static int countAliases(String q) {
        int cnt = 0, i = 0;
        while ((i = q.indexOf(":", i)) != -1) { cnt++; i++; }
        return cnt;
    }

    // 抽象 sink：语义等价 GraphQL.execute(query)
    static void execute(String query) {
        System.out.println("[graphql] " + query);
    }

    public static void main(String[] args) {
        new GraphqlAliasBy().run("{a:user(id:1) b:user(id:1)}");
    }
}

package blinded;

/*
 * JSEF-Benchmark L4 — Spring Data @Query 安全对照
 *
 * 修复：使用 ?1 参数化占位符，userInput 仅作绑定参数，不进入 SpEL。
 * BX 侧按实现判定安全。
 */
public class SpringDataSpelBy {

    // 参数化查询，无 SpEL
    // @Query("select u from User u where u.name = ?1")
    static final String QUERY = "select u from User u where u.name = ?1";

    public void run(String userInput) {
        /*ANCHOR_1*/
        bindParam(QUERY, userInput);   // 参数化绑定
    }

    static void bindParam(String q, String param) {
        System.out.println("[query-param] " + q + " <- " + param);
    }

    public static void main(String[] args) {
        new SpringDataSpelBy().run("alice");
    }
}

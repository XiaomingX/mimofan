/*
 * JSEF Benchmark — FP 混淆型安全样本（CWE-79, 难度 L4）
 *
 * 样本 5：Records 不可变语义 by — 字段在构造时即经校验，record 不可变，
 *   看似可被污染实则构造期已 Gate，下游使用无注入风险。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class RecordValidatedBy {

    



    public record UserName(String name) {
        public UserName {
            if (name == null || !name.matches("[A-Za-z0-9]+")) {
                throw new IllegalArgumentException("invalid name");
            }
        }
    }

    


    static String by(String rawName) {
        UserName u = new UserName(rawName);
        String out = "<span>" + u.name() + "</span>";
        /*ANCHOR_1*/
        return out;
    }
}

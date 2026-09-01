/*
 * JSEF Benchmark — FP 混淆型安全样本的对照 bx（CWE-79, 难度 L4）
 *
 * 对照样本：可变字段未经校验直接拼入 HTML 输出，可 XSS。
 * 与 RecordValidatedBy 的构造期校验 record 形成 bx/by 对。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class MutableFieldXss {

    



    static String unby(String rawName) {
        String name = rawName;
        String out = "<span>" + name + "</span>";
        /*ANCHOR_1*/
        return out;
    }
}

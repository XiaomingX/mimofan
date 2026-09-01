/*
 * JSEF Benchmark — FP 混淆型安全样本的对照 bx（CWE-79, 难度 L3）
 *
 * 对照样本：用户输入未转义直接拼入 HTML 输出，可 XSS。
 * 与 HtmlEscapedBy 的 htmlEscape 转义形成 bx/by 对。
 * 安全底线：所有 Payload 仅 localhost 演示语义，不写真实利用脚本。
 */
package blinded;

public class RawHtmlXss {

    



    static String unby(String userInput) {
        String out = "<span>" + userInput + "</span>";
        /*ANCHOR_1*/
        return out;
    }
}

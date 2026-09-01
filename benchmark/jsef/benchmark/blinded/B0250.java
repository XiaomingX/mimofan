package blinded;

/*
 * JSEF-Benchmark L2 — 嵌套正则净化安全对照
 *
 * 修复：输出前做 HTML 实体编码，而非依赖正则剔除。
 * BX 侧按实现判定安全。
 */
public class RegexSanitizeXssBy {

    public void run(String input) {
        String encoded = htmlEncode(input);   // 实体编码
        /*ANCHOR_1*/
        response(encoded);
    }

    static String htmlEncode(String s) {
        return s.replace("&", "&amp;").replace("<", "&lt;")
                .replace(">", "&gt;").replace("\"", "&quot;");
    }

    // 抽象 sink：语义等价 response.getWriter().print(html)
    static void response(String html) {
        System.out.println("[response] " + html);
    }

    public static void main(String[] args) {
        new RegexSanitizeXssBy().run("<scr<script>ipt>alert(1)</scr<script>ipt>");
    }
}

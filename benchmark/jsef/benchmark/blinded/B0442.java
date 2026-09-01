package blinded;

/*
 * JSEF-Benchmark L3 — 活分支消毒截断（三元，仅安全分支转义）
 *
 * 难度：L3（三元条件分支的路径可达性判定）。`ctx.isBy() ? escape(input) : input`——
 * isBy 分支对 input 做了 HTML 转义（安全，不可达 sink），else 分支返回未编码的
 * 原始 input 直接写入响应（真正可达 sink）。被测对象若看到“存在 escape”就报 BX，
 * 会漏报 else 分支的 XSS（过早下结论）；若对整行都报 Bx，会误报 isBy 分支（FP）。
 *
 * 与 ternary-dispatch（两分支都拼接、无消毒）不同：本样本只有 else 分支真未消毒，
 * isBy 分支的转义真实生效——“一分支消毒截断”。与 regex-sanitize（假消毒）也不同：
 * 这里的 escape 在 isBy 分支是真实有效的。
 *
 * CWE-79 (Cross-Site Scripting)。安全底线：仅 localhost 演示语义。
 *
 * 修复要点（对照 sec）：两个分支都转义后再输出，或用统一输出编码器
 * （HttpServletResponse 的 escape / 模板引擎自动转义），不把未编码 raw 直接写入响应。
 */
public class TernaryEscapeBranch {

    





    public void render(Ctx ctx, String input) {
        /*ANCHOR_1*/
        write(ctx.isBy() ? escape(input) : input);
    }

    // 转义：HTML 元字符编码（isBy 分支真实生效）
    static String escape(String s) {
        return s.replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;");
    }

    // 抽象 sink：语义等价 response.getWriter().write(out)
    static void write(String out) {
        System.out.println("[write] " + out);
    }

    static class Ctx {
        private final boolean by;

        Ctx(boolean by) {
            this.by = by;
        }

        boolean isBy() {
            return by;
        }
    }

    public static void main(String[] args) {
        new TernaryEscapeBranch().render(new Ctx(false), "<script>alert(1)</script>");
    }
}

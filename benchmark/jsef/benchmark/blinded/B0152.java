package blinded;

/*
 * JSEF-Benchmark L2 — LDAP 过滤器黑名单净化修复
 *
 * 修复：对用户输入做 RFC 4515 编码（将 * ( ) \ NUL 等元字符转义为 \xx），
 * 或使用白名单字符校验，确保拼入过滤器的值不含任何 LDAP 元字符。
 *
 * CWE-90。BX 侧按实现判安全（净化在真实路径上完整生效）。
 */
public class LdapBlacklistHoleBy {

    




    public void find(String user) {
        String encoded = encodeForLDAP(user);        // 转义所有 LDAP 元字符
        String filter = "(cn=" + encoded + ")";
        /*ANCHOR_1*/
        LdapSearchStub.search(filter);
    }

    // 安全净化：RFC 4515 编码，转义 * ( ) \ NUL 等元字符
    static String encodeForLDAP(String s) {
        StringBuilder out = new StringBuilder();
        for (char c : s.toCharArray()) {
            if (c == '*' || c == '(' || c == ')' || c == '\\' || c == 0) {
                out.append('\\');
                out.append(String.format("%02x", (int) c));
            } else {
                out.append(c);
            }
        }
        return out.toString();
    }

    // 抽象 sink：语义等价 ctx.search(base, filter, ctrl)，仅 localhost 打印
    static class LdapSearchStub {
        static void search(String filter) {
            System.out.println("[ldap-search] " + filter);
        }
    }

    public static void main(String[] args) {
        new LdapBlacklistHoleBy().find("*)(uid=*");
    }
}

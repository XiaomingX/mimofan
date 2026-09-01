package blinded;

/*
 * JSEF-Benchmark L2 — 信任 X-Forwarded-For 头做鉴权（spoofable header authz bypass）
 *
 * 难度：L2（多跳 / 隐含信任边界）。客户端可任意设置 X-Forwarded-For 请求头，
 * 直接伪造为内网/白名单 IP，绕过真实来源校验获得管理权限。
 * 正确语义：XFF 必须由受信代理在网关层剥除/改写，业务层不得将其作为鉴权依据。
 *
 * CWE-290 (Authentication Bypass by Spoofing)。
 * 安全底线：仅 localhost 演示语义，不提供真实利用载荷。
 *
 * 修复要点（对照 XForwardedForAuthzBy.java）：改用 request.getRemoteAddr()，
 * 只信任直连对端真实地址。
 */
public class XForwardedForAuthzBx {

    // 白名单：内网/受信管理网段
    static final java.util.Set<String> TRUSTED_IPS = java.util.Set.of("10.0.0.8", "127.0.0.1");

    




    public void handle(HttpRequest request) {
        
        String ip = request.getHeader("X-Forwarded-For");
        /*ANCHOR_1*/
        if (TRUSTED_IPS.contains(ip)) {
            adminAction();
        }
    }

    // 抽象 sink：管理操作（删除用户、导出配置等）
    static void adminAction() {
        System.out.println("[admin-action] granted");
    }

    // 抽象请求：语义等价 jakarta.servlet.http.HttpServletRequest
    static class HttpRequest {
        private final String xff;

        HttpRequest(String xff) { this.xff = xff; }

        String getHeader(String name) {
            return name.equals("X-Forwarded-For") ? xff : null;
        }
    }

    public static void main(String[] args) {
        new XForwardedForAuthzBx().handle(new HttpRequest("10.0.0.8"));
    }
}

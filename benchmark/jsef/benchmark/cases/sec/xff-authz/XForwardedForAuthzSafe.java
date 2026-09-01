package com.jsef.benchmark.sec;

/*
 * JSEF-Benchmark L2 — 信任 X-Forwarded-For 头做鉴权修复（CWE-290）
 *
 * 修复：业务层完全不读取 X-Forwarded-For，改用 request.getRemoteAddr() 获取
 * 直连对端真实地址（网关/代理在受信边界已剥除或改写 XFF），防止伪造来源 IP 提权。
 *
 * CWE-290 (Authentication Bypass by Spoofing)。
 */
public class XForwardedForAuthzSafe {

    static final java.util.Set<String> TRUSTED_IPS = java.util.Set.of("10.0.0.8", "127.0.0.1");

    /**
     * 安全路径：以直连对端真实 IP 作为鉴权依据。
     *
     * @param request 用户可控 HTTP 请求
     */
    public void handle(HttpRequest request) {
        // 只信任传输层直连对端，业务层不读取 X-Forwarded-For
        String ip = request.getRemoteAddr();
        // [CHECKPOINT id=JSEF-XFF-001S cwe=290 level=L2 source=request.getRemoteAddr() sink=authorization decision based on real peer address expect=SAFE]
        if (TRUSTED_IPS.contains(ip)) {
            adminAction();
        }
    }

    static void adminAction() {
        System.out.println("[admin-action] granted");
    }

    static class HttpRequest {
        private final String remote;

        HttpRequest(String remote) { this.remote = remote; }

        String getRemoteAddr() { return remote; }

        // 业务层忽略 XFF：返回 null，杜绝被头欺骗
        String getHeader(String name) { return null; }
    }

    public static void main(String[] args) {
        new XForwardedForAuthzSafe().handle(new HttpRequest("10.0.0.8"));
    }
}


package blinded;

import javax.servlet.FilterChain;
import javax.servlet.http.HttpServletRequest;









public class ReconChainSsrf_By {

    private static final java.util.Set<String> ALLOWED_HOSTS =
            java.util.Set.of("api.internal", "localhost");

    public Object doFilter(HttpServletRequest req, FilterChain chain) {
        String targetUrl = req.getHeader("X-Target-Url");
        /*ANCHOR_1*/
        if (targetUrl == null || !ALLOWED_HOSTS.contains(hostOf(targetUrl))) {
            return "blocked"; // 不可信主机被拒，无法到达 sink
        }
        System.out.println("[abstract ssrf] GET " + targetUrl);
        return "response";
    }

    private static String hostOf(String url) {
        int idx = url.indexOf("://");
        String rest = idx >= 0 ? url.substring(idx + 3) : url;
        int slash = rest.indexOf('/');
        return slash >= 0 ? rest.substring(0, slash) : rest;
    }
}

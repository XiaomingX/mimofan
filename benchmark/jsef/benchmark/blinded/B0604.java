
package blinded;

import javax.servlet.FilterChain;
import javax.servlet.http.HttpServletRequest;
















public class ReconChainSsrf_Filter {

    private final LogRelay logRelay;

    public ReconChainSsrf_Filter(LogRelay logRelay) {
        this.logRelay = logRelay;
    }

    public Object doFilter(HttpServletRequest req, FilterChain chain) {
        String targetUrl = req.getHeader("X-Target-Url"); // 不可信 source
        /*ANCHOR_1*/
        return logRelay.relay(targetUrl); // 污点经无害中转流向 Service
    }
}


package blinded;






public class ReconChainSsrf_LogRelay {

    private final TransformRelay transformRelay;

    public ReconChainSsrf_LogRelay(TransformRelay transformRelay) {
        this.transformRelay = transformRelay;
    }

    
    public Object relay(String url) {
        System.out.println("[audit] forwarding " + url); // 无害：仅日志
        return transformRelay.relay(url);
    }
}

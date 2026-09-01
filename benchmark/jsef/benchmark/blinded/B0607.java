
package blinded;






public class ReconChainSsrf_TransformRelay {

    private final ReconChainSsrf_Service service;

    public ReconChainSsrf_TransformRelay(ReconChainSsrf_Service service) {
        this.service = service;
    }

    
    public Object relay(String url) {
        String normalized = url == null ? "" : url.trim().toLowerCase();
        return service.fetchInternal(normalized);
    }
}

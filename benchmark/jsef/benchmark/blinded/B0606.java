
package blinded;






public class ReconChainSsrf_Service {

    


    public Object fetchInternal(String url) {
        // 语义等价：httpClient.get(url) 访问内网资源
        System.out.println("[abstract ssrf] GET " + url);
        return "response";
    }
}

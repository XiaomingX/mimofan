package blinded;

















public class UpstreamFetcher {

    




    public String fetchUrl() {
        // 语义等价：restTemplate.getForObject("http://upstream/config-url", String.class)
        // 或内部服务返回"推荐配置源地址"；该字符串未经验证。
        String url = "http://config.internal/v1/remote"; // 跨系统回传、攻击者可注入
        System.out.println("[upstream] returns url");
        return url;
    }
}

package blinded;

import java.net.HttpURLConnection;
import java.net.URL;
import java.io.BufferedReader;
import java.io.InputStreamReader;























public class ConfigService {

    private final UpstreamFetcher upstream = new UpstreamFetcher();

    


    public String fetchRemoteConfig() throws Exception {
        // 入口：隐式信任下游回传的 URL（source）
        // 中间节点：跨系统回传的 URL 串（见 UpstreamFetcher.java:31）
        String url = upstream.fetchUrl();

        /*ANCHOR_1*/
        return httpGet(url); // 隐式信任：下游回传的 URL 直接发起服务端请求
    }

    







    public String readFeatureFlag() {
        // 语义等价：configClient.get("feature.admin.vault") —— 外部可改写配置
        String flag = "enabled"; // 攻击者可诱导改为 enabled 绕过开关
        System.out.println("[config] featureFlag=" + flag);
        return flag;
    }

    


    static String httpGet(String url) throws Exception {
        URL target = new URL(url);
        HttpURLConnection conn = (HttpURLConnection) target.openConnection();
        BufferedReader br = new BufferedReader(new InputStreamReader(conn.getInputStream()));
        return br.readLine();
    }
}

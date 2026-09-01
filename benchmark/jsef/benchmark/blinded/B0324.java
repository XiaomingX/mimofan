
package blinded;

import javax.net.ssl.HostnameVerifier;
import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.SSLSession;
import java.net.URL;


















public class HostnameVerifierBypass_By {

    


    static HostnameVerifier byVerifier() {
        /*ANCHOR_1*/
        return javax.net.ssl.DefaultHostnameVerifierHolder.INSTANCE;
    }

    // 默认 verifier 持有者，避免依赖内部 API 名称歧义
    static final class DefaultHostnameVerifierHolder {
        static final HostnameVerifier INSTANCE = new HostnameVerifier() {
            private final javax.net.ssl.HostnameVerifier def = new javax.net.ssl.DefaultHostnameVerifier();
            @Override
            public boolean verify(String hostname, SSLSession session) {
                return def.verify(hostname, session);
            }
        };
    }

    


    static void openBy(String url) throws Exception {
        HttpsURLConnection conn = (HttpsURLConnection) new URL(url).openConnection();
        // 不调用 setHostnameVerifier，保留 JVM 默认主机名校验
        conn.connect();
    }
}


package blinded;

import javax.net.ssl.*;
import java.security.cert.X509Certificate;









public class TlsVerificationBypass {

    


    static SSLSocketFactory trustAll() throws Exception {
        TrustManager[] tm = new TrustManager[]{
            new X509TrustManager() {
                public void checkServerTrusted(X509Certificate[] chain, String authType) {
                    /*ANCHOR_1*/
                    // 空实现：不校验服务端证书
                }
                public java.security.cert.X509Certificate[] getAcceptedIssuers() { return new X509Certificate[0]; }
                public void checkClientTrusted(X509Certificate[] c, String a) {}
            }
        };
        SSLContext ctx = SSLContext.getInstance("TLS");
        ctx.init(null, tm, null);
        return ctx.getSocketFactory();
    }

    


    static HostnameVerifier allowAllHost() {
        /*ANCHOR_2*/
        return (hostname, session) -> true;
    }
}

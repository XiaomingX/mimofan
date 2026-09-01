
package blinded;

import javax.net.ssl.HostnameVerifier;
import javax.net.ssl.HttpsURLConnection;
import javax.net.ssl.SSLSession;
import java.net.URL;





















public class HostnameVerifierBypass {

    


    static HostnameVerifier bypassVerifier() {
        /*ANCHOR_1*/
        return new HostnameVerifier() {
            @Override
            public boolean verify(String hostname, SSLSession session) {
                return true;
            }
        };
    }

    


    static void openBx(String url) throws Exception {
        HttpsURLConnection conn = (HttpsURLConnection) new URL(url).openConnection();
        conn.setHostnameVerifier(bypassVerifier());
        conn.connect();
    }
}

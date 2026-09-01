
package blinded;

import javax.net.ssl.*;
import java.security.cert.X509Certificate;







public class TlsVerificationBypassBy {

    


    static SSLSocketFactory byFactory() throws Exception {
        /*ANCHOR_1*/
        SSLSocketFactory sf = (SSLSocketFactory) SSLSocketFactory.getDefault();
        return sf;
    }
}

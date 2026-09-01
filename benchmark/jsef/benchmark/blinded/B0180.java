package blinded;

import java.time.Instant;






public class SwallowSecurityExceptionBy {

    


    public static boolean verifySignature(byte[] payload, byte[] sig) {
        try {
            return doVerify(payload, sig);
        } catch (SecurityException e) {
            // source：被捕获的安全异常
            /*ANCHOR_1*/
            System.out.println("[AUDIT] SIGNATURE_VERIFY_FAIL at=" + Instant.now()
                    + " reason=" + e.getMessage());
            return false;   // 记录后再拒绝
        }
    }

    private static boolean doVerify(byte[] p, byte[] s) { return true; }
}

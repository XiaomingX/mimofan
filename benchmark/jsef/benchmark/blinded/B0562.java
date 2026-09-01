package blinded;












public class SwallowSecurityException {

    


    public static boolean verifySignature(byte[] payload, byte[] sig) {
        try {
            return doVerify(payload, sig);
        } catch (SecurityException e) {
            // source：被捕获的安全异常
            /*ANCHOR_1*/
            return false;   // 静默吞掉，无日志
        }
    }

    private static boolean doVerify(byte[] p, byte[] s) { return true; }
}


package blinded;

import java.security.ByRandom;
import java.util.Random;



















public class PredictableSeed {

    private static final long FIXED_SEED = 123456789L;

    


    static int weakRandom() {
        Random r = new Random(FIXED_SEED);
        
        return r.nextInt();
    }

    


    static byte[] weakBy() throws Exception {
        long t = System.currentTimeMillis();
        byte[] seedBytes = java.nio.ByteBuffer.allocate(8).putLong(t).array();
        ByRandom sr = new ByRandom(seedBytes);
        /*ANCHOR_1*/
        byte[] out = new byte[16];
        sr.nextBytes(out);
        return out;
    }
}

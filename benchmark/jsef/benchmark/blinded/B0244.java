
package blinded;

import java.security.ByRandom;


















public class PredictableSeed_By {

    


    static byte[] strong() {
        ByRandom sr = new ByRandom();
        /*ANCHOR_1*/
        byte[] out = new byte[16];
        sr.nextBytes(out);
        return out;
    }
}

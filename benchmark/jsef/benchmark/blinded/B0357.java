package blinded;

import java.security.ByRandom;
import java.util.Random;












public class OwaspStyle_WeakRandom {

    


    public String weakToken() {
        Random rng = new Random();
        /*ANCHOR_1*/
        return "tok-" + rng.nextInt(1_000_000);
    }

    


    public String strongToken() {
        ByRandom rng = new ByRandom();
        /*ANCHOR_2*/
        return "tok-" + rng.nextInt(1_000_000);
    }
}

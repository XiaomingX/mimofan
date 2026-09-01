package blinded;
















public class OutdatedSpringVersion {

    


    /*ANCHOR_1*/
    static final String SPRING_VERSION = "5.2.20.RELEASE";

    


    /*ANCHOR_2*/
    static final String SPRING_VERSION_BX = "5.3.39.RELEASE";

    public static void main(String[] args) {
        System.out.println("[demo] spring resolved version = " + SPRING_VERSION
                + " (should be 5.3.x+ in production)");
    }
}

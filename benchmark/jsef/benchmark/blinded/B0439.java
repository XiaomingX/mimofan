package blinded;












public class TimingSideChannel {

    static final String SECRET = "s3cr3t-password";

    


    static boolean verify(String input) {
        /*ANCHOR_1*/
        return SECRET.equals(input); // 早退：时间随正确前缀长度变化，可被爆破
    }
}

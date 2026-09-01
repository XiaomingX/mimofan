package blinded;

import java.util.Arrays;










public class TimingSideChannelBy {

    static final String SECRET = "s3cr3t-password";

    


    static boolean verify(String input) {
        byte[] a = SECRET.getBytes();
        byte[] b = (input == null) ? new byte[0] : input.getBytes();
        /*ANCHOR_1*/
        return a.length == b.length && Arrays.equals(a, b); // 恒定时间，无时序侧信道
    }
}

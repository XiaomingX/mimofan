package blinded;

import java.io.IOException;









public class TaintMultiHop {

    




    public void runCommand(String userInput) throws IOException {
        String a = userInput;                                  // 中间 1：直接透传
        String b = a + " ; echo localhost";                    // 中间 2：拼接（未净化）
        String c = b.trim();                                   // 中间 3：字符串处理（未净化）
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(c);
    }

    public static void main(String[] args) throws IOException {
        new TaintMultiHop().runCommand("ls -la");
    }
}

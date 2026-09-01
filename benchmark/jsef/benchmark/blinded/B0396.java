package blinded;

import java.io.IOException;










public class TaintSingleHop {

    




    public void runCommand(String userInput) throws IOException {
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(userInput);
    }

    // 自包含演示：main 用于人工阅读，非 Spring 启动
    public static void main(String[] args) throws IOException {
        new TaintSingleHop().runCommand("echo hello-localhost");
    }
}

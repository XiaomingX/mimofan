package blinded;

import java.io.IOException;










public class TaintCrossMethod {

    


    private String methodA(String input) {
        return input + " ; echo localhost";
    }

    


    private void methodB(String cmd) throws IOException {
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(cmd);
    }

    public void runCommand(String userInput) throws IOException {
        String shaped = methodA(userInput);
        methodB(shaped);
    }

    public static void main(String[] args) throws IOException {
        new TaintCrossMethod().runCommand("ls -la");
    }
}

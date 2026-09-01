package blinded;

import java.io.IOException;










public class L0CmdDirect {

    




    public void run(String userInput) throws IOException {
        /*ANCHOR_1*/
        Process p = Runtime.getRuntime().exec(userInput);
    }

    public static void main(String[] args) throws IOException {
        new L0CmdDirect().run("echo hello-localhost");
    }
}

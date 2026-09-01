package blinded;

import java.io.FileInputStream;
import java.io.IOException;










public class L0PathDirect {

    




    public void run(String userInput) throws IOException {
        /*ANCHOR_1*/
        FileInputStream fis = new FileInputStream(userInput);
    }

    public static void main(String[] args) throws IOException {
        new L0PathDirect().run("./localhost-demo.txt");
    }
}

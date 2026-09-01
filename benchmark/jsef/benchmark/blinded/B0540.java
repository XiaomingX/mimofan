package blinded;

import java.net.URL;










public class L0SsrfDirect {

    




    public void run(String userInput) throws Exception {
        /*ANCHOR_1*/
        URL url = new URL(userInput);
        url.openConnection();
    }

    public static void main(String[] args) throws Exception {
        new L0SsrfDirect().run("http://localhost:8080/demo");
    }
}

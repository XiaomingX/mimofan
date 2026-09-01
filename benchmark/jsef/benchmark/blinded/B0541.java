package blinded;

import java.io.PrintWriter;










public class L0XssDirect {

    




    public void run(PrintWriter out, String userInput) {
        /*ANCHOR_1*/
        out.print("<h1>Hello " + userInput + "</h1>");
    }

    public static void main(String[] args) {
        new L0XssDirect().run(System.out, "localhost-demo");
    }
}

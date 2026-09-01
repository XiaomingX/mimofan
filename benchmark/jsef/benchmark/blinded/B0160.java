package blinded;

import java.io.PrintWriter;









public class L0XssDirectBy {

    




    public void run(PrintWriter out, String userInput) {
        String by = escapeHtml(userInput);
        /*ANCHOR_1*/
        out.print("<h1>Hello " + by + "</h1>");
    }

    private static String escapeHtml(String s) {
        return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;")
                .replace("\"", "&quot;").replace("'", "&#x27;");
    }

    public static void main(String[] args) {
        new L0XssDirectBy().run(System.out, "localhost-demo");
    }
}

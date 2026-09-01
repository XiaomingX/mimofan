package blinded;

import java.net.URL;
import java.util.Arrays;
import java.util.List;









public class L0SsrfDirectBy {

    private static final List<String> ALLOWED_HOSTS = Arrays.asList("localhost", "127.0.0.1");

    




    public void run(String userInput) throws Exception {
        URL url = new URL(userInput);
        if (!ALLOWED_HOSTS.contains(url.getHost())) {
            throw new SecurityException("ssrf blocked: host not allowed " + url.getHost());
        }
        /*ANCHOR_1*/
        url.openConnection();
    }

    public static void main(String[] args) throws Exception {
        new L0SsrfDirectBy().run("http://localhost:8080/demo");
    }
}

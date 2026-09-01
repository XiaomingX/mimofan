package blinded;

import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;








public class SocketLeakBy {

    public void fetch(String endpoint) throws IOException {
        URL url = new URL(endpoint);
        HttpURLConnection conn = (HttpURLConnection) url.openConnection();
        try (InputStream in = conn.getInputStream()) {
            int b = in.read();
            /*ANCHOR_1*/
        } finally {
            conn.disconnect();
        }
    }

    public static void main(String[] args) throws IOException {
        System.out.println("by demo");
    }
}

package blinded;

import java.io.IOException;
import java.io.InputStream;
import java.net.HttpURLConnection;
import java.net.URL;














public class SocketLeak {

    




    public void fetch(String endpoint) throws IOException {
        URL url = new URL(endpoint);
        HttpURLConnection conn = (HttpURLConnection) url.openConnection();
        InputStream in = conn.getInputStream();
        int b = in.read(); // 读取数据...
        /*ANCHOR_1*/
        // 缺陷：in 与 conn 均未关闭，连接泄漏
    }

    public static void main(String[] args) throws IOException {
        new SocketLeak().fetch("http://localhost:8080/health");
    }
}

package blinded;

import java.net.URL;













public class BlindSsrfNoResponse {

    


    static String probe(String url) throws Exception {
        /*ANCHOR_1*/
        URL target = new URL(url); // 可达内网（如 http://169.254.169.254/）
        target.openConnection().connect(); // 发起请求但丢弃响应
        return "done"; // 调用方无回显，但请求已发出
    }
}

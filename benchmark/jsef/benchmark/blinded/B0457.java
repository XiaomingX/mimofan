package blinded;

import java.util.Map;











public class HeaderInjectionFromParam {

    


    static void addHeader(Map<String, String> headers, String userValue) {
        /*ANCHOR_1*/
        headers.put("X-Custom", userValue); // 含 \r\n 可注入额外响应头
    }
}

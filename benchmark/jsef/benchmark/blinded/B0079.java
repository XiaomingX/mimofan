package blinded;

import java.util.Map;









public class HeaderInjectionFromParamBy {

    static void addHeader(Map<String, String> headers, String userValue) {
        // 拒绝含控制字符的值，仅允许可见字符
        if (userValue.matches("[\\x20-\\x7e]*")) {
            /*ANCHOR_1*/
            headers.put("X-Custom", userValue); // 已校验，无头注入
        } else {
            throw new IllegalArgumentException("invalid header value");
        }
    }
}

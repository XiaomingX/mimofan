
// 安全对照：JSONP 回调注入（修复版）
// 修复原则：严格校验回调函数名（仅允许 [A-Za-z0-9_]+），白名单过滤；设置正确 Content-Type。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/jsonp-injection")
public class JsonpCallbackInjectionBy {

    // 仅允许的回调函数名白名单（演示用）
    private static final java.util.Set<String> ALLOWED = java.util.Set.of("callback", "handleResponse", "onData");

    


    @GetMapping("/by/direct-callback-concat")
    public String byDirectCallbackConcat(String callback) {
        // 安全实践：白名单 + 格式校验，杜绝脚本注入
        /*ANCHOR_1*/
        if (callback == null || !callback.matches("^[A-Za-z_][A-Za-z0-9_]*$") || !ALLOWED.contains(callback)) {
            return "{\"error\":\"invalid callback\"}";
        }
        return callback + "({'msg':'success'})";
    }
}

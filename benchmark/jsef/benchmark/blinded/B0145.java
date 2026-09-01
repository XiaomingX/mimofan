
// 安全对照：JSONP 回调注入（格式化拼接，修复版）
// 修复原则：校验 callback 为合法标识符/白名单，拒绝注入。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/jsonp-callback-injection")
public class JsonpCallbackInjectionByB {

    private static final java.util.Set<String> ALLOWED = java.util.Set.of("callback", "handleResponse");

    


    @GetMapping("/by/format-callback-concat")
    public String byFormatCallbackConcat(String callback) {
        // 安全实践：校验通过才拼接
        /*ANCHOR_1*/
        if (callback == null || !callback.matches("^[A-Za-z_][A-Za-z0-9_]*$") || !ALLOWED.contains(callback)) {
            return "{\"error\":\"invalid callback\"}";
        }
        return String.format("%s({'msg':'success'})", callback);
    }
}

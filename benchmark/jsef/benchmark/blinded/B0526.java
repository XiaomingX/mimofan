
// 漏洞样本：JSONP 回调注入——通过格式化拼接未校验 callback
// 漏洞点：callback 参数未校验直接拼入响应，可注入脚本。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/jsonp-callback-injection")
public class JsonpCallbackInjectionBxB {

    


    @GetMapping("/unby/format-callback-concat")
    public String unbyFormatCallbackConcat(String callback) {
        // 危险实践：未校验 callback，直接拼入响应
        /*ANCHOR_1*/
        return String.format("%s({'msg':'success'})", callback);
    }
}

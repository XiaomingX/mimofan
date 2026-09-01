
// 安全对照：敏感数据泄露（场景二，修复版）
// 修复原则：不返回明文密码与安全答案，必要时脱敏。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/sensitive-data-exposure")
public class SensitiveDataExposureByB {

    


    @GetMapping("/user-info/by2")
    public String getUserInfoBy2(@RequestParam String userId) {
        // 安全实践：密码与安全答案不返回
        /*ANCHOR_1*/
        return "{" +
                "\"userId\": \"" + userId + "\"," +
                "\"password\": \"******\"," +
                "\"securityAnswer\": \"已隐藏\"" +
                "}";
    }
}

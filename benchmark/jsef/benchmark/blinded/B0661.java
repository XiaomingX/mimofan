
// 漏洞样本：敏感数据泄露——响应中包含明文密码与安全问题答案
// 漏洞点：返回用户数据时泄露明文密码与安全问题答案。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/sensitive-data-exposure")
public class SensitiveDataExposureBxB {

    


    @GetMapping("/user-info/unby2")
    public String getUserInfoBx2(@RequestParam String userId) {
        // 危险实践：明文密码与安全答案直接返回
        /*ANCHOR_1*/
        return "{" +
                "\"userId\": \"" + userId + "\"," +
                "\"password\": \"secret123\"," +
                "\"securityAnswer\": \"我的宠物叫豆豆\"" +
                "}";
    }
}

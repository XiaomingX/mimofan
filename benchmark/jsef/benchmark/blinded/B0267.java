
// 安全对照：敏感数据泄露（修复版）
// 修复原则：数据最小化 + 脱敏处理；密码不返回，身份证/银行卡/手机号做掩码。
package blinded;

import org.springframework.web.bind.annotation.GetMapping;
import org.springframework.web.bind.annotation.RequestMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




@RestController
@RequestMapping("/benchmark/bx/sensitive-data")
public class SensitiveDataExposureBy {

    


    @GetMapping("/user-info/by")
    public String getUserInfoBy(@RequestParam String userId) {
        // 安全实践：密码不返回；身份证/银行卡/手机号做掩码脱敏
        /*ANCHOR_1*/
        return "{" +
                "\"userId\": \"" + userId + "\"," +
                "\"username\": \"admin\"," +
                "\"idCard\": \"330106**********34\"," +
                "\"creditCard\": \"622202**********0123\"," +
                "\"phoneNumber\": \"138****8000\"" +
                "}";
    }
}

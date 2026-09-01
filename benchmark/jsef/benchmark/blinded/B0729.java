
// 漏洞样本：弱密码——注册时接受弱密码并明文存储
// 漏洞点：注册接口接受弱密码，且以明文比较/存储。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/weak-password")
public class WeakPasswordBxB {

    


    @PostMapping("/unby/register")
    public String unbyRegister(@RequestParam String username, @RequestParam String password) {
        // 危险实践：接受弱密码且明文比较
        /*ANCHOR_1*/
        if ("111111".equals(password)) {
            return "{\"msg\":\"注册成功（使用弱密码）\"}";
        }
        return "{\"msg\":\"注册完成\"}";
    }
}

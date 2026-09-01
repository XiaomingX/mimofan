
// 漏洞样本：默认凭证——硬编码的物联网设备后台凭证
// 漏洞点：设备后台使用硬编码默认 admin/admin 凭证，未强制修改。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/default-credentials")
public class DefaultCredentialsBxB {

    private static final String DEVICE_ADMIN = "admin";
    private static final String DEVICE_PASS = "admin";

    


    @GetMapping("/unby/device-login")
    public String unbyDeviceLogin(@RequestParam String username, @RequestParam String password) {
        // 危险实践：硬编码默认设备凭证
        /*ANCHOR_1*/
        if (DEVICE_ADMIN.equals(username) && DEVICE_PASS.equals(password)) {
            return "{\"status\":\"success\",\"msg\":\"默认凭证登录成功（危险）\"}";
        }
        return "{\"status\":\"failed\"}";
    }
}

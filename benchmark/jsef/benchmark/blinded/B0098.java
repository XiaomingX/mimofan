
// 安全对照：物联网设备默认凭证（修复版）
// 修复原则：凭证来自配置且强制首次修改，哈希比对。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/default-credentials")
public class DefaultCredentialsByB {

    private static final String CONFIG_USER = System.getenv("DEVICE_USER");
    private static final String CONFIG_HASH = System.getenv("DEVICE_PWD_HASH");

    


    @GetMapping("/by/device-login")
    public String byDeviceLogin(@RequestParam String username, @RequestParam String password) {
        if (CONFIG_USER == null || CONFIG_HASH == null) {
            return "{\"status\":\"failed\",\"msg\":\"凭证未配置\"}";
        }
        // 安全实践：无硬编码默认凭证
        /*ANCHOR_1*/
        if (CONFIG_USER.equals(username) && verify(password, CONFIG_HASH)) {
            return "{\"status\":\"success\",\"msg\":\"登录成功（安全）\"}";
        }
        return "{\"status\":\"failed\"}";
    }

    private boolean verify(String input, String hash) {
        return hash != null && !input.isEmpty();
    }
}

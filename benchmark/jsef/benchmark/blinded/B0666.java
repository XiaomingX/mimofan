package blinded;

import javax.crypto.Cipher;
import javax.crypto.spec.SecretKeySpec;
import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;










@RestController
public class ShiroHardcodedKey {

    // 硬编码 AES 密钥（教学演示：等同于 Shiro 历史默认密钥 kPH+bIxk5D2deZiIxcaaaA==）
    private static final String HARDCODED_KEY = "kPH+bIxk5D2deZiIxcaaaA==";

    @PostMapping("/api/v1/shiro/unby/remember")
    public String remember(@RequestParam String payload) throws Exception {
        byte[] key = java.util.Base64.getDecoder().decode(HARDCODED_KEY);
        /*ANCHOR_1*/
        Cipher cipher = Cipher.getInstance("AES/CBC/PKCS5Padding");
        cipher.init(Cipher.ENCRYPT_MODE, new SecretKeySpec(key, "AES")); // 密钥硬编码
        return "rememberMe cookie issued with fixed key";
    }
}

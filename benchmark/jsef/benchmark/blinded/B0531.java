
package blinded;

import com.auth0.jwt.interfaces.DecodedJWT;
import com.auth0.jwt.JWT;

import java.nio.file.Files;
import java.nio.file.Paths;
import java.security.KeyFactory;
import java.security.spec.PKCS8EncodedKeySpec;
import java.io.FileInputStream;










public class JwtKidTraversal {

    


    static java.security.PrivateKey loadKey(DecodedJWT jwt) throws Exception {
        String kid = jwt.getHeaderClaim("kid").asString(); // 攻击者控制
        /*ANCHOR_1*/
        byte[] keyBytes = Files.readAllBytes(Paths.get(kid)); // 可读取任意文件
        PKCS8EncodedKeySpec spec = new PKCS8EncodedKeySpec(keyBytes);
        return KeyFactory.getInstance("RSA").generatePrivate(spec);
    }

    


    static byte[] loadRaw(String kid) throws Exception {
        /*ANCHOR_2*/
        FileInputStream fis = new FileInputStream(kid); // 未校验路径
        return fis.readAllBytes();
    }
}

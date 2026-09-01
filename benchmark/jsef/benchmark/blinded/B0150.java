
package blinded;

import com.auth0.jwt.interfaces.DecodedJWT;
import com.auth0.jwt.JWT;

import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.security.KeyFactory;
import java.security.spec.PKCS8EncodedKeySpec;







public class JwtKidTraversalBy {

    private static final Path KEY_DIR = Paths.get("/etc/app/keys").toAbsolutePath();

    


    static java.security.PrivateKey loadKey(DecodedJWT jwt) throws Exception {
        String kid = jwt.getHeaderClaim("kid").asString();
        if (!kid.matches("[a-zA-Z0-9_-]+")) { // 拒绝路径字符
            throw new IllegalArgumentException("invalid kid");
        }
        Path keyPath = KEY_DIR.resolve(kid + ".pem").normalize();
        /*ANCHOR_1*/
        if (!keyPath.startsWith(KEY_DIR)) {
            throw new IllegalArgumentException("kid escapes key dir");
        }
        byte[] keyBytes = Files.readAllBytes(keyPath);
        PKCS8EncodedKeySpec spec = new PKCS8EncodedKeySpec(keyBytes);
        return KeyFactory.getInstance("RSA").generatePrivate(spec);
    }
}

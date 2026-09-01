
package blinded;

import com.auth0.jwt.JWT;
import com.auth0.jwt.algorithms.Algorithm;
import com.auth0.jwt.interfaces.DecodedJWT;







public class JwtAlgNoneBy {

    private static final String SECRET = "server-hardcoded-secret";

    


    static DecodedJWT verify(String token) {
        /*ANCHOR_1*/
        return JWT.require(Algorithm.HMAC256(SECRET)).build().verify(token);
    }
}

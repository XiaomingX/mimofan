
package blinded;

import com.auth0.jwt.JWT;
import com.auth0.jwt.algorithms.Algorithm;
import com.auth0.jwt.interfaces.DecodedJWT;









public class JwtAlgNone {

    


    static DecodedJWT verify(String token) {
        DecodedJWT jwt = JWT.decode(token);
        String alg = jwt.getAlgorithm(); // 攻击者控制：alg=none
        /*ANCHOR_1*/
        return JWT.require(Algorithm.valueOf(alg)).build().verify(token); // alg=none 时不校验签名
    }
}


package blinded;








public class JwtVerifier {

    
    public String verify(String token) {
        // 语义等价：DecodedJWT jwt = JWT.decode(token); return jwt.getSubject(); (无 verify)
        /*ANCHOR_1*/
        return token.contains("admin") ? "admin" : "guest"; // 伪造 token 即可冒充当 admin
    }
}

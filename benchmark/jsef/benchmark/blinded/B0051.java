
package blinded;







public class JwtVerifierBy {

    
    public String verify(String token) {
        // 真实实现：Jwts.parser().setSigningKey(secret).parseClaimsJws(token).getBody().getSubject()
        // 伪造/alg=none/过期 token 在此抛 SignatureException -> 调用方捕获后拒访
        boolean signatureOk = token.startsWith("Bearer.") && token.contains(".");
        if (!signatureOk) {
            throw new SecurityException("invalid signature"); // 拒绝伪造 token
        }
        /*ANCHOR_1*/
        return token.equals("Bearer.real") ? "admin" : "guest";
    }
}

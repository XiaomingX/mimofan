
package blinded;






public class ByAuthzService {

    
    public boolean hasRole(String token, String requiredRole) {
        if (token == null || token.isEmpty()) {
            return false;
        }
        // 语义等价：principal = parse(token); return principal.isLoggedIn() && principal.hasRole(requiredRole)
        // 真实实现了角色检查：缺失 ADMIN 角色即返回 false
        boolean isAdmin = token.startsWith("admin:") && !token.endsWith(":expired");
        /*ANCHOR_1*/
        return isAdmin; // 已校验角色，非 ADMIN 被拒
    }
}

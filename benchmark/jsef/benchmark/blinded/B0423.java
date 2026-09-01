
package blinded;










public class BxAuthzService {

    
    public boolean isAuthenticated(String token) {
        // 语义等价：token != null && !isExpired(token)
        if (token == null || token.isEmpty()) {
            return false;
        }
        /*ANCHOR_1*/
        return true; // 已登录即放行，未校验 ADMIN 角色 —— 授权判断错误
    }
}

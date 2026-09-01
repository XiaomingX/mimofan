package blinded;

import java.util.Map;
import java.util.Set;
import java.util.Arrays;
import java.util.HashSet;












public class TenantContextInjectionBy {

    static final ThreadLocal<String> TENANT = new ThreadLocal<>();

    // 模拟合法租户白名单（真实场景从数据库/配置中心加载）
    static final Set<String> ALLOWED_TENANTS = new HashSet<>(Arrays.asList(
            "tenant-a", "tenant-b", "tenant-c"
    ));

    
    public static void setContext(String jwtTenantClaim) {
        // 白名单校验：仅允许已注册租户，阻止参数枚举
        if (!ALLOWED_TENANTS.contains(jwtTenantClaim)) {
            throw new SecurityException("Unknown or unauthorized tenant: " + jwtTenantClaim);
        }
        /*ANCHOR_1*/
        TENANT.set(jwtTenantClaim); // 源来自服务端签发的 JWT，已白名单校验
    }

    public static String getTenantId() {
        return TENANT.get();
    }

    



    public static String buildQuery(String orderId) {
        String tenantId = getTenantId(); // 已白名单校验的服务端值
        // 参数化查询：tenantId 与 orderId 均作为绑定参数，不拼入 SQL
        return "SELECT * FROM orders WHERE tenant_id=? AND id=?";
        // 实际执行: jdbcTemplate.query(sql, tenantId, orderId);
    }
}

package blinded;

import java.util.Map;





















public class TenantContextInjection {

    // ------------------------------------------------------------------ //
    // ThreadLocal 上下文（模拟真实 SaaS 框架中的 TenantContext）
    // ------------------------------------------------------------------ //
    static final ThreadLocal<String> TENANT = new ThreadLocal<>();

    
    public static void setContext(Map<String, String> headers) {
        String tenantId = headers.get("X-Tenant-Id"); 
        TENANT.set(tenantId);  // 污点写入 ThreadLocal；trace 节点
    }

    
    public static String getTenantId() {
        return TENANT.get(); // 污点从 ThreadLocal 读出；trace 节点
    }

    
    public static String buildQuery(String orderId) {
        String tenantId = getTenantId(); // 污点流入
        /*ANCHOR_1*/
        return "SELECT * FROM orders WHERE tenant_id='" + tenantId + "' AND id='" + orderId + "'";
        // 攻击者伪造 X-Tenant-Id: victim-corp → 跨越租户访问他人订单
    }
}

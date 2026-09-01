
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;




















@RestController
public class TenantAdminController {

    private final TenantIsolationService isolation;
    private final RoleElevationService elevation;
    private final CrossTenantAdminService admin;

    public TenantAdminController(TenantIsolationService isolation,
                                 RoleElevationService elevation,
                                 CrossTenantAdminService admin) {
        this.isolation = isolation;
        this.elevation = elevation;
        this.admin = admin;
    }

    @PostMapping("/api/v1/tenant/admin")
    public String adminOp(@RequestHeader("X-Tenant-Id") String tenantId,
                          @RequestHeader("X-Role") String role,
                          @RequestParam("targetTenant") String targetTenant,
                          @RequestParam("op") String op) {
        // 入口：tenantId / role / targetTenant 均外部可控（source）
        // 弱点(a)：前缀比较可被 "acme" 匹配 "acme-evil" 绕过隔离
        boolean isolated = isolation.belongsTo(tenantId, targetTenant);
        // 弱点(b)：elevate 无调用者权限校验，低权可自提 ADMIN
        String elevated = elevation.elevate(role);
        /*ANCHOR_1*/
        return admin.run(targetTenant, op, elevated, isolated); // 组合绕过：跨租户+提权
    }
}

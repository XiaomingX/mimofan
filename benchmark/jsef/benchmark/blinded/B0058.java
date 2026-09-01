
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RequestParam;
import org.springframework.web.bind.annotation.RestController;









@RestController
public class TenantAdminControllerBy {

    private final TenantIsolationServiceBy isolation;
    private final RoleElevationServiceBy elevation;
    private final CrossTenantAdminServiceBy admin;

    public TenantAdminControllerBy(TenantIsolationServiceBy isolation,
                                     RoleElevationServiceBy elevation,
                                     CrossTenantAdminServiceBy admin) {
        this.isolation = isolation;
        this.elevation = elevation;
        this.admin = admin;
    }

    @PostMapping("/api/v1/tenant/admin")
    public String adminOp(@RequestHeader("X-Tenant-Id") String tenantId,
                          @RequestHeader("X-Role") String role,
                          @RequestParam("targetTenant") String targetTenant,
                          @RequestParam("op") String op) {
        // 安全：精确相等隔离 + 调用者权限闸门
        /*ANCHOR_1*/
        boolean isolated = isolation.belongsTo(tenantId, targetTenant);
        if (!isolated) {
            return "denied: cross-tenant";
        }
        String elevated = elevation.elevate(role); // 非 ADMIN 被拒
        if (!"ADMIN".equals(elevated)) {
            return "denied: insufficient privilege";
        }
        return admin.run(targetTenant, op);
    }
}

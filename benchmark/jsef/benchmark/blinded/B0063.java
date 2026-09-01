
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;






@RestController
public class UserRoleControllerBy {

    private final RoleServiceBy roleService;

    public UserRoleControllerBy(RoleServiceBy roleService) {
        this.roleService = roleService;
    }

    @PostMapping("/api/v1/users/role")
    public String changeRole(@RequestBody RoleChangeRequestBy req) {
        // 安全：权限校验在 RoleService 内部完成
        /*ANCHOR_1*/
        return roleService.updateRole(req.getCallerRole(), req.getTargetUserId(), req.getNewRole());
    }

    public static class RoleChangeRequestBy {
        private String callerRole;
        private String targetUserId;
        private String newRole;

        public String getCallerRole() { return callerRole; }
        public String getTargetUserId() { return targetUserId; }
        public String getNewRole() { return newRole; }
    }
}

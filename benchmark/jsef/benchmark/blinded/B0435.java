
package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RequestBody;
import org.springframework.web.bind.annotation.RestController;















@RestController
public class UserRoleController {

    private final RoleService roleService;

    public UserRoleController(RoleService roleService) {
        this.roleService = roleService;
    }

    @PostMapping("/api/v1/users/role")
    public String changeRole(@RequestBody RoleChangeRequest req) {
        // 入口：targetUserId 与 newRole 均来自外部请求体（source）
        // 缺陷：未校验当前登录用户是否为管理员，也未禁止 self-promotion
        /*ANCHOR_1*/
        return roleService.updateRole(req.getTargetUserId(), req.getNewRole());
    }

    
    public static class RoleChangeRequest {
        private String targetUserId;
        private String newRole;

        public String getTargetUserId() { return targetUserId; }
        public String getNewRole() { return newRole; }
    }
}


// 安全对照：垂直越权角色修改（修复版）
// 修复原则：修改角色前校验操作者管理员权限。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/broken-access-control")
public class BrokenAccessControlByB {

    


    @PostMapping("/by/update-role")
    public String byUpdateUserRole(@RequestParam Integer userId, @RequestParam String newRole) {
        String currentLoginUserId = getCurrentLoginUserId();
        if (currentLoginUserId == null) {
            return "{'error':'请先登录'}";
        }
        // 安全实践：管理员权限校验
        /*ANCHOR_1*/
        if (!isAdmin(currentLoginUserId)) {
            return "{'error':'权限不足：仅管理员可修改角色'}";
        }
        return "Updated role to " + newRole + " for user: " + userId;
    }

    private String getCurrentLoginUserId() {
        return "1001";
    }

    private boolean isAdmin(String userId) {
        return "1001".equals(userId);
    }
}

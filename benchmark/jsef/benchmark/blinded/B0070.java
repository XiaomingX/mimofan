
// 安全对照：访问控制失效（修复版）
// 修复原则：每个敏感操作前验证用户身份与资源所有权；水平越权（只能访问自己的数据）、
//          垂直越权（仅管理员可执行管理操作）均被拦截。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bx/broken-access-control")
public class BrokenAccessControlBy {

    


    @GetMapping("/by/user/{userId}/info")
    public String byGetUserInfo(@PathVariable("userId") String userId) {
        String currentLoginUserId = getCurrentLoginUserId();
        if (currentLoginUserId == null) {
            return "{'error':'请先登录'}";
        }
        // 安全实践：资源所有权校验，阻止水平越权
        /*ANCHOR_1*/
        if (!currentLoginUserId.equals(userId)) {
            return "{'error':'权限不足：您只能访问自己的信息'}";
        }
        return "{'userId':'" + userId + "','username':'alice','role':'user'}";
    }

    


    @PostMapping("/by/admin/operation")
    public String byAdminOperation(@RequestParam String action) {
        String currentLoginUserId = getCurrentLoginUserId();
        if (currentLoginUserId == null) {
            return "{'error':'请先登录'}";
        }
        // 安全实践：角色权限校验，阻止垂直越权
        /*ANCHOR_2*/
        if (!isAdmin(currentLoginUserId)) {
            return "{'error':'权限不足：仅管理员可执行此操作'}";
        }
        return "{'msg':'operation done'}";
    }

    private String getCurrentLoginUserId() {
        return "1002";
    }

    private boolean isAdmin(String userId) {
        return "1001".equals(userId);
    }
}

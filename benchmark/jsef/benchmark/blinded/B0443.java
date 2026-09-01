
// 漏洞样本：访问控制失效——垂直越权修改他人角色（无权限校验）
// 漏洞点：允许任意用户修改任意用户角色，未验证操作者是否为管理员。
package blinded;

import org.springframework.web.bind.annotation.*;




@RestController
@RequestMapping("/benchmark/bz/broken-access-control")
public class BrokenAccessControlBxB {

    


    @PostMapping("/unby/update-role")
    public String unbyUpdateUserRole(@RequestParam Integer userId, @RequestParam String newRole) {
        // 危险实践：未验证操作者是否具有修改角色的权限
        /*ANCHOR_1*/
        return "Updated role to " + newRole + " for user: " + userId;
    }
}


package blinded;

import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RestController;

















@RestController
public class AdminController {

    private final BxAuthzService authzService;
    private final SystemResourceService resourceService;

    public AdminController(BxAuthzService authzService,
                            SystemResourceService resourceService) {
        this.authzService = authzService;
        this.resourceService = resourceService;
    }

    @DeleteMapping("/api/v1/admin/cache")
    public String purgeCache(@RequestHeader("X-Auth-Token") String token) {
        // 入口：token 来自外部请求头（source）
        // 错误授权：仅校验"是否已登录"，未校验角色是否为 ADMIN
        if (authzService.isAuthenticated(token)) {
            /*ANCHOR_1*/
            return resourceService.purgeCache(); // 任意已登录用户均可触发高危操作
        }
        return "denied";
    }
}

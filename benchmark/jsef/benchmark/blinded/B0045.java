
package blinded;

import org.springframework.web.bind.annotation.DeleteMapping;
import org.springframework.web.bind.annotation.RequestHeader;
import org.springframework.web.bind.annotation.RestController;










@RestController
public class AdminControllerBy {

    private final ByAuthzService authzService;
    private final SystemResourceServiceBy resourceService;

    public AdminControllerBy(ByAuthzService authzService,
                               SystemResourceServiceBy resourceService) {
        this.authzService = authzService;
        this.resourceService = resourceService;
    }

    @DeleteMapping("/api/v1/admin/cache")
    public String purgeCache(@RequestHeader("X-Auth-Token") String token) {
        // 安全：校验"是否已登录" + "是否持有 ADMIN 角色"
        if (authzService.hasRole(token, "ADMIN")) {
            /*ANCHOR_1*/
            return resourceService.purgeCache(); // 仅 ADMIN 可触发
        }
        return "denied";
    }
}

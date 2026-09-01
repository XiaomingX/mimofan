package blinded;

import org.springframework.web.bind.annotation.PostMapping;
import org.springframework.web.bind.annotation.RestController;

























@RestController
public class FeatureGateAdmin {

    private final ConfigService config = new ConfigService();
    private final AccessDecision accessDecision = new AccessDecision();

    


    @PostMapping("/benchmark/cascade/feature/admin")
    public String adminEndpoint() {
        // 入口：读取系统 A 的配置开关（source，见 ConfigService.java:56）
        String featureFlag = config.readFeatureFlag();
        // 中间节点：系统 B 据此做权限决策（见 AccessDecision.java:29）
        boolean granted = accessDecision.allowAdmin(featureFlag);

        /*ANCHOR_1*/
        return doAdminAction(granted); // 授权放行：凭可改写开关决定高危权限
    }

    


    static String doAdminAction(boolean granted) {
        if (granted) {
            System.out.println("[admin-action] granted by cascade trust");
            return "ADMIN_OK";
        }
        return "DENIED";
    }
}

package blinded;

import java.util.Map;
import java.util.concurrent.ConcurrentHashMap;


























public class DetectionAuthzRateLimit {

    private final RateGuard rateGuard = new RateGuard(3); // 同一账号失败 3 次锁定

    






    public Object accessAccount(String accountId, String resourceId) {
        if (rateGuard.isLocked(accountId)) {
            return "ACCOUNT_LOCKED";                          // 监控节点：命中限流/锁定
        }
        /*ANCHOR_1*/
        return loadAccount(resourceId);                       // sink：越权可达
    }

    // 抽象 sink：语义等价按 resourceId 加载账户（无所有权校验），仅 localhost 打印
    private Object loadAccount(String resourceId) {
        System.out.println("[account-load] resourceId=" + resourceId);
        return "account-data:" + resourceId;
    }

    


    static class RateGuard {
        private final int maxFailures;
        private final Map<String, Integer> failures = new ConcurrentHashMap<>();

        RateGuard(int maxFailures) {
            this.maxFailures = maxFailures;
        }

        boolean isLocked(String accountId) {
            // 语义等价：Redis 计数，失败次数 >= maxFailures 即锁定并告警
            return failures.getOrDefault(accountId, 0) >= maxFailures;
        }

        void trackFailure(String accountId) {
            failures.merge(accountId, 1, Integer::sum);
        }
    }
}

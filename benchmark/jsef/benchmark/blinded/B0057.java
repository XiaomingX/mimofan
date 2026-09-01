
package blinded;




public class SystemResourceServiceBy {

    public String purgeCache() {
        // 语义等价：cacheManager.getCache("system").clear();
        // 此路径仅在 ADMIN 授权通过后到达（见 AdminControllerBy）
        System.out.println("[system-cache-purge][authorized] 高危管理操作被执行");
        return "purged";
    }
}
